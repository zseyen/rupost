use super::model::HistoryEntry;
use crate::Result;
use crate::error::RupostError;
use fs2::FileExt;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    /// Uses `fs2::lock_exclusive` to ensure atomic writes across multiple processes.
    /// This is safer than relying on `O_APPEND` across different OSs (e.g. Windows).
    ///
    /// # Performance
    /// The lock is held only for the duration of the write (microseconds).
    pub fn append(&self, entry: &HistoryEntry) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string(entry)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .map_err(RupostError::IoError)?;

        // Lock for writing
        file.lock_exclusive().map_err(RupostError::IoError)?;

        writeln!(file, "{}", json).map_err(RupostError::IoError)?;

        // Unlock happens automatically when file is dropped, but excessive scoping is good habit
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
        // we can take shared lock. `fs2` supports shared lock.
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
        file.lock_exclusive().map_err(RupostError::IoError)?;

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
                method: "GET".to_string(),
                url: "https://example.com".to_string(),
                headers: HeaderMap::new(),
                body: None,
            },
            source: None,
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
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
}
