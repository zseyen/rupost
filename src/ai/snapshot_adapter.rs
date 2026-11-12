//! 黑匣子快照 → AI 分诊报告适配器（03 §7.2 脱敏先于 AI 红线守护）。

use crate::history::model::{ExecutionSnapshotEntry, ExecutionSnapshotFile, MaskingMode};
use crate::report::json::{JsonAssertionReport, JsonFileReport, JsonTestReport};

/// 将离线事件快照（Schema v1.0）适配为聚合聚类消费的 `JsonFileReport` 列表。
/// 若快照尚未脱敏（`MaskingMode::None`），在映射前就地副本执行全链路脱敏（脱敏先于 AI）。
pub fn snapshot_to_json_reports(snapshot: &ExecutionSnapshotFile) -> Vec<JsonFileReport> {
    let results: Vec<JsonTestReport> = snapshot
        .entries
        .iter()
        .map(|entry| {
            if matches!(snapshot.masking, MaskingMode::None) {
                let mut masked = entry.clone();
                crate::history::snapshot_mask::mask_execution_snapshot_entry(&mut masked);
                adapt_entry_to_json_report(&masked)
            } else {
                adapt_entry_to_json_report(entry)
            }
        })
        .collect();
    let file_label = snapshot
        .environment
        .clone()
        .unwrap_or_else(|| "incident".to_string());
    vec![JsonFileReport {
        file_path: file_label,
        results,
    }]
}

fn adapt_entry_to_json_report(e: &ExecutionSnapshotEntry) -> JsonTestReport {
    JsonTestReport {
        request_number: e.request_number,
        name: e.name.clone(),
        method: e.request.method.clone(),
        url: e.request.url.clone(),
        status: e.status,
        duration_ms: e.duration_us / 1000,
        success: e.success,
        error: e.error.as_ref().map(|err| err.message.clone()),
        skipped: false,
        assertions: e
            .assertions
            .iter()
            .map(|a| JsonAssertionReport {
                raw: a.expression.clone(),
                passed: a.passed,
                message: a.message.clone(),
                stream_event_index: None,
                expected: a.expected.clone(),
                actual: a.actual.clone(),
            })
            .collect(),
        warnings: e.warnings.clone(),
        diagnose_report: None,
        response_body_snippet: None,
    }
}

/// 返回面向用户终端的脱敏状态提示（与 `ExecutionSnapshotFile.masking` 对齐）。
pub fn snapshot_masking_notice(snapshot: &ExecutionSnapshotFile) -> Option<&'static str> {
    match snapshot.masking {
        MaskingMode::Secrets => Some(
            "该快照为掩码形态（masking=secrets）：凭据/PII 字段已 [REDACTED]，签名与 Diff 类结论按数据不完整降级",
        ),
        MaskingMode::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{
        ExecutionErrorDetail, ExecutionOutcome, RunnerFingerprint, SNAPSHOT_SCHEMA_VERSION,
        SnapshotAssertion, SnapshotRequest,
    };

    #[test]
    fn test_snapshot_to_json_reports_redacts_unmasked_snapshot_before_ai() {
        let snap = ExecutionSnapshotFile {
            schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
            generated_at: chrono::Utc::now(),
            environment: Some("prod".to_string()),
            runner_fingerprint: RunnerFingerprint {
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
                rupost_version: "0.2.0".to_string(),
            },
            masking: MaskingMode::None,
            entries: vec![ExecutionSnapshotEntry {
                request_number: 1,
                name: Some("pay".to_string()),
                file_path: "pay.http".to_string(),
                success: false,
                outcome: ExecutionOutcome::Fail,
                status: Some(401),
                duration_us: 2_000,
                plugin_duration_us: None,
                request: SnapshotRequest {
                    method: "POST".to_string(),
                    url: "https://u:p@api.example.com/pay?token=raw_query_secret".to_string(),
                    headers: vec![],
                    body: None,
                },
                response: None,
                error: Some(ExecutionErrorDetail {
                    kind: "HttpTransportError".to_string(),
                    message: "rejected Bearer raw_err_secret".to_string(),
                    duration_ms: 2,
                    occurred_at: chrono::Utc::now(),
                }),
                timing_waterfall: None,
                socket_diagnostics: None,
                assertions: vec![SnapshotAssertion {
                    expression: "body.token == 'ok'".to_string(),
                    passed: false,
                    actual: Some("sk_live_raw_actual".to_string()),
                    expected: Some("expected_val".to_string()),
                    message: Some("https://u:p@host?api_key=msg_tok".to_string()),
                }],
                warnings: vec!["Authorization: Bearer warn_raw_secret".to_string()],
            }],
        };

        let reports = snapshot_to_json_reports(&snap);
        let item = &reports[0].results[0];
        assert!(!item.url.contains("raw_query_secret"), "url: {}", item.url);
        assert!(!item.url.contains(":p@"), "url userinfo: {}", item.url);
        let err = item.error.as_deref().unwrap();
        assert!(!err.contains("raw_err_secret"), "err: {err}");
        let actual = item.assertions[0].actual.as_deref().unwrap();
        assert!(!actual.contains("sk_live_raw_actual"), "actual: {actual}");
        assert!(!item.warnings.join("\n").contains("warn_raw_secret"));
    }
}
