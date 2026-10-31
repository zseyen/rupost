use crate::assertion::types::{AssertError, AssertExpr, AssertValue, CompareOp, ValuePath};

/// 支持的比较运算符。按长度降序排列，使同一位置命中时最长者优先（`>=` 胜 `>`）。
const OPERATORS: [&str; 7] = ["contains", ">=", "<=", "==", "!=", ">", "<"];

/// 在断言/匹配表达式中定位**最左侧**的运算符，返回 (字节位置, 运算符)。
///
/// 规则：
/// - 从左到右逐字符扫描，引号（`"` / `'`）内的内容整体跳过——否则
///   `body contains "<html>"` 会先命中引号内的 `<`，`contains "a==b"` 会命中 `==`；
/// - 同一位置多个运算符可命中时取最长（`a >= b` 不会被切成 `a > "= b"`）；
/// - `contains` 必须独立成词，避免 `body.contains_flag == 1` 中的路径名被当作运算符。
///
/// 此前实现按固定列表顺序对整串 `find`，取的是"列表里第一个在任意位置出现的运算符"，
/// 而非最左运算符，断言会被切成与用户所写完全不同的表达式。
pub fn find_operator(input: &str) -> Option<(usize, &'static str)> {
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;

    for (i, c) in input.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            prev = Some(c);
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            prev = Some(c);
            continue;
        }

        let rest = &input[i..];
        for op in OPERATORS {
            if !rest.starts_with(op) {
                continue;
            }
            if op == "contains" {
                let before_ok = prev.is_none_or(|p| p.is_whitespace());
                let after_ok = rest[op.len()..]
                    .chars()
                    .next()
                    .is_none_or(|n| n.is_whitespace());
                if !(before_ok && after_ok) {
                    continue;
                }
            }
            return Some((i, op));
        }
        prev = Some(c);
    }
    None
}

/// 解析断言表达式
///
/// 支持的格式：
/// - `status == 200`
/// - `headers.content-type contains "json"`
/// - `body.user.id > 0`
/// - `response.time < 1000`
/// - `body.token exists`
pub fn parse_assertion(input: &str) -> Result<AssertExpr, AssertError> {
    let input = input.trim();

    // 检查是否是 exists 断言
    if let Some(path_str) = input.strip_suffix("exists") {
        let path = parse_value_path(path_str.trim())?;
        return Ok(AssertExpr::Exists { path });
    }

    // 查找最左侧运算符（引号感知）
    let (op_pos, op_str) = find_operator(input).ok_or_else(|| {
        AssertError::InvalidSyntax(format!("No valid operator found in assertion: {}", input))
    })?;

    let op = CompareOp::parse(op_str)
        .ok_or_else(|| AssertError::InvalidOperator(format!("Invalid operator: {}", op_str)))?;

    // 分割左值和右值
    let left_str = input[..op_pos].trim();
    let right_str = input[op_pos + op_str.len()..].trim();

    if left_str.is_empty() {
        return Err(AssertError::InvalidSyntax(
            "Left side of assertion is empty".to_string(),
        ));
    }

    if right_str.is_empty() {
        return Err(AssertError::InvalidSyntax(
            "Right side of assertion is empty".to_string(),
        ));
    }

    let left = parse_value_path(left_str)?;
    let right = parse_assert_value(right_str)?;

    Ok(AssertExpr::Compare { left, op, right })
}

/// 解析值路径
fn parse_value_path(input: &str) -> Result<ValuePath, AssertError> {
    let input = input.trim();

    if input == "status" {
        return Ok(ValuePath::Status);
    }

    if input == "response.time" {
        return Ok(ValuePath::ResponseTime);
    }

    if input == "stream.event" {
        return Ok(ValuePath::StreamEvent);
    }

    if input == "stream.id" {
        return Ok(ValuePath::StreamId);
    }

    if input == "stream.llm.content" {
        return Ok(ValuePath::StreamLlmContent);
    }

    if input == "timing.dns" {
        return Ok(ValuePath::TimingDns);
    }

    if input == "timing.tcp" {
        return Ok(ValuePath::TimingTcp);
    }

    if input == "timing.tls" {
        return Ok(ValuePath::TimingTls);
    }

    if input == "timing.ttfb" {
        return Ok(ValuePath::TimingTtfb);
    }

    if input == "cert.days_remaining" {
        return Ok(ValuePath::CertDays);
    }

    if input == "cert.issuer" {
        return Ok(ValuePath::CertIssuer);
    }

    if input == "cert.subject" {
        return Ok(ValuePath::CertSubject);
    }

    if let Some(rest) = input.strip_prefix("headers.") {
        return Ok(ValuePath::Header(rest.to_string()));
    }

    if let Some(rest) = input.strip_prefix("body.") {
        let segments = crate::utils::jsonpath::parse_jsonpath_to_segments(rest);
        if segments.is_empty() {
            return Err(AssertError::InvalidSyntax(
                "Body path cannot be empty".to_string(),
            ));
        }
        return Ok(ValuePath::Body(segments));
    }

    if let Some(rest) = input.strip_prefix("stream.body.") {
        let segments: Vec<String> = rest.split('.').map(|s| s.to_string()).collect();
        if segments.is_empty() {
            return Err(AssertError::InvalidSyntax(
                "Stream body path cannot be empty".to_string(),
            ));
        }
        return Ok(ValuePath::StreamBody(segments));
    }

    Err(AssertError::InvalidSyntax(format!(
        "Invalid value path: {}. Must start with 'status', 'headers.', 'body.', 'response.time', 'timing.', 'cert.', or 'stream.'",
        input
    )))
}

/// 解析断言值（右值）
pub fn parse_assert_value(input: &str) -> Result<AssertValue, AssertError> {
    let input = input.trim();

    // Null
    if input == "null" {
        return Ok(AssertValue::Null);
    }

    // 统一空值语义（未包裹引号）
    let none_variants = ["None", "undefined", "nil", "NULL"];
    if none_variants.contains(&input) {
        return Ok(AssertValue::None);
    }

    // 布尔值
    if input == "true" {
        return Ok(AssertValue::Bool(true));
    }
    if input == "false" {
        return Ok(AssertValue::Bool(false));
    }

    // 字符串（带引号）
    // len() >= 2 守卫：单字符 "\"" 同时满足 starts_with/ends_with，
    // 直接切片会 panic（byte range starts at 1 but ends at 0）
    if ((input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\'')))
        && input.len() >= 2
    {
        let s = &input[1..input.len() - 1];
        return Ok(AssertValue::String(s.to_string()));
    }

    // 数字
    if let Ok(n) = input.parse::<f64>() {
        return Ok(AssertValue::Number(n));
    }

    // 未带引号的字符串（用于 contains 等）
    Ok(AssertValue::String(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lone_quote_value_does_not_panic() {
        // 回归：期望值为单个引号字符时（如 `@assert body.path contains "<"`），
        // 此前在 parse_assert_value 的引号剥离切片处 panic（range 1..0）。
        // 注意：运算符被引号内字符误切属 2.1 范畴，此处只锁定"不 panic"。
        let expr = parse_assertion(r#"body.path contains "<""#).unwrap();
        assert!(matches!(expr, AssertExpr::Compare { .. }));

        let expr2 = parse_assertion(r#"body.x == "'""#).unwrap();
        assert!(matches!(expr2, AssertExpr::Compare { .. }));

        // 直接守卫验证：单字符引号按未包裹字符串处理
        let v = parse_assert_value("\"").unwrap();
        assert_eq!(v, AssertValue::String("\"".to_string()));
    }

    #[test]
    fn test_find_operator_leftmost_and_quote_aware() {
        // 引号内的运算符字符不参与匹配（原实测误切用例）
        assert_eq!(
            find_operator(r#"body.path contains "<""#),
            Some((10, "contains"))
        );
        assert_eq!(
            find_operator(r#"body.path contains "a==b""#),
            Some((10, "contains"))
        );
        assert_eq!(
            find_operator(r#"body.html contains "<b>x</b>""#),
            Some((10, "contains"))
        );
        // 同位置平局取最长：>= 不会被切成 > 和 "= 10"
        assert_eq!(find_operator("body.a >= 10"), Some((7, ">=")));
        assert_eq!(find_operator("body.a <= 10"), Some((7, "<=")));
        // contains 必须独立成词：路径名里的 contains 不算运算符
        assert_eq!(
            find_operator("body.contains_flag == true"),
            Some((19, "=="))
        );
        // 单引号 JSONPath 键内的内容跳过
        assert_eq!(
            find_operator(r#"body['first name'] == "x""#),
            Some((19, "=="))
        );
        // 无运算符
        assert_eq!(find_operator("this is not valid"), None);
        // 非 ASCII 内容不 panic 且正确定位
        assert_eq!(find_operator(r#"body.msg == "中文""#), Some((9, "==")));
    }

    #[test]
    fn test_parse_assertion_with_operator_chars_in_quoted_value() {
        // 端到端：引号值含运算符字符时，解析出的表达式必须与用户所写一致
        let expr = parse_assertion(r#"body.path contains "a==b""#).unwrap();
        match expr {
            AssertExpr::Compare { op, right, .. } => {
                assert_eq!(op, CompareOp::Contains);
                assert_eq!(right, AssertValue::String("a==b".to_string()));
            }
            _ => panic!("Expected Compare"),
        }

        let expr = parse_assertion(r#"body.html contains "<b>x</b>""#).unwrap();
        match expr {
            AssertExpr::Compare { op, right, .. } => {
                assert_eq!(op, CompareOp::Contains);
                assert_eq!(right, AssertValue::String("<b>x</b>".to_string()));
            }
            _ => panic!("Expected Compare"),
        }

        let expr = parse_assertion(r#"body.path contains "<""#).unwrap();
        match expr {
            AssertExpr::Compare { op, right, .. } => {
                assert_eq!(op, CompareOp::Contains);
                assert_eq!(right, AssertValue::String("<".to_string()));
            }
            _ => panic!("Expected Compare"),
        }

        let expr = parse_assertion("body.a >= 10").unwrap();
        match expr {
            AssertExpr::Compare { op, right, .. } => {
                assert_eq!(op, CompareOp::GreaterOrEqual);
                assert_eq!(right, AssertValue::Number(10.0));
            }
            _ => panic!("Expected Compare"),
        }
    }

    #[test]
    fn test_parse_status_assertion() {
        let expr = parse_assertion("status == 200").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(left, ValuePath::Status);
                assert_eq!(op, CompareOp::Equal);
                assert_eq!(right, AssertValue::Number(200.0));
            }
            _ => panic!("Expected Compare assertion"),
        }
    }

    #[test]
    fn test_parse_header_assertion() {
        let expr = parse_assertion("headers.content-type contains \"json\"").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(left, ValuePath::Header("content-type".to_string()));
                assert_eq!(op, CompareOp::Contains);
                assert_eq!(right, AssertValue::String("json".to_string()));
            }
            _ => panic!("Expected Compare assertion"),
        }
    }

    #[test]
    fn test_parse_body_assertion() {
        let expr = parse_assertion("body.user.id > 0").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(
                    left,
                    ValuePath::Body(vec!["user".to_string(), "id".to_string()])
                );
                assert_eq!(op, CompareOp::Greater);
                assert_eq!(right, AssertValue::Number(0.0));
            }
            _ => panic!("Expected Compare assertion"),
        }
    }

    #[test]
    fn test_parse_response_time_assertion() {
        let expr = parse_assertion("response.time < 1000").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(left, ValuePath::ResponseTime);
                assert_eq!(op, CompareOp::Less);
                assert_eq!(right, AssertValue::Number(1000.0));
            }
            _ => panic!("Expected Compare assertion"),
        }
    }

    #[test]
    fn test_parse_exists_assertion() {
        let expr = parse_assertion("body.token exists").unwrap();
        match expr {
            AssertExpr::Exists { path } => {
                assert_eq!(path, ValuePath::Body(vec!["token".to_string()]));
            }
            _ => panic!("Expected Exists assertion"),
        }
    }

    #[test]
    fn test_parse_operators() {
        assert!(parse_assertion("status == 200").is_ok());
        assert!(parse_assertion("status != 404").is_ok());
        assert!(parse_assertion("status > 199").is_ok());
        assert!(parse_assertion("status < 300").is_ok());
        assert!(parse_assertion("status >= 200").is_ok());
        assert!(parse_assertion("status <= 299").is_ok());
    }

    #[test]
    fn test_parse_values() {
        // Numbers
        let expr = parse_assertion("body.count == 42").unwrap();
        match expr {
            AssertExpr::Compare { right, .. } => {
                assert_eq!(right, AssertValue::Number(42.0));
            }
            _ => panic!(),
        }

        // Strings with quotes
        let expr = parse_assertion("body.name == \"test\"").unwrap();
        match expr {
            AssertExpr::Compare { right, .. } => {
                assert_eq!(right, AssertValue::String("test".to_string()));
            }
            _ => panic!(),
        }

        // Boolean
        let expr = parse_assertion("body.active == true").unwrap();
        match expr {
            AssertExpr::Compare { right, .. } => {
                assert_eq!(right, AssertValue::Bool(true));
            }
            _ => panic!(),
        }

        // Null
        let expr = parse_assertion("body.data == null").unwrap();
        match expr {
            AssertExpr::Compare { right, .. } => {
                assert_eq!(right, AssertValue::Null);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_invalid_syntax() {
        assert!(parse_assertion("invalid").is_err());
        assert!(parse_assertion("status").is_err());
        assert!(parse_assertion("== 200").is_err());
        assert!(parse_assertion("status ==").is_err());
    }

    #[test]
    fn test_parse_nested_body_path() {
        let expr = parse_assertion("body.user.profile.email == \"test@example.com\"").unwrap();
        match expr {
            AssertExpr::Compare { left, .. } => {
                assert_eq!(
                    left,
                    ValuePath::Body(vec![
                        "user".to_string(),
                        "profile".to_string(),
                        "email".to_string()
                    ])
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_stream_assertions() {
        let expr = parse_assertion("stream.event == \"chat\"").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(left, ValuePath::StreamEvent);
                assert_eq!(op, CompareOp::Equal);
                assert_eq!(right, AssertValue::String("chat".to_string()));
            }
            _ => panic!(),
        }

        let expr = parse_assertion("stream.id exists").unwrap();
        match expr {
            AssertExpr::Exists { path } => {
                assert_eq!(path, ValuePath::StreamId);
            }
            _ => panic!(),
        }

        let expr = parse_assertion("stream.body.choices.0.delta.content == \"Hello\"").unwrap();
        match expr {
            AssertExpr::Compare { left, op, right } => {
                assert_eq!(
                    left,
                    ValuePath::StreamBody(vec![
                        "choices".to_string(),
                        "0".to_string(),
                        "delta".to_string(),
                        "content".to_string()
                    ])
                );
                assert_eq!(op, CompareOp::Equal);
                assert_eq!(right, AssertValue::String("Hello".to_string()));
            }
            _ => panic!(),
        }
    }
}
