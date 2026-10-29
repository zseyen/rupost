//! 消息搜索域（R4/R5）：三视图（HTTP/SSE/WS）的搜索空间解析、匹配定位与跳转。
//! 纯函数优先，便于决策表测试；日志文件态走逐行 grep（5MB 上限由写侧保证）。

use super::AppState;

/// 当前视图的搜索空间（与 ui/response.rs 的分支选择同构）
#[derive(Debug, PartialEq, Eq)]
pub enum SearchSpace {
    /// 内存视觉行（HTTP 完成态 / SSE、WS 流式态）
    VisualLines(Vec<String>),
    /// 完成态长连接日志文件（行号即滑动窗口 scroll 锚点）
    LogFile(std::path::PathBuf),
}

impl AppState {
    /// 打开搜索（Response 面板 `/`）：携带当前 query 重建输入框
    pub fn open_search(&mut self) {
        let mut textarea = ratatui_textarea::TextArea::default();
        if !self.search_query.is_empty() {
            textarea.insert_str(self.search_query.clone());
        }
        self.input_mode = super::InputMode::Search(Box::new(textarea));
    }

    /// 实时更新 query（Search 模态逐键调用）：变更时重置匹配游标
    pub fn set_search_query(&mut self, query: &str) {
        if self.search_query != query {
            self.search_query = query.to_string();
            self.search_match_index = 0;
        }
    }

    /// 当前视图的搜索空间
    pub fn search_space(&self) -> SearchSpace {
        let is_ws = !self.ws_frames.is_empty()
            || self
                .current_request
                .as_ref()
                .map(|r| r.metadata.websocket)
                .unwrap_or(false);
        let is_sse = !self.sse_stream_body.is_empty();
        let streaming = is_ws || is_sse;

        if self.active_detail_tab != super::DetailTab::Response {
            return SearchSpace::VisualLines(
                self.response_visual_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect(),
            );
        }
        if streaming {
            if self.is_loading {
                // 流式态：内存视觉行（ws 优先，与渲染分支一致）
                if is_ws {
                    SearchSpace::VisualLines(
                        self.ws_visual_lines.iter().map(|l| l.to_string()).collect(),
                    )
                } else {
                    SearchSpace::VisualLines(
                        self.sse_visual_lines
                            .iter()
                            .map(|l| l.to_string())
                            .collect(),
                    )
                }
            } else if let Some(path) = &self.log_file_path {
                // 完成态：日志文件逐行 grep（行号即滑窗 scroll 锚点）
                SearchSpace::LogFile(path.clone())
            } else {
                SearchSpace::VisualLines(Vec::new())
            }
        } else {
            SearchSpace::VisualLines(
                self.response_visual_lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect(),
            )
        }
    }

    /// 全部匹配行索引（VisualLines 下标 / LogFile 行号）
    pub fn find_matches(&self) -> Vec<usize> {
        match self.search_space() {
            SearchSpace::VisualLines(lines) => find_matches_in_texts(&lines, &self.search_query),
            SearchSpace::LogFile(path) => {
                find_matches_in_log_file(&path, &self.search_query).unwrap_or_default()
            }
        }
    }

    /// n/N 跳转（环绕）：定位下一/上一匹配并回写 response_scroll。
    /// 无匹配时浮出状态栏提示。
    pub fn jump_to_match(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            return;
        }
        let matches = self.find_matches();
        if matches.is_empty() {
            self.last_error = Some(format!(
                "search: no match for '{}' (in current view)",
                self.search_query
            ));
            return;
        }
        let next = next_match(&matches, self.response_scroll, forward)
            .expect("non-empty matches always yield a target");
        self.search_match_index = matches
            .iter()
            .position(|&m| m == next)
            .unwrap_or(self.search_match_index);
        self.response_scroll = next;
        self.last_error = None;
    }
}

/// 大小写不敏感子串匹配的行索引列表
pub fn find_matches_in_texts(texts: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let q = query.to_lowercase();
    texts
        .iter()
        .enumerate()
        .filter(|(_, t)| t.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

/// 日志文件逐行 grep（行号从 0 计，与滑窗 scroll 锚点同语义）
pub fn find_matches_in_log_file(
    path: &std::path::Path,
    query: &str,
) -> std::io::Result<Vec<usize>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    Ok(find_matches_in_texts(
        &content.lines().map(|l| l.to_string()).collect::<Vec<_>>(),
        query,
    ))
}

/// 环绕跳转：从 current 出发找下一/上一匹配（含等于 current 的命中重回自身）
pub fn next_match(matches: &[usize], current: usize, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    if forward {
        matches
            .iter()
            .find(|&&m| m > current)
            .copied()
            .or_else(|| matches.first().copied())
    } else {
        matches
            .iter()
            .rev()
            .find(|&&m| m < current)
            .copied()
            .or_else(|| matches.last().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts() -> Vec<String> {
        vec![
            "GET /users".to_string(),
            "data: hello world".to_string(),
            "data: Hello RUPost".to_string(),
            "[→] 12:00:00 | PING".to_string(),
        ]
    }

    #[test]
    fn test_find_matches_case_insensitive_and_empty_query() {
        assert_eq!(find_matches_in_texts(&texts(), "hello"), vec![1, 2]);
        assert_eq!(find_matches_in_texts(&texts(), "HELLO"), vec![1, 2]);
        assert_eq!(find_matches_in_texts(&texts(), "ping"), vec![3]);
        // 空 query 零匹配（不做全量高亮）
        assert_eq!(find_matches_in_texts(&texts(), ""), Vec::<usize>::new());
        assert_eq!(find_matches_in_texts(&texts(), "nope"), Vec::<usize>::new());
    }

    #[test]
    fn test_next_match_wrapping_decision_table() {
        let matches = [1usize, 5, 9];
        // 向下：下一个更大的；末尾环绕到首个
        assert_eq!(next_match(&matches, 0, true), Some(1));
        assert_eq!(next_match(&matches, 1, true), Some(5));
        assert_eq!(next_match(&matches, 5, true), Some(9));
        assert_eq!(next_match(&matches, 9, true), Some(1));
        // 向上：前一个更小的；首个环绕到末尾
        assert_eq!(next_match(&matches, 9, false), Some(5));
        assert_eq!(next_match(&matches, 1, false), Some(9)); // 环绕到末尾
        assert_eq!(next_match(&matches, 0, false), Some(9));
        assert_eq!(next_match(&[], 3, true), None);
    }

    #[test]
    fn test_log_file_grep() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ws.log");
        std::fs::write(
            &path,
            "[→] 12:00:00 | PING\n[←] 12:00:01 | PONG\n[→] 12:00:02 | PING again\n",
        )
        .unwrap();
        assert_eq!(find_matches_in_log_file(&path, "PING").unwrap(), vec![0, 2]);
        assert_eq!(find_matches_in_log_file(&path, "PONG").unwrap(), vec![1]);
        assert_eq!(
            find_matches_in_log_file(&path, "").unwrap(),
            Vec::<usize>::new()
        );
    }
}

#[cfg(test)]
mod jump_tests {
    use super::*;
    use crate::parser::ParsedRequest;

    fn state_with_visual_lines(lines: &[&str]) -> AppState {
        let mut state = AppState::new();
        state.active_panel = crate::tui::state::Panel::Response;
        state.active_detail_tab = super::super::DetailTab::Response;
        state.response_visual_lines = lines
            .iter()
            .map(|l| ratatui::text::Line::raw(l.to_string()))
            .collect();
        state
    }

    /// R4 验收：/ 输入 → 实时 query → n/N 环绕跳转 + 无匹配浮错
    #[test]
    fn test_search_jump_flow() {
        let mut state = state_with_visual_lines(&[
            "GET /users",
            "data: hello world",
            "data: Hello RUPost",
            "tail",
        ]);

        state.open_search();
        assert!(matches!(
            state.input_mode,
            super::super::InputMode::Search(_)
        ));
        state.set_search_query("hello");

        // n：向下跳到第一个匹配（当前 scroll=0）
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 1);
        // n：环绕（9 > 当前 1 → 2；末尾后回首）
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 2);
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 1); // 环回首
        // N：向上
        state.jump_to_match(false);
        assert_eq!(state.response_scroll, 2);

        // 无匹配 → last_error 浮出、scroll 不动
        state.set_search_query("nope");
        state.response_scroll = 0;
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 0);
        assert!(state.last_error.as_deref().unwrap().contains("no match"));

        // Esc/Enter 关闭后 query 留存（n/N 续跳语义）
        state.set_search_query("tail");
        state.input_mode = super::super::InputMode::None;
        assert_eq!(state.search_query, "tail");
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 3);
    }

    /// 完成态长连接：搜索空间为日志文件，跳转写入 scroll（滑窗锚点）
    #[test]
    fn test_search_space_completed_uses_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("ws.log");
        std::fs::write(&log, "[→] PING\n[←] PONG\n[→] PING again\n").unwrap();

        let mut state = AppState::new();
        state.active_detail_tab = super::super::DetailTab::Response;
        state.ws_frames.push(crate::tui::state::WsFrameRecord {
            is_send: true,
            timestamp: "12:00:00".to_string(),
            content: "PING".to_string(),
        });
        state.is_loading = false;
        state.log_file_path = Some(log.clone());
        state.search_query = "PONG".to_string();

        match state.search_space() {
            SearchSpace::LogFile(p) => assert_eq!(p, log),
            other => panic!("expected LogFile space, got {other:?}"),
        }
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 1);
    }

    /// 非搜索态流式自动吸底不受影响（search_query 为空才吸底）
    #[test]
    fn test_stream_autoscroll_paused_while_searching() {
        let mut state = AppState::new();
        state.active_panel = crate::tui::state::Panel::Response;
        state.current_request = Some(ParsedRequest::new(1));
        state.current_request.as_mut().unwrap().metadata.websocket = true;

        // 无 query：自动吸底
        state.handle_ws_frame(false, "frame-1".to_string());
        assert_eq!(state.response_scroll, 9999);

        // 有 query：暂停吸底，滚动保持
        state.response_scroll = 3;
        state.search_query = "frame".to_string();
        state.handle_ws_frame(false, "frame-2".to_string());
        assert_eq!(state.response_scroll, 3);
    }
}
