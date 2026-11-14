//! 测试套件结果呈现与落盘（CLI 表现层职责，从 `runner::app` 剥离）。

use colored::Colorize;
use rupost::Result;
use rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES;
use rupost::history::save_batch_snapshot;
use rupost::runner::app::SuiteNotice;
use rupost::runner::{TestReporter, TestSuiteOptions, TestSuiteOutcome};

/// 渲染测试套件执行结果并按需落盘快照/报告文件。
pub async fn present_suite_outcome(
    options: &TestSuiteOptions,
    outcome: &TestSuiteOutcome,
) -> Result<()> {
    let is_json = options.report.eq_ignore_ascii_case("json");
    for notice in &outcome.notices {
        match notice {
            SuiteNotice::Stderr(msg) => eprintln!("{msg}"),
            SuiteNotice::TerminalStdout(msg) if !is_json => println!("{msg}"),
            SuiteNotice::TerminalStdout(_) => {}
        }
    }

    if outcome.batch_results.is_empty() {
        return Ok(());
    }

    if outcome.degraded_count > 0 {
        eprintln!(
            "[WARN] {} 个请求以降级形态发送{}",
            outcome.degraded_count,
            if options.strict_plugins {
                "（--strict-plugins：整体判失败，阻断假阳性流入生产）"
            } else {
                "（现场数据已完整录入快照；CI 门禁请加 --strict-plugins）"
            }
        );
    }

    if is_json {
        if options.explain || options.triage {
            eprintln!(
                "{} --explain/--triage 仅在终端报告模式下渲染（--report json 输出保持纯净，已忽略）",
                "⚠".yellow()
            );
        }
        rupost::report::json::report_json(&outcome.batch_results, &mut std::io::stdout())?;
    } else {
        let reporter =
            TestReporter::new(options.verbose).with_debug(options.debug, options.debug_on_failure);
        let mut explain_session = rupost::ai::explain::ExplainSession::new();
        let ai_client = if options.explain {
            rupost::ai::config::resolve()
                .await
                .ok()
                .map(|resolved| resolved.client)
        } else {
            None
        };
        for (path, file_results) in &outcome.batch_results {
            let path_str = path.to_string_lossy().to_string();
            reporter.print_header(&path_str, file_results.len());
            for r in file_results {
                reporter.print_result(r);
                // WP-P2a：失败请求的稳定定位（json 模式走字段，终端给行号）
                if !r.success
                    && !r.skipped
                    && let Some(line) = r.source_line
                {
                    let display_path = rupost::utils::path_guard::normalize_posix_path(path);
                    println!("    {display_path}:{line}");
                }
                if options.explain {
                    explain_session
                        .explain_failure_with_client(r, ai_client.as_deref())
                        .await;
                }
            }
        }
        // R-5（免重填）：疑似会话过期汇总——401 失败集中出现时给出再获取指引
        let unauthorized_failed = outcome
            .batch_results
            .iter()
            .flat_map(|(_, results)| results.iter())
            .filter(|r| !r.success && r.status == Some(401))
            .count();
        if unauthorized_failed > 0 {
            eprintln!(
                "[i] {} 个请求以 401 失败——疑似登录态/token 过期：可重放登录请求，或更新 --cookie-file / --var-state 后重试",
                unauthorized_failed
            );
        }

        TestReporter::print_batch_summary(&outcome.batch_results, outcome.duration);
        if options.triage {
            rupost::ai::triage::run_inline_triage(&outcome.batch_results).await;
        }
    }

    // 保存快照（黑匣子契约 Schema v1.0，02 §2；--env 记入 environment 字段）
    if let Some(snapshot_path) = &options.save_snapshot {
        save_batch_snapshot(
            &outcome.batch_results,
            &outcome.resolved_paths,
            snapshot_path,
            options.mask_snapshot,
            &options.mask_secret_values,
            options.env.as_deref(),
            options
                .max_snapshot_body_bytes
                .unwrap_or(DEFAULT_MAX_SNAPSHOT_BODY_BYTES),
        )?;
    }

    // 报告文件输出（表现层职责）
    if let Some(written) = rupost::report::write_report_files(
        &outcome.batch_results,
        &options.report,
        options.report_file.as_deref(),
        options.env.as_deref(),
    )? {
        if written.overwritten {
            eprintln!("[!] 报告文件已存在，将覆盖: {}", written.path.display());
        }
        let is_html = matches!(written.format, rupost::report::ReportFileFormat::Html);
        let is_junit = matches!(written.format, rupost::report::ReportFileFormat::Junit);
        if (is_html && (options.verbose || options.report.eq_ignore_ascii_case("html")))
            || (is_junit
                && (options.verbose
                    || options.report.eq_ignore_ascii_case("junit")
                    || options.report.eq_ignore_ascii_case("xml")))
        {
            println!("  报告已生成至: {}", written.path.display());
        }
    }

    // WP-4 故障复现包（在报告与普通快照呈现之后）：仅失败时创建；
    // 强制脱敏、目标必须不存在；成功全绿不写目录只说明。
    // 工件 I/O 失败经专用错误变体 exit 2，不掩盖测试本身的 exit 1 结果。
    if let Some(incident_out) = &options.incident_out {
        if outcome.has_failure {
            rupost::history::create_incident_bundle(
                &outcome.batch_results,
                &outcome.resolved_paths,
                options.env.as_deref(),
                &options.mask_secret_values,
                options
                    .max_snapshot_body_bytes
                    .unwrap_or(DEFAULT_MAX_SNAPSHOT_BODY_BYTES),
                std::path::Path::new(incident_out),
            )
            .map_err(|e| rupost::RupostError::IncidentBundleError(e.to_user_friendly_string()))?;
            eprintln!(
                "[i] 故障复现包已生成: {incident_out}（强制脱敏；可用 `rupost replay {incident_out}/snapshot.json --target <URL>` 重放，`rupost ai triage --snapshot {incident_out}/snapshot.json` 分诊）"
            );
        } else {
            eprintln!(
                "[i] --incident-out 已提供，但本次运行全绿：未创建复现包目录（仅失败时产出）"
            );
        }
    }

    Ok(())
}
