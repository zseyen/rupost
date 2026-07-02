use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use crate::variable::VariableContext;
use std::collections::HashMap;
use super::event::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Files,
    Editor,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Narrow,
    Stacked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    Quit,
    SwitchFile(usize),
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub active_panel: Panel,
    pub is_loading: bool,
    pub show_help: bool,
    pub layout_mode: LayoutMode,
    pub terminal_width: u16,
    pub terminal_height: u16,
    
    // 数据模型
    pub file_tree: Vec<String>, // 扁平文件树列表数据
    pub selected_file_index: usize,
    pub current_request: Option<ParsedRequest>,
    pub last_response: Option<Response>,
    pub assertions: Vec<AssertionResult>,
    pub variables: VariableContext,
    pub quick_input: Option<String>,
    pub is_quitting: bool,

    // 编辑器及未保存确认状态
    pub editor_text: String,
    pub editor_file_path: Option<String>,
    pub is_dirty: bool,
    pub show_unsaved_confirm: bool,
    pub pending_action: Option<PendingAction>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_panel: Panel::Files,
            is_loading: false,
            show_help: false,
            layout_mode: LayoutMode::Wide,
            terminal_width: 120,
            terminal_height: 30,
            file_tree: Vec::new(),
            selected_file_index: 0,
            current_request: None,
            last_response: None,
            assertions: Vec::new(),
            variables: VariableContext::new(),
            quick_input: None,
            is_quitting: false,
            editor_text: String::new(),
            editor_file_path: None,
            is_dirty: false,
            show_unsaved_confirm: false,
            pending_action: None,
        }
    }

    /// 防御性自适应布局尺寸计算
    pub fn update_layout(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;

        // 硬下限防御：如果极小，强制为 Stacked，UI 会渲染简易警告
        if width < 40 || height < 10 {
            self.layout_mode = LayoutMode::Stacked;
            return;
        }

        if width >= 120 {
            self.layout_mode = LayoutMode::Wide;
        } else if width >= 80 {
            self.layout_mode = LayoutMode::Narrow;
        } else {
            self.layout_mode = LayoutMode::Stacked;
        }
    }

    /// 单向状态机更新
    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.is_quitting = true;
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::SwitchPanel(panel) => {
                self.active_panel = panel;
            }
            Action::SendRequest(req) => {
                self.current_request = Some(req);
                self.is_loading = true;
            }
            Action::UpdateQuickInput(val) => {
                self.quick_input = Some(val);
            }
        }
    }

    /// 处理完成的请求结果同步变量
    pub fn handle_request_finished(
        &mut self,
        result: Result<Response, String>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
    ) {
        self.is_loading = false;
        self.assertions = assertions;
        match result {
            Ok(resp) => {
                self.last_response = Some(resp);
                // 同步变量到全局上下文，防止多线程借用争用
                self.variables.extend(captured_vars);
            }
            Err(e) => {
                self.last_response = Some(Response::error(e));
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
