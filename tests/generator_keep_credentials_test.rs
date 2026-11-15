//! P1-4/P1-3（review 修复验证）：--keep-credentials 语义守护。
//!
//! 豁免面一致性：header 与 URL query 敏感键必须同等豁免（token 在 query 的
//! 请求同样免手工回填）；对照组：keep=false 维持凭据模板化契约。

use rupost::generator::HttpGenerator;
use rupost::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};

fn entry_with_creds(url: &str) -> HistoryEntry {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "authorization",
        reqwest::header::HeaderValue::from_static("Bearer sk-live-header-token"),
    );
    HistoryEntry {
        id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        duration_ms: 5,
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: url.to_string(),
            headers,
            body: None,
        },
        source: Some("test".to_string()),
        response: ResponseMeta {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: Some("{}".to_string()),
        },
    }
}

/// 豁免面（P1-3 修复）：keep_credentials=true 时 header 与 query 敏感键均保留字面值。
#[test]
fn test_keep_credentials_spares_header_and_query() {
    let entry = entry_with_creds("https://vault.example.com/x?access_token=sk-live-query-token");
    let (text, _) = HttpGenerator::generate_with_policy(
        &[entry],
        false,
        None,
        true, // keep_credentials
    )
    .unwrap();
    assert!(
        text.contains("sk-live-header-token"),
        "header 敏感键必须保留字面值: {text}"
    );
    assert!(
        text.contains("sk-live-query-token"),
        "query 敏感键必须同等豁免（P1-3）: {text}"
    );
    assert!(!text.contains("{{authorization}}"), "不得模板化");
}

/// 对照组（既有契约）：keep_credentials=false 维持凭据模板化（禁止明文落盘）。
#[test]
fn test_default_templating_still_redacts_credentials() {
    let entry = entry_with_creds("https://vault.example.com/x?access_token=sk-live-query-token");
    let (text, variables) =
        HttpGenerator::generate_with_policy(&[entry], false, None, false).unwrap();
    assert!(
        !text.contains("sk-live-header-token") && !text.contains("sk-live-query-token"),
        "默认语义严禁明文凭据落盘: {text}"
    );
    assert!(text.contains("{{"), "必须产出模板变量: {text}");
    assert!(!variables.is_empty());
}
