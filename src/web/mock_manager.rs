use crate::error::RupostError;
use crate::mock::server::axum_adapter::serve_matcher_with_shutdown;
use crate::mock::server::build_mock_matcher;
use crate::security::PathSandbox;
use crate::utils::path_guard::{
    LIVE_DOC_EXTENSIONS, is_http_doc, is_live_doc, normalize_posix_path,
};
use crate::web::http_parse::parse_http_document_to_cards;
use crate::web::markdown::render_markdown_to_html;
use crate::web::types::{MockRouteItemDto, MockServerInfo};
use axum::http::StatusCode;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

const MAX_MANAGED_MOCK_SERVERS: usize = 8;
const MAX_SCAN_DEPTH: usize = 6;
const SKIP_SCAN_DIRS: &[&str] = &["target", "node_modules", "benches"];

struct RunningMockEntry {
    info: MockServerInfo,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

/// Web 托管 Mock 服务生命周期管理器（支持多实例启停与端口/沙箱安全约束）
#[derive(Default)]
pub struct WebMockManager {
    servers: Mutex<HashMap<String, RunningMockEntry>>,
}

impl Drop for WebMockManager {
    fn drop(&mut self) {
        let mut guard = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        for (_, mut entry) in guard.drain() {
            if let Some(tx) = entry.shutdown_tx.take() {
                let _ = tx.send(());
            }
            entry.task.abort();
        }
    }
}

#[derive(Debug)]
pub struct MockManagerError {
    pub status: StatusCode,
    pub message: String,
}

impl MockManagerError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl WebMockManager {
    pub fn list_servers(&self) -> Vec<MockServerInfo> {
        let guard = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<MockServerInfo> = guard.values().map(|e| e.info.clone()).collect();
        list.sort_by_key(|s| s.port);
        list
    }

    pub async fn start_server(
        &self,
        working_dir: &Path,
        file_path: &str,
        requested_port: Option<u16>,
        enable_cors: bool,
    ) -> Result<MockServerInfo, MockManagerError> {
        let sandbox = PathSandbox::new(working_dir)
            .with_extensions(LIVE_DOC_EXTENSIONS)
            .allow_hidden(false);
        let safe_path = sandbox.resolve_safe_path(file_path).map_err(|e| {
            let status = if matches!(e, RupostError::SecurityViolation(_)) {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            };
            MockManagerError::new(status, e.to_string())
        })?;

        if !safe_path.is_file() {
            return Err(MockManagerError::new(
                StatusCode::NOT_FOUND,
                format!("Mock document not found: {file_path}"),
            ));
        }

        let bind_port = requested_port.unwrap_or(0);
        if bind_port != 0 && bind_port < 1024 {
            return Err(MockManagerError::new(
                StatusCode::BAD_REQUEST,
                "Port must be 0 (auto) or >= 1024",
            ));
        }

        {
            let guard = self.servers.lock().unwrap_or_else(|e| e.into_inner());
            if guard.len() >= MAX_MANAGED_MOCK_SERVERS {
                return Err(MockManagerError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Maximum {MAX_MANAGED_MOCK_SERVERS} running mock servers reached"),
                ));
            }
            if bind_port != 0 && guard.values().any(|e| e.info.port == bind_port) {
                return Err(MockManagerError::new(
                    StatusCode::CONFLICT,
                    format!("Mock server already running on port {bind_port}"),
                ));
            }
        }

        let safe_path_str = safe_path.to_string_lossy().to_string();
        let (matcher, route_count) = build_mock_matcher(&safe_path_str).map_err(|e| {
            MockManagerError::new(StatusCode::BAD_REQUEST, format!("Failed to compile mock: {e}"))
        })?;
        if route_count == 0 {
            return Err(MockManagerError::new(
                StatusCode::BAD_REQUEST,
                format!("No @mock routes found in {file_path}"),
            ));
        }

        let addr = format!("127.0.0.1:{bind_port}");
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            MockManagerError::new(
                StatusCode::CONFLICT,
                format!("Failed to bind {addr}: {e}"),
            )
        })?;
        let actual_port = listener
            .local_addr()
            .map_err(|e| MockManagerError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .port();

        let id = Uuid::new_v4().simple().to_string()[..8].to_string();
        let abs_root =
            std::fs::canonicalize(working_dir).unwrap_or_else(|_| working_dir.to_path_buf());
        let norm_path = safe_path
            .strip_prefix(&abs_root)
            .map(normalize_posix_path)
            .unwrap_or_else(|_| file_path.to_string());

        let info = MockServerInfo {
            id: id.clone(),
            file_path: norm_path,
            port: actual_port,
            url: format!("http://127.0.0.1:{actual_port}"),
            route_count,
        };

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let matcher_arc = Arc::new(matcher);
        let task = tokio::spawn(async move {
            let _ = serve_matcher_with_shutdown(listener, matcher_arc, enable_cors, async move {
                let _ = shutdown_rx.await;
            })
            .await;
        });

        let mut guard = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            id,
            RunningMockEntry {
                info: info.clone(),
                shutdown_tx: Some(shutdown_tx),
                task,
            },
        );

        Ok(info)
    }

    pub fn stop_server(
        &self,
        id: Option<&str>,
        port: Option<u16>,
    ) -> Result<MockServerInfo, MockManagerError> {
        let mut guard = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        let target_id = if let Some(id_val) = id {
            if guard.contains_key(id_val) {
                Some(id_val.to_string())
            } else {
                None
            }
        } else if let Some(p) = port {
            guard
                .iter()
                .find(|(_, entry)| entry.info.port == p)
                .map(|(k, _)| k.clone())
        } else {
            return Err(MockManagerError::new(
                StatusCode::BAD_REQUEST,
                "Provide either 'id' or 'port' to stop a mock server",
            ));
        };

        let Some(key) = target_id else {
            return Err(MockManagerError::new(
                StatusCode::NOT_FOUND,
                "Running mock server not found",
            ));
        };

        let mut entry = guard.remove(&key).unwrap();
        if let Some(tx) = entry.shutdown_tx.take() {
            let _ = tx.send(());
        }
        entry.task.abort();
        Ok(entry.info)
    }
}

/// 扫描工作区活文档，提取所有含 Mock 变体的路由清单（复用 SSOT 卡片解析器）
pub fn discover_workspace_mock_routes(working_dir: &Path) -> Vec<MockRouteItemDto> {
    let mut routes = Vec::new();
    let abs_root = std::fs::canonicalize(working_dir).unwrap_or_else(|_| working_dir.to_path_buf());
    scan_dir_for_mocks(&abs_root, &abs_root, 0, &mut routes);
    routes.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.title.cmp(&b.title)));
    routes
}

fn scan_dir_for_mocks(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<MockRouteItemDto>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if !SKIP_SCAN_DIRS.contains(&name.as_str()) {
                scan_dir_for_mocks(root, &path, depth + 1, out);
            }
            continue;
        }
        if !is_live_doc(&name) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        // 快速预筛，避免对不含 @mock 的大文档做全量解析
        if !content.contains("@mock") {
            continue;
        }
        let rel_path = path
            .strip_prefix(root)
            .map(normalize_posix_path)
            .unwrap_or(name.clone());

        let cards = if is_http_doc(&name) {
            parse_http_document_to_cards(&content)
                .map(|r| r.cards)
                .unwrap_or_default()
        } else {
            render_markdown_to_html(&content).executable_cards
        };

        for card in cards {
            if card.has_mock {
                out.push(MockRouteItemDto {
                    file_path: rel_path.clone(),
                    title: card.title,
                    method: card.method,
                    path: card.url,
                    raw_block: card.raw_block,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_mock_manager_start_serve_and_stop_lifecycle() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("orders.md");
        std::fs::write(
            &doc,
            "\
```http
GET /api/orders/1

@mock-when query.status == done
HTTP/1.1 200 OK
Content-Type: application/json

{\"status\":\"done\"}

@mock-default
HTTP/1.1 404 Not Found

not found
```
",
        )
        .unwrap();

        let routes = discover_workspace_mock_routes(dir.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].file_path, "orders.md");

        let mgr = WebMockManager::default();
        // 拒绝特权端口
        assert!(
            mgr.start_server(dir.path(), "orders.md", Some(80), true)
                .await
                .is_err()
        );
        // 拒绝目录穿越
        assert!(
            mgr.start_server(dir.path(), "../evil.md", Some(0), true)
                .await
                .is_err()
        );

        let info = mgr
            .start_server(dir.path(), "orders.md", Some(0), true)
            .await
            .unwrap();
        assert_eq!(info.route_count, 1);
        assert_eq!(mgr.list_servers().len(), 1);

        // 真实 HTTP 请求验证 Mock 服务可用
        let resp = reqwest::get(format!("{}/api/orders/1?status=done", info.url))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("done"));

        // 停止服务
        let stopped = mgr.stop_server(Some(&info.id), None).unwrap();
        assert_eq!(stopped.port, info.port);
        assert!(mgr.list_servers().is_empty());
    }
}
