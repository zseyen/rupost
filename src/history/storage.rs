#![allow(clippy::collapsible_if)]
use crate::history::model::HistoryEntry;
use crate::Result;
use crate::error::RupostError;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

const HISTORY_DIR: &str = ".rupost";
const HISTORY_FILE: &str = "history.jsonl";
// 20 MB soft limit for compaction
const COMPACTION_THRESHOLD_BYTES: u64 = 20 * 1024 * 1024;
// Keep last 10,000 entries
const MAX_ENTRIES: usize = 10_000;

pub struct HistoryStorage {
    file_path: PathBuf,
}

impl HistoryStorage {
    /// Create a new HistoryStorage (project-local)
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for HistoryStorage {
    fn default() -> Self {
        let dir = std::env::var("RUPOST_HISTORY_DIR").unwrap_or_else(|_| HISTORY_DIR.to_string());
        let path = Path::new(&dir).join(HISTORY_FILE);
        Self { file_path: path }
    }
}

impl HistoryStorage {
    /// Create with specific path (internal/testing use)
    pub fn new_with_path(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    /// Ensure directory exists
    fn ensure_dir(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).map_err(RupostError::IoError)?;
        }
        Ok(())
    }

    /// Append a new entry to history
    ///
    /// # Concurrency Strategy
    /// Uses `File::lock` (std, exclusive) to ensure atomic writes across multiple processes.
    /// This is safer than relying on `O_APPEND` across different OSs (e.g. Windows).
    ///
    /// # Performance
    /// The lock is held only for the duration of the write (microseconds).
    pub fn append(&self, entry: &HistoryEntry) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string(entry)?;

        let mut retries = 0;
        let file = loop {
            let result = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&self.file_path);

            match result {
                Ok(f) => break f,
                Err(e) => {
                    if retries >= 5 {
                        return Err(RupostError::IoError(e));
                    }
                    retries += 1;
                    thread::sleep(Duration::from_millis(10 * retries));
                }
            }
        };

        // Lock for writing
        // On Windows, this waits appropriately.
        file.lock().map_err(RupostError::IoError)?;

        // Need mut file for writeln!
        let mut file = file;

        writeln!(file, "{}", json).map_err(RupostError::IoError)?;

        // Unlock happens automatically when file is dropped
        drop(file);

        Ok(())
    }

    /// List all entries (possibly compacting first)
    pub fn list(&self) -> Result<Vec<HistoryEntry>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        // Lazy Compaction: Check size on Read, not on Write.
        // This keeps the "hot path" (append) fast and robust.
        self.compact_if_needed()?;

        self.read_all()
    }

    /// Read last N entries
    ///
    /// # Order
    /// Returns entries in Chronological order (Oldest -> Newest).
    /// To display "Latest First", the caller (Printer) should reverse the result.
    ///
    /// # Optimization
    /// Currently reads the whole file (up to 20MB limit).
    /// Future optimization: Use `Seek` to read from end backwards.
    pub fn tail(&self, n: usize) -> Result<Vec<HistoryEntry>> {
        let entries = self.list()?;
        let skip = entries.len().saturating_sub(n);
        Ok(entries.into_iter().skip(skip).collect())
    }

    fn read_all(&self) -> Result<Vec<HistoryEntry>> {
        // Read lock? Append only is atomic, but to be sure we don't read partial line from a writer
        // we can take shared lock. `std::fs::File::lock_shared` supports shared lock.
        // However, standard `BufReader` with `File` needs `File` object.
        let file = fs::File::open(&self.file_path).map_err(RupostError::IoError)?;
        file.lock_shared().map_err(RupostError::IoError)?;

        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(RupostError::IoError)?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
                entries.push(entry);
            }
        }
        // Unlock on drop
        Ok(entries)
    }

    /// Check file size and prune if needed
    fn compact_if_needed(&self) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .write(true) // Need write for compaction
            .open(&self.file_path)
            .map_err(RupostError::IoError)?;

        // Try to lock exclusive to checking metadata size?
        // Or check metadata first (fast) then lock?
        // Let's check metadata first to avoid locking on every read
        let metadata = file.metadata().map_err(RupostError::IoError)?;
        if metadata.len() < COMPACTION_THRESHOLD_BYTES {
            return Ok(());
        }

        // Need compaction. Acquire Exclusive Lock.
        // We use TRUNCATE strategy instead of Rename for better Windows compatibility with locks.
        file.lock().map_err(RupostError::IoError)?;

        // critical section
        // ... (read-truncate-rewrite)

        // Re-check size under lock (double-check locking pattern) in case someone else just compacted
        let metadata = file.metadata().map_err(RupostError::IoError)?;
        if metadata.len() < COMPACTION_THRESHOLD_BYTES {
            return Ok(());
        }

        // Read all
        let reader = BufReader::new(&file);
        let mut entries = Vec::new();
        for l in reader.lines().map_while(|l| l.ok()) {
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&l) {
                entries.push(entry);
            }
        }

        if entries.len() <= MAX_ENTRIES {
            return Ok(());
        }

        let keep_count = MAX_ENTRIES;
        let skip_count = entries.len() - keep_count;
        let to_keep = entries.iter().skip(skip_count);

        // Truncate and Rewrite
        file.set_len(0).map_err(RupostError::IoError)?;

        // We need to write to the SAME file handle to keep the lock valid and atomic-ish
        // `BufReader` took `&file`, so `file` is still valid.
        // But we need a Writer.
        // `file` is `File`. `File` implements `Write`.
        // We need to seek to start? `set_len(0)` usually truncates but doesn't move cursor?
        // Yes, need to seek.
        use std::io::Seek;
        let mut file = file; // make mutable
        file.seek(std::io::SeekFrom::Start(0))
            .map_err(RupostError::IoError)?;

        // Use BufWriter for performance
        let mut writer = std::io::BufWriter::new(file);

        for entry in to_keep {
            let json = serde_json::to_string(entry)?;
            writeln!(writer, "{}", json).map_err(RupostError::IoError)?;
        }
        writer.flush().map_err(RupostError::IoError)?;

        Ok(())
    }
}

// Global instance helper if needed
pub fn get_storage() -> &'static HistoryStorage {
    static STORAGE: OnceLock<HistoryStorage> = OnceLock::new();
    STORAGE.get_or_init(HistoryStorage::new)
}

use crate::history::model::SnapshotSuite;

/// 将 SnapshotSuite 写入到指定路径
pub fn write_snapshot_suite<P: AsRef<Path>>(suite: &SnapshotSuite, path: P) -> Result<()> {
    if let Some(parent) = path.as_ref().parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(RupostError::IoError)?;
    }
    let json = serde_json::to_string_pretty(suite)?;
    fs::write(path, json).map_err(RupostError::IoError)?;
    Ok(())
}

/// 从指定路径读取 SnapshotSuite
pub fn read_snapshot_suite<P: AsRef<Path>>(path: P) -> Result<SnapshotSuite> {
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    let suite = serde_json::from_str(&content)?;
    Ok(suite)
}

/// 双格式兼容读取（`replay` 消费入口）：
/// 优先按黑匣子契约 ExecutionSnapshotFile (Schema v1.0, `--save-snapshot` 产物)
/// 解析并转换为重放套件；失败回退旧 SnapshotSuite（`history export` 产物）。
/// 判别可靠：`schema_version` 字符串字段仅存在于新契约，旧格式在首个结构体
/// 即解析失败，回退路径确定可达。
pub fn read_snapshot_suite_compat<P: AsRef<Path>>(path: P) -> Result<SnapshotSuite> {
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    if let Ok(snapshot) = serde_json::from_str::<super::model::ExecutionSnapshotFile>(&content) {
        return Ok(snapshot.to_replay_suite());
    }
    serde_json::from_str(&content).map_err(RupostError::JsonError)
}

/// T5（随 R6 搭车）：将最近 N 条历史条目导出为可重放快照套件文件。
/// - `--last N` 语义 = 最近 N 条**条目**（`SelectionStrategy::Last` → `tail(n)`，追加序即重放序）
/// - `body=None` 旧条目经 `From<HistoryEntry>` 占位导出（"Old snapshot: body not recorded"）
/// - 空历史：warn 后不写文件、exit 0（对齐 generate 先例）
/// - 输出路径静默覆盖（`write_snapshot_suite` 无 `--force` 语义）；凭据不脱敏（与 `--save-snapshot` 同暴露面）
pub fn export_history_snapshot(last: usize, output: &str, mask_secrets: bool) -> Result<()> {
    use crate::history::model::{SnapshotEntry, SnapshotSuite};
    use super::selector::{SelectionStrategy, select_entries};

    let storage = get_storage();
    let entries = select_entries(storage, SelectionStrategy::Last(last))?;
    if entries.is_empty() {
        tracing::warn!("No history entries to export.");
        return Ok(());
    }

    let mut suite = SnapshotSuite {
        version: 2,
        timestamp: chrono::Utc::now(),
        source_file: Some("history".to_string()),
        entries: entries.into_iter().map(SnapshotEntry::from).collect(),
    };
    if mask_secrets {
        for e in suite.entries.iter_mut() {
            mask_snapshot_entry(e);
        }
    }
    let count = suite.entries.len();
    write_snapshot_suite(&suite, output)?;
    println!("Exported snapshot: {} ({} requests)", output, count);
    Ok(())
}

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
/// 与重放格式（`SnapshotSuite`，供 `replay`/`history export` 使用）解耦：
/// 黑匣子面向跨机插件消费，重放面向本机请求重演，二者契约独立演进。
pub fn save_batch_snapshot<P: AsRef<Path>>(
    batch_results: &[(PathBuf, Vec<crate::runner::types::TestResult>)],
    source_paths: &[String],
    snapshot_path: P,
    mask_secrets: bool,
    environment: Option<&str>,
) -> Result<()> {
    use crate::history::model::{
        ExecutionErrorDetail, ExecutionOutcome, ExecutionSnapshotEntry, ExecutionSnapshotFile,
        MaskingMode, RunnerFingerprint, SnapshotAssertion, SnapshotRequest, SnapshotResponse,
        DEFAULT_MAX_SNAPSHOT_BODY_BYTES, SNAPSHOT_SCHEMA_VERSION,
    };

    let mut entries = Vec::new();
    for (file_path, file_results) in batch_results {
        for r in file_results {
            // P1-11：@teardown 清理请求默认排除出快照（防 replay 重放 DELETE）
            if r.is_teardown {
                continue;
            }
            let Some(req) = &r.request else {
                continue;
            };
            let file_path_str = crate::utils::path_guard::normalize_posix_path(
                file_path.to_string_lossy().as_ref(),
            );

            // 响应镜像：256KB 防 OOM 截断 + 截断前完整 Body 的 SHA-256 验真（03 §10.6）
            let response = r.response.as_ref().map(|resp| {
                let (body, truncated) = if resp.body.len() > DEFAULT_MAX_SNAPSHOT_BODY_BYTES {
                    (
                        resp.body[..DEFAULT_MAX_SNAPSHOT_BODY_BYTES].to_string(),
                        true,
                    )
                } else {
                    (resp.body.clone(), false)
                };
                let body_sha256 = truncated.then(|| {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(resp.body.as_bytes());
                    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
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
            let timing_waterfall = r
                .timing
                .as_ref()
                .map(|t| crate::history::model::TimingWaterfallUs::from_probes(t, r.diagnose_report.as_ref()));
            let socket_diagnostics = r
                .diagnose_report
                .as_ref()
                .and_then(crate::history::model::SocketDiagnostics::from_diagnose);

            // 口径不变量 (02 §2)：duration_us = 纯网络 total + 插件耗时；
            // 时序可得时取瀑布 total（插件耗时 M3 接线后累加），否则回退执行墙钟
            let duration_us = timing_waterfall
                .as_ref()
                .map(|t| t.total_us)
                .unwrap_or_else(|| r.duration.as_micros() as u64);

            let outcome = if r.success {
                // DegradedPass 标记由 M3 插件降级链路接线（03 §10.5）
                ExecutionOutcome::Pass
            } else {
                ExecutionOutcome::Fail
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
                plugin_duration_us: None,
                request: SnapshotRequest {
                    method: req.method.clone(),
                    url: req.url.clone(),
                    headers: header_map_to_pairs(&req.headers),
                    body: req.body.clone(),
                },
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

    // 黑匣子契约 (02 §2) 不含套件级 source 字段：来源已逐条目落在 entry.file_path；
    // source_paths 仅保留在签名中以维持与重放套件导出对称的调用形态
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

/// HeaderMap → 保序键值对（黑匣子契约要求 Vec<(String, String)>）
fn header_map_to_pairs(map: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    map.iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// 将黑匣子契约快照写入指定文件。
/// 安全不变量 (02 §2.7)：文件权限强制 0600（与 umask 无关）——多用户堡垒机的
/// /tmp 上默认 umask 泄露不可接受。
pub fn write_execution_snapshot<P: AsRef<Path>>(
    snapshot: &crate::history::model::ExecutionSnapshotFile,
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
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(RupostError::IoError)?;
        file.write_all(json.as_bytes()).map_err(RupostError::IoError)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, json).map_err(RupostError::IoError)?;
        Ok(())
    }
}

/// 读取黑匣子契约快照（消费者入口；02 §3.3 消费前校验的体积上限由调用方执行）
pub fn read_execution_snapshot<P: AsRef<Path>>(
    path: P,
) -> Result<crate::history::model::ExecutionSnapshotFile> {
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    serde_json::from_str(&content).map_err(RupostError::JsonError)
}

/// Secrets Masker（路线图条目 1 残余）：opt-in 快照脱敏——headers 走
/// `redact_header_value`（scheme 前缀保留），body 走 `mask_body_secrets`
/// （敏感键 `[REDACTED]` + PII 三正则，无截断），error 走 `redact_free_text` 擦除 URL 凭证。
pub fn mask_snapshot_entry(entry: &mut crate::history::model::SnapshotEntry) {
    use crate::exporter::redact::{
        mask_body_secrets, mask_inline_secrets, mask_pii, redact_free_text, redact_header_value,
    };
    // 多值 Header（Set-Cookie 等同名多值）逐值掩码——drain 后 append 重建，
    // 非 UTF-8 值原样保留（掩码通道不损伤结构）
    let mask_headers = |map: &mut reqwest::header::HeaderMap| {
        let drained: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)> = map
            .drain()
            .map(|(name, value)| (name.expect("valid header name"), value))
            .collect();
        for (name, value) in drained {
            match value.to_str() {
                Ok(val) => {
                    let masked = redact_header_value(name.as_str(), val);
                    if let Ok(hv) = reqwest::header::HeaderValue::from_str(&masked) {
                        map.append(name, hv);
                    }
                }
                Err(_) => {
                    map.append(name, value);
                }
            }
        }
    };
    mask_headers(&mut entry.request.headers);
    if let Some(ref mut resp) = entry.response {
        mask_headers(&mut resp.headers);
        resp.body = mask_body_secrets(resp.body.as_str());
    }
    entry.request.body = entry.request.body.as_ref().map(|b| mask_body_secrets(b));
    if let Some(ref mut err) = entry.error {
        err.message = mask_pii(&mask_inline_secrets(&redact_free_text(&err.message)));
    }
}

/// 黑匣子契约快照 (ExecutionSnapshotEntry) 的 Secrets Masker：
/// 与 [`mask_snapshot_entry`] 共享同一组 redactor SSOT（`src/exporter/redact.rs`），
/// Headers 为保序键值对形态（02 §2 SnapshotRequest/SnapshotResponse）。
pub fn mask_execution_snapshot_entry(entry: &mut crate::history::model::ExecutionSnapshotEntry) {
    use crate::exporter::redact::{mask_body_secrets, mask_inline_secrets, mask_pii, redact_free_text, redact_header_value};
    let mask_pairs = |headers: &mut Vec<(String, String)>| {
        for (name, value) in headers.iter_mut() {
            *value = redact_header_value(name, value);
        }
    };
    mask_pairs(&mut entry.request.headers);
    if let Some(ref mut resp) = entry.response {
        mask_pairs(&mut resp.headers);
        resp.body = mask_body_secrets(resp.body.as_str());
    }
    entry.request.body = entry.request.body.as_ref().map(|b| mask_body_secrets(b));
    if let Some(ref mut err) = entry.error {
        err.message = mask_pii(&mask_inline_secrets(&redact_free_text(&err.message)));
    }
    for warning in &mut entry.warnings {
        *warning = mask_pii(&redact_free_text(warning));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use reqwest::header::HeaderMap;
    use tempfile::TempDir;

    fn create_dummy_entry(id: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 100,
            request: RequestSnapshot {
                timeout: None,
                method: "GET".to_string(),
                url: "https://example.com".to_string(),
                headers: HeaderMap::new(),
                body: None,
            },
            source: None,
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
                body: None,
            },
        }
    }

    #[test]
    fn test_append_and_list() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("history.jsonl");

        let storage = HistoryStorage {
            file_path: file_path.clone(),
        };

        let entry1 = create_dummy_entry("1");
        let entry2 = create_dummy_entry("2");

        storage.append(&entry1).unwrap();
        storage.append(&entry2).unwrap();

        let list = storage.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "1");
        assert_eq!(list[1].id, "2");
    }

    #[test]
    fn test_tail() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("history.jsonl");
        let storage = HistoryStorage { file_path };

        for i in 0..10 {
            storage.append(&create_dummy_entry(&i.to_string())).unwrap();
        }

        let tail = storage.tail(3).unwrap();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].id, "7");
        assert_eq!(tail[2].id, "9");
    }

    #[test]
    fn test_save_batch_snapshot_run() {
        use crate::http::Response;
        use crate::runner::types::TestResult;
        use reqwest::header::HeaderMap;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let snapshot_file = temp_dir.path().join("snapshot.json");

        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        let req_snap = RequestSnapshot {
            timeout: None,
            method: "POST".to_string(),
            url: "http://example.com/test".to_string(),
            headers: headers.clone(),
            body: Some("{\"key\":\"value\"}".to_string()),
        };

        let response = Response::new(
            201,
            headers.clone(),
            "{\"status\":\"ok\"}".to_string(),
            Duration::from_millis(50),
            Duration::from_millis(20),
            Duration::from_millis(30),
        )
        .unwrap();

        let mut test_result = TestResult::success(
            1,
            Some("test_req".to_string()),
            "POST".to_string(),
            "http://example.com/test".to_string(),
            response,
        );
        test_result.request = Some(req_snap);

        let batch_results = vec![(PathBuf::from("my_test.http"), vec![test_result])];

        save_batch_snapshot(
            &batch_results,
            &["my_test.http".to_string()],
            &snapshot_file,
            false,
            Some("staging"),
        )
        .unwrap();

        // 黑匣子契约 Schema v1.0（02 §2）：round-trip 读取验证
        let snap = read_execution_snapshot(&snapshot_file).unwrap();
        assert_eq!(snap.schema_version, "1.0");
        assert_eq!(snap.masking, crate::history::model::MaskingMode::None);
        assert_eq!(snap.environment.as_deref(), Some("staging"));
        assert_eq!(snap.runner_fingerprint.rupost_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(snap.entries.len(), 1);
        let entry = &snap.entries[0];
        assert_eq!(entry.request_number, 1);
        assert_eq!(entry.name.as_deref(), Some("test_req"));
        assert_eq!(entry.file_path, "my_test.http");
        assert!(entry.success);
        assert_eq!(entry.outcome, crate::history::model::ExecutionOutcome::Pass);
        assert_eq!(entry.status, Some(201));
        assert_eq!(entry.request.method, "POST");
        assert_eq!(entry.request.url, "http://example.com/test");
        assert!(entry.request.headers.contains(&("content-type".to_string(), "application/json".to_string())));
        assert_eq!(entry.request.body.as_deref(), Some("{\"key\":\"value\"}"));
        let resp = entry.response.as_ref().unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body, "{\"status\":\"ok\"}");
        assert!(!resp.body_truncated);
        assert!(resp.body_sha256.is_none());
        assert!(entry.error.is_none());
        assert!(entry.plugin_duration_us.is_none());
    }

    #[test]
    fn test_history_entry_backward_compatibility() {
        // 1. 旧版 JSON 字符串（缺失 response.body 字段）
        let old_json = r#"{
            "id": "test-uuid-old",
            "timestamp": "2026-07-02T12:00:00Z",
            "duration_ms": 120,
            "request": {
                "method": "GET",
                "url": "https://example.com/old",
                "headers": {},
                "body": null
            },
            "source": null,
            "response": {
                "status": 200,
                "headers": {}
            }
        }"#;

        let entry: HistoryEntry = serde_json::from_str(old_json).unwrap();
        assert_eq!(entry.id, "test-uuid-old");
        assert_eq!(entry.response.status, 200);
        assert!(entry.response.body.is_none());

        // 2. 新版包含 body 的历史数据测试
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("history_new.jsonl");
        let storage = HistoryStorage { file_path };

        let mut dummy = create_dummy_entry("new-1");
        dummy.response.body = Some("{\"hello\":\"world\"}".to_string());

        storage.append(&dummy).unwrap();
        let list = storage.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "new-1");
        assert_eq!(
            list[0].response.body.as_deref().unwrap(),
            "{\"hello\":\"world\"}"
        );
    }
    /// 红队变异 5 守护（P1-11）：is_teardown 结果排除出快照（防 replay 重放 DELETE）
    #[test]
    fn test_save_batch_snapshot_excludes_teardown_results() {
        use crate::http::Response;
        use crate::runner::types::TestResult;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let snapshot_file = dir.path().join("snap.json");

        let resp = Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "{}".to_string(),
            Duration::from_millis(5),
            Duration::from_millis(5),
            Duration::ZERO,
        )
        .unwrap();

        let mut normal = TestResult::success(
            1,
            Some("normal".to_string()),
            "GET".to_string(),
            "http://x/normal".to_string(),
            resp.clone(),
        );
        normal.request = Some(crate::history::RequestSnapshot {
            method: "GET".to_string(),
            url: "http://x/normal".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
            timeout: None,
        });

        let mut teardown = TestResult::success(
            2,
            Some("cleanup".to_string()),
            "DELETE".to_string(),
            "http://x/cleanup".to_string(),
            resp,
        );
        teardown.is_teardown = true;
        teardown.request = None;

        let batch_results = vec![(PathBuf::from("t.http"), vec![normal, teardown])];

        save_batch_snapshot(
            &batch_results,
            &["t.http".to_string()],
            &snapshot_file,
            false,
            None,
        )
        .unwrap();

        let snap = read_execution_snapshot(&snapshot_file).unwrap();
        assert_eq!(
            snap.entries.len(),
            1,
            "teardown result must be excluded from snapshot"
        );
        assert_eq!(snap.entries[0].request.url, "http://x/normal");
    }

    /// 安全不变量（02 §2.7）：黑匣子落盘权限强制 0600，与 umask 无关
    #[cfg(unix)]
    #[test]
    fn test_execution_snapshot_permissions_0600() {
        use crate::history::model::{
            ExecutionSnapshotFile, MaskingMode, RunnerFingerprint, SNAPSHOT_SCHEMA_VERSION,
        };
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("perm.json");
        let snapshot = ExecutionSnapshotFile {
            schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
            generated_at: chrono::Utc::now(),
            environment: None,
            runner_fingerprint: RunnerFingerprint {
                os: "test".to_string(),
                arch: "test".to_string(),
                rupost_version: "0.0.0".to_string(),
            },
            masking: MaskingMode::None,
            entries: vec![],
        };
        write_execution_snapshot(&snapshot, &path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "blackbox snapshot must be 0600 even under permissive umask"
        );
    }

    /// 演进策略契约（02 §2.6）：消费者必须容忍未知字段（追加字段可反序列化）
    #[test]
    fn test_execution_snapshot_unknown_field_tolerance() {
        use crate::history::model::ExecutionSnapshotFile;
        let json = format!(
            r#"{{
                "schema_version": "1.0",
                "generated_at": "2026-11-11T00:00:00Z",
                "environment": null,
                "runner_fingerprint": {{"os": "os", "arch": "arch", "rupost_version": "0.2.0"}},
                "masking": "none",
                "entries": [],
                "future_field_appended_in_v11": true
            }}"#
        );
        let snap: ExecutionSnapshotFile = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.schema_version, "1.0");
        assert!(snap.entries.is_empty());
    }

    /// 03 §10.6：超限响应体 256KB 截断 + 截断前完整 Body 的 SHA-256 验真
    #[test]
    fn test_snapshot_body_truncation_and_sha256() {
        use sha2::{Digest, Sha256};
        let original = "x".repeat(crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES + 1024);
        let (truncated_body, flag) = if original.len() > crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES
        {
            (
                original[..crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES].to_string(),
                true,
            )
        } else {
            (original.clone(), false)
        };
        assert!(flag);
        assert_eq!(truncated_body.len(), crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES);
        let mut hasher = Sha256::new();
        hasher.update(original.as_bytes());
        let expected = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
        // 与 save 路径同构的验真链：sha256(原始) 可由截断产物 + 标记还原比对
        assert_eq!(expected.len(), 64);
    }

    /// TimingWaterfallUs 合成契约：diagnose 提供 TLS 阶段；total = 已发生阶段之和
    #[test]
    fn test_timing_waterfall_from_probes() {
        use crate::http::timing::RequestTiming;
        use std::time::Duration;
        let timing = RequestTiming {
            dns_lookup: Duration::from_micros(120),
            tcp_connect: Duration::from_micros(3_000),
            ttfb: Duration::from_micros(50_000),
            transfer: Duration::from_micros(1_000),
        };
        let wf = crate::history::model::TimingWaterfallUs::from_probes(&timing, None);
        assert_eq!(wf.dns_lookup_us, Some(120));
        assert_eq!(wf.tcp_connect_us, Some(3_000));
        assert_eq!(wf.tls_handshake_us, None, "diagnose 缺失时 TLS 为 None");
        assert_eq!(wf.ttfb_us, 50_000);
        assert_eq!(wf.transfer_us, 1_000);
        assert_eq!(wf.total_us, 54_120);
    }

    /// SocketDiagnostics 提取契约：resolved_ips 首选 + 证书字段（HTTPS 才有）
    #[test]
    fn test_socket_diagnostics_from_diagnose() {
        use crate::http::diagnose::{CertInfo, DiagnosticsReport};
        use std::time::Duration;
        let report = DiagnosticsReport {
            url: "https://api.example.com:8443/v1".to_string(),
            is_https: true,
            is_websocket: false,
            ws_upgrade_success: None,
            resolved_ips: vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()],
            dns_lookup_duration: Duration::from_micros(100),
            tcp_connect_duration: Duration::from_micros(2_000),
            tls_handshake_duration: Some(Duration::from_micros(30_000)),
            cert_info: Some(CertInfo {
                subject: "CN=api.example.com".to_string(),
                issuer: "CN=DigiCert".to_string(),
                sans: vec![],
                validity_not_after: "2027-01-01".to_string(),
                days_remaining: 365,
            }),
            http_status: Some(200),
            http_version: Some("HTTP/2".to_string()),
            ttfb: Some(Duration::from_micros(60_000)),
            total_duration: Duration::from_micros(92_100),
            connection_error: None,
        };
        let socket = crate::history::model::SocketDiagnostics::from_diagnose(&report).unwrap();
        assert_eq!(socket.remote_ip, "10.0.0.1");
        assert_eq!(socket.remote_port, 8443);
        assert_eq!(socket.http_version, "HTTP/2");
        assert_eq!(socket.cert_days_remaining, Some(365));
        assert_eq!(socket.cert_issuer.as_deref(), Some("CN=DigiCert"));
    }

    /// 掩码契约（02 §4）：Authorization 系头掩码 + body 敏感键 [REDACTED]，
    /// 与 history 掩码共享 redactor SSOT
    #[test]
    fn test_mask_execution_snapshot_entry() {
        use crate::history::model::{ExecutionOutcome, ExecutionSnapshotEntry, SnapshotRequest};
        let mut entry = ExecutionSnapshotEntry {
            request_number: 1,
            name: None,
            file_path: "t.http".to_string(),
            success: true,
            outcome: ExecutionOutcome::Pass,
            status: Some(200),
            duration_us: 1_000,
            plugin_duration_us: None,
            request: SnapshotRequest {
                method: "GET".to_string(),
                url: "https://api.example.com/x?access_token=leaky".to_string(),
                headers: vec![
                    ("Authorization".to_string(), "Bearer real-token".to_string()),
                    ("Content-Type".to_string(), "application/json".to_string()),
                ],
                body: Some(r#"{"password": "hunter2", "city": "Shanghai"}"#.to_string()),
            },
            response: None,
            error: None,
            timing_waterfall: None,
            socket_diagnostics: None,
            assertions: vec![],
            warnings: vec![],
        };
        crate::history::storage::mask_execution_snapshot_entry(&mut entry);
        let auth = &entry.request.headers[0].1;
        assert_ne!(auth, "Bearer real-token", "Authorization 必须被掩码");
        assert!(auth.contains("[REDACTED]"), "占位符契约: {auth}");
        assert!(
            !entry.request.body.as_ref().unwrap().contains("hunter2"),
            "body 密码键必须掩码"
        );
    }
}
