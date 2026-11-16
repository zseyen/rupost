use crate::history::mask_snapshot_entry;
use crate::history::model::{HistoryEntry, SnapshotEntry};
use crate::history::storage::HistoryStorage;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, HistoryDetailDto, HistoryItemSummary, HttpHeaderDto};
use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
}

/// 判断历史记录条目是否归属于当前工作区（防跨项目历史泄露）
pub fn entry_belongs_to_workspace(entry: &HistoryEntry, working_dir: &Path) -> bool {
    let Some(raw_src) = entry.source.as_deref() else {
        return false;
    };
    let path_str = raw_src.strip_prefix("file:").unwrap_or(raw_src).trim();
    if path_str.is_empty() || path_str.eq_ignore_ascii_case("cli") {
        return false;
    }

    let candidate = Path::new(path_str);
    if candidate.is_absolute() {
        let canonical_root =
            std::fs::canonicalize(working_dir).unwrap_or_else(|_| working_dir.to_path_buf());
        let canonical_cand =
            std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
        canonical_cand.starts_with(&canonical_root)
    } else {
        // 相对路径：拒绝包含 `..` 穿越，且必须在工作区内真实存在
        if path_str.split(['/', '\\']).any(|seg| seg == "..") {
            return false;
        }
        working_dir.join(candidate).is_file()
    }
}

/// 过滤并汇总归属于当前工作区的历史记录
pub fn filter_history_for_workspace(
    entries: Vec<HistoryEntry>,
    working_dir: &Path,
    limit: usize,
) -> Vec<HistoryItemSummary> {
    entries
        .into_iter()
        .rev()
        .filter(|e| entry_belongs_to_workspace(e, working_dir))
        .take(limit)
        .map(|e| HistoryItemSummary {
            id: e.id,
            timestamp: e.timestamp.to_rfc3339(),
            method: e.request.method,
            url: e.request.url,
            status: e.response.status,
            duration_ms: e.duration_ms,
            source: e.source,
        })
        .collect()
}

/// 获取当前工作区隔离后的历史记录（只读模式下拒绝访问宿主机个人历史）
pub async fn list_history_handler(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "History access is disabled in read-only mode",
            )),
        );
    }

    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let storage = HistoryStorage::new();
    match storage.tail(limit.saturating_mul(5).max(100)) {
        Ok(entries) => {
            let items = filter_history_for_workspace(entries, &state.working_dir, limit);
            (StatusCode::OK, Json(ApiResponse::ok(items)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(format!("Failed to load history: {}", e))),
        ),
    }
}

fn header_map_to_dtos(map: &HeaderMap) -> Vec<HttpHeaderDto> {
    map.iter()
        .map(|(k, v)| HttpHeaderDto {
            key: k.to_string(),
            value: String::from_utf8_lossy(v.as_bytes()).into_owned(),
        })
        .collect()
}

/// 在工作区隔离范围内查找单条历史记录并执行强制脱敏投影
pub fn find_history_detail_for_workspace(
    entries: Vec<HistoryEntry>,
    working_dir: &Path,
    id: &str,
) -> Option<HistoryDetailDto> {
    let entry = entries
        .into_iter()
        .rev()
        .find(|e| e.id == id && entry_belongs_to_workspace(e, working_dir))?;

    let timestamp = entry.timestamp.to_rfc3339();
    let duration_ms = entry.duration_ms;
    let source = entry.source.clone();
    let mut snap = SnapshotEntry::from(entry);
    mask_snapshot_entry(&mut snap);

    let (status, response_headers, response_body) = match snap.response {
        Some(resp) => (
            Some(resp.status),
            header_map_to_dtos(&resp.headers),
            Some(resp.body),
        ),
        None => (None, Vec::new(), None),
    };

    Some(HistoryDetailDto {
        id: snap.id,
        timestamp,
        duration_ms,
        source,
        method: snap.request.method,
        url: snap.request.url,
        request_headers: header_map_to_dtos(&snap.request.headers),
        request_body: snap.request.body,
        status,
        response_headers,
        response_body,
    })
}

/// 获取当前工作区隔离后的单条历史记录脱敏详情（只读模式禁止）
pub async fn get_history_detail_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "History access is disabled in read-only mode",
            )),
        );
    }

    let storage = HistoryStorage::new();
    match storage.tail(500) {
        Ok(entries) => match find_history_detail_for_workspace(entries, &state.working_dir, &id) {
            Some(detail) => (StatusCode::OK, Json(ApiResponse::ok(detail))),
            None => (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::err(format!("History entry not found: {id}"))),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(format!("Failed to load history: {}", e))),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use chrono::Utc;
    use reqwest::header::HeaderMap;
    use tempfile::TempDir;

    fn make_entry(id: &str, source: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            timestamp: Utc::now(),
            duration_ms: 15,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: "https://example.com".to_string(),
                headers: HeaderMap::new(),
                body: None,
                timeout: None,
            },
            source: source.map(str::to_string),
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
                body: None,
            },
        }
    }

    #[test]
    fn test_history_workspace_isolation_filters_foreign_entries() {
        let ws = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        std::fs::write(ws.path().join("api.http"), "GET https://example.com\n").unwrap();
        let other_file = other.path().join("secret.http");
        std::fs::write(&other_file, "GET https://internal.corp\n").unwrap();

        let entries = vec![
            make_entry("1", Some("cli")),
            make_entry("2", None),
            make_entry("3", Some(&format!("file:{}", other_file.display()))),
            make_entry("4", Some("file:../escape.http")),
            make_entry("5", Some("file:api.http")),
        ];

        let filtered = filter_history_for_workspace(entries, ws.path(), 20);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "5");
    }

    #[test]
    fn test_find_history_detail_masks_secrets_and_isolates_workspace() {
        let ws = TempDir::new().unwrap();
        std::fs::write(ws.path().join("api.http"), "GET https://example.com\n").unwrap();

        let mut own = make_entry("own-1", Some("file:api.http"));
        own.request.headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer super-secret-token"),
        );
        let foreign = make_entry("foreign-1", Some("cli"));
        let entries = vec![own, foreign];

        assert!(find_history_detail_for_workspace(entries.clone(), ws.path(), "foreign-1").is_none());
        let detail =
            find_history_detail_for_workspace(entries, ws.path(), "own-1").expect("detail");
        let auth = detail
            .request_headers
            .iter()
            .find(|h| h.key.eq_ignore_ascii_case("authorization"))
            .expect("auth header");
        assert!(!auth.value.contains("super-secret-token"));
    }
}
