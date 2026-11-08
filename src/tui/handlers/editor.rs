//! 编辑器组件按键处理：纯粹的键位模式匹配与滚动路由。
//! 跨界业务动作（执行编排/导出/快照/历史生成/WS 会话）委托 `actions/`
//! （解耦提案 v2 Phase 2）；`V`/`M` 键委托 `tui::integrations` 为既有先例。

use super::nav;
use crate::tui::state::{AppState, InputMode};
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

/// 处理编辑器面板按键（包括触发运行测试请求与 WS 交互会话）
pub(super) async fn handle_editor_key(
    state: &mut AppState,
    key: KeyEvent,
    event_tx: &mpsc::Sender<crate::tui::event::TuiEvent>,
) {
    if state.show_help {
        return;
    }
    let total_lines = state.editor_text.lines().count();

    if nav::is_run_key(&key) {
        super::actions::execution::trigger_request_execution(state, event_tx).await;
    } else {
        let max_offset = nav::editor_max_offset(total_lines);
        match key.code {
            // R6：`W` 建立交互式 WS 会话（编辑器须为 @websocket 请求）
            KeyCode::Char('W') => {
                super::actions::ws_session::start_ws_session(state, event_tx).await
            }
            // R4：`#` 打开单请求选择器模态（`#3` / `@name`）
            KeyCode::Char('#') => {
                state.input_mode = InputMode::Selector(Box::new(
                    super::sidebar::new_input_textarea(state.palette()),
                ));
            }
            // R8：`X`/`Shift+X` 导出当前请求为 curl 命令（脱敏/原样）
            KeyCode::Char('X') => {
                let redact = !key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT);
                super::actions::export::export_current_request(state, redact);
            }
            // R9：`Ctrl+S` 保存当前请求/响应快照（默认脱敏）
            KeyCode::Char('s')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                super::actions::snapshot::save_current_snapshot(state);
            }
            // R10：`V` 重放最新快照
            KeyCode::Char('V') => {
                crate::tui::integrations::replay_latest_snapshot(state, event_tx).await;
            }
            // R11：`M` 开关当前文件 mock 服务器
            KeyCode::Char('M') => {
                crate::tui::integrations::toggle_mock_server(state).await;
            }
            // R12：`I` AI 断言翻译模态
            KeyCode::Char('I') => {
                state.input_mode = InputMode::AiAssert(Box::new(
                    super::sidebar::new_input_textarea(state.palette()),
                ));
            }
            // R6：`D` 关闭活跃会话（drop → SessionManager 退出 → 连接关闭）
            KeyCode::Char('D') => {
                if state.ws_session.take().is_some() {
                    state.last_error = None;
                }
            }
            // R6：`S` 打开发送输入框（会话活跃时）
            KeyCode::Char('S') => {
                if state.ws_session.is_some() {
                    state.input_mode = InputMode::WsSend(Box::new(
                        super::sidebar::new_input_textarea(state.palette()),
                    ));
                } else {
                    state.last_error =
                        Some("no active WS session — press W to open one".to_string());
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.editor_scroll = nav::scroll_up(state.editor_scroll, 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.editor_scroll = nav::scroll_down(state.editor_scroll, max_offset, 1);
            }
            KeyCode::PageUp => {
                state.editor_scroll = nav::scroll_up(state.editor_scroll, 10);
            }
            KeyCode::PageDown => {
                state.editor_scroll = nav::scroll_down(state.editor_scroll, max_offset, 10);
            }
            _ => {}
        }
    }
}
