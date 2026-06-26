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
                    if let crate::assertion::types::AssertError::PathNotFound(_) = e {
                        if right == &crate::assertion::types::AssertValue::None
                            || right == &crate::assertion::types::AssertValue::Null
                        {
                            crate::assertion::types::AssertValue::None
                        } else {
                            return AssertionResult::error(raw, e);
                        }
                    } else {
                        return AssertionResult::error(raw, e);
                    }
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
            ttfb: Duration::from_millis(0),
            transfer: Duration::from_millis(0),
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
    fn test_evaluate_exists_object_and_array_success() {
        let response = create_test_response(200, r#"{"user": {"id": 123}, "items": [1, 2]}"#, 100);

        let assertion_obj = parse_assertion("body.user exists").unwrap();
        let result_obj = evaluate_assertion(&assertion_obj, &response);
        assert!(result_obj.passed);
        assert_eq!(result_obj.actual, Some("object".to_string()));

        let assertion_arr = parse_assertion("body.items exists").unwrap();
        let result_arr = evaluate_assertion(&assertion_arr, &response);
        assert!(result_arr.passed);
        assert_eq!(result_arr.actual, Some("array".to_string()));
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

    #[test]
    fn test_evaluate_none_and_null_semantics() {
        let response_empty = create_test_response(200, r#"{}"#, 100);
        let response_null = create_test_response(200, r#"{"token": null}"#, 100);
        let response_value = create_test_response(200, r#"{"token": "abc"}"#, 100);

        // 1. 验证缺失字段 == None
        let assertion_missing_none = parse_assertion("body.token == None").unwrap();
        let result = evaluate_assertion(&assertion_missing_none, &response_empty);
        assert!(result.passed);

        // 2. 验证缺失字段 == null (泛化语义)
        let assertion_missing_null = parse_assertion("body.token == null").unwrap();
        let result = evaluate_assertion(&assertion_missing_null, &response_empty);
        assert!(result.passed);

        // 3. 验证显式 null 字段 == None
        let result = evaluate_assertion(&assertion_missing_none, &response_null);
        assert!(result.passed);

        // 4. 验证显式 null 字段 == null
        let result = evaluate_assertion(&assertion_missing_null, &response_null);
        assert!(result.passed);

        // 5. 验证非空字段 == None 应该失败
        let result = evaluate_assertion(&assertion_missing_none, &response_value);
        assert!(!result.passed);

        // 6. 验证非空字段 != None 应该成功
        let assertion_not_none = parse_assertion("body.token != None").unwrap();
        let result = evaluate_assertion(&assertion_not_none, &response_value);
        assert!(result.passed);

        // 7. 验证缺失字段 != None 应该失败
        let result = evaluate_assertion(&assertion_not_none, &response_empty);
        assert!(!result.passed);

        // 8. 验证带双引号的 "None" 仅作普通字符串校验
        let assertion_quoted_none = parse_assertion("body.token == \"None\"").unwrap();
        let response_str_none = create_test_response(200, r#"{"token": "None"}"#, 100);

        let result = evaluate_assertion(&assertion_quoted_none, &response_str_none);
        assert!(result.passed);

        let result = evaluate_assertion(&assertion_quoted_none, &response_empty);
        assert!(!result.passed);
    }

    #[test]
    fn test_evaluate_assertions_batch() {
        let response = create_test_response(200, r#"{"id": 42}"#, 100);
        let resolved = vec!["status == 200".to_string(), "body.id > 0".to_string()];
        let results = evaluate_assertions(&resolved, &response);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.passed));
    }

    #[test]
    fn test_evaluate_sse_handshake_assertions() {
        let response = create_test_response(200, "{}", 100);
        let resolved = vec![
            "status == 200".to_string(),
            "stream.event == \"message\"".to_string(),
        ];
        let results = evaluate_sse_handshake_assertions(&resolved, &response);
        assert_eq!(results.len(), 1); // 应该过滤掉包含 stream. 的断言
        assert!(results[0].passed);
    }

    #[test]
    fn test_evaluate_sse_event_assertions() {
        let response = create_test_response(200, r#"{"data": "stream-delta"}"#, 100);
        let resolved = vec![
            "status == 200".to_string(), // 不包含 stream. 不应该在此处理
            "stream.data == \"stream-delta\"".to_string(),
            "stream.llm.content == \"hello\"".to_string(), // 包含 llm.content 不应该在此处理
        ];
        let results = evaluate_sse_event_assertions(&resolved, &response, 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert_eq!(results[0].stream_event_index, Some(5));
    }

    #[test]
    fn test_evaluate_sse_llm_content_assertions() {
        let response = create_test_response(200, r#"{"reply": "hello"}"#, 100);
        let resolved = vec![
            "status == 200".to_string(),
            "stream.llm.content == \"hello\"".to_string(), // 应该匹配
        ];
        let results = evaluate_sse_llm_content_assertions(&resolved, &response);
        assert_eq!(results.len(), 1);
        // 因为我们只是匹配断言字符串包含 "stream.llm.content"，实际评估时它会在 response (比如虚拟响应) 上求值
        // 这里的测试只是为了验证断言过滤
    }
}

use crate::assertion::parser::parse_assertion;

/// 评估常规的非流式断言
pub fn evaluate_assertions(
    resolved_assertions: &[String],
    response: &Response,
) -> Vec<AssertionResult> {
    let mut assertion_results = Vec::new();
    for assertion_str in resolved_assertions {
        match parse_assertion(assertion_str) {
            Ok(assertion_expr) => {
                let result = evaluate_assertion(&assertion_expr, response);
                assertion_results.push(result);
            }
            Err(e) => {
                assertion_results.push(AssertionResult::error(assertion_str.clone(), e));
            }
        }
    }
    assertion_results
}

/// 评估 SSE 握手断言 (过滤掉以 "stream." 开头的断言)
pub fn evaluate_sse_handshake_assertions(
    resolved_assertions: &[String],
    handshake_response: &Response,
) -> Vec<AssertionResult> {
    let mut assertion_results = Vec::new();
    for assertion_str in resolved_assertions {
        if !assertion_str.contains("stream.") {
            match parse_assertion(assertion_str) {
                Ok(assertion_expr) => {
                    let result = evaluate_assertion(&assertion_expr, handshake_response);
                    assertion_results.push(result);
                }
                Err(e) => {
                    assertion_results.push(AssertionResult::error(assertion_str.clone(), e));
                }
            }
        }
    }
    assertion_results
}

/// 评估实时流事件断言 (包含 "stream." 但排除 "stream.llm.content")
pub fn evaluate_sse_event_assertions(
    resolved_assertions: &[String],
    virtual_response: &Response,
    event_count: usize,
) -> Vec<AssertionResult> {
    let mut assertion_results = Vec::new();
    for assertion_str in resolved_assertions {
        if assertion_str.contains("stream.") && !assertion_str.contains("stream.llm.content") {
            match parse_assertion(assertion_str) {
                Ok(assertion_expr) => {
                    let result = evaluate_assertion(&assertion_expr, virtual_response);
                    assertion_results.push(result.with_stream_index(event_count));
                }
                Err(e) => {
                    assertion_results.push(
                        AssertionResult::error(assertion_str.clone(), e)
                            .with_stream_index(event_count),
                    );
                }
            }
        }
    }
    assertion_results
}

/// 评估包含 "stream.llm.content" 的断言
pub fn evaluate_sse_llm_content_assertions(
    resolved_assertions: &[String],
    final_response: &Response,
) -> Vec<AssertionResult> {
    let mut assertion_results = Vec::new();
    for assertion_str in resolved_assertions {
        if assertion_str.contains("stream.llm.content") {
            match parse_assertion(assertion_str) {
                Ok(assertion_expr) => {
                    let result = evaluate_assertion(&assertion_expr, final_response);
                    assertion_results.push(result);
                }
                Err(e) => {
                    assertion_results.push(AssertionResult::error(assertion_str.clone(), e));
                }
            }
        }
    }
    assertion_results
}

