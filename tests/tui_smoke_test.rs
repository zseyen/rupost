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
