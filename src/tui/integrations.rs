//! CLI 能力接入集成层（R10–R13）：快照重放、mock 服务、AI 断言、cookie 持久化。
//! 全部复用既有 lib 公开入口，TUI 侧仅做编排与错误浮出。

use super::state::AppState;
use crate::tui::event::TuiEvent;
use std::sync::Arc;

/// 活跃 mock 会话（`M` 键开关）；abort 即停止服务
pub struct MockSessionState {
    pub abort: tokio::task::AbortHandle,
    pub port: u16,
    pub file: String,
}

impl Clone for MockSessionState {
    fn clone(&self) -> Self {
        Self {
            abort: self.abort.clone(),
            port: self.port,
            file: self.file.clone(),
        }
    }
}

impl std::fmt::Debug for MockSessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockSessionState")
            .field("port", &self.port)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// R10：重放最近的快照文件（`V` 键）。结果经事件回流，Results tab 呈现。
pub async fn replay_latest_snapshot(
    state: &mut AppState,
    event_tx: &tokio::sync::mpsc::Sender<TuiEvent>,
) {
    let Some(path) = latest_snapshot_path(state) else {
        state.last_error = Some(
            "no snapshot files in ./snapshots/ — press Ctrl+S after a run to save one".to_string(),
        );
        return;
    };

    let path = path.clone();
    let tx = event_tx.clone();
    tokio::spawn(async move {
        // target 恒为 None，构造不可能失败；expect 兼容 async block 无 Result 返回
        let executor = crate::runner::ReplayExecutor::new(None, true)
            .expect("ReplayExecutor::new(None) 不可能失败");
        match executor.replay_file(&path).await {
            Ok(report) => {
                let _ = tx
                    .send(TuiEvent::ReplayDone(
                        Box::new(report),
                        path.to_string_lossy().into_owned(),
                    ))
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(TuiEvent::ReplayFailed(format!("replay failed: {e}")))
                    .await;
            }
        }
    });
    state.last_error = None;
}

/// 最近一份快照文件（文件名含时间戳，字典序即时间序）
fn latest_snapshot_path(state: &AppState) -> Option<std::path::PathBuf> {
    let dir = state.workspace_root.join("snapshots");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("snapshot_") && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.pop()
}

/// R11：mock 服务开关（`M` 键）。无路由/未保存文件浮出可读错误。
pub async fn toggle_mock_server(state: &mut AppState) {
    if let Some(sess) = state.mock_session.take() {
        sess.abort.abort();
        state.last_error = None;
        return;
    }

    // 未保存文件不可用（build_mock_matcher 走磁盘）
    let Some(path) = state.editor_file_path.clone() else {
        state.last_error =
            Some("save the file first (unsaved editor content cannot host a mock)".to_string());
        return;
    };

    // 解析 + 编译路由（编辑器脏内容不参与——mock 以磁盘为准，与 CLI mock start 同语义）
    let (matcher, route_count) = match crate::mock::server::build_mock_matcher(&path) {
        Ok(pair) => pair,
        Err(e) => {
            state.last_error = Some(format!("mock config: {e}"));
            return;
        }
    };
    if route_count == 0 {
        state.last_error = Some("no mock routes in file".to_string());
        return;
    }

    // 探测空闲端口（bind :0 → 立即释放 → 传入 serve；微小竞窗可接受）
    let probe = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            state.last_error = Some(format!("mock port probe: {e}"));
            return;
        }
    };
    let port = probe.local_addr().map(|a| a.port()).unwrap_or(0);
    drop(probe);

    let listener = match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            state.last_error = Some(format!("mock bind: {e}"));
            return;
        }
    };

    let matcher = Arc::new(matcher);
    let handle = tokio::spawn(async move {
        let _ = crate::mock::server::axum_adapter::serve_matcher(listener, matcher, true).await;
    });

    state.mock_session = Some(MockSessionState {
        abort: handle.abort_handle(),
        port,
        file: path,
    });
    state.last_error = None;
}

/// R12：AI 断言翻译（`I` 键模态提交后）。结果经事件回流插入编辑器。
pub fn spawn_ai_assert(event_tx: &tokio::sync::mpsc::Sender<TuiEvent>, text: &str) {
    let text = text.to_string();
    let tx = event_tx.clone();
    tokio::spawn(async move {
        let outcome = async {
            let resolved = crate::ai::config::resolve().await?;
            let translator = crate::ai::AssertionTranslator::new(resolved.client);
            translator.translate(&text).await.map_err(|e| e.to_string())
        }
        .await;
        match outcome {
            Ok(assertions) => {
                let _ = tx.send(TuiEvent::AiAssertDone(Ok(assertions))).await;
            }
            Err(e) => {
                let _ = tx
                    .send(TuiEvent::AiAssertDone(Err(format!("AI assert: {e}"))))
                    .await;
            }
        }
    });
}

/// R13：cookie 持久化——桥状态写回 `--cookie-file`（有配置时每次触发末尾调用）
pub fn persist_cookie_bridge(state: &AppState) {
    if let Some(path) = &state.cookie_file_path
        && let Some(json) = state
            .cookie_bridge
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        && let Err(e) = std::fs::write(path, json.to_string())
    {
        tracing::warn!("{}: {}: {}", crate::middleware::ERR_COOKIE_PERSIST, path, e);
    }
}
