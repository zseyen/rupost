use crate::assertion::extractor::extract_value;
use crate::assertion::types::{AssertExpr, AssertionResult};
use crate::http::Response;

/// 执行断言求值
pub fn evaluate_assertion(assertion: &AssertExpr, response: &Response) -> AssertionResult {
    let raw = format_assertion(assertion);

    match assertion {
        AssertExpr::Compare { left, op, right } => {
            // 提取实际值
            let actual_value = match extract_value(response, left) {
                Ok(v) => v,
                Err(e) => {
                    return AssertionResult::error(raw, e);
                }
            };

            // 比较值
            match actual_value.compare(*op, right) {
                Ok(passed) => {
                    let actual_str = actual_value.to_string();
                    let expected_str = format!("{} {}", op, right);

                    if passed {
                        AssertionResult::success(raw, actual_str, expected_str)
                    } else {
                        let message = format!(
                            "Expected {} to be {}, but got {}",
                            left, expected_str, actual_str
                        );
                        AssertionResult::failure(raw, actual_str, expected_str, message)
                    }
                }
                Err(e) => AssertionResult::error(raw, e),
            }
        }

        AssertExpr::Exists { path } => {
            // 检查路径是否存在
            match extract_value(response, path) {
                Ok(value) => {
                    let actual_str = value.to_string();
                    let expected_str = "exists".to_string();
                    AssertionResult::success(raw, actual_str, expected_str)
                }
                Err(_) => {
                    let message = format!("Expected {} to exist, but it was not found", path);
                    AssertionResult::failure(
                        raw,
                        "not found".to_string(),
                        "exists".to_string(),
                        message,
                    )
                }
            }
        }
    }
}

/// 格式化断言表达式为字符串
fn format_assertion(assertion: &AssertExpr) -> String {
    match assertion {
        AssertExpr::Compare { left, op, right } => {
            format!("{} {} {}", left, op, right)
        }
        AssertExpr::Exists { path } => {
            format!("{} exists", path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::parser::parse_assertion;
    use crate::http::Response;
    use crate::http::types::Status;
    use reqwest::header::HeaderMap;
    use std::time::Duration;

    fn create_test_response(status: u16, body: &str, duration_ms: u64) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        Response {
            status: Status::new(status).unwrap(),
            headers,
            body: body.to_string(),
            duration: Duration::from_millis(duration_ms),
        }
    }

    #[test]
    fn test_evaluate_status_success() {
        let assertion = parse_assertion("status == 200").unwrap();
        let response = create_test_response(200, "{}", 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
        assert_eq!(result.actual, Some("200".to_string()));
    }

    #[test]
    fn test_evaluate_status_failure() {
        let assertion = parse_assertion("status == 200").unwrap();
        let response = create_test_response(404, "{}", 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(!result.passed);
        assert_eq!(result.actual, Some("404".to_string()));
        assert!(result.message.is_some());
    }

    #[test]
    fn test_evaluate_header_contains() {
        let assertion = parse_assertion("headers.content-type contains \"json\"").unwrap();
        let response = create_test_response(200, "{}", 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_body_number() {
        let assertion = parse_assertion("body.id > 0").unwrap();
        let response = create_test_response(200, r#"{"id": 42}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
        assert_eq!(result.actual, Some("42".to_string()));
    }

    #[test]
    fn test_evaluate_body_string() {
        let assertion = parse_assertion("body.name == \"test\"").unwrap();
        let response = create_test_response(200, r#"{"name": "test"}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_response_time() {
        let assertion = parse_assertion("response.time < 200").unwrap();
        let response = create_test_response(200, "{}", 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
        assert_eq!(result.actual, Some("100".to_string()));
    }

    #[test]
    fn test_evaluate_exists_success() {
        let assertion = parse_assertion("body.token exists").unwrap();
        let response = create_test_response(200, r#"{"token": "abc123"}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_exists_failure() {
        let assertion = parse_assertion("body.token exists").unwrap();
        let response = create_test_response(200, r#"{}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(!result.passed);
        assert!(result.message.is_some());
    }

    #[test]
    fn test_evaluate_nested_body() {
        let assertion = parse_assertion("body.user.id == 123").unwrap();
        let response = create_test_response(200, r#"{"user": {"id": 123}}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_path_not_found() {
        let assertion = parse_assertion("body.missing == 123").unwrap();
        let response = create_test_response(200, r#"{}"#, 100);
        let result = evaluate_assertion(&assertion, &response);

        assert!(!result.passed);
        assert!(result.message.is_some());
    }
}
