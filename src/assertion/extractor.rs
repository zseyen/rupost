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
    }
}

/// 从 JSON body 中提取值
fn extract_from_json_body(body: &str, segments: &[String]) -> Result<AssertValue, AssertError> {
    let json_value: serde_json::Value = serde_json::from_str(body)?;

    let mut current = &json_value;
    for segment in segments {
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
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(
            AssertError::ExtractionError("Cannot compare arrays or objects directly".to_string()),
        ),
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
    fn test_extract_body_path_not_found() {
        let response = create_test_response(200, r#"{"id": 42}"#);
        let result = extract_value(&response, &ValuePath::Body(vec!["missing".to_string()]));
        assert!(result.is_err());
    }
}
