use rupost::{Result, RupostError};

#[test]
fn test_parse_error() {
    let err = RupostError::ParseError("test error".to_string());
    assert_eq!(err.to_string(), "解析错误: test error");
}

#[test]
fn test_invalid_url() {
    let err = RupostError::InvalidUrl("not a url".to_string());
    assert_eq!(err.to_string(), "无效的 URL: not a url");
}

#[test]
fn test_error_conversion_from_anyhow() {
    let anyhow_err = anyhow::anyhow!("test anyhow error");
    let rupost_err: RupostError = anyhow_err.into();
    assert!(rupost_err.to_string().contains("test anyhow error"));
}

#[test]
fn test_result_type() {
    fn returns_error() -> Result<()> {
        Err(RupostError::ParseError("test".to_string()))
    }

    let result = returns_error();
    assert!(result.is_err());
    match result {
        Err(RupostError::ParseError(msg)) => assert_eq!(msg, "test"),
        _ => panic!("Expected ParseError"),
    }
}
