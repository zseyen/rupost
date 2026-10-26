//! POSIX Shell 安全转义工具

/// POSIX Shell 参数转义（兼容 sh / bash / zsh）
///
/// 规则：
/// 1. 空字符串：返回 `''`
/// 2. 普通模式：使用单引号 `'...'` 包裹，内部单引号 `'` 转义为 `'\''`
/// 3. compact 单行模式且含真实换行，或含有不可见控制字符（防 Pastejacking）：升级为 ANSI-C `$'...'`
pub fn escape_shell_word(input: &str, compact: bool) -> String {
    if input.is_empty() {
        return "''".to_string();
    }

    let has_newline = input.contains('\n') || input.contains('\r');
    let has_control = input
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t');

    // 在 compact 模式下，换行符会导致单行命令断裂，故升级为 ANSI-C 转义
    // 存在不可见控制字符时，也强制使用 ANSI-C 转义以防终端逃逸
    if (compact && has_newline) || has_control {
        let mut out = String::from("$'");
        for c in input.chars() {
            match c {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }

    let mut out = String::from("'");
    for c in input.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        assert_eq!(escape_shell_word("", false), "''");
        assert_eq!(escape_shell_word("", true), "''");
    }

    #[test]
    fn test_simple_string() {
        assert_eq!(escape_shell_word("https://api.example.com/users", false), "'https://api.example.com/users'");
    }

    #[test]
    fn test_single_quote_escape() {
        assert_eq!(escape_shell_word("O'Reilly", false), "'O'\\''Reilly'");
    }

    #[test]
    fn test_multiline_in_normal_mode() {
        let input = "{\n  \"name\": \"Alice\"\n}";
        // 普通多行模式原生支持单引号包含换行
        let escaped = escape_shell_word(input, false);
        assert_eq!(escaped, "'{\n  \"name\": \"Alice\"\n}'");
    }

    #[test]
    fn test_multiline_in_compact_mode() {
        let input = "{\n  \"name\": \"Alice\"\n}";
        // 单行模式下必须转义为 $'...'
        let escaped = escape_shell_word(input, true);
        assert_eq!(escaped, "$'{\\n  \"name\": \"Alice\"\\n}'");
    }

    #[test]
    fn test_control_characters() {
        let input = "hello\x1b[31mworld";
        let escaped = escape_shell_word(input, false);
        assert!(escaped.starts_with("$'"));
        assert!(escaped.contains("\\x1b"));
    }
}
