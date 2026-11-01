//! F7 `rupost ai resolve`：文件内 `@ai-assert` 指令 → 标准 `@assert` 断言（原地展开）。
//!
//! B2 纯 AI 操作（无规则基线）：AI 全局失败 → 文件原样不动 + 指引 + exit 1；
//! 个别指令失败 → 该行保留原文 + 行内警告 + exit 0。原子写（临时文件 + rename）。

use crate::ai::config;
use crate::assertion::parse_assertion;
use crate::{Result, RupostError};
use colored::Colorize;
use std::path::Path;

/// 单文件最多展开指令数（防 token 失控；超出保留原文 + 汇总提示）
pub const MAX_RESOLVE_DIRECTIVES: usize = 20;
/// 连续 AI 调用失败熔断阈值
pub const CIRCUIT_BREAKER_THRESHOLD: usize = 2;

/// 识别行内指令：trim 后 `# @ai-assert <nl>` / `@ai-assert <nl>` / `// @ai-assert <nl>`，
/// 意图非空才命中（文档性提及不误伤）。返回 (是否 http 注释形态, 意图)。
pub fn parse_directive(line: &str) -> Option<(bool, String)> {
    let trimmed = line.trim_start();
    let (comment, rest) = if let Some(r) = trimmed.strip_prefix("# @ai-assert ") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("// @ai-assert ") {
        (true, r)
    } else {
        trimmed.strip_prefix("@ai-assert ").map(|r| (false, r))?
    };
    let intent = rest.trim().to_string();
    (!intent.is_empty()).then_some((comment, intent))
}

/// 按指令行的形态渲染断言行（.http 注释形态 / .md 裸行形态）
fn render_assert_line(comment_form: bool, expr: &str) -> String {
    if comment_form {
        format!("# @assert {expr}")
    } else {
        format!("@assert {expr}")
    }
}

/// 展开统计
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolveReport {
    pub expanded: usize,
    pub skipped: usize,
    pub failed: Vec<(usize, String)>,
}

/// F7 入口：原地展开 `@ai-assert` 指令（`--dry-run` 仅打印提议，不落盘）。
/// AI 全局失败 → Err（文件原样）；个别指令失败 → 保留原指令 + 行内警告。
pub async fn resolve_file(path: &Path, dry_run: bool) -> Result<ResolveReport> {
    let original = std::fs::read_to_string(path).map_err(RupostError::IoError)?;
    // 无指令早退：普通 .http 文件（无 @ai-assert）不触发 B4 配置解析，
    // 避免未配置 AI 的用户对普通文件也收到"未配置"误报指引
    if !original.contains("@ai-assert") {
        return Ok(ResolveReport::default());
    }
    let resolved = config::resolve().await.map_err(RupostError::Other)?;
    let translator = rupost_ai::AssertionTranslator::new(resolved.client);

    let mut out = String::new();
    let mut report = ResolveReport::default();
    let mut expanded_count = 0usize;
    let mut consecutive_errors = 0usize;
    let mut breaker_open = false;

    for (lineno, line) in original.lines().enumerate() {
        let Some((comment_form, intent)) = parse_directive(line) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };

        if expanded_count >= MAX_RESOLVE_DIRECTIVES || breaker_open {
            report.skipped += 1;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        expanded_count += 1;
        match translator.translate(&intent).await {
            Ok(assertions) => {
                consecutive_errors = 0;
                let mut valid = 0usize;
                for expr in &assertions {
                    let expr = expr.trim();
                    if expr.is_empty() {
                        continue;
                    }
                    // 本地预检（Gap R7-1）：非法断言不得落盘
                    if parse_assertion(expr).is_err() {
                        report
                            .failed
                            .push((lineno + 1, format!("AI 断言非法已丢弃: {expr}")));
                        continue;
                    }
                    out.push_str(&render_assert_line(comment_form, expr));
                    out.push('\n');
                    valid += 1;
                }
                if valid > 0 {
                    report.expanded += 1;
                } else {
                    report
                        .failed
                        .push((lineno + 1, "AI 未返回可用断言".to_string()));
                    out.push_str(line);
                    out.push_str("  # ⚠ 展开失败: 无可用断言\n");
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                // AI 错误文本自带完整原始 URL（密钥外泄主通道，见 redactor.rs），
                // 落盘与行内警告前必须脱敏——与其余 ai 出站路径同口径
                let safe_err = truncate(
                    &crate::ai::redactor::redact_free_text(&e.to_string()),
                    120,
                );
                report.failed.push((lineno + 1, safe_err.clone()));
                out.push_str(line);
                out.push_str(&format!("  # ⚠ 展开失败: {safe_err}\n"));
                if consecutive_errors >= CIRCUIT_BREAKER_THRESHOLD {
                    breaker_open = true;
                    eprintln!(
                        "{} AI 连续失败 {} 次，熔断已开启——剩余指令保留原样",
                        "⚠".yellow(),
                        CIRCUIT_BREAKER_THRESHOLD
                    );
                }
            }
        }
    }

    // B2 语义：全部指令展开失败 = 全局失败 → 文件原样不动 + exit 1；
    // 部分成功 → 落盘 + 警告 + exit 0（失败行保留原指令 + 行内警告）
    if dry_run {
        // R7-5：dry-run 恒 exit 0——打印提议展开，不落盘
        println!("{}", out);
        return Ok(report);
    }

    // B2 语义：全部指令展开失败 = 全局失败 → 文件原样不动 + exit 1；
    // 部分成功 → 落盘 + 警告 + exit 0（失败行保留原指令 + 行内警告）
    if report.expanded == 0 && !report.failed.is_empty() {
        // 明细随 Err 带出（用户可直接看到每行失败原因），文件保持原样
        let detail: Vec<String> = report
            .failed
            .iter()
            .map(|(line, reason)| format!("行 {line}: {reason}"))
            .collect();
        return Err(RupostError::Other(format!(
            "全部 @ai-assert 指令展开失败（AI 错误），文件未修改。失败明细: {}",
            detail.join("；")
        )));
    }

    atomic_write(path, &out)?;
    Ok(report)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// 原子写：同目录临时文件 + rename（防半文件/防覆盖语义由调用方界定）
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("resolve.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        RupostError::IoError(e)
    })?;
    Ok(())
}

/// CLI 入口：解析参数并执行（dry_run / 输出摘要）
pub async fn run_resolve(path: &str, dry_run: bool) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(RupostError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("文件不存在: {path}"),
        )));
    }
    let report = resolve_file(p, dry_run).await?;
    println!(
        "{} 展开 {} 条指令（跳过 {}，失败 {}）{}",
        "[✓]".green().bold(),
        report.expanded,
        report.skipped,
        report.failed.len(),
        if dry_run {
            "（dry-run 未落盘）"
        } else {
            ""
        }
    );
    for (line, reason) in &report.failed {
        println!("  {} 行 {line}: {reason}", "⚠".yellow());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_directive_forms() {
        assert_eq!(
            parse_directive("# @ai-assert 登录成功"),
            Some((true, "登录成功".to_string()))
        );
        assert_eq!(
            parse_directive("  // @ai-assert token exists"),
            Some((true, "token exists".to_string()))
        );
        assert_eq!(
            parse_directive("@ai-assert status ok"),
            Some((false, "status ok".to_string()))
        );
        assert_eq!(parse_directive("# @ai-assert"), None, "空意图不命中");
        assert_eq!(
            parse_directive("使用 @ai-assert 指令"),
            None,
            "句中提及不命中"
        );
        assert_eq!(parse_directive("# @assert status == 200"), None);
    }

    #[test]
    fn test_render_matches_directive_form() {
        assert_eq!(
            render_assert_line(true, "status == 200"),
            "# @assert status == 200"
        );
        assert_eq!(
            render_assert_line(false, "status == 200"),
            "@assert status == 200"
        );
    }

    #[test]
    fn test_directive_budget_constants() {
        assert_eq!(MAX_RESOLVE_DIRECTIVES, 20);
        assert_eq!(CIRCUIT_BREAKER_THRESHOLD, 2);
    }
}
