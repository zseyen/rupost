use crate::Result;
use crate::ai::config as ai_config;
use crate::ai::rules::build_signature_buckets;
use crate::ai::snapshot_adapter::{snapshot_masking_notice, snapshot_to_json_reports};
use crate::error::RupostError;
use crate::history::read_execution_snapshot;
use crate::report::json::{JsonFileReport, build_json_reports};
use crate::runner::TestResult;
use crate::security::redact::truncate_utf8_safe;
use rupost_ai::{AiClient, FailureClassifier, TriageReport, bucket_to_deterministic_cluster};
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;

/// 从 `JsonFileReport` 切片与可选 `AiClient` 构建 `TriageReport`。
/// 纯函数入口：显式注入 client（`None` = 确定性基线），完全免疫宿主机环境变量（CR-6）。
pub async fn build_triage_report_with_client(
    reports: &[JsonFileReport],
    client: Option<&dyn AiClient>,
) -> TriageReport {
    let total_requests: usize = reports.iter().map(|f| f.results.len()).sum();
    let failed_requests: usize = reports
        .iter()
        .flat_map(|f| &f.results)
        .filter(|r| !r.success && !r.skipped)
        .count();

    if failed_requests == 0 {
        return TriageReport {
            total_requests,
            failed_requests: 0,
            cluster_count: 0,
            ai_enhanced: false,
            clusters: Vec::new(),
        };
    }

    let buckets = build_signature_buckets(reports);

    let Some(ai_client) = client else {
        let clusters: Vec<_> = buckets
            .iter()
            .map(bucket_to_deterministic_cluster)
            .collect();
        return TriageReport {
            total_requests,
            failed_requests,
            cluster_count: clusters.len(),
            ai_enhanced: false,
            clusters,
        };
    };

    let classifier = FailureClassifier::new(ai_client);
    match classifier.classify(&buckets).await {
        Ok(clusters) => TriageReport {
            total_requests,
            failed_requests,
            cluster_count: clusters.len(),
            ai_enhanced: true,
            clusters,
        },
        Err(e) => {
            let safe_err = truncate_utf8_safe(&e.to_string(), 160);
            tracing::warn!(
                "AI triage failed ({}). Falling back to deterministic signature clustering:",
                safe_err
            );
            let clusters: Vec<_> = buckets
                .iter()
                .map(bucket_to_deterministic_cluster)
                .collect();
            TriageReport {
                total_requests,
                failed_requests,
                cluster_count: clusters.len(),
                ai_enhanced: false,
                clusters,
            }
        }
    }
}

/// 从 B4 配置链（env > 用户级文件 > 本地探测）解析客户端并生成 `TriageReport`。
/// 解析失败（GAP-3）时经 tracing 警示并降级确定性分组，exit 0 语义由调用方保证。
pub async fn build_triage_report(reports: &[JsonFileReport]) -> TriageReport {
    // 0 失败短路：无归因对象时跳过 B4 配置解析（含 Ollama 2s 探测窗口），
    // 避免全绿报告也触发"已输出确定性签名分组结果"的误导性降级提示
    let has_failure = reports
        .iter()
        .any(|f| f.results.iter().any(|r| !r.success && !r.skipped));
    if !has_failure {
        return build_triage_report_with_client(reports, None).await;
    }
    let client_arc = match ai_config::resolve().await {
        Ok(resolved) => {
            if let Some(notice) = &resolved.notice {
                // notice 经 tracing（logger stderr writer）：--json 的 stdout 是机器可读输出，不得混入
                tracing::info!("{notice}");
            }
            Some(resolved.client)
        }
        Err(msg) => {
            tracing::warn!("{msg} 已输出确定性签名分组结果");
            None
        }
    };
    build_triage_report_with_client(reports, client_arc.as_deref()).await
}

/// 将 `TriageReport` 格式化为 Markdown 文本
pub fn format_triage_markdown(report: &TriageReport) -> String {
    if report.failed_requests == 0 {
        return format!(
            "## Failure Triage Summary\n\nAll {} requests passed. Zero failures to triage.\n",
            report.total_requests
        );
    }

    let mode_label = if report.ai_enhanced {
        "AI Failure Triage Summary"
    } else {
        "Failure Triage Summary (Deterministic Baseline)"
    };

    let mut out = format!(
        "## {} ({} failures across {} requests -> {} root-cause clusters)\n\n",
        mode_label, report.failed_requests, report.total_requests, report.cluster_count
    );

    for (idx, c) in report.clusters.iter().enumerate() {
        out.push_str(&format!(
            "### {}. [{}] {} ({} failures)\n",
            idx + 1,
            c.category.as_str(),
            c.title,
            c.count
        ));
        out.push_str(&format!(
            "- **Affected**: {}\n",
            c.affected_requests.join(", ")
        ));
        out.push_str(&format!("- **Root Cause**: {}\n", c.root_cause));
        out.push_str(&format!("- **Suggested Fix**: {}\n\n", c.fix_suggestion));
    }

    out
}

/// 供 `rupost test --triage` 在内存批量结果上直接调用并打印（GAP-11：全绿时也输出单行结论）
pub async fn run_inline_triage(batch_results: &[(PathBuf, Vec<TestResult>)]) {
    let json_reports = build_json_reports(batch_results);
    let triage = build_triage_report(&json_reports).await;
    println!("{}", format_triage_markdown(&triage));
}

/// 供 `rupost ai triage <report_path> [--json] [--format markdown|json]` 独立子命令调用
pub async fn run_ai_triage_from_file(
    report_path: &str,
    json_flag: bool,
    format: &str,
) -> Result<()> {
    let content = std::fs::read_to_string(report_path)?;
    let json_reports: Vec<JsonFileReport> = serde_json::from_str(&content).map_err(|e| {
        RupostError::ParseError(format!(
            "Failed to parse JSON test report '{}': {}",
            report_path, e
        ))
    })?;

    let triage = build_triage_report(&json_reports).await;
    if json_flag || format.eq_ignore_ascii_case("json") {
        let s = serde_json::to_string_pretty(&triage)?;
        println!("{s}");
    } else {
        print!("{}", format_triage_markdown(&triage));
    }
    Ok(())
}

/// 离线黑匣子挖掘入口（02-artifact-contract-for-plugins.md §3.2 / 05 ADR-D9：首个内置 miner 消费者，M5）。
///
/// 读取现场拷回的 incident snapshot（ExecutionSnapshotFile Schema v1.0），适配为
/// `JsonFileReport` 后复用失败聚类 SSOT（确定性机械归因为 B1 基线；配置 AI Key 时
/// 五阶段管线的 C 阶段自动增强——与 `ai triage` 同一判定路径）。消费前执行
/// 02-artifact-contract-for-plugins.md §3.3 不可信输入校验：体积上限 64MB。
pub async fn run_snapshot_triage(snapshot_path: &str, json_flag: bool, format: &str) -> Result<()> {
    const MAX_CONSUME_BYTES: u64 = 64 * 1024 * 1024;
    let meta = std::fs::metadata(snapshot_path).map_err(|e| {
        RupostError::IoError(IoError::new(
            ErrorKind::NotFound,
            format!("snapshot 打开失败: {e}"),
        ))
    })?;
    if meta.len() > MAX_CONSUME_BYTES {
        return Err(RupostError::ParseError(format!(
            "snapshot 体积 {} 超过消费上限 64MB（02-artifact-contract-for-plugins.md §3.3 不可信输入防线）",
            meta.len()
        )));
    }
    let snapshot = read_execution_snapshot(snapshot_path)?;

    let masking_note = snapshot_masking_notice(&snapshot);
    let reports = snapshot_to_json_reports(&snapshot);

    if let Some(note) = masking_note {
        tracing::warn!("{note}");
    }
    tracing::info!(
        "快照指纹: {} entries, schema {}, masking={:?}, runner {}/{} v{}",
        snapshot.entries.len(),
        snapshot.schema_version,
        snapshot.masking,
        snapshot.runner_fingerprint.os,
        snapshot.runner_fingerprint.arch,
        snapshot.runner_fingerprint.rupost_version
    );

    let triage = build_triage_report(&reports).await;
    if json_flag || format.eq_ignore_ascii_case("json") {
        let s = serde_json::to_string_pretty(&triage)?;
        println!("{s}");
    } else {
        print!("{}", format_triage_markdown(&triage));
    }
    Ok(())
}
