//! 布局领域：视口断点与响应面板宽度计算（自 state/mod.rs 拆出）。

use super::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Narrow,
    Stacked,
}

impl AppState {
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

        let response_width = match self.layout_mode {
            LayoutMode::Wide => {
                let w = (width as u32 * 35 / 100) as u16;
                w.saturating_sub(2) as usize
            }
            LayoutMode::Narrow => {
                let w = (width as u32 * 40 / 100) as u16;
                w.saturating_sub(2) as usize
            }
            LayoutMode::Stacked => width.saturating_sub(2) as usize,
        };

        if response_width != self.current_response_width {
            self.current_response_width = response_width;
            self.response_visual_lines.clear();
            self.sse_visual_lines.clear();
            self.sse_last_processed_pos = 0;
            self.ws_visual_lines.clear();
        }
    }
}
