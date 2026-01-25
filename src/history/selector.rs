// ... (imports)
use crate::Result;
use crate::RupostError;
use crate::history::model::HistoryEntry;
use crate::history::storage::HistoryStorage;
use inquire::MultiSelect;

/// Selection strategy for history entries
pub enum SelectionStrategy {
    Interactive,
    Last(usize),
}

/// Select history entries based on the given strategy
pub fn select_entries(
    storage: &HistoryStorage,
    strategy: SelectionStrategy,
) -> Result<Vec<HistoryEntry>> {
    match strategy {
        SelectionStrategy::Interactive => select_interactive(storage),
        SelectionStrategy::Last(n) => storage.tail(n),
    }
}

/// Interactively select history entries using a TUI
fn select_interactive(storage: &HistoryStorage) -> Result<Vec<HistoryEntry>> {
    // 1. Fetch recent history
    let mut entries = storage.tail(50)?;
    entries.reverse();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    // 2. Format options using helper
    let options: Vec<String> = entries.iter().map(format_entry_for_display).collect();

    // 3. Prompt user (using wrapped struct for indices)
    #[derive(Clone)]
    struct EntryWrapper {
        index: usize,
        display: String,
    }

    impl std::fmt::Display for EntryWrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.display)
        }
    }

    let wrapped_options: Vec<EntryWrapper> = options
        .into_iter()
        .enumerate()
        .map(|(i, s)| EntryWrapper {
            index: i,
            display: s,
        })
        .collect();

    let selected_wrappers = MultiSelect::new("Select requests to generate:", wrapped_options)
        .with_page_size(15)
        .with_help_message("Space to select, Enter to finish, type to filter")
        .prompt()
        .map_err(|e| RupostError::Other(format!("Interaction canceled or failed: {}", e)))?;

    // 4. Map back to entries
    let selected_entries: Vec<HistoryEntry> = selected_wrappers
        .into_iter()
        .map(|w| entries[w.index].clone())
        .collect();

    Ok(selected_entries)
}

fn format_entry_for_display(e: &HistoryEntry) -> String {
    let time = e.timestamp.format("%Y-%m-%d %H:%M:%S");
    let method = &e.request.method;
    let url = &e.request.url;
    let status = e.response.status;
    format!("[{status}] {method} {url} ({time})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use reqwest::header::HeaderMap;
    use tempfile::TempDir;

    fn create_dummy_entry(id: &str, url: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 100,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: url.to_string(),
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
    fn test_format_display() {
        let entry = create_dummy_entry("1", "https://example.com");
        let display = format_entry_for_display(&entry);
        // "[200] GET https://example.com (2024-...)"
        assert!(display.contains("[200] GET https://example.com"));
    }

    #[test]
    fn test_select_last() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("history.jsonl");
        let storage = HistoryStorage::new_with_path(file_path);

        storage.append(&create_dummy_entry("1", "u1")).unwrap();
        storage.append(&create_dummy_entry("2", "u2")).unwrap();
        storage.append(&create_dummy_entry("3", "u3")).unwrap();

        let selected = select_entries(&storage, SelectionStrategy::Last(2)).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, "2");
        assert_eq!(selected[1].id, "3");
    }
}
