//! 搜索高亮（R4）：大小写不敏感子串匹配 + accent 背景渲染。
//! 仅作用于当前可见切片（不影响底层视觉行缓存）。

use crate::tui::state::AppState;
use ratatui::text::{Line, Span};

/// 对可见切片应用搜索高亮（query 为空时原样返回）
pub(crate) fn apply_search_highlight(
    sliced: Vec<Line<'static>>,
    state: &AppState,
) -> Vec<Line<'static>> {
    let query = state.search_query.clone();
    if query.is_empty() {
        return sliced;
    }
    let accent = state.palette().accent;
    sliced
        .into_iter()
        .map(|l| highlight_query_line(&l.to_string(), &query, accent))
        .collect()
}

/// 在原串字符边界上做大小写不敏感子串匹配，返回命中字节区间（升序、不重叠）。
/// 不采用 `to_lowercase()` 整体偏移：它不保字节长度（如 `İ` → `i`+`◌̇`、`ß` → `ss`），
/// 拿小写串的偏移切原串会落入多字节字符内部导致 panic（char boundary 越界）。
fn find_match_ranges(line: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let offsets: Vec<usize> = line.char_indices().map(|(o, _)| o).collect();
    // 每个原字符的小写展开（可能 1→多字符），记录其源字符下标
    let lowered: Vec<(usize, char)> = line
        .char_indices()
        .enumerate()
        .flat_map(|(i, (_, ch))| ch.to_lowercase().map(move |c| (i, c)))
        .collect();
    let ql: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    if ql.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut i = 0;
    while i + ql.len() <= lowered.len() {
        if lowered[i..i + ql.len()]
            .iter()
            .map(|&(_, c)| c)
            .eq(ql.iter().copied())
        {
            let start = offsets[lowered[i].0];
            let end = offsets
                .get(lowered[i + ql.len() - 1].0 + 1)
                .copied()
                .unwrap_or(line.len());
            ranges.push((start, end));
            i += ql.len();
        } else {
            i += 1;
        }
    }
    ranges
}

/// 搜索高亮（R4）：把行文本按 query 大小写不敏感切分，命中段 accent 背景 + 加粗。
/// query 为空时原样返回；仅作用于当前可见切片（不影响底层视觉行缓存）。
fn highlight_query_line(line: &str, query: &str, accent: ratatui::style::Color) -> Line<'static> {
    use ratatui::style::{Color, Style};
    if query.is_empty() {
        return Line::from(line.to_string());
    }
    let mut spans = Vec::new();
    let mut cursor = 0;
    for (start, end) in find_match_ranges(line, query) {
        if start > cursor {
            spans.push(Span::raw(line[cursor..start].to_string()));
        }
        spans.push(Span::styled(
            line[start..end].to_string(),
            Style::default().bg(accent).fg(Color::Black),
        ));
        cursor = end;
    }
    if cursor < line.len() {
        spans.push(Span::raw(line[cursor..].to_string()));
    }
    Line::from(spans)
}

#[cfg(test)]
mod highlight_tests {
    use super::*;

    const ACCENT: ratatui::style::Color = ratatui::style::Color::Rgb(1, 2, 3);

    fn spans_of(line: Line<'static>) -> Vec<(String, bool)> {
        line.spans
            .iter()
            .map(|s| {
                let styled = s.style.bg == Some(ACCENT);
                (s.content.to_string(), styled)
            })
            .collect()
    }

    /// 高亮必须保持全文拼接不变（任何切分错误首先破坏这一点）
    fn assert_roundtrip(line: &str, query: &str) {
        let l = highlight_query_line(line, query, ACCENT);
        let joined: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, line, "query={query:?} must preserve full text");
    }

    #[test]
    fn test_highlight_ascii_basic_segments() {
        let l = highlight_query_line("data: hello world", "hello", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("data: ".to_string(), false),
                ("hello".to_string(), true),
                (" world".to_string(), false),
            ]
        );
        assert_roundtrip("data: hello world", "hello");
    }

    /// P0-1 回归锚点：`İ`（U+0130，2 字节）小写化为 `i`+U+0307（3 字节），
    /// 旧实现拿小写串字节偏移切原串 → char boundary panic（UI 崩溃）
    #[test]
    fn test_highlight_turkish_capital_i_no_panic() {
        // 旧实现此输入确定性 panic；命中段必须落在原字符边界上
        let l = highlight_query_line("İx", "i", ACCENT);
        let spans = spans_of(l);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0], ("İ".to_string(), true), "İ must be the hit span");
        assert_eq!(spans[1], ("x".to_string(), false));
        assert_roundtrip("İx", "i");
    }

    /// `ß` 恒等小写（展开 "ss" 的是 to_uppercase）：多字节字符不干扰后续命中偏移
    #[test]
    fn test_highlight_sharp_s_boundary() {
        let l = highlight_query_line("ßSS", "ss", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![("ß".to_string(), false), ("SS".to_string(), true)]
        );
        assert_roundtrip("ßSS", "ss");
    }

    /// CJK 多字节字符（每字符 3 字节）边界：命中段必须整字符覆盖
    #[test]
    fn test_highlight_cjk_boundary() {
        let l = highlight_query_line("你好世界", "好世", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("你".to_string(), false),
                ("好世".to_string(), true),
                ("界".to_string(), false),
            ]
        );
        assert_roundtrip("你好世界", "好");
    }

    #[test]
    fn test_highlight_no_match_single_raw_span() {
        let l = highlight_query_line("plain text", "zzz", ACCENT);
        assert_eq!(spans_of(l), vec![("plain text".to_string(), false)]);
        // 空 query 原样返回
        let l = highlight_query_line("plain text", "", ACCENT);
        assert_eq!(spans_of(l), vec![("plain text".to_string(), false)]);
    }

    /// 多命中不重叠推进：连续命中间的非命中段保留
    #[test]
    fn test_highlight_multiple_hits_with_gaps() {
        let l = highlight_query_line("ab ab ab", "ab", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("ab".to_string(), true),
                (" ".to_string(), false),
                ("ab".to_string(), true),
                (" ".to_string(), false),
                ("ab".to_string(), true),
            ]
        );
    }
}
