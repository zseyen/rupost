use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use std::collections::HashMap;
use uuid::Uuid;

pub enum TuiEvent {
    Input(crossterm::event::KeyEvent),
    Tick,
    Resize(u16, u16),
    RequestStarted(Uuid),
    RequestFinished {
        id: Uuid,
        result: Box<Result<Response, String>>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
    },
    StreamChunk {
        id: Uuid,
        chunk: String,
    },
    WsFrame {
        id: Uuid,
        is_send: bool,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    ToggleHelp,
    SwitchPanel(super::state::Panel),
    SendRequest(Box<ParsedRequest>),
    UpdateQuickInput(String),
}
