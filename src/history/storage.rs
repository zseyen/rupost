#![allow(clippy::collapsible_if)]
use super::model::HistoryEntry;
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

use super::model::SnapshotSuite;

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

/// T5（随 R6 搭车）：将最近 N 条历史条目导出为可重放快照套件文件。
/// - `--last N` 语义 = 最近 N 条**条目**（`SelectionStrategy::Last` → `tail(n)`，追加序即重放序）
/// - `body=None` 旧条目经 `From<HistoryEntry>` 占位导出（"Old snapshot: body not recorded"）
/// - 空历史：warn 后不写文件、exit 0（对齐 generate 先例）
/// - 输出路径静默覆盖（`write_snapshot_suite` 无 `--force` 语义）；凭据不脱敏（与 `--save-snapshot` 同暴露面）
pub fn export_history_snapshot(last: usize, output: &str, mask_secrets: bool) -> Result<()> {
    use super::model::{SnapshotEntry, SnapshotSuite};
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
pub fn save_batch_snapshot<P: AsRef<Path>>(
    batch_results: &[(PathBuf, Vec<crate::runner::types::TestResult>)],
    source_paths: &[String],
    snapshot_path: P,
    mask_secrets: bool,
) -> Result<()> {
    use super::model::{ExecutionErrorDetail, ResponseSnapshot, SnapshotEntry, SnapshotSuite};
    let mut entries = Vec::new();
    for (_, file_results) in batch_results {
        for r in file_results {
            // P1-11：@teardown 清理请求默认排除出快照（防 replay 重放 DELETE）
            if r.is_teardown {
                continue;
            }
            if let Some(req) = &r.request {
                let response = r.response.as_ref().map(|resp| ResponseSnapshot {
                    status: resp.status.code(),
                    headers: resp.headers.clone(),
                    body: resp.body.clone(),
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

                let mut entry = SnapshotEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    request: req.clone(),
                    response,
                    error,
                };
                if mask_secrets {
                    mask_snapshot_entry(&mut entry);
                }
                entries.push(entry);
            }
        }
    }
    let normalized_source = source_paths
        .iter()
        .map(crate::utils::path_guard::normalize_posix_path)
        .collect::<Vec<_>>()
        .join(", ");
    let suite = SnapshotSuite {
        version: 2,
        timestamp: chrono::Utc::now(),
        source_file: Some(normalized_source),
        entries,
    };
    write_snapshot_suite(&suite, snapshot_path)
}

/// Secrets Masker（路线图条目 1 残余）：opt-in 快照脱敏——headers 走
/// `redact_header_value`（scheme 前缀保留），body 走 `mask_body_secrets`
/// （敏感键 `[REDACTED]` + PII 三正则，无截断），error 走 `redact_free_text` 擦除 URL 凭证。
pub fn mask_snapshot_entry(entry: &mut super::model::SnapshotEntry) {
    use crate::exporter::redact::{mask_body_secrets, redact_free_text, redact_header_value};
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
        err.message = redact_free_text(&err.message);
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
        )
        .unwrap();

        // 验证读取快照
        let suite = read_snapshot_suite(&snapshot_file).unwrap();
        assert_eq!(suite.entries.len(), 1);
        assert_eq!(suite.entries[0].request.method, "POST");
        let resp = suite.entries[0].response.as_ref().unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body, "{\"status\":\"ok\"}");
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
}
