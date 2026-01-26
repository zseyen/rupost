use crate::assertion::types::{AssertError, AssertExpr, AssertValue, CompareOp, ValuePath};

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

    // 查找运算符
    let operators = [">=", "<=", "==", "!=", ">", "<", "contains"];
    let mut found_op = None;
    let mut op_pos = 0;

    for op_str in &operators {
        if let Some(pos) = input.find(op_str) {
            found_op = Some(*op_str);
            op_pos = pos;
            break;
        }
    }

    let op_str = found_op.ok_or_else(|| {
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

    if let Some(rest) = input.strip_prefix("headers.") {
        return Ok(ValuePath::Header(rest.to_string()));
    }

    if let Some(rest) = input.strip_prefix("body.") {
        let segments: Vec<String> = rest.split('.').map(|s| s.to_string()).collect();
        if segments.is_empty() {
            return Err(AssertError::InvalidSyntax(
                "Body path cannot be empty".to_string(),
            ));
        }
        return Ok(ValuePath::Body(segments));
    }

    Err(AssertError::InvalidSyntax(format!(
        "Invalid value path: {}. Must start with 'status', 'headers.', 'body.', or 'response.time'",
        input
    )))
}

/// 解析断言值（右值）
fn parse_assert_value(input: &str) -> Result<AssertValue, AssertError> {
    let input = input.trim();

    // Null
    if input == "null" {
        return Ok(AssertValue::Null);
    }

    // 布尔值
    if input == "true" {
        return Ok(AssertValue::Bool(true));
    }
    if input == "false" {
        return Ok(AssertValue::Bool(false));
    }

    // 字符串（带引号）
    if (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''))
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
}
