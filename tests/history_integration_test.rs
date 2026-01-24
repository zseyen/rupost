use rupost::parser::{ParsedFile, ParsedRequest, RequestMetadata};
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_history_recording() {
    // 1. Setup Mock Server
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/history-test"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
             "status": "created"
        })))
        .mount(&mock_server)
        .await;

    // 2. Setup isolated history storage
    let temp_dir = TempDir::new().unwrap();
    let history_dir_path = temp_dir.path().to_str().unwrap();
    // Use env var to redirect history storage (requires HistoryStorage modification)
    unsafe {
        std::env::set_var("RUPOST_HISTORY_DIR", history_dir_path);
    }

    // 3. Construct a Request
    let url = format!("{}/history-test", mock_server.uri());
    let parsed_request = ParsedRequest {
        url: url.clone(),
        method: Some("POST".to_string()),
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: Some(r#"{"test": "data"}"#.to_string()),
        metadata: RequestMetadata::default(),
        line_number: 1,
    };

    let parsed_file = ParsedFile {
        requests: vec![parsed_request],
        source_path: None, // Added field
    };
    let mut context = VariableContext::new();

    // 4. Execute
    let executor = TestExecutor::new();
    let results = executor
        .execute_all(parsed_file, &mut context)
        .await
        .unwrap();

    // Verify execution success
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    // TestResult stores response in Option<Response>, and Response has Status
    assert_eq!(results[0].response.as_ref().unwrap().status.code(), 201);

    // 5. Verify History File
    let history_file = temp_dir.path().join("history.jsonl");
    assert!(history_file.exists(), "History file should be created");

    let content = fs::read_to_string(&history_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 1, "Should have exactly 1 history entry");

    // 6. Verify Entry Content
    let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();

    // Check ID exists
    assert!(entry.get("id").is_some());
    // Check Request Snapshot
    assert_eq!(entry["request"]["method"], "POST");
    assert_eq!(entry["request"]["url"], url);
    assert_eq!(entry["request"]["body"], r#"{"test": "data"}"#);
    // Check Response Meta
    assert_eq!(entry["response"]["status"], 201);
}
