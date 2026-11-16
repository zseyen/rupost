#![allow(clippy::collapsible_if)]
use crate::Result;
use crate::diagnostics::BufferedWarningSink;
use crate::error::RupostError;
use crate::history::model::HistoryEntry;
use crate::platform;
use crate::variable::WorkspaceConfig;
use std::env;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

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
        if let Some(env_dir) = env::var_os("RUPOST_HISTORY_DIR").filter(|v| !v.is_empty()) {
            return Self {
                file_path: PathBuf::from(env_dir).join(HISTORY_FILE),
            };
        }
        if let Ok(cwd) = env::current_dir()
            && cwd.join("rupost.toml").exists()
        {
            let ws = WorkspaceConfig::load(&cwd, &BufferedWarningSink::new());
            return Self {
                file_path: ws.paths.history_file,
            };
        }
        Self {
            file_path: platform::history_dir().join(HISTORY_FILE),
        }
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
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                Err(e) => return Err(RupostError::IoError(e)),
            }
            // 既有文件：收紧宽松权限（幂等；chmod 跟随 symlink 的 TOCTOU 由
            // append 前置的 regular-file 语义缓解——history 路径不经用户可控 symlink）
            let meta = fs::metadata(&self.file_path).map_err(RupostError::IoError)?;
            if meta.is_file() && meta.permissions().mode() & 0o777 & 0o077 != 0 {
                fs::set_permissions(&self.file_path, Permissions::from_mode(0o600))?;
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
    /// Uses reverse `Seek` from end of file in chunks so `tail(N)` is O(N) instead of
    /// reading the full 20MB file, while still preserving the compaction threshold check.
    pub fn tail(&self, n: usize) -> Result<Vec<HistoryEntry>> {
        if n == 0 || !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let file_len = fs::metadata(&self.file_path)
            .map_err(RupostError::IoError)?
            .len();
        if file_len >= COMPACTION_THRESHOLD_BYTES {
            self.compact_if_needed()?;
        }

        let mut file = fs::File::open(&self.file_path).map_err(RupostError::IoError)?;
        file.lock_shared().map_err(RupostError::IoError)?;
        Self::read_tail_with_chunk_size(&mut file, n, 8192)
    }

    fn read_tail_with_chunk_size(
        file: &mut fs::File,
        n: usize,
        chunk_size: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let mut pos = file.seek(SeekFrom::End(0)).map_err(RupostError::IoError)?;
        // 预分配上限与用户可控 n 解耦（审查 P1-5：`--limit usize::MAX` 此前
        // capacity overflow abort；实际收集量仍由 n 与文件行数双重约束，
        // 预分配只是优化手段，cap 不改变语义）
        let mut collected = Vec::with_capacity(n.min(4096));
        // 逆向暂存未遇到 '\n' 的尾部块（由后向前存放），避免单条大行反复 O(K^2) 拷贝
        let mut pending_tail_chunks: Vec<Vec<u8>> = Vec::new();
        let chunk_size = chunk_size.max(1);
        let mut buf = vec![0u8; chunk_size];

        while pos > 0 && collected.len() < n {
            let read_len = usize::try_from(pos).unwrap_or(chunk_size).min(chunk_size);
            pos -= read_len as u64;
            file.seek(SeekFrom::Start(pos)).map_err(RupostError::IoError)?;
            file.read_exact(&mut buf[..read_len])
                .map_err(RupostError::IoError)?;

            let chunk = &buf[..read_len];
            let mut end = chunk.len();
            while let Some(nl_idx) = chunk[..end].iter().rposition(|&b| b == b'\n') {
                let head = &chunk[nl_idx + 1..end];
                if pending_tail_chunks.is_empty() {
                    Self::try_push_json_line(head, &mut collected);
                } else {
                    let assembled = Self::assemble_line(head, &mut pending_tail_chunks);
                    Self::try_push_json_line(&assembled, &mut collected);
                }
                end = nl_idx;
                if collected.len() >= n {
                    break;
                }
            }

            if collected.len() < n && end > 0 {
                pending_tail_chunks.push(chunk[..end].to_vec());
            }
        }

        if collected.len() < n && !pending_tail_chunks.is_empty() {
            let assembled = Self::assemble_line(&[], &mut pending_tail_chunks);
            Self::try_push_json_line(&assembled, &mut collected);
        }

        collected.reverse();
        Ok(collected)
    }

    fn assemble_line(head: &[u8], reversed_tail_chunks: &mut Vec<Vec<u8>>) -> Vec<u8> {
        let total = head.len() + reversed_tail_chunks.iter().map(Vec::len).sum::<usize>();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(head);
        for chunk in reversed_tail_chunks.iter().rev() {
            out.extend_from_slice(chunk);
        }
        reversed_tail_chunks.clear();
        out
    }

    fn try_push_json_line(line: &[u8], out: &mut Vec<HistoryEntry>) {
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            return;
        }
        if let Ok(entry) = serde_json::from_slice::<HistoryEntry>(line) {
            out.push(entry);
        }
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
        let mut file = file; // make mutable
        file.seek(SeekFrom::Start(0))
            .map_err(RupostError::IoError)?;

        // Use BufWriter for performance
        let mut writer = BufWriter::new(file);

        for entry in to_keep {
            let json = serde_json::to_string(entry)?;
            writeln!(writer, "{}", json).map_err(RupostError::IoError)?;
        }
        writer.flush().map_err(RupostError::IoError)?;

        Ok(())
    }
}

// 兼容重导出：黑匣子/重放快照 I/O 与脱敏已按职责拆至 snapshot_io / snapshot_legacy / snapshot_mask
pub use super::snapshot_io::{
    read_execution_snapshot, save_batch_snapshot, write_execution_snapshot,
};
pub use super::snapshot_legacy::{
    export_history_snapshot, read_snapshot_suite, read_snapshot_suite_compat, write_snapshot_suite,
};
pub use super::snapshot_mask::{mask_execution_snapshot_entry, mask_snapshot_entry};

#[cfg(all(test, unix))]
mod privacy_tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use chrono::Utc;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    /// S-1：history.jsonl 权限必须与快照防线对齐——新建即 0600，宽松旧文件收紧。
    #[test]
    fn test_history_file_permissions_tightened() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        // 1) 预置 0644 旧文件 → append 后必须收紧
        fs::write(&path, "{}\n").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        let store = HistoryStorage::new_with_path(path.clone());
        store.append(&sample_entry()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "宽松权限必须被收紧");
        // 2) 新建路径 → append 创建即 0600
        let path2 = dir.path().join("history2.jsonl");
        let store2 = HistoryStorage::new_with_path(path2.clone());
        store2.append(&sample_entry()).unwrap();
        let mode2 = fs::metadata(&path2).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o600, "新建文件必须 0600");
    }

    fn sample_entry() -> HistoryEntry {
        HistoryEntry {
            id: "s1".to_string(),
            timestamp: Utc::now(),
            duration_ms: 1,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: "http://x".to_string(),
                headers: Default::default(),
                body: None,
                timeout: None,
            },
            source: Some("test".to_string()),
            response: ResponseMeta {
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

    #[test]
    fn test_tail_reverse_seek_handles_utf8_large_lines_and_corrupt_entries() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("history_tail.jsonl");
        let storage = HistoryStorage::new_with_path(file_path.clone());

        assert!(storage.tail(0).unwrap().is_empty());

        let mut e1 = create_dummy_entry("id-1");
        e1.response.body = Some("第一条历史".to_string());
        let mut e2 = create_dummy_entry("id-2");
        // 构造超过 8KB 且含多字节中文的响应体，验证跨块逆向读取与 UTF-8 完整性
        e2.response.body = Some("中文跨块测试-".repeat(1200));
        let mut e3 = create_dummy_entry("id-3");
        e3.response.body = Some("第三条".to_string());

        storage.append(&e1).unwrap();
        // 手动注入损坏行与空行
        {
            let mut f = OpenOptions::new().append(true).open(&file_path).unwrap();
            writeln!(f, "{{corrupt-json-line").unwrap();
            writeln!(f).unwrap();
        }
        storage.append(&e2).unwrap();
        storage.append(&e3).unwrap();

        // 小块尺寸强制触发多次跨块逆向读取
        let mut file = fs::File::open(&file_path).unwrap();
        let last2 = HistoryStorage::read_tail_with_chunk_size(&mut file, 2, 64).unwrap();
        assert_eq!(last2.len(), 2);
        assert_eq!(last2[0].id, "id-2");
        assert_eq!(last2[1].id, "id-3");
        assert_eq!(last2[0].response.body, e2.response.body);

        // 正常入口 tail(3) 跳过损坏行与空行，返回全部 3 条有效条目（由旧到新）
        let all3 = storage.tail(3).unwrap();
        assert_eq!(all3.len(), 3);
        assert_eq!(all3[0].id, "id-1");
        assert_eq!(all3[1].id, "id-2");
        assert_eq!(all3[2].id, "id-3");
    }
}
