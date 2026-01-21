# Request History & Test Generation Architecture

## Goal
Implement a mechanism to persist request history and generate reusable `.http` test files from it. This allows users to "record" their manual explorations and convert them into automated regression tests.

## Architecture

### 1. History Data Model (`src/history/model.rs`)
Structure for a history entry:
```rust
#[derive(Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,          // UUID
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
    pub request: RequestSnapshot,
    pub response_status: u16,
    // Response body is OMITTED to save space.
    // For test generation, we primarily need the request definition.
}
```

### 2. History Storage Strategy

**Concern**: Single file growing too large and becoming slow to query.

**Data Size Estimation**:
-   Typical Entry: Method (4B) + URL (50B) + Headers (300B) + Request Body (1KB avg) + Meta (Time/Status/Duration ~50B).
-   **Total per Entry**: ~1.5 KB.
-   **Capacity**:
    -   1 MB ≈ 600 requests.
    -   50 MB ≈ 33,000 requests (More than enough for immediate history).

**Storage Design**:
-   **Format**: JSONL (JSON Lines) at `.rupost/history.jsonl` (Project-local).
-   **Constraint**: Max items (soft limit): 10,000.
-   **Compaction (Lazy Pruning)**:
    -   **Write (Append)**: Always fast append.
    -   **Read (List)**: If file size > 20MB, trigger a cleanup:
        -   Read all entries.
        -   Keep last 10,000.
        -   Rewrite file.

### 3. Record Hook (`src/runner/executor.rs`)
- `TestExecutor` appends to history after each request.
- **Optimization**: Do NOT store the full *Response* Body. Only store Request (test definition) and Response Metadata.

### 4. CLI Commands & UI

**`rupost history list [OPTIONS]`**
Displays a table of recent requests:

```text
ID       TIME      METHOD  URL                       STATUS  DURATION
a1b2c3d  10:23:45  POST    https://api.example.com   200     150ms
e4f5g6h  10:24:01  GET     https://api.example.com   200     45ms
...
```

**`rupost generate [OPTIONS] <OUTPUT_FILE>`**
- `--last N`: Generate from the last N requests.
- `--id <ID>`: Generate from specific request IDs.

### 5. Data Flow

1.  **Execution Phase**:
    *   `TestExecutor` sends HTTP request.
    *   On response, `TestExecutor` creates a `HistoryEntry` (Snapshot of Request + Response Meta).
    *   `HistoryEntry` is serialized to JSON and appended to `.rupost/history.jsonl`.

2.  **Generation Phase** (`rupost generate`):
    *   `HistoryStorage` reads `.rupost/history.jsonl`.
    *   Filters entries based on CLI args (e.g., `--last 5`).
    *   `HistoryToHttp` converter iterates over entries:
        *   Formats Method + URL.
        *   Formats Headers (excluding sensitive auto-headers like Content-Length).
        *   Formats Body (JSON pretty-print).
        *   Adds `@name` derived from URL path + Timestamp.
        *   Adds basic assertion: `# @assert status == {response_status}`.
    *   Writes all formatted requests to the target `.http` file.
