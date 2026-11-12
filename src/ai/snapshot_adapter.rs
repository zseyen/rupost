//! 黑匣子快照 → AI 分诊报告适配器（03 §7.2 脱敏先于 AI 红线守护）。

use crate::history::model::{ExecutionSnapshotEntry, ExecutionSnapshotFile, MaskingMode};
use crate::report::json::{JsonAssertionReport, JsonFileReport, JsonTestReport};

/// 将离线事件快照（Schema v1.0）适配为聚合聚类消费的 `JsonFileReport` 列表。
/// 对所有条目无条件执行就地副本全链路幂等脱敏（不信任外部输入声明的 `masking` 标记，脱敏先于 AI）。
pub fn snapshot_to_json_reports(snapshot: &ExecutionSnapshotFile) -> Vec<JsonFileReport> {
    let results: Vec<JsonTestReport> = snapshot
        .entries
        .iter()
        .map(|entry| {
            let mut masked = entry.clone();
            crate::history::snapshot_mask::mask_execution_snapshot_entry(&mut masked, &[]);
            adapt_entry_to_json_report(&masked)
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

const HIGH_TTFB_THRESHOLD_US: u64 = 500_000;
const CERT_EXPIRY_WARN_DAYS: i64 = 7;

fn adapt_entry_to_json_report(e: &ExecutionSnapshotEntry) -> JsonTestReport {
    let response_body_snippet = if !e.success {
        e.response
            .as_ref()
            .map(|resp| {
                crate::ai::redactor::redact_and_truncate_body(
                    &resp.body,
                    crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
                )
            })
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    let mut warnings = e.warnings.clone();
    if let Some(wf) = &e.timing_waterfall
        && wf.ttfb_us >= HIGH_TTFB_THRESHOLD_US
    {
        warnings.push(format!(
            "高 TTFB 时延: {}ms (total={}ms)",
            wf.ttfb_us / 1000,
            wf.total_us / 1000
        ));
    }
    if let Some(sock) = &e.socket_diagnostics {
        if let Some(days) = sock.cert_days_remaining
            && days <= CERT_EXPIRY_WARN_DAYS
        {
            let issuer = sock
                .cert_issuer
                .as_deref()
                .map(crate::ai::redactor::redact_free_text)
                .unwrap_or_else(|| "unknown".to_string());
            warnings.push(format!(
                "TLS 证书临近过期或已过期: 剩余 {days} 天 (issuer={issuer})"
            ));
        }
        if !e.success && !sock.remote_ip.is_empty() {
            warnings.push(format!(
                "连接诊断: {}:{} ({})",
                crate::ai::redactor::redact_free_text(&sock.remote_ip),
                sock.remote_port,
                crate::ai::redactor::redact_free_text(&sock.http_version)
            ));
        }
    }

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
        // DegradedPass（可选插件降级形态的成功）在 triage 报告中如实呈现
        degraded: e.outcome == crate::history::model::ExecutionOutcome::DegradedPass,
        timing: None,
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
        warnings,
        diagnose_report: None,
        response_body_snippet,
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
                    timeout_ms: None,
                },
                response: None,
                error: Some(ExecutionErrorDetail {
                    kind: "HttpTransportError".to_string(),
                    message: "rejected Bearer raw_err_secret".to_string(),
                    duration_ms: 2,
                    occurred_at: chrono::Utc::now(),
                }),
                timing_waterfall: None,
                timing_source: None,
                timing_attribution: None,
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

    #[test]
    fn test_snapshot_adapter_always_masks_even_if_claimed_secrets_and_surfaces_diagnostics() {
        use crate::history::model::{SnapshotResponse, SocketDiagnostics, TimingWaterfallUs};

        let snap = ExecutionSnapshotFile {
            schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
            generated_at: chrono::Utc::now(),
            environment: Some("prod".to_string()),
            runner_fingerprint: RunnerFingerprint {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                rupost_version: "0.2.0".to_string(),
            },
            // 恶意或外部手写快照谎称已脱敏
            masking: MaskingMode::Secrets,
            entries: vec![ExecutionSnapshotEntry {
                request_number: 1,
                name: Some("gateway".to_string()),
                file_path: "gw.http".to_string(),
                success: false,
                outcome: ExecutionOutcome::Fail,
                status: Some(504),
                duration_us: 620_000,
                plugin_duration_us: None,
                request: SnapshotRequest {
                    method: "POST".to_string(),
                    url: "https://api.example.com/gw?token=forged_mask_secret".to_string(),
                    headers: vec![],
                    body: None,
                    timeout_ms: None,
                },
                response: Some(SnapshotResponse {
                    status: 504,
                    headers: vec![],
                    body: r#"{"error":"upstream timeout","access_token":"raw_resp_secret"}"#
                        .to_string(),
                    body_truncated: false,
                    body_sha256: None,
                }),
                error: None,
                timing_waterfall: Some(TimingWaterfallUs {
                    dns_lookup_us: Some(1_000),
                    tcp_connect_us: Some(2_000),
                    tls_handshake_us: Some(5_000),
                    ttfb_us: 610_000,
                    transfer_us: 2_000,
                    total_us: 620_000,
                }),
                timing_source: None,
                timing_attribution: None,
                socket_diagnostics: Some(SocketDiagnostics {
                    remote_ip: "10.0.0.9".to_string(),
                    remote_port: 443,
                    http_version: "HTTP/2".to_string(),
                    tls_cipher_suite: None,
                    tls_version: None,
                    cert_days_remaining: Some(3),
                    cert_issuer: Some("CN=InternalCA".to_string()),
                }),
                assertions: vec![],
                warnings: vec![],
            }],
        };

        assert!(
            snapshot_masking_notice(&snap).is_some(),
            "masking_notice 仍应读取原始 masking 标记"
        );
        let reports = snapshot_to_json_reports(&snap);
        let item = &reports[0].results[0];
        assert!(
            !item.url.contains("forged_mask_secret"),
            "即使 masking=secrets 也必须强制幂等脱敏: {}",
            item.url
        );
        let snippet = item
            .response_body_snippet
            .as_deref()
            .expect("失败条目应填充 response_body_snippet");
        assert!(snippet.contains("upstream timeout"), "snippet: {snippet}");
        assert!(
            !snippet.contains("raw_resp_secret"),
            "snippet 必须脱敏: {snippet}"
        );
        let joined_warnings = item.warnings.join("\n");
        assert!(joined_warnings.contains("TTFB"), "{joined_warnings}");
        assert!(joined_warnings.contains("3 天"), "{joined_warnings}");
        assert!(
            joined_warnings.contains("10.0.0.9:443"),
            "{joined_warnings}"
        );
    }
}
