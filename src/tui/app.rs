use super::event::TuiEvent;
use super::state::AppState;
use crate::Result;
use crossterm::{
    event::{self, Event as CrosstermEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::time::Duration;
use tokio::sync::mpsc;

/// 启动并运行 TUI 主事件循环
pub async fn run(initial_file: Option<String>) -> Result<()> {
    run_async(initial_file).await
}

async fn run_async(initial_file: Option<String>) -> Result<()> {
    // 1. 初始化终端
    enable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )
    .map_err(crate::error::RupostError::IoError)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::error::RupostError::IoError(std::io::Error::other(e.to_string())))?;

    // 2. 建立 MPSC 事件通道
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // 派发 crossterm 事件捕获 Task
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            // 阻断式轮询是否有 crossterm 事件
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx_clone.send(TuiEvent::Input(key)).await.is_err() {
                            break;
                        }
                    }
                    #[allow(clippy::collapsible_match)]
                    Ok(CrosstermEvent::Mouse(mouse)) => {
                        if tx_clone.send(TuiEvent::MouseInput(mouse)).await.is_err() {
                            break;
                        }
                    }
                    #[allow(clippy::collapsible_match)]
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx_clone.send(TuiEvent::Resize(w, h)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            // 内部心跳
            if tx_clone.send(TuiEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    let mut state = AppState::new();
    if let Ok(files) = crate::runner::scanner::DirectoryScanner::scan(&[".".to_string()]) {
        state.raw_file_list = filter_tui_test_files(files);
        // 默认展开所有顶层目录
        let root_canon = std::fs::canonicalize(&state.workspace_root)
            .unwrap_or_else(|_| state.workspace_root.clone());
        for f in &state.raw_file_list {
            if let Ok(canon) = std::path::Path::new(f).canonicalize()
                && let Ok(rel) = canon.strip_prefix(&root_canon)
                && let Some(first_comp) = rel.components().next()
                && let std::path::Component::Normal(name) = first_comp
            {
                let name_str = name.to_string_lossy().into_owned();
                let dir_path = root_canon.join(&name_str);
                if dir_path.is_dir() {
                    state.expanded_dirs.insert(name_str);
                }
            }
        }
        state.rebuild_visible_tree_nodes();
    }
    // 载入历史记录
    state.history_list = crate::history::storage::get_storage()
        .tail(100)
        .unwrap_or_default();
    state.history_list.reverse();

    // 默认加载首个文件或指定的 initial_file
    let mut initial_idx = 0;
    if let Some(init_path) = &initial_file
        && let Ok(canon_init) = std::path::Path::new(init_path).canonicalize()
    {
        let root_canon = std::fs::canonicalize(&state.workspace_root)
            .unwrap_or_else(|_| state.workspace_root.clone());
        if let Ok(rel) = canon_init.strip_prefix(&root_canon) {
            let mut current = std::path::PathBuf::new();
            for comp in rel.components() {
                if let std::path::Component::Normal(name) = comp {
                    let step = current.join(name);
                    let step_abs = root_canon.join(&step);
                    if step_abs.is_dir() {
                        state
                            .expanded_dirs
                            .insert(step.to_string_lossy().replace('\\', "/"));
                    }
                    current = step;
                }
            }
            state.rebuild_visible_tree_nodes();
        }

        let canon_init_str = canon_init.to_string_lossy().into_owned();
        let matched = state.visible_file_nodes.iter().position(|n| {
            if let Ok(c) = std::path::Path::new(&n.abs_path).canonicalize() {
                c.to_string_lossy() == canon_init_str
            } else {
                n.abs_path == canon_init_str
            }
        });
        if let Some(pos) = matched {
            initial_idx = pos;
        }
    }

    if !state.visible_file_nodes.is_empty() && initial_idx < state.visible_file_nodes.len() {
        let first_node = &state.visible_file_nodes[initial_idx];
        if !first_node.is_dir {
            if let Ok(content) = std::fs::read_to_string(&first_node.abs_path) {
                state.editor_text = content;
                state.editor_file_path = Some(first_node.abs_path.clone());
                if let Ok(metadata) = std::fs::metadata(&first_node.abs_path) {
                    state.editor_file_mtime = metadata.modified().ok();
                }
                state.selected_file_index = initial_idx;
                state.files_state.selected_index = initial_idx;
                state.loaded_file_index = initial_idx;
            }
        } else {
            state.files_state.selected_index = initial_idx;
        }
    }

    // 捕获初始尺寸
    if let Ok((w, h)) = crossterm::terminal::size() {
        state.update_layout(w, h);
    }

    // 3. 事件循环主流程
    loop {
        // A. 渲染当前状态
        terminal
            .draw(|frame| {
                super::ui::render(frame, &mut state);
            })
            .map_err(|e| {
                crate::error::RupostError::IoError(std::io::Error::other(e.to_string()))
            })?;

        // B. 接收事件
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(key) => {
                    super::handlers::handle_key(&mut state, key, &event_tx).await;
                }
                TuiEvent::Resize(w, h) => {
                    state.update_layout(w, h);
                }
                TuiEvent::MouseInput(mouse_event) => {
                    super::handlers::handle_mouse(&mut state, mouse_event);
                }
                TuiEvent::RequestFinished {
                    result,
                    captured_vars,
                    assertions,
                    ..
                } => {
                    state.handle_request_finished(*result, captured_vars, assertions);
                    state.response_visual_lines.clear(); // 清空缓存以强迫重新计算预折行
                    state.response_scroll = 0; // 请求结束重置滚动

                    // 从存储重新载入最新 100 条请求历史并倒序
                    state.history_list = crate::history::storage::get_storage()
                        .tail(100)
                        .unwrap_or_default();
                    state.history_list.reverse();
                    state.history_state.selected_index = 0;
                    state.history_state.scroll_offset = 0;
                }
                TuiEvent::StreamChunk { chunk, .. } => {
                    state.handle_stream_chunk(chunk);
                }
                TuiEvent::WsFrame {
                    is_send, content, ..
                } => {
                    state.handle_ws_frame(is_send, content);
                }
                TuiEvent::InitLogPath { path, .. } => {
                    state.log_file_path = Some(std::path::PathBuf::from(path));
                    state.total_log_lines = 0;
                    state.viewport_cache.clear();
                }
                TuiEvent::Tick => {
                    state.check_and_reload_editor_file();
                    if state.is_loading {
                        state.loading_tick = state.loading_tick.wrapping_add(1);
                    }
                }
                _ => {}
            }
        }

        if state.is_quitting {
            break;
        }
    }

    // 4. 清理并还原终端
    disable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .map_err(crate::error::RupostError::IoError)?;
    terminal
        .show_cursor()
        .map_err(|e| crate::error::RupostError::IoError(std::io::Error::other(e.to_string())))?;

    Ok(())
}

/// 对 TUI 中扫描出的所有候选文件路径进行精细化过滤
/// 1. 过滤文件名黑名单：README.md, SUMMARY.md, CHANGELOG.md, AGENTS.md, checkpoint.md, walkthrough.md 等
/// 2. 过滤目录黑名单：.git, .rupost, target, node_modules, .agents, .gemini 等
/// 3. 轻量内容启发式扫描：读取前缀 1024 字节，检查是否是包含有效 HTTP 请求的 http 文件或包含 http 代码块的 md 文件
pub fn filter_tui_test_files(paths: Vec<std::path::PathBuf>) -> Vec<String> {
    // 黑名单目录列表
    let blacklisted_dirs = [
        ".git",
        ".rupost",
        "target",
        "node_modules",
        ".agents",
        ".gemini",
    ];

    // 黑名单文件名列表
    let blacklisted_files = [
        "README.md",
        "readme.md",
        "SUMMARY.md",
        "summary.md",
        "CHANGELOG.md",
        "changelog.md",
        "AGENTS.md",
        "agents.md",
        "checkpoint.md",
        "walkthrough.md",
    ];

    paths
        .into_iter()
        .filter(|path| should_keep_file(path, &blacklisted_dirs, &blacklisted_files))
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 检查单个文件是否应当保留作为测试文件候选
fn should_keep_file(
    path: &std::path::Path,
    blacklisted_dirs: &[&str],
    blacklisted_files: &[&str],
) -> bool {
    // 1. 第一层过滤：路径名及黑名单拦截（零 IO 读盘）
    // 检查是否包含黑名单目录（仅针对相对于当前工作目录的路径组件，防止父级目录包含隐藏目录名称时被误杀）
    let relative_path = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            let canonical_cwd = cwd.canonicalize().ok()?;
            path.strip_prefix(&canonical_cwd).ok()
        })
        .unwrap_or(path);

    let has_blacklisted_dir = blacklisted_dirs
        .iter()
        .any(|&dir| relative_path.components().any(|c| c.as_os_str() == dir));
    if has_blacklisted_dir {
        return false;
    }

    // 检查文件名是否在黑名单中
    if let Some(file_name) = path.file_name() {
        let name_str = file_name.to_string_lossy();
        if blacklisted_files
            .iter()
            .any(|&f| name_str.eq_ignore_ascii_case(f))
        {
            return false;
        }
    }

    // 检查扩展名，避免对 non-http/md 文件执行无谓的磁盘读取操作
    let Some(ext) = path.extension() else {
        return false;
    };
    let ext_str = ext.to_string_lossy();
    let is_http = ext_str.eq_ignore_ascii_case("http");
    let is_md = ext_str.eq_ignore_ascii_case("md");
    if !is_http && !is_md {
        return false;
    }

    // 2. 第二层过滤：轻量级启发式读盘检视
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };

    use std::io::Read;
    let mut buf = [0u8; 1024];
    let Ok(bytes_read) = file.read(&mut buf) else {
        return false;
    };

    let content = String::from_utf8_lossy(&buf[..bytes_read]);

    if is_http {
        content.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("GET ")
                || trimmed.starts_with("POST ")
                || trimmed.starts_with("PUT ")
                || trimmed.starts_with("DELETE ")
                || trimmed.starts_with("PATCH ")
                || trimmed.starts_with("HEAD ")
                || trimmed.starts_with("OPTIONS ")
                || trimmed.starts_with("WEBSOCKET ")
                || trimmed.starts_with("WS ")
        })
    } else {
        // is_md
        content.contains("```http") || content.contains("```rest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_filter_tui_test_files_cases() {
        let dir = tempdir().unwrap();

        // 1. 创建合格的 .http 文件
        let http_ok = dir.path().join("ok.http");
        std::fs::write(&http_ok, "GET http://example.com").unwrap();

        // 2. 创建合格的 .md 文件
        let md_ok = dir.path().join("ok.md");
        std::fs::write(&md_ok, "# Doc\n```http\nPOST /login\n```").unwrap();

        // 3. 创建不合格的普通 README
        let readme = dir.path().join("README.md");
        std::fs::write(&readme, "# README\nThis is just a doc.").unwrap();

        // 4. 创建空 .http 文件
        let empty_http = dir.path().join("empty.http");
        std::fs::write(&empty_http, "   \n  ").unwrap();

        // 5. 创建非 UTF-8 二进制大文件
        let binary_http = dir.path().join("binary.http");
        let mut f = std::fs::File::create(&binary_http).unwrap();
        f.write_all(&[0u8, 159u8, 146u8, 150u8, 0xffu8, 0x00u8])
            .unwrap();

        // 6. 创建黑名单目录下的合格文件
        let node_modules_dir = dir.path().join("node_modules");
        std::fs::create_dir(&node_modules_dir).unwrap();
        let http_node = node_modules_dir.join("test.http");
        std::fs::write(&http_node, "GET http://localhost").unwrap();

        let paths = vec![
            http_ok.clone(),
            md_ok.clone(),
            readme,
            empty_http,
            binary_http,
            http_node,
        ];

        let filtered = filter_tui_test_files(paths);
        assert_eq!(filtered.len(), 2);

        let has_http = filtered.iter().any(|p| p.contains("ok.http"));
        let has_md = filtered.iter().any(|p| p.contains("ok.md"));
        assert!(has_http);
        assert!(has_md);
    }

    #[test]
    fn test_should_keep_file_with_parent_blacklist_leak() {
        let cwd = std::path::Path::new("/Users/user/.gemini/worktrees/project");
        let blacklisted_dirs = [".gemini"];

        // 模拟一个文件，其绝对路径中包含黑名单目录名 ".gemini"，但相对于工作目录不包含该目录
        let file_path =
            std::path::Path::new("/Users/user/.gemini/worktrees/project/examples/ok.http");

        // 1. 模拟旧版 any 逻辑（必会因为绝对路径包含 .gemini 而拦截）
        let has_blacklisted_dir_old = blacklisted_dirs
            .iter()
            .any(|&dir| file_path.components().any(|c| c.as_os_str() == dir));
        assert!(has_blacklisted_dir_old);

        // 2. 模拟新版相对路径逻辑（剔除父级前缀，保留相对路径组件）
        let relative_path = file_path.strip_prefix(cwd).unwrap(); // "examples/ok.http"
        let has_blacklisted_dir_new = blacklisted_dirs
            .iter()
            .any(|&dir| relative_path.components().any(|c| c.as_os_str() == dir));
        assert!(!has_blacklisted_dir_new); // 不应拦截
    }

    #[test]
    fn test_should_keep_file_symlink_scenario() {
        // 模拟 macOS 常见的符号链接与 canonical 物理绝对路径的前缀差异
        let symlink_cwd = std::path::Path::new("/Users/user/.gemini/worktrees/project");
        let canonical_cwd = std::path::Path::new("/private/Users/user/.gemini/worktrees/project");

        let file_path =
            std::path::Path::new("/private/Users/user/.gemini/worktrees/project/examples/ok.http");

        // 1. 如果直接对带有符号链接的 cwd 进行 strip_prefix，会因为前缀不匹配而失败
        let strip_direct_fail = file_path.strip_prefix(symlink_cwd).is_err();
        assert!(strip_direct_fail);

        // 2. 经过展开（模拟 canonicalize 后相同物理路径前缀），strip_prefix 能够完美剥离前缀并放行
        let relative_path = file_path.strip_prefix(canonical_cwd).unwrap();
        assert_eq!(relative_path, std::path::Path::new("examples/ok.http"));
    }
}
