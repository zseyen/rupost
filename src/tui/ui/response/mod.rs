//! Response 面板门面：Tab 栏/标题/边框布局 + **渲染期副作用前置重建** + Tab 路由分发。
//!
//! 拆分纪律（解耦提案 v2 Phase 3）：状态写入（滑窗装载、SSE 惰性重构、HTTP 缓存
//! 重建）全部收敛到门面的前置重建段（按原臂序逐一复制条件），子组件一律
//! `&AppState` 只读——渲染期隐式副作用归零，且每处写入的条件与迁移前逐臂等价。

mod diagnose;
mod headers;
mod highlight;
mod request_tab;
mod response_tab;
mod results;
mod streaming;

use crate::tui::state::{AppState, DetailTab, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub fn render(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let t = state.palette();
    let focus = state.active_panel == Panel::Response;
    let border_color = if focus { t.info } else { t.dim };

    // 1. 垂直划分选项卡 Tab 栏和内容显示区
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // 顶部 Tab 栏
            Constraint::Min(0),    // 主要内容区
        ])
        .split(area);
    let tab_area = chunks[0];
    let content_area = chunks[1];

    // 2. 渲染详情子选项卡
    let tab_titles = vec![
        "[1] Response",
        "[2] Headers",
        "[3] Request",
        "[4] Results",
        "[5] Diagnose",
    ];
    let active_idx = match state.active_detail_tab {
        DetailTab::Response => 0,
        DetailTab::Headers => 1,
        DetailTab::Request => 2,
        DetailTab::Results => 3,
        DetailTab::Diagnose => 4,
    };
    let tabs = Tabs::new(tab_titles)
        .select(active_idx)
        .style(Style::default().fg(t.dim))
        .highlight_style(Style::default().fg(t.info).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, tab_area);

    let is_ws = !state.ws_frames.is_empty()
        || state
            .current_request
            .as_ref()
            .map(|r| r.metadata.websocket)
            .unwrap_or(false);
    let is_sse = !state.sse_stream_body.is_empty();

    let title = if state.is_loading {
        let spinner_char = state.get_spinner_char();
        if is_ws {
            format!(" Response Viewer [WS Streaming {}] ", spinner_char)
        } else if is_sse {
            format!(" Response Viewer [SSE Streaming {}] ", spinner_char)
        } else {
            format!(" Response Viewer [Loading {}] ", spinner_char)
        }
    } else if state.ws_session.is_some() {
        " Response Viewer [WS Interactive] ".to_string()
    } else {
        if is_ws {
            " Response Viewer [WS Completed] ".to_string()
        } else if is_sse {
            " Response Viewer [SSE Completed] ".to_string()
        } else {
            " Response Viewer ".to_string()
        }
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let visible_height = content_area.height.saturating_sub(2) as usize; // 可视行数
    let max_width = content_area.width.saturating_sub(2) as usize; // 可视列数

    let use_streaming = (is_ws || is_sse) && state.active_detail_tab == DetailTab::Response;
    // 交互会话（R6）无 runner 日志文件参与，帧只存在于内存视觉行——
    // 必须与流式态同路渲染，否则视口恒空且可能串入上一轮运行的旧日志
    let session_active = state.ws_session.is_some();

    // ===== 副作用前置重建段（按迁移前臂序逐臂复制条件；子组件只读） =====
    if state.active_detail_tab == DetailTab::Results
        || state.active_detail_tab == DetailTab::Diagnose
    {
        // Results/Diagnose 有专属渲染臂，不经任何缓存管线
    } else if use_streaming && is_ws {
        // WS 完成态 + 日志文件：滑窗装载（原 WS 臂 :340）
        if !(state.is_loading || session_active) && state.log_file_path.is_some() {
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
        }
    } else if is_sse {
        if state.is_loading {
            // SSE 惰性全量重构（Resize 清缓存后一次性重建，原 SSE 臂 :418-430）
            if state.sse_visual_lines.is_empty() && !state.sse_stream_body.is_empty() {
                let body = &state.sse_stream_body;
                if let Some(last_newline_idx) = body.rfind('\n') {
                    let completed_text = &body[..=last_newline_idx];
                    for line in completed_text.lines() {
                        let wrapped =
                            crate::tui::state::VisualLineProcessor::wrap_text(line, max_width);
                        for w in wrapped {
                            state.sse_visual_lines.push(w);
                        }
                    }
                    state.sse_last_processed_pos = last_newline_idx + 1;
                }
            }
        } else {
            // 完成态滑窗装载（原 SSE 臂 :445）
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
        }
    } else if state.is_loading {
        // RUNNING 臂无缓存写入
    } else if state.last_response.is_some() && state.response_visual_lines.is_empty() {
        // HTTP 缓存管线重建（原 HTTP 臂 :509-511）
        response_tab::rebuild_http_visual_lines(state, max_width);
    }

    // ===== Tab 路由分发（子组件只读） =====
    if state.active_detail_tab == DetailTab::Results {
        results::render(frame, content_area, block, state);
    } else if state.active_detail_tab == DetailTab::Diagnose {
        diagnose::render(frame, content_area, block, state);
    } else if use_streaming && is_ws {
        streaming::render_ws(frame, area, block, state, visible_height, max_width);
    } else if is_sse {
        streaming::render_sse(frame, area, block, state, visible_height, max_width);
    } else if state.is_loading {
        frame.render_widget(
            Paragraph::new(format!(
                "[RUNNING] Sending request... {}",
                state.get_spinner_char()
            ))
            .style(Style::default().fg(t.accent).add_modifier(Modifier::BOLD))
            .block(block),
            area,
        );
    } else if state.last_response.is_some() {
        response_tab::render_http_completed(frame, content_area, block, state, visible_height);
    } else {
        frame.render_widget(
            Paragraph::new("No response data. Trigger execution via Ctrl+Enter.")
                .style(Style::default().fg(t.dim))
                .block(block),
            content_area,
        );
    }
}
