#![allow(clippy::collapsible_if)]
use crate::Result;
use crate::error::RupostError;
use crate::history::model::HistoryEntry;
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

    /// S-1（安全对齐，P0-1 review 修复）：history.jsonl 含完整请求/响应明文，权限
    /// 必须与快照防线对齐——首建走 `create_new + mode(0o600)`（mode 创建时原子生效，
    /// 无截断竞态、无 0644 窗口）；并发首建时败方走 AlreadyExists 分支收紧既有文件。
    fn ensure_private_file(&self) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&self.file_path)
            {
                Ok(_) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(RupostError::IoError(e)),
            }
            // 既有文件：收紧宽松权限（幂等；chmod 跟随 symlink 的 TOCTOU 由
            // append 前置的 regular-file 语义缓解——history 路径不经用户可控 symlink）
            let meta = std::fs::metadata(&self.file_path).map_err(RupostError::IoError)?;
            if meta.is_file() && meta.permissions().mode() & 0o777 & 0o077 != 0 {
                std::fs::set_permissions(&self.file_path, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        #[cfg(not(unix))]
        {
            // Windows 权限模型不同（ACL），维持现状；CHANGELOG 已登记平台边界
        }
        Ok(())
    }

    /// 确保父目录存在。
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
        self.ensure_private_file()?;
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

// 兼容重导出：黑匣子/重放快照 I/O 与脱敏已按职责拆至 snapshot_io / snapshot_legacy / snapshot_mask
pub use super::snapshot_io::{
    read_execution_snapshot, save_batch_snapshot, write_execution_snapshot,
};
pub use super::snapshot_legacy::{
    export_history_snapshot, read_snapshot_suite, read_snapshot_suite_compat, write_snapshot_suite,
};
pub use super::snapshot_mask::{mask_execution_snapshot_entry, mask_snapshot_entry};

#[cfg(test)]
mod privacy_tests {
    use super::*;

    /// S-1：history.jsonl 权限必须与快照防线对齐——新建即 0600，宽松旧文件收紧。
    #[cfg(unix)]
    #[test]
    fn test_history_file_permissions_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        // 1) 预置 0644 旧文件 → append 后必须收紧
        std::fs::write(&path, "{}\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let store = HistoryStorage::new_with_path(path.clone());
        store.append(&sample_entry()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "宽松权限必须被收紧");
        // 2) 新建路径 → append 创建即 0600
        let path2 = dir.path().join("history2.jsonl");
        let store2 = HistoryStorage::new_with_path(path2.clone());
        store2.append(&sample_entry()).unwrap();
        let mode2 = std::fs::metadata(&path2).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o600, "新建文件必须 0600");
    }

    fn sample_entry() -> HistoryEntry {
        HistoryEntry {
            id: "s1".to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 1,
            request: crate::history::model::RequestSnapshot {
                method: "GET".to_string(),
                url: "http://x".to_string(),
                headers: Default::default(),
                body: None,
                timeout: None,
            },
            source: Some("test".to_string()),
            response: crate::history::model::ResponseMeta {
                status: 200,
                headers: Default::default(),
                body: Some("{}".to_string()),
            },
        }
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
