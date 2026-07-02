use crate::Result;
use std::time::Duration;
use tokio::sync::mpsc;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{self, Event as CrosstermEvent},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use super::event::TuiEvent;
use super::state::AppState;

/// 启动并运行 TUI 主事件循环
pub fn run() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| crate::error::RupostError::IoError(e))?;
        
    rt.block_on(async {
        run_async().await
    })
}

async fn run_async() -> Result<()> {
    // 1. 初始化终端
    enable_raw_mode().map_err(|e| crate::error::RupostError::IoError(e))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| crate::error::RupostError::IoError(e))?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    
    // 2. 建立 MPSC 事件通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    
    // 派发 crossterm 事件捕获 Task
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            // 阻断式轮询是否有 crossterm 事件
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(CrosstermEvent::Key(key)) = event::read() {
                    if tx_clone.send(TuiEvent::Input(key)).await.is_err() {
                        break;
                    }
                }
            }
            // 内部心跳
            if tx_clone.send(TuiEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    let mut state = AppState::new();
    
    // 捕获初始尺寸
    if let Ok((w, h)) = crossterm::terminal::size() {
        state.update_layout(w, h);
    }
    
    // 3. 事件循环主流程
    loop {
        // A. 渲染当前状态
        terminal.draw(|frame| {
            super::ui::render(frame, &mut state);
        }).map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        
        // B. 接收事件
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(key) => {
                    // 如果按 q 退出
                    if key.code == crossterm::event::KeyCode::Char('q') {
                        state.update(super::event::Action::Quit);
                    }
                    // 面板切换
                    if key.code == crossterm::event::KeyCode::Tab {
                        let next_panel = match state.active_panel {
                            super::state::Panel::Files => super::state::Panel::Editor,
                            super::state::Panel::Editor => super::state::Panel::Response,
                            super::state::Panel::Response => super::state::Panel::Files,
                        };
                        state.update(super::event::Action::SwitchPanel(next_panel));
                    }
                    // 帮助菜单切换
                    if key.code == crossterm::event::KeyCode::Char('?') {
                        state.update(super::event::Action::ToggleHelp);
                    }
                }
                TuiEvent::Resize(w, h) => {
                    state.update_layout(w, h);
                }
                TuiEvent::Tick => {
                    // 定时心跳自刷新
                }
                _ => {}
            }
        }
        
        if state.is_quitting {
            break;
        }
    }
    
    // 4. 清理并还原终端
    disable_raw_mode().map_err(|e| crate::error::RupostError::IoError(e))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| crate::error::RupostError::IoError(e))?;
    terminal.show_cursor().map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    
    Ok(())
}
