//! CLI 能力接入集成层（R10–R13）：快照重放、mock 服务、AI 断言、cookie 持久化。
//! 全部复用既有 lib 公开入口，TUI 侧仅做编排与错误浮出。

use super::state::AppState;
use crate::ai::AssertionTranslator;
use crate::ai::config::resolve as resolve_ai_config;
use crate::diagnostics::BufferedWarningSink;
use crate::middleware::ERR_COOKIE_PERSIST;
use crate::mock::server::axum_adapter::MockTrafficEvent;
use crate::mock::server::build_mock_matcher;
use crate::runner::ReplayExecutor;
use crate::tui::event::TuiEvent;
use crate::variable::WorkspaceConfig;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc::{Sender, unbounded_channel};
use tokio::task::AbortHandle;

#[derive(Clone)]
pub struct MockSessionState {
    /// 批三-3：共享会话句柄（Arc——克隆后仅最后一个持有者 drop 才关停，
    /// 原语义由 Arc 计数保留；关停从 abort 强杀改为 session 内 oneshot 优雅退）
    session: Arc<crate::mock::session::MockServerSession>,
    pub file: String,
}

impl MockSessionState {
    pub fn new(session: crate::mock::session::MockServerSession, file: String) -> Self {
        Self {
            session: Arc::new(session),
            file,
        }
    }

    pub fn port(&self) -> u16 {
        self.session.port
    }
}

impl fmt::Debug for MockSessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockSessionState")
            .field("port", &self.port())
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// R10：重放最近的快照文件（`V` 键）。结果经事件回流，Results tab 呈现。
pub async fn replay_latest_snapshot(state: &mut AppState, event_tx: &Sender<TuiEvent>) {
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
        let executor =
            ReplayExecutor::new(None, true).expect("ReplayExecutor::new(None) 不可能失败");
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

/// 解析 TUI 快照存储目录：若 `rupost.toml [paths].snapshots_dir` 显式配置且合法则采用之，
/// 否则保持历史默认 `<workspace_root>/snapshots`。
pub(crate) fn tui_snapshots_dir(workspace_root: &Path) -> PathBuf {
    let ws = WorkspaceConfig::load(workspace_root, &BufferedWarningSink::new());
    ws.explicit_snapshots_dir
        .unwrap_or_else(|| workspace_root.join("snapshots"))
}

/// 最近一份快照文件（文件名含时间戳，字典序即时间序）
fn latest_snapshot_path(state: &AppState) -> Option<PathBuf> {
    let dir = tui_snapshots_dir(&state.workspace_root);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
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

/// R11：mock 服务开关（`M` 键）。无路由/未保存文件浮出可读错误；入站流量经 `event_tx` 回流到界面。
pub async fn toggle_mock_server(state: &mut AppState, event_tx: &Sender<TuiEvent>) {
    if state.mock_session.take().is_some() {
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
    let (matcher, route_count) = match build_mock_matcher(&path) {
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

    // 批三-3：bind+serve+shutdown 核心收敛到 MockServerSession（宿主无关）；
    // 单次绑定语义由 session 内部保证，关停走 oneshot 优雅退出（Drop 兜底）
    state.mock_traffic.clear();
    let (mock_tx, mock_rx) = unbounded_channel::<MockTrafficEvent>();
    let tui_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut mock_rx = mock_rx;
        while let Some(ev) = mock_rx.recv().await {
            if tui_tx.send(TuiEvent::MockTraffic(ev)).await.is_err() {
                break;
            }
        }
    });

    let session =
        match crate::mock::session::MockServerSession::start(0, Arc::new(matcher), true, mock_tx)
            .await
        {
            Ok(sess) => sess,
            Err(e) => {
                state.last_error = Some(format!("mock bind: {e}"));
                return;
            }
        };

    state.mock_session = Some(MockSessionState::new(session, path));
    state.last_error = None;
}

/// R12：AI 断言翻译（`I` 键模态提交后）。结果经事件回流插入编辑器。
pub fn spawn_ai_assert(event_tx: &Sender<TuiEvent>, text: &str) {
    let text = text.to_string();
    let tx = event_tx.clone();
    tokio::spawn(async move {
        let outcome = async {
            let resolved = resolve_ai_config().await?;
            let translator = AssertionTranslator::new(resolved.client);
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
        && let Err(e) = fs::write(path, json.to_string())
    {
        tracing::warn!("{}: {}: {}", ERR_COOKIE_PERSIST, path, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::task::yield_now;
    use tokio::time::sleep;

    #[test]
    fn test_tui_snapshots_dir_defaults_and_honors_custom_toml_path() {
        let dir = tempdir().unwrap();
        // 1. 无配置时保持历史默认 snapshots/
        assert_eq!(tui_snapshots_dir(dir.path()), dir.path().join("snapshots"));

        // 2. 配置合法相对路径时命中自定义目录；配置逃逸路径时回退默认
        fs::write(
            dir.path().join("rupost.toml"),
            "[paths]\nsnapshots_dir = \"custom/snaps\"\n",
        )
        .unwrap();
        assert_eq!(
            tui_snapshots_dir(dir.path()),
            dir.path().join("custom/snaps")
        );
    }

    #[tokio::test]
    async fn test_toggle_mock_server_starts_serves_and_stops() {
        let dir = tempdir().unwrap();
        let doc_path = dir.path().join("mock_api.http");
        fs::write(
            &doc_path,
            "GET /api/ping\n\n@mock-default\nHTTP/1.1 200 OK\n\npong\n",
        )
        .unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut state = AppState::new();
        state.editor_file_path = Some(doc_path.to_string_lossy().into_owned());

        toggle_mock_server(&mut state, &tx).await;
        assert!(
            state.last_error.is_none(),
            "unexpected error: {:?}",
            state.last_error
        );
        let port = state
            .mock_session
            .as_ref()
            .expect("mock session should start")
            .port();
        assert!(port > 0);

        let resp = Client::new()
            .get(format!("http://127.0.0.1:{port}/api/ping"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        assert_eq!(resp.text().await.unwrap(), "pong");

        let ev = rx.recv().await.expect("should emit MockTraffic event");
        match ev {
            TuiEvent::MockTraffic(traffic) => {
                assert_eq!(traffic.method, "GET");
                assert_eq!(traffic.path, "/api/ping");
                assert_eq!(traffic.status, 200);
            }
            other => panic!("unexpected event: {:?}", std::mem::discriminant(&other)),
        }

        // 再次 toggle 停止
        toggle_mock_server(&mut state, &tx).await;
        assert!(state.mock_session.is_none());
    }

    #[tokio::test]
    async fn test_mock_session_drop_aborts_server() {
        let dir = tempdir().unwrap();
        let doc_path = dir.path().join("drop_mock.http");
        fs::write(
            &doc_path,
            "GET /api/ping\n\n@mock-default\nHTTP/1.1 200 OK\n\npong\n",
        )
        .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let mut state = AppState::new();
        state.editor_file_path = Some(doc_path.to_string_lossy().into_owned());
        toggle_mock_server(&mut state, &tx).await;
        let port = state.mock_session.as_ref().unwrap().port();

        // 克隆后再释放克隆体不应终止服务；仅当最后一个持有者 drop 时才终止
        let cloned = state.mock_session.clone();
        drop(cloned);
        let resp = Client::new()
            .get(format!("http://127.0.0.1:{port}/api/ping"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        drop(state);
        yield_now().await;
        sleep(Duration::from_millis(20)).await;
        let err = Client::new()
            .get(format!("http://127.0.0.1:{port}/api/ping"))
            .send()
            .await;
        assert!(err.is_err(), "server should stop after last AppState drop");
    }
}
