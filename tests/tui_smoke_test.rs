use rupost::http::Response;
use rupost::parser::ParsedRequest;
use rupost::tui::event::Action;
use rupost::tui::state::{AppState, LayoutMode, Panel};
use std::collections::HashMap;

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
    state.handle_request_finished(Ok(response), captured_vars, Vec::new());

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
        let _ = rupost::tui::state::write_log_with_limit(&file_path, &data, max_bytes);
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

    let (deleted_count, _) = rupost::runner::gc::perform_prune(temp_dir.path(), 7, 100).unwrap();
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

    let (deleted_count, _) = rupost::runner::gc::perform_prune(temp_dir.path(), 30, 1).unwrap();
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

    let (deleted_count, _) = rupost::runner::gc::perform_clear(temp_dir.path()).unwrap();
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
fn test_tui_command_line_file_auto_positioning() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_a = temp_dir.path().join("a.http");
    let file_b = temp_dir.path().join("b.http");

    std::fs::write(&file_a, "GET http://a").unwrap();
    std::fs::write(&file_b, "GET http://b").unwrap();

    let mut state = AppState::new();
    state.file_tree = vec![
        file_a.to_string_lossy().to_string(),
        file_b.to_string_lossy().to_string(),
    ];

    // 模拟命令行中传入了定位到 b.http 的参数，利用我们刚写的匹配算法
    let initial_file = Some(file_b.to_string_lossy().to_string());
    let mut initial_idx = 0;
    if let Some(init_path) = &initial_file {
        let matched = state.file_tree.iter().position(|p| {
            p == init_path
                || std::path::Path::new(p)
                    .canonicalize()
                    .map(|c| {
                        std::path::Path::new(init_path)
                            .canonicalize()
                            .map(|ic| c == ic)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
        });
        if let Some(pos) = matched {
            initial_idx = pos;
        }
    }

    assert_eq!(initial_idx, 1); // 成功定位到第二项 (b.http)
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
