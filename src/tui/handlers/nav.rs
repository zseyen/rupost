//! 纯函数导航层：按键几何与滚动数学，与 AppState 无关（可表格驱动测试）。

use crate::tui::state::{DetailTab, LayoutMode, Panel, SidebarTab};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub const PANEL_ORDER: [Panel; 3] = [Panel::Files, Panel::Editor, Panel::Response];
pub const DETAIL_ORDER: [DetailTab; 3] =
    [DetailTab::Response, DetailTab::Headers, DetailTab::Request];

/// Tab / Shift+Tab 面板轮转
pub fn next_panel(current: Panel, forward: bool) -> Panel {
    cycle(PANEL_ORDER, current, forward)
}

/// Response 详情子选项卡 h/l 循环
pub fn next_detail_tab(current: DetailTab, forward: bool) -> DetailTab {
    cycle(DETAIL_ORDER, current, forward)
}

fn cycle<T: Copy + PartialEq>(order: [T; 3], current: T, forward: bool) -> T {
    let i = order.iter().position(|p| *p == current).unwrap_or(0);
    let step = if forward { 1 } else { order.len() - 1 };
    order[(i + step) % order.len()]
}

/// 上滚 amount（饱和于 0）
pub fn scroll_up(cur: usize, amount: usize) -> usize {
    cur.saturating_sub(amount)
}

/// 下滚 amount，封顶 max_offset（与旧的 `if cur < max { cur+1 }` / 页滚 `min(cur+N, cap)` 等价）
pub fn scroll_down(cur: usize, max_offset: usize, amount: usize) -> usize {
    (cur + amount).min(max_offset)
}

/// 编辑器行滚动上限：最后一行可见
pub fn editor_max_offset(total_lines: usize) -> usize {
    total_lines.saturating_sub(1)
}

/// Response 滚动上限：内容尾部贴住视口底部
pub fn response_max_offset(total_lines: usize, viewport_height: usize) -> usize {
    total_lines.saturating_sub(viewport_height)
}

/// 滚动边界钳制（侧边栏可视区“向心保持”）
pub fn clamp_scroll_offset(selected: usize, offset: usize, viewport_height: usize) -> usize {
    if selected < offset {
        selected
    } else if selected >= offset + viewport_height {
        selected + 1 - viewport_height
    } else {
        offset
    }
}

/// 鼠标命中目标（几何判定，不含任何状态副作用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitTarget {
    /// 面板头部条：切到 Files 并（可能）切 Tab；None 表示保持当前 Tab
    Header { tab: Option<SidebarTab> },
    /// 列表区第 row 可见行
    SidebarList(usize),
    /// 仅切换面板（无同步副作用）
    Panel(Panel),
}

const fn header_tab_at(x: u16) -> Option<SidebarTab> {
    match x {
        0..=10 => Some(SidebarTab::Files),
        12..=23 => Some(SidebarTab::History),
        _ => None,
    }
}

/// 判定 (x, y) 落在哪一个交互区。边界语义与拆分前的 if/else 链逐条一致：
/// 宽/窄布局下 y==0 仅左栏算头部，中/右区 y==0 仍命中 Editor/Response；
/// 底部帮助条 (y == h-1) 与右下角不命中任何目标。
pub fn hit_test(
    layout: LayoutMode,
    w: u16,
    h: u16,
    x: u16,
    y: u16,
    active_panel: Panel,
) -> Option<HitTarget> {
    match layout {
        LayoutMode::Wide | LayoutMode::Narrow => {
            let (sidebar_ratio, editor_ratio) = match layout {
                LayoutMode::Wide => (0.25, 0.40),
                _ => (0.20, 0.40),
            };
            let sidebar_w = (w as f32 * sidebar_ratio) as u16;
            let editor_w = (w as f32 * editor_ratio) as u16;

            if y == 0 && x < sidebar_w {
                Some(HitTarget::Header {
                    tab: header_tab_at(x),
                })
            } else if (1..h - 1).contains(&y) && x < sidebar_w {
                Some(HitTarget::SidebarList((y - 2) as usize))
            } else if (sidebar_w..sidebar_w + editor_w).contains(&x) && y < h - 1 {
                Some(HitTarget::Panel(Panel::Editor))
            } else if (sidebar_w + editor_w..w).contains(&x) && y < h - 1 {
                Some(HitTarget::Panel(Panel::Response))
            } else {
                None
            }
        }
        LayoutMode::Stacked => {
            let col_w = w / 3;
            if y <= 2 {
                Some(match x {
                    v if v < col_w => HitTarget::Panel(Panel::Files),
                    v if v < col_w * 2 => HitTarget::Panel(Panel::Editor),
                    _ => HitTarget::Panel(Panel::Response),
                })
            } else if y < h - 1 && active_panel == Panel::Files {
                match y {
                    3 => Some(HitTarget::Header {
                        tab: header_tab_at(x),
                    }),
                    4.. => Some(HitTarget::SidebarList((y - 5) as usize)),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}

/// 运行键判定（Editor 面板 Ctrl+R / Ctrl+Enter）
pub fn is_run_key(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Enter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_panel_cycles() {
        assert_eq!(next_panel(Panel::Files, true), Panel::Editor);
        assert_eq!(next_panel(Panel::Editor, true), Panel::Response);
        assert_eq!(next_panel(Panel::Response, true), Panel::Files);
        assert_eq!(next_panel(Panel::Files, false), Panel::Response);
        assert_eq!(next_panel(Panel::Response, false), Panel::Editor);
        assert_eq!(next_panel(Panel::Editor, false), Panel::Files);
    }

    #[test]
    fn test_next_detail_tab_cycles() {
        // 顺序 [Response, Headers, Request]：backward 从 Response 回卷到 Request
        assert_eq!(
            next_detail_tab(DetailTab::Response, true),
            DetailTab::Headers
        );
        assert_eq!(
            next_detail_tab(DetailTab::Headers, true),
            DetailTab::Request
        );
        assert_eq!(
            next_detail_tab(DetailTab::Request, true),
            DetailTab::Response
        );
        assert_eq!(
            next_detail_tab(DetailTab::Response, false),
            DetailTab::Request
        );
        assert_eq!(
            next_detail_tab(DetailTab::Headers, false),
            DetailTab::Response
        );
        assert_eq!(
            next_detail_tab(DetailTab::Request, false),
            DetailTab::Headers
        );
    }

    #[test]
    fn test_scroll_math_table() {
        // (cur, max, amount, expect)
        let down_cases = [
            (0, 5, 1, 1),
            (4, 5, 1, 5),
            (5, 5, 1, 5),
            (0, 0, 1, 0),
            (8, 20, 10, 18),
            (15, 20, 10, 20),
        ];
        for (cur, max, amt, want) in down_cases {
            assert_eq!(scroll_down(cur, max, amt), want, "down cur={cur} max={max}");
        }
        assert_eq!(scroll_up(0, 10), 0);
        assert_eq!(scroll_up(7, 10), 0);
        assert_eq!(scroll_up(12, 10), 2);
        assert_eq!(editor_max_offset(0), 0);
        assert_eq!(editor_max_offset(10), 9);
        assert_eq!(response_max_offset(20, 8), 12);
        assert_eq!(response_max_offset(5, 8), 0);
    }

    #[test]
    fn test_clamp_scroll_offset() {
        // (selected, offset, viewport, expect)
        let cases = [
            (0, 2, 5, 0),
            (3, 2, 5, 2),
            (6, 2, 5, 2),
            (7, 2, 5, 3),
            (9, 2, 5, 5),
        ];
        for (sel, off, vp, want) in cases {
            assert_eq!(
                clamp_scroll_offset(sel, off, vp),
                want,
                "sel={sel} off={off}"
            );
        }
    }

    #[test]
    fn test_hit_test_wide_regions() {
        let w = 100u16;
        let h = 20u16;
        // 宽布局：sidebar 25、editor 40、response 35
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 5, 0, Panel::Files),
            Some(HitTarget::Header {
                tab: Some(SidebarTab::Files)
            })
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 15, 0, Panel::Files),
            Some(HitTarget::Header {
                tab: Some(SidebarTab::History)
            })
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 11, 0, Panel::Files),
            Some(HitTarget::Header { tab: None })
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 5, 4, Panel::Files),
            Some(HitTarget::SidebarList(2))
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 40, 4, Panel::Files),
            Some(HitTarget::Panel(Panel::Editor))
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 90, 4, Panel::Files),
            Some(HitTarget::Panel(Panel::Response))
        );
        // 帮助条行与右下角外不命中
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 5, h - 1, Panel::Files),
            None
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, w - 1, h - 1, Panel::Files),
            None
        );
        // y==0 的中/右区仍命中面板（旧行为保留）
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 40, 0, Panel::Files),
            Some(HitTarget::Panel(Panel::Editor))
        );
        assert_eq!(
            hit_test(LayoutMode::Wide, w, h, 95, 0, Panel::Files),
            Some(HitTarget::Panel(Panel::Response))
        );
    }

    #[test]
    fn test_hit_test_narrow_and_stacked() {
        let w = 100u16;
        let h = 20u16;
        // 窄布局：sidebar 20
        assert_eq!(
            hit_test(LayoutMode::Narrow, w, h, 10, 4, Panel::Files),
            Some(HitTarget::SidebarList(2))
        );
        assert_eq!(
            hit_test(LayoutMode::Narrow, w, h, 30, 4, Panel::Files),
            Some(HitTarget::Panel(Panel::Editor))
        );

        // 堆叠布局：顶部三等分条
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 10, 1, Panel::Files),
            Some(HitTarget::Panel(Panel::Files))
        );
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 40, 1, Panel::Files),
            Some(HitTarget::Panel(Panel::Editor))
        );
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 80, 1, Panel::Files),
            Some(HitTarget::Panel(Panel::Response))
        );
        // y==3 头部条 / y>=4 列表（仅 Files 面板激活时）
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 5, 3, Panel::Files),
            Some(HitTarget::Header {
                tab: Some(SidebarTab::Files)
            })
        );
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 5, 5, Panel::Files),
            Some(HitTarget::SidebarList(0))
        );
        // 非 Files 面板激活时，中下部无目标
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 5, 5, Panel::Editor),
            None
        );
        // 帮助条不命中
        assert_eq!(
            hit_test(LayoutMode::Stacked, w, h, 5, h - 1, Panel::Files),
            None
        );
    }

    #[test]
    fn test_is_run_key() {
        assert!(is_run_key(&KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::CONTROL
        )));
        assert!(is_run_key(&KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL
        )));
        assert!(!is_run_key(&KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::NONE
        )));
        assert!(!is_run_key(&KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL
        )));
    }
}
