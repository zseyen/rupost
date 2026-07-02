use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::history::serialization;

/// 历史记录条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 唯一 ID (UUID)
    pub id: String,

    /// 请求时间
    pub timestamp: DateTime<Utc>,

    /// 请求耗时 (毫秒)
    pub duration_ms: u64,

    /// 请求快照
    pub request: RequestSnapshot,

    /// 请求来源 (例如 "cli", "file:test.http")
    pub source: Option<String>,

    /// 响应元数据
    pub response: ResponseMeta,
}

/// 请求快照 (用于测试生成)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestSnapshot {
    pub method: String,
    pub url: String,

    #[serde(with = "serialization::header_map")]
    pub headers: HeaderMap,

    pub body: Option<String>,
}

/// 响应元数据 (TUI 模式下可选包含 Body，节省空间且向前兼容)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub status: u16,

    #[serde(with = "serialization::header_map")]
    pub headers: HeaderMap,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// 单个 HTTP 交互快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// 交互唯一 ID
    pub id: String,
    /// 关联的请求快照
    pub request: RequestSnapshot,
    /// 关联的响应快照 (包含 Body)
    pub response: ResponseSnapshot,
}

/// 快照套件结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSuite {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub source_file: Option<String>,
    pub entries: Vec<SnapshotEntry>,
}

/// 响应快照 (带 Body)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseSnapshot {
    pub status: u16,

    #[serde(with = "serialization::header_map")]
    pub headers: reqwest::header::HeaderMap,

    pub body: String,
}

impl From<HistoryEntry> for SnapshotEntry {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            request: entry.request,
            response: ResponseSnapshot {
                status: entry.response.status,
                headers: entry.response.headers,
                body: entry.response.body.unwrap_or_else(|| "Old snapshot: body not recorded".to_string()),
            },
        }
    }
}

impl RequestSnapshot {
    /// 从解析实例化后的 `ParsedRequest` 构建请求历史记录快照
    pub fn from_parsed(parsed: &crate::parser::ParsedRequest) -> Self {
        use reqwest::header::{HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();
        for (k, v) in &parsed.headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(n, v);
            }
        }

        Self {
            method: parsed.method_or_default().to_string(),
            url: parsed.url.clone(),
            headers,
            body: parsed.body.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedRequest;

    #[test]
    fn test_request_snapshot_from_parsed() {
        let mut parsed = ParsedRequest::new(1);
        parsed.method = Some("POST".to_string());
        parsed.url = "http://example.com".to_string();
        parsed.headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        parsed.body = Some("{}".to_string());

        let snapshot = RequestSnapshot::from_parsed(&parsed);
        assert_eq!(snapshot.method, "POST");
        assert_eq!(snapshot.url, "http://example.com");
        assert_eq!(
            snapshot
                .headers
                .get("Content-Type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
        assert_eq!(snapshot.body, Some("{}".to_string()));
    }
}
