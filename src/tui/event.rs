use crate::parser::ParsedRequest;
use std::collections::HashMap;
use uuid::Uuid;

pub enum TuiEvent {
    Input(crossterm::event::KeyEvent),
    MouseInput(crossterm::event::MouseEvent),
    Tick,
    Resize(u16, u16),
    RequestStarted(Uuid),
    RequestResult {
        result: Box<crate::runner::types::TestResult>,
        captured_vars: HashMap<String, String>,
        /// 本次批运行的请求总数（多请求执行的进度语义）
        run_total: usize,
    },
    StreamChunk {
        id: Uuid,
        chunk: String,
        total_lines: usize,
    },
    WsFrame {
        id: Uuid,
        is_send: bool,
        content: String,
        total_lines: usize,
    },
    InitLogPath {
        id: Uuid,
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    ToggleHelp,
    SwitchPanel(super::state::Panel),
    SendRequest(Box<ParsedRequest>),
}
