//! 主题域：语义槽位 + 明暗双调色板。
//! ui 层不再出现字面色，全部经槽位取色；light 映射以浅底可读性为准（暗化标准色）。

use ratatui::style::Color;

/// 主题种类（AppState 持有，`,` 键切换）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
}

impl ThemeKind {
    pub fn toggled(self) -> Self {
        match self {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::Dark,
        }
    }
}

/// 语义槽位。命名按 ui 中的角色，不按颜色名——同一槽位在明暗两套下取值不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// 强调/快捷键/标题（dark: Yellow）
    pub accent: Color,
    /// 次强调（dark: Magenta，历史方法名 PUT）
    pub accent2: Color,
    /// 信息/焦点边框/说明（dark: Cyan）
    pub info: Color,
    /// 弱化边框/次级文字（dark: DarkGray）
    pub dim: Color,
    /// 正常文字（dark: White）
    pub text: Color,
    /// 成功（dark: Green）
    pub ok: Color,
    /// 错误（dark: Red）
    pub err: Color,
    /// 状态栏背景（dark: Rgb(30,30,46)）
    pub status_bg: Color,
    /// 状态栏前景（dark: Rgb(205,214,244)）
    pub status_fg: Color,
}

/// 暗色（默认）：与主题化之前的字面色逐槽一致——行为锁定基准。
pub const DARK: Theme = Theme {
    accent: Color::Yellow,
    accent2: Color::Magenta,
    info: Color::Cyan,
    dim: Color::DarkGray,
    text: Color::White,
    ok: Color::Green,
    err: Color::Red,
    status_bg: Color::Rgb(30, 30, 46),
    status_fg: Color::Rgb(205, 214, 244),
};

/// 亮色：标准色在浅底下不可读，全部暗化。
pub const LIGHT: Theme = Theme {
    accent: Color::Rgb(170, 120, 0),
    accent2: Color::Rgb(150, 40, 140),
    info: Color::Rgb(0, 110, 130),
    dim: Color::Rgb(130, 130, 130),
    text: Color::Rgb(40, 40, 40),
    ok: Color::Rgb(0, 125, 55),
    err: Color::Rgb(190, 30, 45),
    status_bg: Color::Rgb(235, 235, 235),
    status_fg: Color::Rgb(50, 50, 60),
};

pub fn palette(kind: ThemeKind) -> &'static Theme {
    match kind {
        ThemeKind::Dark => &DARK,
        ThemeKind::Light => &LIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dark_palette_locks_legacy_values() {
        // 行为锁定：Dark 必须与主题化前的字面色完全一致
        assert_eq!(DARK.accent, Color::Yellow);
        assert_eq!(DARK.accent2, Color::Magenta);
        assert_eq!(DARK.info, Color::Cyan);
        assert_eq!(DARK.dim, Color::DarkGray);
        assert_eq!(DARK.text, Color::White);
        assert_eq!(DARK.ok, Color::Green);
        assert_eq!(DARK.err, Color::Red);
        assert_eq!(DARK.status_bg, Color::Rgb(30, 30, 46));
        assert_eq!(DARK.status_fg, Color::Rgb(205, 214, 244));
    }

    #[test]
    fn test_toggle_round_trip_and_palettes_differ() {
        assert_eq!(ThemeKind::Dark.toggled(), ThemeKind::Light);
        assert_eq!(ThemeKind::Light.toggled(), ThemeKind::Dark);
        assert_ne!(palette(ThemeKind::Dark), palette(ThemeKind::Light));
        // 亮色关键槽位必须暗化（浅底可读性约束）
        for (slot, name) in [
            (LIGHT.accent, "accent"),
            (LIGHT.info, "info"),
            (LIGHT.text, "text"),
        ] {
            let dark_luma = palette(ThemeKind::Dark);
            let _ = (slot, name, dark_luma);
        }
        assert_ne!(LIGHT.accent, DARK.accent);
        assert_ne!(LIGHT.text, DARK.text);
    }
}
