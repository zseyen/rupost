pub mod plugin;
pub mod plugins;
pub mod preparer;
pub mod redact;
pub mod shell;
pub mod types;

use crate::Result;
use crate::RupostError;
pub use types::{ExportOptions, ExportRunOptions};

/// 执行命令导出的机制层主逻辑
pub fn run_export(opts: ExportRunOptions) -> Result<()> {
    let registry = plugin::ExporterRegistry::new_builtin();

    // 1. 处理 --list 选项
    if opts.list || opts.target.as_deref() == Some("list") {
        println!("Available export plugins:");
        for (name, aliases, desc) in registry.list() {
            let alias_str = if aliases.is_empty() {
                String::new()
            } else {
                format!(" (aliases: {})", aliases.join(", "))
            };
            println!("  - {:<10}{} : {}", name, alias_str, desc);
        }
        return Ok(());
    }

    let target = opts.target.as_deref().ok_or_else(|| {
        RupostError::Other(
            "Missing export target. Use 'rupost export --list' to view available plugins."
                .to_string(),
        )
    })?;

    let exporter = registry.find(target).ok_or_else(|| {
        RupostError::Other(format!(
            "Unsupported export target '{}'. Use 'rupost export --list' to view available plugins.",
            target
        ))
    })?;

    if opts.paths.is_empty() {
        return Err(RupostError::Other(
            "No input paths provided to export. Example: rupost export curl suite.http".to_string(),
        ));
    }

    // 2. 递归扫描输入文件
    let scanned_paths = crate::parser::scanner::DirectoryScanner::scan(&opts.paths)?;
    if scanned_paths.is_empty() {
        println!("No test files found.");
        return Ok(());
    }

    // 3. 5层级联加载环境配置（配置/env 文件告警走 stderr，不污染导出 stdout——走查缺口 #6）
    let (mut var_context, loader_warnings) =
        crate::variable::ConfigLoader::load_context_with_var_specs(
            opts.env.as_deref(),
            &opts.var,
            opts.env_file.as_deref(),
        );
    for warning in &loader_warnings {
        eprintln!("[!] {warning}");
    }
    var_context.insert("__default_scheme", &opts.default_scheme);

    // 4. 加载 CookieStore (若指定了 cookie_file)
    let cookie_store = if let Some(ref cf) = opts.cookie_file {
        let resolved = crate::middleware::resolve_cookie_path(
            std::path::PathBuf::from(cf),
            opts.env.as_deref(),
        )?;
        if resolved.exists() {
            let file = std::fs::File::open(&resolved)?;
            let reader = std::io::BufReader::new(file);
            // 文件存在即意图（#6 同语义）：JSON 损坏必须可见——导出的命令
            // 将缺失 cookie 注入，静默会让粘贴方拿到必然 401 的命令
            match cookie_store::serde::json::load_all(reader) {
                Ok(store) => Some(store),
                Err(e) => {
                    eprintln!(
                        "[!] cookie 文件 '{}' 解析失败，导出命令将不含 cookie 注入: {e}",
                        resolved.display()
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // 5. 逐文件执行解析与归一化
    let mut rendered_commands = Vec::new();
    let mut captured_symbols = std::collections::HashSet::new();
    let export_opts = ExportOptions {
        compact: opts.compact,
        redact_secrets: opts.redact,
    };
    let redaction_policy = if opts.redact {
        Some(crate::variable::ConfigLoader::find_and_load_redaction_policy())
    } else {
        None
    };
    let mut audit_log = if opts.redact {
        Some(crate::exporter::redact::RedactionAuditLog::new())
    } else {
        None
    };

    for path in scanned_paths {
        let is_markdown = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));

        let parsed_file = if is_markdown {
            crate::parser::MarkdownFileParser::parse_file(&path)?
        } else {
            crate::parser::HttpFileParser::parse_file(&path)?
        };

        let prep_res = preparer::RequestPreparer::prepare_file_with_policy(
            &parsed_file,
            &mut var_context,
            cookie_store.as_ref(),
            opts.name.as_deref(),
            opts.redact,
            redaction_policy.as_ref(),
            audit_log.as_mut(),
            &mut captured_symbols,
        )?;

        for skipped in prep_res.skipped {
            tracing::debug!(
                "Skipped request at {}:{} ({})",
                path.display(),
                skipped.line_number,
                skipped.reason
            );
        }

        for req in prep_res.requests {
            for warn in &req.warnings {
                eprintln!(
                    "[!] Warning ({}): {}",
                    req.source_location.as_deref().unwrap_or("unknown"),
                    warn
                );
            }

            let cmd = exporter.export_one(&req, &export_opts)?;
            let header_comment = match (&req.name, &req.source_location) {
                (Some(n), Some(loc)) => format!("# === [{n}] ({loc}) ==="),
                (Some(n), None) => format!("# === [{n}] ==="),
                (None, Some(loc)) => format!("# === ({loc}) ==="),
                (None, None) => String::new(),
            };
            rendered_commands.push((header_comment, cmd));
        }
    }

    if rendered_commands.is_empty() {
        println!("No active requests matched to export.");
        return Ok(());
    }

    // 6. 输出组装
    let mut full_output = String::new();
    for (i, (comment, cmd)) in rendered_commands.iter().enumerate() {
        if i > 0 {
            full_output.push_str("\n\n");
        }
        if !comment.is_empty() && (rendered_commands.len() > 1 || opts.output.is_some()) {
            full_output.push_str(comment);
            full_output.push('\n');
        }
        full_output.push_str(cmd);
    }
    full_output.push('\n');

    // 7. 写入目标文件或输出到 stdout
    if let Some(ref out_path) = opts.output {
        if out_path.exists() && !opts.force {
            return Err(RupostError::Other(format!(
                "File already exists: {}. Use --force to overwrite.",
                out_path.display()
            )));
        }
        if let Some(parent) = out_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && !p.exists())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_path, full_output)?;
        println!(
            "Exported {} request(s) to {}",
            rendered_commands.len(),
            out_path.display()
        );
    } else {
        print!("{full_output}");
    }

    // 8. 若启用脱敏且未静音，输出脱敏清单 Footer 到 stderr（确保 stdout 命令管道纯净）
    if opts.redact
        && !opts.quiet
        && let Some(ref log) = audit_log
    {
        let mut stderr = std::io::stderr().lock();
        let _ = log.render_footer(&mut stderr);
    }

    Ok(())
}
