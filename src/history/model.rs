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

/// 响应元数据 (不包含 Body，节省空间)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub status: u16,

    #[serde(with = "serialization::header_map")]
    pub headers: HeaderMap,
}
