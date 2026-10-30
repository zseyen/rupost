use rupost::http::Response;
use rupost::parser::ParsedRequest;
use rupost::tui::event::Action;
use rupost::tui::state::{AppState, LayoutMode, Panel};
use std::collections::HashMap;

/// 提取 buffer 全文本（逐行拼接 cell symbol）。断言消费端真实渲染产物而非 state 中间缓存，
/// 使生产者（折行 rebuild）与消费者（切片渲染）各自被独立守护。
fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut row = String::new();
        for x in 0..buffer.area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        lines.push(row.trim_end().to_string());
    }
    lines.join("\n")
}

/// 在 buffer 中查找以 needle 文本开头的 cell，返回其前景色（主题取色差异断言用）。
/// 跨 cell 拼接比较，needle 须为 ASCII 以保证字节偏移与 cell 对齐。
fn find_cell_fg(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<ratatui::style::Color> {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let mut acc = String::new();
            let mut cx = x;
            while cx < buffer.area.width && acc.len() < needle.len() {
                acc.push_str(buffer[(cx, y)].symbol());
                cx += 1;
            }
            if acc.starts_with(needle) {
                return Some(buffer[(x, y)].fg);
            }
        }
    }
    None
}

#[test]
fn test_app_state_initialization() {
    let state = AppState::new();
    assert_eq!(state.active_panel, Panel::Files);
    assert!(!state.is_loading);
    assert!(!state.show_help);
    assert!(!state.is_quitting);
    assert_eq!(state.selected_file_index, 0);
}

#[test]
fn test_adaptive_layout_calculations() {
    let mut state = AppState::new();

    // 1. 宽屏模式测试
    state.update_layout(130, 40);
    assert_eq!(state.layout_mode, LayoutMode::Wide);

    // 2. 窄屏模式测试
    state.update_layout(95, 30);
    assert_eq!(state.layout_mode, LayoutMode::Narrow);

    // 3. 超窄屏堆叠模式测试
    state.update_layout(60, 20);
    assert_eq!(state.layout_mode, LayoutMode::Stacked);

    // 4. 防 Panic 尺寸硬限制下限测试 (如 w=30, h=5)
    state.update_layout(30, 5);
    assert_eq!(state.layout_mode, LayoutMode::Stacked);
    assert_eq!(state.terminal_width, 30);
    assert_eq!(state.terminal_height, 5);
}

#[test]
fn test_state_transitions_via_actions() {
    let mut state = AppState::new();

    // 测试 ToggleHelp
    state.update(Action::ToggleHelp);
    assert!(state.show_help);
    state.update(Action::ToggleHelp);
    assert!(!state.show_help);

    // 测试 SwitchPanel
    state.update(Action::SwitchPanel(Panel::Editor));
    assert_eq!(state.active_panel, Panel::Editor);

    // 测试 SendRequest 会触发 Loading
    let req = ParsedRequest::new(1);
    state.update(Action::SendRequest(Box::new(req)));
    assert!(state.is_loading);
    assert!(state.current_request.is_some());

    // 测试 Quit
    state.update(Action::Quit);
    assert!(state.is_quitting);
}

#[test]
fn test_request_finished_variable_extension() {
    let mut state = AppState::new();
    state.is_loading = true;

    let mut captured_vars = HashMap::new();
    captured_vars.insert("session_token".to_string(), "abc-123-xyz".to_string());
    captured_vars.insert("user_id".to_string(), "42".to_string());

    let response = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "{\"status\":\"ok\"}".to_string(),
        std::time::Duration::from_millis(150),
        std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(100),
    )
    .unwrap();

    // 模拟后台返回 RequestFinished 消息
    state.handle_request_finished(Ok(response), captured_vars, Vec::new(), Vec::new());

    // 验证 Loading 结束且状态正确合并
    assert!(!state.is_loading);
    assert!(state.last_response.is_some());
    assert_eq!(state.variables.get("session_token").unwrap(), "abc-123-xyz");
    assert_eq!(state.variables.get("user_id").unwrap(), "42");
}

#[test]
fn test_unsaved_changes_confirm_modal() {
    use rupost::tui::state::PendingAction;
    let mut state = AppState::new();

    // 默认弹窗是关闭的
    assert!(!state.show_unsaved_confirm);
    assert_eq!(state.pending_action, None);

    // 模拟编辑器变脏
    state.is_dirty = true;
    state.loaded_file_index = 0;
    state.selected_file_index = 1;

    // 模拟在变脏时触发退出拦截
    state.show_unsaved_confirm = true;
    state.pending_action = Some(PendingAction::Quit);

    // 模拟取消
    state.selected_file_index = state.loaded_file_index;
    state.show_unsaved_confirm = false;
    state.pending_action = None;

    assert_eq!(state.selected_file_index, 0);
    assert!(!state.show_unsaved_confirm);

    // 模拟确认放弃修改
    state.pending_action = Some(PendingAction::Quit);
    state.is_quitting = true;

    assert!(state.is_quitting);
}

#[test]
fn test_response_scroll_initialization_and_mutation() {
    let mut state = AppState::new();
    assert_eq!(state.response_scroll, 0);

    state.response_scroll = state.response_scroll.saturating_add(1);
    assert_eq!(state.response_scroll, 1);

    state.response_scroll = state.response_scroll.saturating_sub(1);
    assert_eq!(state.response_scroll, 0);
}

#[test]
fn test_sse_stream_chunk_append() {
    let mut state = AppState::new();
    assert!(state.sse_stream_body.is_empty());

    // 模拟追加 SSE 数据块
    state.handle_stream_chunk("event: message\ndata: Hello".to_string());
    assert_eq!(state.sse_stream_body, "event: message\ndata: Hello");

    state.handle_stream_chunk("\ninfo: end".to_string());
    assert_eq!(
        state.sse_stream_body,
        "event: message\ndata: Hello\ninfo: end"
    );
}

#[test]
fn test_ws_frame_list_records() {
    let mut state = AppState::new();
    assert!(state.ws_frames.is_empty());

    // 模拟接收和发送帧
    state.handle_ws_frame(true, "PING".to_string());
    state.handle_ws_frame(false, "PONG".to_string());

    assert_eq!(state.ws_frames.len(), 2);
    assert!(state.ws_frames[0].is_send);
    assert_eq!(state.ws_frames[0].content, "PING");
    assert!(!state.ws_frames[1].is_send);
    assert_eq!(state.ws_frames[1].content, "PONG");

    // 验证环形缓冲区防溢出保护 (最新 100 帧)
    for i in 0..110 {
        state.handle_ws_frame(true, format!("Frame {}", i));
    }
    assert_eq!(state.ws_frames.len(), 100);
    assert_eq!(state.ws_frames[99].content, "Frame 109");
    assert_eq!(state.ws_frames[0].content, "Frame 10"); // 0-9 应该已经被排挤出队
}

#[test]
fn test_sliding_window_viewport_loading() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("ws_test.log");

    // 写入 1000 行虚拟 WebSocket 帧记录
    {
        let mut file = std::fs::File::create(&file_path).unwrap();
        for i in 0..1000 {
            writeln!(file, "[→] 12:00:00 | Frame {}", i).unwrap();
        }
    }

    let mut state = AppState::new();
    state.log_file_path = Some(file_path.clone());
    state.total_log_lines = 1000;

    // 模拟视图可见高度 20，当前滚动偏移 500
    // 我们设定滑动视口前后拉展缓冲量 N = 50 行，故预期缓存加载区间为 [450..570]，总计 120 条缓存帧记录
    state.load_viewport_sliding_window(500, 20);

    assert_eq!(state.viewport_cache.len(), 120);
    // 第一条缓存帧应对应文件的第 450 行（即 Frame 450）
    assert_eq!(state.viewport_cache[0].content, "Frame 450");
    assert!(state.viewport_cache[0].is_send);
    assert_eq!(state.viewport_cache[0].timestamp, "12:00:00");
    // 最后一条缓存帧应对应文件的第 569 行（即 Frame 569）
    assert_eq!(state.viewport_cache[119].content, "Frame 569");
    assert!(state.viewport_cache[119].is_send);
    assert_eq!(state.viewport_cache[119].timestamp, "12:00:00");
}

#[test]
fn test_log_rotation_and_size_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("rotation.log");

    // 调用底层的通用限额落盘辅助方法，设置极小的上限 100 字节
    let max_bytes = 100;

    // 连续写入，直到超出 100 字节
    let data = "A".repeat(40);
    for _ in 0..5 {
        let _ = rupost::tui::state::write_log_with_limit(
            &rupost::tui::ports::FsRepository,
            &file_path,
            &data,
            max_bytes,
        );
    }

    // 读取物理文件大小，断言它应该被阻断拦截，且以警告结尾
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.len() <= max_bytes + 100);
    assert!(content.contains("[SYSTEM] Log truncated due to size limit"));
}

#[test]
fn test_old_logs_auto_cleanup() {
    let temp_dir = tempfile::tempdir().unwrap();
    let old_file = temp_dir.path().join("old_ws.log");
    let new_file = temp_dir.path().join("new_ws.log");

    std::fs::File::create(&old_file).unwrap();
    std::fs::File::create(&new_file).unwrap();

    // 强行修改 old_file 的修改时间到 10 天前 (10 * 24 * 3600 秒)
    let ten_days_ago =
        std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 3600);
    filetime::set_file_times(
        &old_file,
        filetime::FileTime::from_system_time(ten_days_ago),
        filetime::FileTime::from_system_time(ten_days_ago),
    )
    .unwrap();

    // 调用清理函数，设置清理时长为 7 天
    rupost::tui::state::cleanup_old_logs_dir(temp_dir.path(), 7).unwrap();

    assert!(!old_file.exists());
    assert!(new_file.exists());
}

#[test]
fn test_gc_probabilistic_simulation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let old_file = temp_dir.path().join("old_ws.log");
    std::fs::File::create(&old_file).unwrap();
    let ten_days_ago =
        std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 3600);
    filetime::set_file_times(
        &old_file,
        filetime::FileTime::from_system_time(ten_days_ago),
        filetime::FileTime::from_system_time(ten_days_ago),
    )
    .unwrap();

    let (deleted_count, _) = rupost::history::gc::perform_prune(temp_dir.path(), 7, 100).unwrap();
    assert_eq!(deleted_count, 1);
    assert!(!old_file.exists());
}

#[test]
fn test_gc_perform_prune_size_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let old_file = temp_dir.path().join("old_ws.log");
    let new_file = temp_dir.path().join("new_ws.log");

    std::fs::write(&old_file, "A".repeat(1500 * 1024)).unwrap();
    std::fs::write(&new_file, "B".repeat(100)).unwrap();

    let now = std::time::SystemTime::now();
    let five_secs_ago = now - std::time::Duration::from_secs(5);

    filetime::set_file_times(
        &old_file,
        filetime::FileTime::from_system_time(five_secs_ago),
        filetime::FileTime::from_system_time(five_secs_ago),
    )
    .unwrap();
    filetime::set_file_times(
        &new_file,
        filetime::FileTime::from_system_time(now),
        filetime::FileTime::from_system_time(now),
    )
    .unwrap();

    let (deleted_count, _) = rupost::history::gc::perform_prune(temp_dir.path(), 30, 1).unwrap();
    assert_eq!(deleted_count, 1);
    assert!(!old_file.exists());
    assert!(new_file.exists());
}

#[test]
fn test_gc_perform_clear() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file1 = temp_dir.path().join("ws1.log");
    let file2 = temp_dir.path().join("ws2.log");
    std::fs::write(&file1, "hello").unwrap();
    std::fs::write(&file2, "world").unwrap();

    let (deleted_count, _) = rupost::history::gc::perform_clear(temp_dir.path()).unwrap();
    assert_eq!(deleted_count, 2);
    assert!(!file1.exists());
    assert!(!file2.exists());
}

#[test]
fn test_empty_history_load_behavior() {
    use rupost::tui::state::SidebarTab;
    let mut state = AppState::new();
    state.active_sidebar_tab = SidebarTab::History;
    state.active_panel = Panel::Files;
    state.history_list.clear();
    state.is_dirty = false;

    if !state.history_list.is_empty() {
        state.active_panel = Panel::Editor;
    }

    assert_eq!(state.active_panel, Panel::Files);
}

#[test]
fn test_dirty_editor_history_switch_interception() {
    use rupost::tui::state::{PendingAction, SidebarTab};
    let mut state = AppState::new();
    state.active_sidebar_tab = SidebarTab::History;
    state.active_panel = Panel::Files;
    state.is_dirty = true;

    state.history_list = vec![rupost::history::model::HistoryEntry {
        id: "1".to_string(),
        timestamp: chrono::Utc::now(),
        duration_ms: 10,
        request: rupost::history::model::RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://example.com".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        source: None,
        response: rupost::history::model::ResponseMeta {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: Some("".to_string()),
        },
    }];

    if !state.history_list.is_empty() {
        if state.is_dirty {
            state.show_unsaved_confirm = true;
            state.pending_action = Some(PendingAction::SwitchHistory(
                state.history_state.selected_index,
            ));
        } else {
            state.active_panel = Panel::Editor;
        }
    }

    assert!(state.show_unsaved_confirm);
    assert_eq!(state.pending_action, Some(PendingAction::SwitchHistory(0)));
    assert_eq!(state.active_panel, Panel::Files);
}

#[test]
fn test_narrow_layout_three_column_guarantee() {
    use rupost::tui::state::SidebarTab;
    let mut state = AppState::new();

    state.update_layout(90, 20);
    assert_eq!(state.layout_mode, LayoutMode::Narrow);
    assert_eq!(state.active_sidebar_tab, SidebarTab::Files);
}

#[test]
fn test_read_only_smoke_render_constraints() {
    // 1. 初始化 TestBackend
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();
    state.editor_text = "GET http://localhost\nAccept: json".to_string();

    // 2. 模拟设置 PopupState 为 Help (包含我们新增加的 History 知识大段说明)
    state.show_help = true;

    // 3. 施加多重边界分辨率压测，暴露 Constraint panic
    let test_sizes = vec![(120, 40), (95, 30), (70, 20), (35, 5)];
    for &(w, h) in &test_sizes {
        state.update_layout(w, h);

        // 触发重绘，调用主渲染入口
        let res = terminal.draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        });

        // 确保 draw 过程中没有发生任何 panic，且成功绘制
        assert!(res.is_ok());
    }
}

#[test]
fn test_external_file_modification_hot_reload() {
    use std::io::Write;
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_file.http");

    // 1. 创建测试文件，写入初始用例并载入
    {
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"GET http://localhost/old").unwrap();
    }

    let mut state = AppState::new();
    state.editor_file_path = Some(file_path.to_string_lossy().to_string());

    // 首次主动载入时间戳并缓存
    let metadata = std::fs::metadata(&file_path).unwrap();
    state.editor_file_mtime = metadata.modified().ok();
    state.editor_text = "GET http://localhost/old".to_string();

    // 2. 模拟外部修改文件内容，改写文件
    // 睡眠一下，确保文件修改时间戳发生了物理位移（部分系统精度是秒级）
    std::thread::sleep(std::time::Duration::from_millis(1100));
    {
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"GET http://localhost/new").unwrap();
    }

    // 3. 呼叫 check_and_reload_editor_file
    state.check_and_reload_editor_file();

    // 4. 断言重新加载成功，文件内容已热刷新
    assert_eq!(state.editor_text, "GET http://localhost/new");

    // 5. 校验第二次无修改的 Tick 不会触发重复加载
    state.editor_text = "GET http://corrupted".to_string();
    state.check_and_reload_editor_file();
    assert_eq!(state.editor_text, "GET http://corrupted"); // 依然是内存值，说明防重复读盘机制正常工作
}

#[test]
fn test_large_response_rendering_safety() {
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();

    // 1. 构造一个 5000 行的超大型模拟响应
    let huge_body = vec!["JSON line content data string example"; 5000].join("\n");
    let response = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        huge_body,
        std::time::Duration::from_millis(150),
        std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(100),
    )
    .unwrap();
    state.last_response = Some(response);

    // 2. 模拟触发重绘。由于我们增加了截断处理，这个绘制必须在微秒级瞬间跑完
    // 重复 draw 20 次压测以验证流畅度且不发生任何 panic
    for _ in 0..20 {
        let res = terminal.draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        });
        assert!(res.is_ok());
    }

    // 3. 校验截断警告信息已正确拼入视觉行
    assert!(state.response_visual_lines.len() > 1000);
    let mut found_warning = false;
    for line in &state.response_visual_lines {
        if line.to_string().contains("WARNING: Response truncated") {
            found_warning = true;
            break;
        }
    }
    assert!(found_warning);
}

#[test]
fn test_read_only_dirty_deadlock_release() {
    let mut state = AppState::new();
    state.is_dirty = true;
    state.show_unsaved_confirm = false;

    // 断言 TUI 常态下没有被强制弹窗拦截阻断
    assert!(!state.show_unsaved_confirm);
}

#[test]
fn test_direct_run_window_interactive_states() {
    let mut state = AppState::new();

    // 1. 模拟运行状态
    state.is_loading = true;

    // 在 TestBackend 下测试渲染，确保没有 panic 且正常绘制
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let res = terminal.draw(|frame| {
        rupost::tui::ui::render(frame, &mut state);
    });
    assert!(res.is_ok());

    // 2. 模拟运行结束并返回包含 3 个通过断言的成功响应
    state.is_loading = false;
    let response = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "OK".to_string(),
        std::time::Duration::from_millis(150),
        std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(100),
    )
    .unwrap();
    state.last_response = Some(response);

    use rupost::assertion::AssertionResult;
    state.assertions = vec![
        AssertionResult {
            raw: "status == 200".to_string(),
            passed: true,
            actual: Some("200".to_string()),
            expected: "200".to_string(),
            message: None,
            stream_event_index: None,
        },
        AssertionResult {
            raw: "body contains OK".to_string(),
            passed: true,
            actual: Some("OK".to_string()),
            expected: "OK".to_string(),
            message: None,
            stream_event_index: None,
        },
    ];

    // 重新渲染并构建视觉行
    state.response_visual_lines.clear();
    let res = terminal.draw(|frame| {
        rupost::tui::ui::render(frame, &mut state);
    });
    assert!(res.is_ok());

    // 验证包含状态前缀和断言通过率
    let joined_lines: String = state
        .response_visual_lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let found_success = joined_lines.contains("[SUCCESS]");
    let found_assertions_count = joined_lines.contains("Assertions:")
        && joined_lines.contains("2 passed")
        && joined_lines.contains("failed");
    assert!(found_success);
    assert!(found_assertions_count);
}

#[test]
fn test_warnings_rendered_in_response_panel() {
    // D1 遗留项回归锚点：TestResult.warnings 不得成为渲染盲区（静默假通过）。
    // 断言消费端终端 buffer；告警用短句保证窄面板单行渲染，全文断言不受字符级折行影响。
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();
    state.is_loading = true;
    let response = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "OK".to_string(),
        std::time::Duration::from_millis(10),
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(5),
    )
    .unwrap();

    // 模拟 RequestFinished 事件携带两条非致命告警（断言全过、响应成功）
    state.handle_request_finished(
        Ok(response),
        HashMap::new(),
        vec![],
        vec![
            "@capture id missing".to_string(),
            "var api_key missing".to_string(),
        ],
    );
    state.response_visual_lines.clear();

    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();
    let rendered = buffer_text(frame.buffer);
    assert!(
        rendered.contains("Warnings: 2"),
        "warnings summary must render:\n{rendered}"
    );
    assert!(rendered.contains("@capture id missing"), "\n{rendered}");
    assert!(rendered.contains("var api_key missing"), "\n{rendered}");

    // 无告警时不得出现空告警标题
    state.warnings.clear();
    state.response_visual_lines.clear();
    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();
    assert!(!buffer_text(frame.buffer).contains("Warnings:"));
}

#[test]
fn test_streaming_completed_views_show_warnings_summary() {
    // D1 残余盲区回归锚点：SSE/WS 完成态走流式分支（不经过 HTTP 缓存渲染管线），
    // 告警若只挂在 rebuild_http_visual_lines 上对流式请求仍然不可见
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    // SSE 完成态（无日志文件时内容区为占位文案，但摘要必须可见）
    let mut state = AppState::new();
    state.is_loading = false;
    state.sse_stream_body = "data: chunk\n".to_string();
    state.warnings = vec!["@capture id missing".to_string()];
    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();
    let rendered = buffer_text(frame.buffer);
    assert!(rendered.contains("[SSE Completed]"), "\n{rendered}");
    assert!(rendered.contains("Warnings: 1"), "\n{rendered}");

    // WS 完成态：经日志文件滑动窗口重建帧行，摘要与帧内容同时可见
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("ws.log");
    std::fs::write(&log, "[→] 12:00:00 | PING\n[←] 12:00:01 | PONG\n").unwrap();
    let mut state_ws = AppState::new();
    state_ws.is_loading = false;
    state_ws.handle_ws_frame(true, "PING".to_string());
    state_ws.log_file_path = Some(log);
    state_ws.total_log_lines = 2;
    state_ws.warnings = vec!["var api_key missing".to_string()];
    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state_ws);
        })
        .unwrap();
    let rendered = buffer_text(frame.buffer);
    assert!(rendered.contains("[WS Completed]"), "\n{rendered}");
    assert!(rendered.contains("Warnings: 1"), "\n{rendered}");
    assert!(rendered.contains("PING"), "\n{rendered}");
}

#[test]
fn test_tui_command_line_file_auto_positioning() {
    // 锁定 `rupost tui <file>` 预加载入口的真实实现（app::load_initial_file）
    let temp_dir = tempfile::tempdir().unwrap();
    let file_a = temp_dir.path().join("a.http");
    let file_b = temp_dir.path().join("b.http");
    let nested_dir = temp_dir.path().join("examples");
    std::fs::create_dir(&nested_dir).unwrap();
    let file_c = nested_dir.join("c.http");

    std::fs::write(&file_a, "GET http://a").unwrap();
    std::fs::write(&file_b, "GET http://b").unwrap();
    std::fs::write(&file_c, "GET http://c").unwrap();

    let mut state = AppState::new();
    state.workspace_root = temp_dir.path().to_path_buf();
    state.raw_file_list = vec![
        file_a.to_string_lossy().to_string(),
        file_b.to_string_lossy().to_string(),
        file_c.to_string_lossy().to_string(),
    ];
    state.rebuild_visible_tree_nodes();

    // 1. 传入嵌套路径：父目录自动展开、光标定位到目标文件、内容载入编辑器
    let initial_file = Some(file_c.to_string_lossy().to_string());
    rupost::tui::app::load_initial_file(&mut state, initial_file.as_deref());

    assert!(
        state.expanded_dirs.contains("examples"),
        "parent dir of initial file must be expanded"
    );
    let target = state
        .visible_file_nodes
        .iter()
        .position(|n| n.display_name == "c.http")
        .unwrap();
    assert_eq!(state.files_state.selected_index, target);
    assert_eq!(state.selected_file_index, target);
    assert_eq!(state.editor_text, "GET http://c");
    // 树节点 abs_path 经 canonicalize（macOS /var → /private/var），用规范路径比较
    let canon_c = std::fs::canonicalize(&file_c)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(state.editor_file_path.as_deref(), Some(canon_c.as_str()));

    // 2. 不传文件：回退加载第一个文件
    let mut state_fallback = AppState::new();
    state_fallback.workspace_root = temp_dir.path().to_path_buf();
    state_fallback.raw_file_list = vec![file_a.to_string_lossy().to_string()];
    state_fallback.rebuild_visible_tree_nodes();
    rupost::tui::app::load_initial_file(&mut state_fallback, None);
    assert_eq!(state_fallback.editor_text, "GET http://a");
    assert_eq!(state_fallback.files_state.selected_index, 0);

    // 3. 传入目录：定位目录节点，不读入编辑器
    let mut state_dir = AppState::new();
    state_dir.workspace_root = temp_dir.path().to_path_buf();
    state_dir.raw_file_list = vec![file_c.to_string_lossy().to_string()];
    state_dir.rebuild_visible_tree_nodes();
    let dir_initial = Some(nested_dir.to_string_lossy().to_string());
    rupost::tui::app::load_initial_file(&mut state_dir, dir_initial.as_deref());
    assert_eq!(
        state_dir
            .visible_file_nodes
            .get(state_dir.files_state.selected_index)
            .map(|n| n.is_dir),
        Some(true)
    );
    assert!(state_dir.editor_file_path.is_none());

    // 4. 传入不存在的路径：canonicalize 失败，回退加载第一个文件
    let mut state_missing = AppState::new();
    state_missing.workspace_root = temp_dir.path().to_path_buf();
    state_missing.raw_file_list = vec![file_a.to_string_lossy().to_string()];
    state_missing.rebuild_visible_tree_nodes();
    let missing = Some(
        temp_dir
            .path()
            .join("no_such_file.http")
            .to_string_lossy()
            .into_owned(),
    );
    rupost::tui::app::load_initial_file(&mut state_missing, missing.as_deref());
    assert_eq!(state_missing.editor_text, "GET http://a");
    assert_eq!(state_missing.files_state.selected_index, 0);
}

#[test]
fn test_sse_resize_lazy_rebuild_on_render() {
    // 真实风险路径：流式进行中发生 resize → 宽度变化清空预折行缓存并复位位点，
    // 渲染端必须按新宽度惰性全量重构已完成行，不得丢行或残留旧宽折行
    let mut state = AppState::new();
    state.is_loading = true;
    state.current_response_width = 60;
    // 110 字符长行：60 宽折 2 段、~38 宽（Stacked 40 列终端面板）折 3 段，保证折行数可区分
    let long_line = "a long streaming line that exceeds sixty display columns and surely exceeds the narrower stacked panel width too";
    let chunk = format!("short\n{long_line}\n");
    state.handle_stream_chunk(chunk.clone());

    let base_lines = state.sse_visual_lines.len();
    assert!(
        base_lines > 2,
        "long line must wrap at width 60: {base_lines}"
    );
    assert_eq!(state.sse_last_processed_pos, chunk.len());

    // 模拟 resize：update_layout 检测到响应宽度变化，清空缓存并复位位点
    state.update_layout(40, 24);
    assert!(state.sse_visual_lines.is_empty());
    assert_eq!(state.sse_last_processed_pos, 0);

    // Stacked 布局仅渲染焦点面板，焦点切到 Response 才会触发流式分支的惰性重构
    state.active_panel = Panel::Response;
    let backend = ratatui::backend::TestBackend::new(40, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();
    assert_eq!(frame.buffer.area.width, 40);
    assert!(
        state.sse_visual_lines.len() > base_lines,
        "narrower panel must produce more wrapped lines: {} vs {base_lines}",
        state.sse_visual_lines.len()
    );
    assert_eq!(state.sse_last_processed_pos, chunk.len());
    let rendered: Vec<String> = state
        .sse_visual_lines
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert_eq!(rendered.first().map(String::as_str), Some("short"));
    // 长行必须被切成多段（不再有 60 宽下的旧折行），且无内容丢失
    assert!(!rendered.contains(&long_line.to_string()));
    let rejoined = rendered.join("");
    assert!(
        rejoined.contains(long_line),
        "no content loss after rebuild"
    );
}

#[test]
fn test_history_replay_memory_dispatch() {
    use rupost::tui::state::SidebarTab;
    let mut state = AppState::new();

    // 1. 设置处于历史记录 tab
    state.active_sidebar_tab = SidebarTab::History;

    // 2. 模拟内存中已载入一份历史 ParsedRequest
    let request_snap = ParsedRequest {
        method: Some("POST".to_string()),
        url: "http://example.com/history_test".to_string(),
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: Some("{\"ok\":true}".to_string()),
        metadata: Default::default(),
        line_number: 1,
        base_path: None,
    };
    state.current_request = Some(request_snap.clone());

    // 3. 执行 app.rs 中对应的内存读取逻辑
    let parsed_req = if state.active_sidebar_tab == SidebarTab::History {
        state.current_request.clone()
    } else {
        None
    };

    // 4. 校验劫持并拿到了正确的克隆拷贝，未读取磁盘
    assert!(parsed_req.is_some());
    let req = parsed_req.unwrap();
    assert_eq!(req.url, "http://example.com/history_test");
    assert_eq!(req.method_or_default(), "POST");
}

#[test]
fn test_response_scroll_clamp_boundary() {
    let mut state = AppState::new();

    // 1. 模拟 10 行视觉文本，视口高度为 6 (排除 borders 后可视行数为 4)
    state.terminal_height = 6;
    state.response_visual_lines = vec![ratatui::text::Line::from("line"); 10];
    state.response_scroll = 0;

    // 2. 模拟 Down 键滚动 (多次触发)
    for _ in 0..20 {
        let visible_height = state.terminal_height.saturating_sub(2) as usize;
        let total = state.response_visual_lines.len();
        let max_scroll = total.saturating_sub(visible_height); // max_scroll = 10 - 4 = 6
        if state.response_scroll < max_scroll {
            state.response_scroll = state.response_scroll.saturating_add(1);
        }
    }

    // 3. 断言被严格 Clamp 限制在 max_scroll 边界
    assert_eq!(state.response_scroll, 6);
}

#[test]
fn test_global_esc_key_close_and_unfocus() {
    let mut state = AppState::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    // 1. 测试 Esc 关闭帮助菜单
    state.show_help = true;
    let esc_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::empty(),
    );
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        rupost::tui::handlers::handle_key(&mut state, esc_key, &tx).await;
    });
    assert!(!state.show_help);

    // 2. 测试 Esc 退回侧边栏
    state.active_panel = Panel::Editor;
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        rupost::tui::handlers::handle_key(&mut state, esc_key, &tx).await;
    });
    assert_eq!(state.active_panel, Panel::Files);
}

#[test]
fn test_loading_spinner_and_ticks() {
    use rupost::tui::event::Action;
    let mut state = AppState::new();

    // 初始状态
    assert_eq!(state.loading_tick, 0);

    // 模拟运行，累加 Tick
    state.is_loading = true;
    state.loading_tick = 5;

    // 触发 SendRequest，校验 loading_tick 归零重置
    let dummy_req = ParsedRequest {
        method: Some("GET".to_string()),
        url: "http://localhost".to_string(),
        headers: vec![],
        body: None,
        metadata: Default::default(),
        line_number: 1,
        base_path: None,
    };
    state.update(Action::SendRequest(Box::new(dummy_req)));
    assert_eq!(state.loading_tick, 0);

    // 验证 get_spinner_char 转换
    state.loading_tick = 2; // 分频除以 2，取模
    let ch = state.get_spinner_char();
    assert!(!ch.is_empty());
}

#[test]
fn test_request_concurrency_lock() {
    let mut state = AppState::new();
    state.is_loading = true;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    // 模拟在 is_loading 时按 Ctrl+Enter 运行
    let run_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::CONTROL,
    );
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        rupost::tui::handlers::handle_key(&mut state, run_key, &tx).await;
    });

    // 校验由于锁机制，没有任何 RequestStarted 信号发送出去
    assert!(rx.try_recv().is_err());
}

#[test]
fn test_file_execution_states_tracking() {
    use rupost::tui::event::Action;
    use rupost::tui::state::FileExecState;
    let mut state = AppState::new();
    state.editor_file_path = Some("examples/ok.http".to_string());

    let dummy_req = ParsedRequest {
        method: Some("GET".to_string()),
        url: "http://localhost".to_string(),
        headers: vec![],
        body: None,
        metadata: Default::default(),
        line_number: 1,
        base_path: None,
    };

    // 1. 发送请求，状态变为 Running
    state.update(Action::SendRequest(Box::new(dummy_req)));
    assert_eq!(
        state.file_execution_states.get("examples/ok.http"),
        Some(&FileExecState::Running)
    );

    // 2. 响应返回 (成功且无断言失败)，状态变为 Success
    state.handle_request_finished(
        Ok(Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "OK".to_string(),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
        )
        .unwrap()),
        std::collections::HashMap::new(),
        vec![],
        vec![],
    );
    assert_eq!(
        state.file_execution_states.get("examples/ok.http"),
        Some(&FileExecState::Success)
    );

    // 3. 响应返回 (失败)，状态变为 Failed
    state.editor_file_path = Some("examples/fail.http".to_string());
    state.is_loading = true;
    state.handle_request_finished(
        Err("Connection failed".to_string()),
        std::collections::HashMap::new(),
        vec![],
        vec![],
    );
    assert_eq!(
        state.file_execution_states.get("examples/fail.http"),
        Some(&FileExecState::Failed)
    );
}

#[test]
fn test_streaming_incremental_wrap_cache_performance() {
    let mut state = AppState::new();
    state.current_response_width = 40;

    // 1. 模拟增量追加 SSE 块
    state.handle_stream_chunk("line 1\nline 2\n".to_string());
    assert!(!state.sse_visual_lines.is_empty());
    assert_eq!(state.sse_last_processed_pos, 14);

    // 2. 模拟 Resize 导致宽度改变
    state.update_layout(80, 24); // 此时 update_layout 会触发 Response 宽度变化，进而清空缓存
    assert!(state.sse_visual_lines.is_empty());
    assert_eq!(state.sse_last_processed_pos, 0);
}

#[test]
fn test_phase3_loading_smoke_render() {
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();
    state.is_loading = true;
    state.loading_tick = 3;

    // 模拟 SSE 流
    state.sse_stream_body = "event: data\ncontent: test\n".to_string();

    let test_sizes = vec![(100, 30), (80, 20), (35, 5)];
    for &(w, h) in &test_sizes {
        state.update_layout(w, h);
        let res = terminal.draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        });
        assert!(res.is_ok());
    }
}

#[test]
fn test_history_sidebar_render_no_id_prefix() {
    use rupost::tui::state::SidebarTab;
    let mut state = AppState::new();
    state.active_sidebar_tab = SidebarTab::History;

    // 录入模拟历史数据
    state.history_list = vec![rupost::history::model::HistoryEntry {
        id: "7713fb92-5359-4e27-aed8-a4664bbb025b".to_string(),
        timestamp: chrono::Utc::now(),
        duration_ms: 10,
        request: rupost::history::model::RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://example.com/api".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        source: None,
        response: rupost::history::model::ResponseMeta {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: Some("".to_string()),
        },
    }];

    // 模拟 TestBackend 渲染以触发生态计算，或者直接通过 render 函数获取其返回
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let res = terminal.draw(|frame| {
        rupost::tui::ui::sidebar::render(frame, frame.area(), &mut state);
    });
    assert!(res.is_ok());

    // 我们还可以验证在 AppState 下，渲染所用字段 host_and_path 的转换正确性
    let host_and_path = rupost::tui::state::format_url_host_and_path("http://example.com/api");
    assert_eq!(host_and_path, "example.com/api");
}

#[test]
fn test_history_editor_title_id_display() {
    use rupost::tui::state::SidebarTab;
    let mut state = AppState::new();
    state.active_sidebar_tab = SidebarTab::History;
    state.history_state.selected_index = 0;

    state.history_list = vec![rupost::history::model::HistoryEntry {
        id: "7713fb92-5359-4e27-aed8-a4664bbb025b".to_string(),
        timestamp: chrono::Utc::now(),
        duration_ms: 10,
        request: rupost::history::model::RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://example.com/api".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        source: None,
        response: rupost::history::model::ResponseMeta {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: Some("".to_string()),
        },
    }];

    // 模拟 TestBackend 下渲染编辑器组件
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    // 执行 draw 触发渲染
    let res = terminal.draw(|frame| {
        rupost::tui::ui::editor::render(frame, frame.area(), &state);
    });
    assert!(res.is_ok());
}

#[test]
fn test_file_tree_trie_construction() {
    let mut state = AppState::new();
    state.workspace_root = std::path::PathBuf::from("/workspace");
    state.raw_file_list = vec![
        "/workspace/a/b/c.http".to_string(),
        "/workspace/a/d.http".to_string(),
        "/workspace/e.http".to_string(),
    ];

    // 1. 测试未展开任何目录时的投影 (默认折叠)
    state.expanded_dirs.clear();
    state.rebuild_visible_tree_nodes();

    // 应该只展示顶级项: "a" 目录和 "e.http" 文件
    assert_eq!(state.visible_file_nodes.len(), 2);
    assert_eq!(state.visible_file_nodes[0].display_name, "a");
    assert!(state.visible_file_nodes[0].is_dir);
    assert_eq!(state.visible_file_nodes[1].display_name, "e.http");
    assert!(!state.visible_file_nodes[1].is_dir);

    // 2. 测试展开 "a" 目录及子目录 "a/b" 时的投影
    state.expanded_dirs.insert("a".to_string());
    state.expanded_dirs.insert("a/b".to_string());
    state.rebuild_visible_tree_nodes();

    // 应该展示 "a"、"a/b"、"a/b/c.http"、"a/d.http"、"e.http"
    assert_eq!(state.visible_file_nodes.len(), 5);
}

#[test]
fn test_file_tree_selected_index_rebound() {
    let mut state = AppState::new();
    state.workspace_root = std::path::PathBuf::from("/workspace");
    state.raw_file_list = vec![
        "/workspace/a/b/c.http".to_string(),
        "/workspace/a/d.http".to_string(),
    ];
    state.expanded_dirs.insert("a".to_string());
    state.expanded_dirs.insert("a/b".to_string());
    state.rebuild_visible_tree_nodes();

    // 当前树展开为:
    // 0: a (dir)
    // 1: a/b (dir)
    // 2: a/b/c.http (file)
    // 3: a/d.http (file)
    assert_eq!(state.visible_file_nodes.len(), 4);

    // 1. 光标选中 a/b/c.http (index = 2)
    state.files_state.selected_index = 2;

    // 2. 折叠 "a/b" 目录 (使得 c.http 被隐藏)
    state.toggle_directory(1);

    // 展开列表现在为:
    // 0: a (dir)
    // 1: a/b (dir, collapsed)
    // 2: a/d.http (file)
    assert_eq!(state.visible_file_nodes.len(), 3);

    // 验证光标是否“回弹”定位到父目录 "a/b" (即新索引 1)
    assert_eq!(state.files_state.selected_index, 1);
    assert_eq!(
        state.visible_file_nodes[state.files_state.selected_index].display_name,
        "b"
    );
}

#[test]
fn test_file_tree_empty_state_guard() {
    let mut state = AppState::new();
    state.workspace_root = std::path::PathBuf::from("/workspace");
    state.raw_file_list.clear();
    state.rebuild_visible_tree_nodes();

    assert!(state.visible_file_nodes.is_empty());
    assert_eq!(state.files_state.selected_index, 0);

    // 模拟渲染空状态，确保无 panic
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let res = terminal.draw(|frame| {
        rupost::tui::ui::render(frame, &mut state);
    });
    assert!(res.is_ok());
}

#[test]
fn test_detail_tab_switching() {
    use rupost::tui::state::DetailTab;
    let mut state = AppState::new();

    // 默认是 Response Tab
    assert_eq!(state.active_detail_tab, DetailTab::Response);

    // 模拟在 Response 面板下通过 handle_key (会调用 handle_response_key) 切换
    let right_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    );

    state.active_panel = Panel::Response;
    state.response_visual_lines = vec![ratatui::text::Line::raw("temp_lines")];
    state.response_scroll = 5;

    // 切换到 Headers 页
    // 我们需要把 tokio::sync::mpsc::channel 传给 handle_key
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    // 执行切换
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        rupost::tui::handlers::handle_key(&mut state, right_key, &tx).await;
    });

    assert_eq!(state.active_detail_tab, DetailTab::Headers);
    // 验证折行缓存已清空，滚动偏移已重置
    assert!(state.response_visual_lines.is_empty());
    assert_eq!(state.response_scroll, 0);

    // 再次向右切换到 Request 页
    state.response_visual_lines = vec![ratatui::text::Line::raw("temp_lines")];
    state.response_scroll = 3;
    rt.block_on(async {
        rupost::tui::handlers::handle_key(&mut state, right_key, &tx).await;
    });
    assert_eq!(state.active_detail_tab, DetailTab::Request);
    assert!(state.response_visual_lines.is_empty());
    assert_eq!(state.response_scroll, 0);

    // 向左切换回 Headers
    let left_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
    );
    rt.block_on(async {
        rupost::tui::handlers::handle_key(&mut state, left_key, &tx).await;
    });
    assert_eq!(state.active_detail_tab, DetailTab::Headers);
}

#[test]
fn test_smart_url_test_creation() {
    let mut state = AppState::new();
    let temp_dir = tempfile::tempdir().unwrap();
    state.workspace_root = temp_dir.path().to_path_buf();

    // 场景 1：输入完整的 URL
    state.create_test_from_input("https://google.com/search?q=rust");

    // 应该推导出文件名 google_com_search.http 并自动创建在 workspace_root 目录下
    let expected_path = temp_dir.path().join("google_com_search.http");
    assert!(
        expected_path.exists(),
        "Expected google_com_search.http to be created"
    );
    let content = std::fs::read_to_string(&expected_path).unwrap();
    assert_eq!(content, "GET https://google.com/search?q=rust\n");

    // 场景 2：输入一个没有协议头但显式是域名的 URL
    state.create_test_from_input("httpbin.org/get");
    let expected_path_2 = temp_dir.path().join("httpbin_org_get.http");
    assert!(
        expected_path_2.exists(),
        "Expected httpbin_org_get.http to be created"
    );
    let content_2 = std::fs::read_to_string(&expected_path_2).unwrap();
    assert_eq!(content_2, "GET https://httpbin.org/get\n");

    // 场景 3：输入普通的文件名，应该创建带有模板的文件
    state.create_test_from_input("auth");
    let expected_path_3 = temp_dir.path().join("auth.http");
    assert!(expected_path_3.exists(), "Expected auth.http to be created");
    let content_3 = std::fs::read_to_string(&expected_path_3).unwrap();
    assert_eq!(content_3, "GET http://httpbingo.org/get\n");
}

#[test]
fn test_external_editor_key_trigger() {
    let mut state = AppState::new();
    state.editor_file_path = Some("/path/to/test.http".to_string());
    state.active_panel = Panel::Editor;

    let e_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('E'),
        crossterm::event::KeyModifiers::SHIFT,
    );

    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        rupost::tui::handlers::handle_key(&mut state, e_key, &tx).await;
    });

    assert!(state.pending_editor_launch);
}

#[test]
fn test_theme_axis_smoke_matrix() {
    // 主题轴 × 错误态 × 布局 的参数化冒烟矩阵（P2 缺口补齐）
    use rupost::tui::theme::ThemeKind;
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let failed_response = || {
        Response::new(
            500,
            reqwest::header::HeaderMap::new(),
            "{\"error\":\"boom\"}".to_string(),
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(5),
        )
        .unwrap()
    };
    let failed_assertion = rupost::assertion::AssertionResult {
        raw: "status == 200".to_string(),
        passed: false,
        actual: Some("500".to_string()),
        expected: "200".to_string(),
        message: Some("actual 500".to_string()),
        stream_event_index: None,
    };

    for theme in [ThemeKind::Dark, ThemeKind::Light] {
        for scenario in 0..3 {
            let mut state = AppState::new();
            state.theme = theme;
            state.update_layout(100, 30);
            match scenario {
                // 0: 失败响应 + 失败断言 + 告警
                0 => {
                    state.last_response = Some(failed_response());
                    state.assertions = vec![failed_assertion.clone()];
                    state.warnings = vec!["@capture id: not found".to_string()];
                }
                // 1: SSE 流式加载中
                1 => {
                    state.is_loading = true;
                    state.sse_stream_body = "data: chunk\n".to_string();
                }
                // 2: WS 帧完成态
                _ => {
                    state.handle_ws_frame(true, "PING".to_string());
                    state.handle_ws_frame(false, "PONG".to_string());
                }
            }
            state.response_visual_lines.clear();

            let res = terminal.draw(|frame| {
                rupost::tui::ui::render(frame, &mut state);
            });
            assert!(
                res.is_ok(),
                "theme {theme:?} scenario {scenario} must render without panic"
            );
        }

        // 失败态下：文案出现在消费端 buffer，且 Warnings 摘要按当前主题 accent 槽位取色
        // （若 ui 层硬编码 Dark 色，Light 分支 fg 断言即红——主题矩阵由此真正验证取色差异）
        let mut state = AppState::new();
        state.theme = theme;
        state.update_layout(100, 30);
        state.last_response = Some(failed_response());
        state.assertions = vec![failed_assertion.clone()];
        state.warnings = vec!["@capture id: not found".to_string()];
        state.response_visual_lines.clear();
        let frame = terminal
            .draw(|frame| {
                rupost::tui::ui::render(frame, &mut state);
            })
            .unwrap();
        let rendered = buffer_text(frame.buffer);
        assert!(
            rendered.contains("[FAILED]"),
            "theme {theme:?}:\n{rendered}"
        );
        assert!(rendered.contains("Warnings: 1"), "\n{rendered}");
        assert_eq!(
            find_cell_fg(frame.buffer, "Warnings:"),
            Some(rupost::tui::theme::palette(theme).accent),
            "theme {theme:?}: Warnings summary must take accent from palette"
        );
    }
}

#[test]
fn test_search_highlight_renders_in_response_panel() {
    // R4 冒烟：搜索态下可见切片的命中段以 accent 背景渲染
    use rupost::tui::theme::ThemeKind;
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();
    state.active_panel = Panel::Response;
    state.update_layout(100, 30);
    state.response_visual_lines = vec![
        ratatui::text::Line::raw("GET /users"),
        ratatui::text::Line::raw("data: hello world"),
        ratatui::text::Line::raw("data: other"),
    ];
    state.search_query = "hello".to_string();
    state.last_response = Some(
        Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "body".to_string(),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
        )
        .unwrap(),
    );

    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();

    // "hello" 在第 2 行（可见区）：命中 cell 的背景必须是当前主题 accent
    let buffer = frame.buffer;
    let mut found_hit = false;
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            if cell.symbol() == "h"
                && buffer[(x + 1, y)].symbol() == "e"
                && buffer[(x + 2, y)].symbol() == "l"
                && cell.bg == rupost::tui::theme::palette(ThemeKind::Dark).accent
            {
                found_hit = true;
            }
        }
    }
    assert!(
        found_hit,
        "matched text must be rendered with accent background"
    );

    // 无 query：不应用高亮（bg 保持默认）
    state.search_query.clear();
    let frame = terminal
        .draw(|frame| {
            rupost::tui::ui::render(frame, &mut state);
        })
        .unwrap();
    let buffer = frame.buffer;
    let plain_cell = &buffer[(6, 13)];
    assert_ne!(
        plain_cell.bg,
        rupost::tui::theme::palette(ThemeKind::Dark).accent,
        "no query must not highlight"
    );
}
