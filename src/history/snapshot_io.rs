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
                },
                None => SnapshotRequest {
                    method: r.method.clone(),
                    url: r.url.clone(),
                    headers: Vec::new(),
                    body: None,
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

            let error = if r.response.is_none() {
                r.error.as_ref().map(|err_msg| ExecutionErrorDetail {
                    kind: classify_error_kind(err_msg),
                    message: err_msg.clone(),
                    duration_ms: r.duration.as_millis() as u64,
                    occurred_at: chrono::Utc::now(),
                })
            } else {
                None
            };

            // 时序瀑布 + Socket 指纹：diagnose 报告优先（含 TLS/证书），退回 RequestTiming
            let timing_waterfall = r.timing.as_ref().map(|t| {
                crate::history::model::TimingWaterfallUs::from_probes(t, r.diagnose_report.as_ref())
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
                socket_diagnostics,
                assertions,
                warnings: r.warnings.clone(),
            };
            if mask_secrets {
                mask_execution_snapshot_entry(&mut entry);
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
        masking: if mask_secrets {
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

/// 响应体截断（UTF-8 边界安全）：多字节字符中点绝不 panic。
pub(crate) fn truncate_response_body(body: &str, max_bytes: usize) -> (String, bool) {
    if body.len() <= max_bytes {
        return (body.to_string(), false);
    }
    let mut end = max_bytes;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    (body[..end].to_string(), true)
}

/// 将黑匣子契约快照写入指定文件（Unix 下权限强制 0600）。
pub fn write_execution_snapshot<P: AsRef<Path>>(
    snapshot: &ExecutionSnapshotFile,
    path: P,
) -> Result<()> {
    if let Some(parent) = path.as_ref().parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(RupostError::IoError)?;
    }
    let json = serde_json::to_string_pretty(snapshot)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(RupostError::IoError)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(RupostError::IoError)?;
        file.write_all(json.as_bytes())
            .map_err(RupostError::IoError)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, json).map_err(RupostError::IoError)?;
        Ok(())
    }
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
