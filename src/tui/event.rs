use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use std::collections::HashMap;
use uuid::Uuid;

pub enum TuiEvent {
    Input(crossterm::event::KeyEvent),
    MouseInput(crossterm::event::MouseEvent),
    Tick,
    Resize(u16, u16),
    RequestStarted(Uuid),
    RequestFinished {
        id: Uuid,
        result: Box<Result<Response, String>>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
        /// 非致命告警（@capture 失败、未定义变量、畸形 JSON 降级等），不影响 success 但必须可见
        warnings: Vec<String>,
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
