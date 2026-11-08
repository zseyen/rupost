//! 流式视图臂（WS/SSE）：流式态、交互会话与完成态滑窗渲染。
//! 状态写入（滑窗装载/惰性重构）由门面前置完成，本模块只读渲染。

use super::highlight::apply_search_highlight;
use crate::tui::state::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

/// 流式（SSE/WS）完成态视图首行插入告警摘要（D1：流式分支不走 HTTP 缓存管线）。
/// 仅 scroll=0 时可见（作为视图头行）；滚动后随内容移出视口。
fn prepend_warnings_summary(lines: &mut Vec<Line<'static>>, state: &AppState) {
    if state.warnings.is_empty() {
        return;
    }
    let t = state.palette();
    lines.insert(
        0,
        Line::from(Span::styled(
            format!("Warnings: {}", state.warnings.len()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )),
    );
}

/// WS 流式/交互/完成态视图（原 use_streaming && is_ws 臂）。
/// 渲染目标为整个 area（含 Tab 栏区）——与迁移前一致。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_ws(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    state: &AppState,
    visible_height: usize,
    max_width: usize,
) {
    let t = state.palette();
    let session_active = state.ws_session.is_some();
    let visual_lines = if state.is_loading || session_active {
        state.ws_visual_lines.clone()
    } else if state.log_file_path.is_some() {
        // 滑窗装载已由门面前置（viewport_cache 就绪）；此处只读拼装
        let mut lines = Vec::new();
        for f in &state.viewport_cache {
            let arrow = if f.is_send { "[→] " } else { "[←] " };
            let arrow_color = if f.is_send { t.info } else { t.accent };
            let text_line = format!("{} {} {}", arrow, f.timestamp, f.content);
            let wrapped = crate::tui::state::VisualLineProcessor::wrap_text(&text_line, max_width);
            for w_line in wrapped {
                let raw_str = w_line.to_string();
                if raw_str.starts_with("[→]") {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "[→] ",
                            Style::default()
                                .fg(arrow_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("{} ", f.timestamp), Style::default().fg(t.dim)),
                        Span::raw(raw_str[15..].to_string()),
                    ]));
                } else if raw_str.starts_with("[←]") {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "[←] ",
                            Style::default()
                                .fg(arrow_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("{} ", f.timestamp), Style::default().fg(t.dim)),
                        Span::raw(raw_str[15..].to_string()),
                    ]));
                } else {
                    lines.push(w_line);
                }
            }
        }
        lines
    } else {
        // 完成态但无日志文件（历史运行/无日志场景）：回退内存帧，与搜索空间一致
        state.ws_visual_lines.clone()
    };

    let mut visual_lines_final = visual_lines;
    if visual_lines_final.is_empty() {
        visual_lines_final.push(Line::from(Span::styled(
            "WebSocket connected. Listening for frames...",
            Style::default().fg(t.dim),
        )));
    }
    prepend_warnings_summary(&mut visual_lines_final, state);

    // 3. 切片：流式态/交互会话 = 底部钳制；完成态 = scroll 即视口顶部日志行号
    //    （滑窗前后各 50 行前导缓冲，视口起点 = min(scroll, 50) + warnings 偏移；
    //    此前按窗口局部长度钳制，日志行号 >50 的搜索跳转目标会落在视口外）
    let sliced = if state.is_loading || session_active {
        let total_lines = visual_lines_final.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);
        let end = (scroll_y + visible_height).min(total_lines);
        visual_lines_final[scroll_y..end].to_vec()
    } else {
        let warn = usize::from(!state.warnings.is_empty());
        let lead = if state.response_scroll == 0 {
            0
        } else {
            warn + state.response_scroll.min(50)
        }
        .min(visual_lines_final.len());
        let end = (lead + visible_height).min(visual_lines_final.len());
        visual_lines_final[lead..end].to_vec()
    };

    let sliced = apply_search_highlight(sliced, state);
    frame.render_widget(Paragraph::new(sliced).block(block), area);
}

/// SSE 流式/完成态视图（原 is_sse 臂）。渲染目标为整个 area。
pub(crate) fn render_sse(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    state: &AppState,
    visible_height: usize,
    max_width: usize,
) {
    let t = state.palette();
    let visual_lines = if state.is_loading {
        // 惰性全量重构已由门面前置（sse_visual_lines/sse_last_processed_pos 就绪）
        // 2. 取出已缓存行，并动态折行最后一行的未闭合活动行 (active line)
        let active_line = &state.sse_stream_body[state.sse_last_processed_pos..];
        let mut lines = state.sse_visual_lines.clone();
        if !active_line.is_empty() {
            let wrapped_active =
                crate::tui::state::VisualLineProcessor::wrap_text(active_line, max_width);
            for w in wrapped_active {
                lines.push(w);
            }
        }
        lines
    } else {
        // 滑窗装载已由门面前置；此处只读拼装
        let raw_body = state
            .viewport_cache
            .iter()
            .map(|f| f.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let mut lines = Vec::new();
        for line in raw_body.lines() {
            let wrapped = crate::tui::state::VisualLineProcessor::wrap_text(line, max_width);
            if wrapped.is_empty() {
                lines.push(Line::from(""));
            } else {
                for w in wrapped {
                    lines.push(w);
                }
            }
        }
        lines
    };

    let mut visual_lines_final = visual_lines;
    if visual_lines_final.is_empty() {
        visual_lines_final.push(Line::from(Span::styled(
            "SSE streaming connected...",
            Style::default().fg(t.dim),
        )));
    }
    prepend_warnings_summary(&mut visual_lines_final, state);

    // 切片语义与 WS 完成态相同（scroll = 视口顶部日志行号）
    let sliced = if state.is_loading {
        let total_lines = visual_lines_final.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);
        let end = (scroll_y + visible_height).min(total_lines);
        visual_lines_final[scroll_y..end].to_vec()
    } else {
        let warn = usize::from(!state.warnings.is_empty());
        let lead = if state.response_scroll == 0 {
            0
        } else {
            warn + state.response_scroll.min(50)
        }
        .min(visual_lines_final.len());
        let end = (lead + visible_height).min(visual_lines_final.len());
        visual_lines_final[lead..end].to_vec()
    };

    let sliced = apply_search_highlight(sliced, state);
    frame.render_widget(Paragraph::new(sliced).block(block), area);
}
