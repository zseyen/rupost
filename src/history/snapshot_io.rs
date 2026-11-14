//! 黑匣子契约快照 I/O 与批量提取（ExecutionSnapshotFile Schema v1.0，02 §2）。

use crate::Result;
use crate::error::RupostError;
use crate::history::model::{
    ExecutionErrorDetail, ExecutionOutcome, ExecutionSnapshotEntry, ExecutionSnapshotFile,
    MaskingMode, RunnerFingerprint, SNAPSHOT_SCHEMA_VERSION, SnapshotAssertion, SnapshotRequest,
    SnapshotResponse,
};
use crate::history::snapshot_mask::mask_execution_snapshot_entry;
use std::fs;
use std::path::{Path, PathBuf};

fn classify_error_kind(err_msg: &str) -> String {
    let lower = err_msg.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        "ConnectionTimeout".to_string()
    } else if lower.contains("dns")
        || lower.contains("resolve")
        || lower.contains("name or service not known")
    {
        "DnsResolutionFailed".to_string()
    } else if lower.contains("tls") || lower.contains("ssl") || lower.contains("certificate") {
        "TlsHandshakeFailed".to_string()
    } else if lower.contains("refused") || lower.contains("connection refused") {
        "ConnectionRefused".to_string()
    } else {
        "HttpTransportError".to_string()
    }
}

/// 从批处理测试结果中提取请求/响应快照并写入指定文件
///
/// 写入格式 = 离线数据黑匣子契约 **ExecutionSnapshotFile (Schema v1.0)**（02 §2 唯一权威）。
#[allow(clippy::too_many_arguments)]
pub fn save_batch_snapshot<P: AsRef<Path>>(
    batch_results: &[(PathBuf, Vec<crate::runner::types::TestResult>)],
    source_paths: &[String],
    snapshot_path: P,
    mask_secrets: bool,
    // S-3：按值掩码清单（--mask-secret-value；独立于键名掩码开关生效）
    secret_values: &[String],
    environment: Option<&str>,
    max_body_bytes: usize,
) -> Result<()> {
    let mut entries = Vec::new();
    for (file_path, file_results) in batch_results {
        for r in file_results {
            // P1-11：@teardown 清理请求默认排除出快照（防 replay 重放 DELETE）
            if r.is_teardown {
                continue;
            }
            let snapshot_request = match &r.request {
                Some(req) => SnapshotRequest {
                    method: req.method.clone(),
                    url: req.url.clone(),
                    headers: header_map_to_pairs(&req.headers),
                    body: req.body.clone(),
                    // B-6：@timeout 往返（重放复现原超时语义，不再退回 30s 默认）
                    timeout_ms: req
                        .timeout
                        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                },
                None => SnapshotRequest {
                    method: r.method.clone(),
                    url: r.url.clone(),
                    headers: Vec::new(),
                    body: None,
                    timeout_ms: None,
                },
            };
            let file_path_str = crate::utils::path_guard::normalize_posix_path(
                file_path.to_string_lossy().as_ref(),
            );

            // 响应镜像：256KB 防 OOM 截断 + 截断前完整 Body 的 SHA-256 验真（03 §10.6）
            let response = r.response.as_ref().map(|resp| {
                let (body, truncated) = truncate_response_body(&resp.body, max_body_bytes);
                let body_sha256 = truncated.then(|| {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(resp.body.as_bytes());
                    hasher
                        .finalize()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                });
                SnapshotResponse {
                    status: resp.status.code(),
                    headers: header_map_to_pairs(&resp.headers),
                    body,
                    body_truncated: truncated,
                    body_sha256,
                }
            });

            // A1（裁决 2026-11-14）：error 语义扩展——三形态全覆盖：
            // ① 网络错（无响应）→ 既有 classify_error_kind 分类；
            // ② 有响应但断言失败 → kind=AssertionFailed + 失败断言摘要
            //   （error != null ⇔ 网络异常 的旧假设作废；消费端判据 = response 缺失，
            //    replay/transport_error_entry 已按 response.is_none() 判定，不受影响）；
            // ③ 成功 → None。
            let error = if r.response.is_none() {
                r.error.as_ref().map(|err_msg| ExecutionErrorDetail {
                    kind: classify_error_kind(err_msg),
                    message: err_msg.clone(),
                    duration_ms: r.duration.as_millis() as u64,
                    occurred_at: chrono::Utc::now(),
                })
            } else if !r.success {
                let failed: Vec<String> = r
                    .assertions
                    .iter()
                    .filter(|a| !a.passed)
                    .map(|a| match &a.actual {
                        Some(act) => format!("{} (expected {}, got {})", a.raw, a.expected, act),
                        None => a.raw.clone(),
                    })
                    .collect();
                let message = if failed.is_empty() {
                    "assertions failed".to_string()
                } else {
                    failed.join("; ")
                };
                Some(ExecutionErrorDetail {
                    kind: "AssertionFailed".to_string(),
                    message,
                    duration_ms: r.duration.as_millis() as u64,
                    occurred_at: chrono::Utc::now(),
                })
            } else {
                None
            };

            // 时序瀑布 + Socket 指纹：M7-A Connector 优先（区分复用 None 与 Some(0)），退回 Prober
            let timing_waterfall = r.timing.as_ref().map(|t| {
                if r.timing_source == Some(crate::http::timing::TimingSource::Connector) {
                    crate::history::model::TimingWaterfallUs::from_connect_span(
                        r.connect_span,
                        t.ttfb,
                        t.transfer,
                        r.diagnose_report.as_ref(),
                    )
                } else {
                    crate::history::model::TimingWaterfallUs::from_probes(
                        t,
                        r.diagnose_report.as_ref(),
                    )
                }
            });
            let socket_diagnostics = r
                .diagnose_report
                .as_ref()
                .and_then(crate::history::model::SocketDiagnostics::from_diagnose);

            // 口径不变量 (02 §2)：duration_us = 纯网络 total + 插件耗时；
            // 时序可得时取瀑布 total + plugin_duration_us，否则回退执行墙钟
            let duration_us = timing_waterfall
                .as_ref()
                .map(|t| t.total_us.saturating_add(r.plugin_duration_us.unwrap_or(0)))
                .unwrap_or_else(|| r.duration.as_micros() as u64);

            // 三态判定（03 §10.5）：降级形态的成功严禁混淆为纯粹 Pass
            let outcome = if !r.success {
                ExecutionOutcome::Fail
            } else if r.degraded {
                ExecutionOutcome::DegradedPass
            } else {
                ExecutionOutcome::Pass
            };

            let assertions = r
                .assertions
                .iter()
                .map(|a| SnapshotAssertion {
                    expression: a.raw.clone(),
                    passed: a.passed,
                    actual: a.actual.clone(),
                    expected: Some(a.expected.clone()),
                    message: None,
                })
                .collect();

            let mut entry = ExecutionSnapshotEntry {
                request_number: r.request_number,
                name: r.name.clone(),
                file_path: file_path_str,
                success: r.success,
                outcome,
                status: r.status,
                duration_us,
                plugin_duration_us: r.plugin_duration_us,
                request: snapshot_request,
                response,
                error,
                timing_waterfall,
                timing_source: r.timing_source,
                timing_attribution: r.timing_attribution,
                socket_diagnostics,
                assertions,
                warnings: r.warnings.clone(),
            };
            if mask_secrets || !secret_values.is_empty() {
                mask_execution_snapshot_entry(&mut entry, secret_values);
            }
            entries.push(entry);
        }
    }

    let _ = source_paths;
    let snapshot = ExecutionSnapshotFile {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        generated_at: chrono::Utc::now(),
        environment: environment.map(|e| e.to_string()),
        runner_fingerprint: RunnerFingerprint {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            rupost_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        masking: if mask_secrets || !secret_values.is_empty() {
            MaskingMode::Secrets
        } else {
            MaskingMode::None
        },
        entries,
    };
    write_execution_snapshot(&snapshot, snapshot_path)
}

/// HeaderMap → 保序键值对（非 UTF-8 值走 lossy 保留可读字节）。
pub(crate) fn header_map_to_pairs(map: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    map.iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// 响应体截断（UTF-8 边界安全）：多字节字符中点绝不 panic（SSOT 委托给 `utils::text::truncate_utf8_prefix`）。
pub(crate) fn truncate_response_body(body: &str, max_bytes: usize) -> (String, bool) {
    let (prefix, truncated) = crate::utils::text::truncate_utf8_prefix(body, max_bytes);
    (prefix.to_string(), truncated)
}

/// 将黑匣子契约快照写入指定文件（Unix 下权限强制 0600，原子写入且不跟随符号链接）。
pub fn write_execution_snapshot<P: AsRef<Path>>(
    snapshot: &ExecutionSnapshotFile,
    path: P,
) -> Result<()> {
    let json = serde_json::to_string_pretty(snapshot)?;
    crate::security::fs::write_private_file_atomic(path.as_ref(), json.as_bytes())
}

/// 读取黑匣子契约快照
pub fn read_execution_snapshot<P: AsRef<Path>>(path: P) -> Result<ExecutionSnapshotFile> {
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    serde_json::from_str(&content).map_err(RupostError::JsonError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{DEFAULT_MAX_SNAPSHOT_BODY_BYTES, RequestSnapshot};
    use crate::http::{RequestTiming, Response};
    use crate::runner::types::TestResult;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_save_batch_snapshot_records_failed_entry_without_request_and_adds_plugin_duration() {
        let dir = TempDir::new().unwrap();
        let snap_path = dir.path().join("snap.json");

        // 1. 前置阻断失败（request == None）必须入快照，绝不静默跳过
        let blocked = TestResult::error(
            1,
            Some("blocked".into()),
            "POST".into(),
            "https://api.example.com/pay".into(),
            "plugin missing".into(),
            Duration::from_millis(5),
        );

        // 2. 带 timing + plugin_duration_us 的请求：duration_us == net_total + plugin_duration_us
        let resp = Response::new(
            200,
            HeaderMap::new(),
            "{}".into(),
            Duration::from_millis(10),
            Duration::from_millis(8),
            Duration::from_millis(2),
        )
        .unwrap();
        let mut ok_res = TestResult::success(
            2,
            Some("signed".into()),
            "GET".into(),
            "https://api.example.com/ok".into(),
            resp,
        );
        ok_res.request = Some(RequestSnapshot {
            method: "GET".into(),
            url: "https://api.example.com/ok".into(),
            headers: HeaderMap::new(),
            body: None,
            timeout: None,
        });
        ok_res.timing = Some(RequestTiming {
            dns_lookup: Duration::from_micros(100),
            tcp_connect: Duration::from_micros(200),
            ttfb: Duration::from_micros(5_000),
            transfer: Duration::from_micros(700),
        });
        ok_res.plugin_duration_us = Some(30_000);

        let batch = vec![(PathBuf::from("case.http"), vec![blocked, ok_res])];
        save_batch_snapshot(
            &batch,
            &[],
            &snap_path,
            false,
            &[],
            None,
            DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
        )
        .unwrap();

        let snap = read_execution_snapshot(&snap_path).unwrap();
        assert_eq!(snap.entries.len(), 2, "request=None 的失败条目必须保留");
        assert_eq!(snap.entries[0].outcome, ExecutionOutcome::Fail);
        assert_eq!(snap.entries[0].request.url, "https://api.example.com/pay");
        assert!(snap.entries[0].error.is_some());

        // 100 + 200 + 5000 + 700 + 30000 = 36000
        assert_eq!(snap.entries[1].duration_us, 36_000);
        assert_eq!(snap.entries[1].plugin_duration_us, Some(30_000));
    }

    /// A1（裁决 2026-11-14）：error 语义扩展——有响应但断言失败的条目也填
    /// error（kind=AssertionFailed + 失败断言摘要）；三形态守护：
    /// ① 504+断言失败 → error=Some(AssertionFailed)、response 保留；
    /// ② 网络错（无响应）→ 维持既有分类；
    /// ③ 成功 → error 恒 None。
    #[test]
    fn test_error_detail_covers_assertion_failures_three_forms() {
        let dir = TempDir::new().unwrap();
        let snap_path = dir.path().join("snap.json");

        // ① 有响应 + 断言失败（504 实得，期望 200）
        let mut failed_504 = TestResult::success(
            1,
            Some("slow-upstream".into()),
            "GET".into(),
            "https://api.example.com/slow".into(),
            Response::new(
                504,
                HeaderMap::new(),
                "{\"error\":\"gateway timeout\"}".into(),
                Duration::from_millis(5100),
                Duration::from_millis(5000),
                Duration::from_millis(100),
            )
            .unwrap(),
        );
        failed_504.success = false;
        failed_504
            .assertions
            .push(crate::assertion::AssertionResult {
                raw: "status == 200".to_string(),
                passed: false,
                actual: Some("504".to_string()),
                expected: "200".to_string(),
                message: Some("expected 200, got 504".to_string()),
                stream_event_index: None,
            });

        // ② 网络错（无响应）
        let net_err = TestResult::error(
            2,
            Some("net-fail".into()),
            "GET".into(),
            "https://api.example.com/net".into(),
            "Connection refused".into(),
            Duration::from_millis(3),
        );

        // ③ 成功
        let ok = TestResult::success(
            3,
            Some("ok".into()),
            "GET".into(),
            "https://api.example.com/ok".into(),
            Response::new(
                200,
                HeaderMap::new(),
                "{}".into(),
                Duration::from_millis(5),
                Duration::from_millis(5),
                Duration::ZERO,
            )
            .unwrap(),
        );

        let batch = vec![(PathBuf::from("case.http"), vec![failed_504, net_err, ok])];
        save_batch_snapshot(
            &batch,
            &[],
            &snap_path,
            false,
            &[],
            None,
            DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
        )
        .unwrap();

        let snap = read_execution_snapshot(&snap_path).unwrap();
        assert_eq!(snap.entries.len(), 3);

        // 形态 ①：断言失败 → error=Some(AssertionFailed) + 失败摘要 + response 保留
        let e1 = &snap.entries[0];
        assert_eq!(e1.outcome, ExecutionOutcome::Fail);
        let err1 = e1
            .error
            .as_ref()
            .expect("断言失败必须填充 error（A1 裁决）");
        assert_eq!(err1.kind, "AssertionFailed");
        assert!(
            err1.message.contains("status == 200") && err1.message.contains("504"),
            "error.message 必须携带失败断言摘要: {}",
            err1.message
        );
        assert!(e1.response.is_some(), "有响应条目 response 镜像保留");

        // 形态 ②：网络错维持既有分类
        let e2 = &snap.entries[1];
        let err2 = e2.error.as_ref().expect("网络错必须有 error");
        assert_eq!(err2.kind, "ConnectionRefused");

        // 形态 ③：成功 → error 恒 None
        assert!(snap.entries[2].error.is_none(), "成功条目 error 恒 None");
    }

    #[test]
    fn test_truncate_response_body_char_boundary_and_lossy_headers() {
        let cjk = "汉".repeat(DEFAULT_MAX_SNAPSHOT_BODY_BYTES / 3 + 10);
        let (truncated, flag) = truncate_response_body(&cjk, DEFAULT_MAX_SNAPSHOT_BODY_BYTES);
        assert!(flag);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.chars().all(|c| c == '汉'));

        let mut map = HeaderMap::new();
        map.insert(
            "x-raw",
            HeaderValue::from_bytes(&[0xFF, b'a', 0xFE]).unwrap(),
        );
        let pairs = header_map_to_pairs(&map);
        assert!(pairs[0].1.contains('a'));
    }
}
