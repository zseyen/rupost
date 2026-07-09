use crate::assertion::types::{AssertError, AssertValue, ValuePath, AssertTarget};
use crate::http::Response;

/// 从断言目标中提取值
pub fn extract_value(target: &dyn AssertTarget, path: &ValuePath) -> Result<AssertValue, AssertError> {
    match path {
        ValuePath::Status => target.extract_status().map(AssertValue::Number),
        ValuePath::Header(name) => target.extract_header(name).map(AssertValue::String),
        ValuePath::Body(segments) => target.extract_body(segments),
        ValuePath::ResponseTime => target.extract_response_time().map(AssertValue::Number),
        ValuePath::StreamEvent => target.extract_stream_event().map(AssertValue::String),
        ValuePath::StreamId => target.extract_stream_id().map(AssertValue::String),
        ValuePath::StreamBody(segments) => target.extract_stream_body(segments),
        ValuePath::StreamLlmContent => target.extract_stream_llm_content().map(AssertValue::String),
        ValuePath::TimingDns => target.extract_timing_dns().map(AssertValue::Number),
        ValuePath::TimingTcp => target.extract_timing_tcp().map(AssertValue::Number),
        ValuePath::TimingTls => target.extract_timing_tls().map(AssertValue::Number),
        ValuePath::TimingTtfb => target.extract_timing_ttfb().map(AssertValue::Number),
        ValuePath::CertDays => target.extract_cert_days().map(AssertValue::Number),
        ValuePath::CertIssuer => target.extract_cert_issuer().map(AssertValue::String),
        ValuePath::CertSubject => target.extract_cert_subject().map(AssertValue::String),
    }
}

impl AssertTarget for Response {
    fn extract_status(&self) -> Result<f64, AssertError> {
        Ok(self.status.code() as f64)
    }

    fn extract_header(&self, name: &str) -> Result<String, AssertError> {
        let value = self
            .headers
            .get(name)
            .ok_or_else(|| AssertError::PathNotFound(format!("Header '{}' not found", name)))?;
        value
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| {
                AssertError::ExtractionError(format!(
                    "Failed to convert header value to string: {}",
                    e
                ))
            })
    }

    fn extract_body(&self, segments: &[String]) -> Result<AssertValue, AssertError> {
        extract_from_json_body(&self.body, segments)
    }

    fn extract_response_time(&self) -> Result<f64, AssertError> {
        Ok(self.duration.as_millis() as f64)
    }

    fn extract_stream_event(&self) -> Result<String, AssertError> {
        let value = self
            .headers
            .get("x-sse-event")
            .ok_or_else(|| AssertError::PathNotFound("stream.event not found".to_string()))?;
        value
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| {
                AssertError::ExtractionError(format!(
                    "Failed to convert stream.event value to string: {}",
                    e
                ))
            })
    }

    fn extract_stream_id(&self) -> Result<String, AssertError> {
        let value = self
            .headers
            .get("x-sse-id")
            .ok_or_else(|| AssertError::PathNotFound("stream.id not found".to_string()))?;
        value
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| {
                AssertError::ExtractionError(format!(
                    "Failed to convert stream.id value to string: {}",
                    e
                ))
            })
    }

    fn extract_stream_body(&self, segments: &[String]) -> Result<AssertValue, AssertError> {
        extract_from_json_body(&self.body, segments)
    }

    fn extract_stream_llm_content(&self) -> Result<String, AssertError> {
        let value = self.headers.get("x-sse-llm-content").ok_or_else(|| {
            AssertError::PathNotFound("stream.llm.content not found".to_string())
        })?;
        let value_str = value.to_str().map_err(|e| {
            AssertError::ExtractionError(format!(
                "Failed to convert stream.llm.content value to string: {}",
                e
            ))
        })?;
        let decoded = url::form_urlencoded::parse(value_str.as_bytes())
            .map(|(key, _)| key)
            .collect::<String>();
        Ok(decoded)
    }

    fn extract_timing_dns(&self) -> Result<f64, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve timing metrics.".to_string())
        })?;
        Ok(report.dns_lookup_duration.as_millis() as f64)
    }

    fn extract_timing_tcp(&self) -> Result<f64, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve timing metrics.".to_string())
        })?;
        Ok(report.tcp_connect_duration.as_millis() as f64)
    }

    fn extract_timing_tls(&self) -> Result<f64, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve timing metrics.".to_string())
        })?;
        let ms = report
            .tls_handshake_duration
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        Ok(ms)
    }

    fn extract_timing_ttfb(&self) -> Result<f64, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve timing metrics.".to_string())
        })?;
        let ms = report.ttfb.map(|d| d.as_millis() as f64).unwrap_or(0.0);
        Ok(ms)
    }

    fn extract_cert_days(&self) -> Result<f64, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve cert metrics.".to_string())
        })?;
        let days = report
            .cert_info
            .as_ref()
            .map(|c| c.days_remaining as f64)
            .unwrap_or(-1.0);
        Ok(days)
    }

    fn extract_cert_issuer(&self) -> Result<String, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve cert metrics.".to_string())
        })?;
        let issuer = report
            .cert_info
            .as_ref()
            .map(|c| c.issuer.clone())
            .unwrap_or_default();
        Ok(issuer)
    }

    fn extract_cert_subject(&self) -> Result<String, AssertError> {
        let report = self.diagnose_report.as_ref().ok_or_else(|| {
            AssertError::PathNotFound("This request did not enable network diagnostics. Add '# @diagnose' to retrieve cert metrics.".to_string())
        })?;
        let subject = report
            .cert_info
            .as_ref()
            .map(|c| c.subject.clone())
            .unwrap_or_default();
        Ok(subject)
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
pub fn json_value_to_assert_value(value: &serde_json::Value) -> Result<AssertValue, AssertError> {
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
            diagnose_report: None,
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
            diagnose_report: None,
        };

        let val = extract_value(&response, &ValuePath::StreamEvent).unwrap();
        assert_eq!(val, AssertValue::String("chat".to_string()));

        let val = extract_value(&response, &ValuePath::StreamId).unwrap();
        assert_eq!(val, AssertValue::String("msg-123".to_string()));

        let val =
            extract_value(&response, &ValuePath::StreamBody(vec!["text".to_string()])).unwrap();
        assert_eq!(val, AssertValue::String("hello".to_string()));
    }

    #[test]
    fn test_extract_diagnose_fields() {
        use crate::http::{CertInfo, DiagnosticsReport};

        let report = DiagnosticsReport {
            url: "https://example.com".to_string(),
            is_https: true,
            is_websocket: false,
            ws_upgrade_success: None,
            resolved_ips: vec!["1.1.1.1".to_string()],
            dns_lookup_duration: Duration::from_millis(10),
            tcp_connect_duration: Duration::from_millis(20),
            tls_handshake_duration: Some(Duration::from_millis(30)),
            cert_info: Some(CertInfo {
                subject: "CN=example.com".to_string(),
                issuer: "CN=Let's Encrypt".to_string(),
                sans: vec!["example.com".to_string()],
                validity_not_after: "2026-08-04".to_string(),
                days_remaining: 35,
            }),
            http_status: Some(200),
            http_version: Some("HTTP/1.1".to_string()),
            ttfb: Some(Duration::from_millis(40)),
            total_duration: Duration::from_millis(100),
        };

        let response = Response {
            status: Status::new(200).unwrap(),
            headers: HeaderMap::new(),
            body: "{}".to_string(),
            duration: Duration::from_millis(100),
            ttfb: Duration::from_millis(0),
            transfer: Duration::from_millis(0),
            diagnose_report: Some(report),
        };

        assert_eq!(
            extract_value(&response, &ValuePath::TimingDns).unwrap(),
            AssertValue::Number(10.0)
        );
        assert_eq!(
            extract_value(&response, &ValuePath::TimingTcp).unwrap(),
            AssertValue::Number(20.0)
        );
        assert_eq!(
            extract_value(&response, &ValuePath::TimingTls).unwrap(),
            AssertValue::Number(30.0)
        );
        assert_eq!(
            extract_value(&response, &ValuePath::TimingTtfb).unwrap(),
            AssertValue::Number(40.0)
        );
        assert_eq!(
            extract_value(&response, &ValuePath::CertDays).unwrap(),
            AssertValue::Number(35.0)
        );
        assert_eq!(
            extract_value(&response, &ValuePath::CertIssuer).unwrap(),
            AssertValue::String("CN=Let's Encrypt".to_string())
        );
        assert_eq!(
            extract_value(&response, &ValuePath::CertSubject).unwrap(),
            AssertValue::String("CN=example.com".to_string())
        );

        // 如果诊断报告为空，应当抛出错误说明未启用 @diagnose
        let empty_response = Response {
            status: Status::new(200).unwrap(),
            headers: HeaderMap::new(),
            body: "{}".to_string(),
            duration: Duration::from_millis(100),
            ttfb: Duration::from_millis(0),
            transfer: Duration::from_millis(0),
            diagnose_report: None,
        };
        assert!(extract_value(&empty_response, &ValuePath::TimingDns).is_err());
    }
}
