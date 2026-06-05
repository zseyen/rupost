use crate::assertion::types::{AssertError, AssertValue, ValuePath};
use crate::http::Response;

/// 从响应中提取值
pub fn extract_value(response: &Response, path: &ValuePath) -> Result<AssertValue, AssertError> {
    match path {
        ValuePath::Status => Ok(AssertValue::Number(response.status.code() as f64)),

        ValuePath::Header(name) => {
            let value = response
                .headers
                .get(name)
                .ok_or_else(|| AssertError::PathNotFound(format!("Header '{}' not found", name)))?;
            // HeaderValue 转换为 String
            Ok(AssertValue::String(
                value
                    .to_str()
                    .map_err(|e| {
                        AssertError::ExtractionError(format!(
                            "Failed to convert header value to string: {}",
                            e
                        ))
                    })?
                    .to_string(),
            ))
        }

        ValuePath::Body(segments) => extract_from_json_body(&response.body, segments),

        ValuePath::ResponseTime => Ok(AssertValue::Number(response.duration.as_millis() as f64)),

        ValuePath::StreamEvent => {
            let value = response
                .headers
                .get("x-sse-event")
                .ok_or_else(|| AssertError::PathNotFound("stream.event not found".to_string()))?;
            Ok(AssertValue::String(
                value
                    .to_str()
                    .map_err(|e| {
                        AssertError::ExtractionError(format!(
                            "Failed to convert stream.event value to string: {}",
                            e
                        ))
                    })?
                    .to_string(),
            ))
        }

        ValuePath::StreamId => {
            let value = response
                .headers
                .get("x-sse-id")
                .ok_or_else(|| AssertError::PathNotFound("stream.id not found".to_string()))?;
            Ok(AssertValue::String(
                value
                    .to_str()
                    .map_err(|e| {
                        AssertError::ExtractionError(format!(
                            "Failed to convert stream.id value to string: {}",
                            e
                        ))
                    })?
                    .to_string(),
            ))
        }

        ValuePath::StreamBody(segments) => extract_from_json_body(&response.body, segments),
        ValuePath::StreamLlmContent => {
            let value = response
                .headers
                .get("x-sse-llm-content")
                .ok_or_else(|| AssertError::PathNotFound("stream.llm.content not found".to_string()))?;
            Ok(AssertValue::String(
                value
                    .to_str()
                    .map_err(|e| {
                        AssertError::ExtractionError(format!(
                            "Failed to convert stream.llm.content value to string: {}",
                            e
                        ))
                    })?
                    .to_string(),
            ))
        }
    }
}

/// 从 JSON body 中提取值
fn extract_from_json_body(body: &str, segments: &[String]) -> Result<AssertValue, AssertError> {
    let json_value: serde_json::Value = serde_json::from_str(body)?;

    let mut current = &json_value;
    for segment in segments {
        if let serde_json::Value::Array(arr) = current
            && let Ok(idx) = segment.parse::<usize>()
        {
            current = arr.get(idx).ok_or_else(|| {
                AssertError::PathNotFound(format!("Path 'body.{}' not found", segments.join(".")))
            })?;
            continue;
        }
        current = current.get(segment).ok_or_else(|| {
            AssertError::PathNotFound(format!("Path 'body.{}' not found", segments.join(".")))
        })?;
    }

    json_value_to_assert_value(current)
}

/// 将 serde_json::Value 转换为 AssertValue
fn json_value_to_assert_value(value: &serde_json::Value) -> Result<AssertValue, AssertError> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Ok(AssertValue::Number(f))
            } else {
                Err(AssertError::ExtractionError(
                    "Number cannot be represented as f64".to_string(),
                ))
            }
        }
        serde_json::Value::String(s) => Ok(AssertValue::String(s.clone())),
        serde_json::Value::Bool(b) => Ok(AssertValue::Bool(*b)),
        serde_json::Value::Null => Ok(AssertValue::Null),
        serde_json::Value::Array(_) => Ok(AssertValue::Array),
        serde_json::Value::Object(_) => Ok(AssertValue::Object),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::http::types::Status;
    use reqwest::header::HeaderMap;
    use std::time::Duration;

    fn create_test_response(status: u16, body: &str) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        Response {
            status: Status::new(status).unwrap(),
            headers,
            body: body.to_string(),
            duration: Duration::from_millis(123),
            ttfb: Duration::from_millis(0),
            transfer: Duration::from_millis(0),
        }
    }

    #[test]
    fn test_extract_status() {
        let response = create_test_response(200, "{}");
        let value = extract_value(&response, &ValuePath::Status).unwrap();
        assert_eq!(value, AssertValue::Number(200.0));
    }

    #[test]
    fn test_extract_header() {
        let response = create_test_response(200, "{}");
        let value =
            extract_value(&response, &ValuePath::Header("content-type".to_string())).unwrap();
        assert_eq!(value, AssertValue::String("application/json".to_string()));
    }

    #[test]
    fn test_extract_header_not_found() {
        let response = create_test_response(200, "{}");
        let result = extract_value(&response, &ValuePath::Header("missing".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_response_time() {
        let response = create_test_response(200, "{}");
        let value = extract_value(&response, &ValuePath::ResponseTime).unwrap();
        assert_eq!(value, AssertValue::Number(123.0));
    }

    #[test]
    fn test_extract_body_number() {
        let response = create_test_response(200, r#"{"id": 42}"#);
        let value = extract_value(&response, &ValuePath::Body(vec!["id".to_string()])).unwrap();
        assert_eq!(value, AssertValue::Number(42.0));
    }

    #[test]
    fn test_extract_body_string() {
        let response = create_test_response(200, r#"{"name": "test"}"#);
        let value = extract_value(&response, &ValuePath::Body(vec!["name".to_string()])).unwrap();
        assert_eq!(value, AssertValue::String("test".to_string()));
    }

    #[test]
    fn test_extract_body_bool() {
        let response = create_test_response(200, r#"{"active": true}"#);
        let value = extract_value(&response, &ValuePath::Body(vec!["active".to_string()])).unwrap();
        assert_eq!(value, AssertValue::Bool(true));
    }

    #[test]
    fn test_extract_body_null() {
        let response = create_test_response(200, r#"{"data": null}"#);
        let value = extract_value(&response, &ValuePath::Body(vec!["data".to_string()])).unwrap();
        assert_eq!(value, AssertValue::Null);
    }

    #[test]
    fn test_extract_nested_body() {
        let response = create_test_response(200, r#"{"user": {"id": 123, "name": "test"}}"#);
        let value = extract_value(
            &response,
            &ValuePath::Body(vec!["user".to_string(), "id".to_string()]),
        )
        .unwrap();
        assert_eq!(value, AssertValue::Number(123.0));
    }

    #[test]
    fn test_extract_array_index() {
        let response = create_test_response(200, r#"["first", "second", "third"]"#);
        let value = extract_value(&response, &ValuePath::Body(vec!["0".to_string()])).unwrap();
        assert_eq!(value, AssertValue::String("first".to_string()));

        let value2 = extract_value(&response, &ValuePath::Body(vec!["1".to_string()])).unwrap();
        assert_eq!(value2, AssertValue::String("second".to_string()));
    }

    #[test]
    fn test_extract_nested_array_index() {
        let response = create_test_response(
            200,
            r#"{"items": [{"name": "item_a"}, {"name": "item_b"}]}"#,
        );
        let value = extract_value(
            &response,
            &ValuePath::Body(vec![
                "items".to_string(),
                "1".to_string(),
                "name".to_string(),
            ]),
        )
        .unwrap();
        assert_eq!(value, AssertValue::String("item_b".to_string()));
    }

    #[test]
    fn test_extract_bracket_syntax() {
        let response = create_test_response(
            200,
            r#"{"users": [{"profile": {"name": "Charlie"}}, {"profile": {"name": "Delta"}}]}"#,
        );
        let segments = crate::utils::jsonpath::parse_jsonpath_to_segments("users[1].profile.name");
        assert_eq!(segments, vec!["users", "1", "profile", "name"]);

        let value = extract_value(&response, &ValuePath::Body(segments)).unwrap();
        assert_eq!(value, AssertValue::String("Delta".to_string()));
    }

    #[test]
    fn test_extract_body_path_not_found() {
        let response = create_test_response(200, r#"{"id": 42}"#);
        let result = extract_value(&response, &ValuePath::Body(vec!["missing".to_string()]));
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_stream_fields() {
        let mut headers = HeaderMap::new();
        headers.insert("x-sse-event", "chat".parse().unwrap());
        headers.insert("x-sse-id", "msg-123".parse().unwrap());
        let response = Response {
            status: Status::new(200).unwrap(),
            headers,
            body: r#"{"text": "hello"}"#.to_string(),
            duration: Duration::from_millis(0),
            ttfb: Duration::from_millis(0),
            transfer: Duration::from_millis(0),
        };

        let val = extract_value(&response, &ValuePath::StreamEvent).unwrap();
        assert_eq!(val, AssertValue::String("chat".to_string()));

        let val = extract_value(&response, &ValuePath::StreamId).unwrap();
        assert_eq!(val, AssertValue::String("msg-123".to_string()));

        let val =
            extract_value(&response, &ValuePath::StreamBody(vec!["text".to_string()])).unwrap();
        assert_eq!(val, AssertValue::String("hello".to_string()));
    }
}
