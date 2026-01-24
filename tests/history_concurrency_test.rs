use chrono::Utc;
use reqwest::header::HeaderMap;
use rupost::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use rupost::history::storage::HistoryStorage;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

fn create_dummy_entry(id: String) -> HistoryEntry {
    HistoryEntry {
        id,
        timestamp: Utc::now(),
        duration_ms: 100,
        request: RequestSnapshot {
            method: "GET".to_string(),
            url: "https://example.com".to_string(),
            headers: HeaderMap::new(),
            body: None,
        },
        response: ResponseMeta {
            status: 200,
            headers: HeaderMap::new(),
        },
    }
}

#[test]
fn test_concurrent_writes() {
    let temp_dir = TempDir::new().unwrap();
    let history_file = temp_dir.path().join("history.jsonl");

    // Use Arc to share path among threads (though each creates its own Storage instance to simulate processes)
    let history_path = Arc::new(history_file.clone());

    let mut handles = vec![];
    let thread_count = 10;
    let entries_per_thread = 50;

    for i in 0..thread_count {
        let path = history_path.clone();
        handles.push(thread::spawn(move || {
            let storage = HistoryStorage::new_with_path((*path).clone());
            for j in 0..entries_per_thread {
                let id = format!("{}-{}", i, j);
                let entry = create_dummy_entry(id);
                // Introduce some randomness/delay to encourage overlap?
                // Actually tight loop is better to force race conditions
                storage.append(&entry).unwrap();
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify
    let storage = HistoryStorage::new_with_path(history_file);
    let entries = storage.list().unwrap();

    // Check total count
    assert_eq!(
        entries.len(),
        thread_count * entries_per_thread,
        "Total entries count mismatch"
    );

    // Check data integrity (basic)
    // Could check if all IDs are present, but count and JSON validity (implied by list() success) is good enough start
}
