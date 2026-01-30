use crate::Result;
use crate::history::model::HistoryEntry;

pub struct HttpGenerator;

impl HttpGenerator {
    /// Convert a list of history entries to .http file content
    pub fn generate(entries: &[HistoryEntry]) -> Result<String> {
        let mut output = String::new();

        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }
            output.push_str(&Self::format_entry(entry));
        }

        if !entries.is_empty() {
            output.push('\n');
        }

        Ok(output)
    }

    fn format_entry(entry: &HistoryEntry) -> String {
        let mut block = String::new();

        // 1. Comment / Name
        // Generate a name like: "GET /api/users - 10:23:45"

        block.push_str(&format!(
            "### Request {}\n",
            entry.id.chars().take(8).collect::<String>()
        ));
        block.push_str(&format!(
            "# @name req_{}_{}\n",
            entry.timestamp.timestamp(),
            i8::abs(entry.id.as_bytes()[0] as i8)
        ));

        // 2. Request Line
        block.push_str(&format!("{} {}\n", entry.request.method, entry.request.url));

        // 3. Headers
        for (key, value) in &entry.request.headers {
            let key_str = key.as_str();
            // Skip common auto-headers that shouldn't be hardcoded in tests
            if Self::should_skip_header(key_str) {
                continue;
            }
            if let Ok(val_str) = value.to_str() {
                block.push_str(&format!("{}: {}\n", key_str, val_str));
            }
        }

        // 4. Body
        if let Some(body) = entry.request.body.as_ref().filter(|b| !b.trim().is_empty()) {
            block.push('\n');
            // Try to pretty print JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                    block.push_str(&pretty);
                } else {
                    block.push_str(body);
                }
            } else {
                block.push_str(body);
            }
            block.push('\n');
        }

        // 5. Assertions (Metadata)
        // Add implicit assertion for status code
        block.push_str(&format!(
            "\n# @assert status == {}\n",
            entry.response.status
        ));

        block
    }

    fn should_skip_header(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        matches!(
            name_lower.as_str(),
            "content-length" | "host" | "connection" | "accept-encoding" | "user-agent"
        )
    }
}
