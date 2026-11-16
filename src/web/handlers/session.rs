use crate::security::redact::redact_header_value;
use crate::web::server::AppState;
use crate::web::session::{cookie_header_of, session_set_cookie_value};
use crate::web::types::{ApiResponse, SessionStateResponse};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{HeaderMap, HeaderValue, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use std::collections::BTreeMap;

/// 查看当前浏览器会话（rupost_session）捕获的变量摘要（敏感键自动脱敏）与 Cookie 存在状态
pub async fn get_session_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let cookie_header = cookie_header_of(&headers);
    let handle = state.sessions.load_or_create(cookie_header);

    let captured_variables: BTreeMap<String, String> = handle
        .snapshot
        .captured_variables
        .into_iter()
        .map(|(k, v)| {
            let masked = redact_header_value(&k, &v);
            (k, masked)
        })
        .collect();
    let has_cookie_state = handle.snapshot.cookie_state.is_some();

    let mut resp = (
        StatusCode::OK,
        Json(ApiResponse::ok(SessionStateResponse {
            captured_variables,
            has_cookie_state,
        })),
    )
        .into_response();

    if handle.created
        && let Ok(val) = HeaderValue::from_str(&session_set_cookie_value(&handle.id))
    {
        resp.headers_mut().insert(SET_COOKIE, val);
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::assets::EmbeddedAssetProvider;
    use crate::web::session::WebSessionStore;
    use crate::web::store::LocalFsDocumentStore;
    use axum::body::to_bytes;
    use axum::http::header::COOKIE;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_session_handler_masks_sensitive_captured_variables() {
        let state = AppState {
            store: Arc::new(LocalFsDocumentStore::new(".")),
            assets: Arc::new(EmbeddedAssetProvider),
            readonly: false,
            working_dir: PathBuf::from("."),
            default_env: None,
            allowed_hosts: Vec::new(),
            sessions: Arc::new(WebSessionStore::default()),
            incident_root: PathBuf::from(".rupost/incidents"),
        };

        let handle = state.sessions.load_or_create(None);
        let mut vars = HashMap::new();
        vars.insert("user_id".to_string(), "42".to_string());
        vars.insert("access_token".to_string(), "secret-jwt-token".to_string());
        state.sessions.persist_execution(&handle.id, None, vars);

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("rupost_session={}", handle.id)).unwrap(),
        );

        let resp = get_session_handler(State(state), headers).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let api_res: ApiResponse<SessionStateResponse> = serde_json::from_slice(&body).unwrap();
        let data = api_res.data.unwrap();
        assert_eq!(data.captured_variables.get("user_id").map(String::as_str), Some("42"));
        assert_ne!(
            data.captured_variables.get("access_token").map(String::as_str),
            Some("secret-jwt-token")
        );
    }
}
