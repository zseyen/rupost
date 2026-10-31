//! 预折行域：unicode-width 精确折行的纯函数（物理行 → 视觉行缓存）。

pub struct VisualLineProcessor;

impl VisualLineProcessor {
    pub fn wrap_text(text: &str, max_width: usize) -> Vec<ratatui::text::Line<'static>> {
        use unicode_width::UnicodeWidthChar;
        let mut visual_lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                visual_lines.push(ratatui::text::Line::from(""));
                continue;
            }
            let mut current_line = String::new();
            let mut current_width = 0;
            for c in line.chars() {
                let char_width = c.width().unwrap_or(0);
                if current_width + char_width > max_width {
                    visual_lines.push(ratatui::text::Line::from(current_line.clone()));
                    current_line.clear();
                    current_width = 0;
                }
                current_line.push(c);
                current_width += char_width;
            }
            if !current_line.is_empty() {
                visual_lines.push(ratatui::text::Line::from(current_line));
            }
        }
        visual_lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_line_wrap_english() {
        let text = "abcdefghij\nklmnopqrst";
        let lines = VisualLineProcessor::wrap_text(text, 5);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].to_string(), "abcde");
        assert_eq!(lines[1].to_string(), "fghij");
        assert_eq!(lines[2].to_string(), "klmno");
        assert_eq!(lines[3].to_string(), "pqrst");
    }

    #[test]
    fn test_visual_line_wrap_chinese_multibyte() {
        let text = "你好世界，你好。"; // 8 个字，逻辑宽度 16
        let lines = VisualLineProcessor::wrap_text(text, 6);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].to_string(), "你好世");
        assert_eq!(lines[1].to_string(), "界，你");
        assert_eq!(lines[2].to_string(), "好。");
    }
}
