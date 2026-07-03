use rupost::parser::types::ParsedRequest;
use rupost::{Result, RupostError};
use tracing::debug;

pub fn parse_args(args: &[String]) -> Result<ParsedRequest> {
    let mut filtered_args = Vec::new();
    let mut is_sse_forced = false;
    let mut is_ws_forced = false;

    for arg in args {
        if arg == "--sse" {
            is_sse_forced = true;
        } else if arg == "--ws" || arg == "--websocket" {
            is_ws_forced = true;
        } else {
            filtered_args.push(arg.clone());
        }
    }

    let mut parsed_request = if filtered_args.first().map(|s| s == "curl").unwrap_or(false) {
        debug!("Detected curl-style command");
        parse_curl(filtered_args[1..].to_vec())?
    } else if filtered_args.first().map(|s| s == "http").unwrap_or(false) {
        debug!("Detected httpie-style command");
        parse_httpie(filtered_args[1..].to_vec())?
    } else {
        // 根据参数特征判断是 curl 风格还是 httpie 风格
        let is_curl = filtered_args.iter().any(|a| a.starts_with('-'));
        if is_curl {
            debug!("Using curl parser");
            parse_curl(filtered_args)?
        } else {
            debug!("Using httpie parser");
            parse_httpie(filtered_args)?
        }
    };

    if is_sse_forced {
        parsed_request.metadata.sse = true;
    }
    if is_ws_forced
        || parsed_request.url.starts_with("ws://")
        || parsed_request.url.starts_with("wss://")
    {
        parsed_request.metadata.websocket = true;
    }

    Ok(parsed_request)
}

fn parse_curl(args: Vec<String>) -> Result<ParsedRequest> {
    let mut method = String::from("GET");
    let mut url = String::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut data_parts = Vec::new();
    let mut force_get = false;

    let mut args_iter = args.into_iter().peekable();

    while let Some(arg) = args_iter.next() {
        match arg.as_str() {
            // HTTP Method
            "-X" | "--request" => {
                if let Some(m) = args_iter.next() {
                    method = m.to_uppercase();
                }
            }
            // Header
            "-H" | "--header" => {
                if let Some(header) = args_iter.next()
                    && let Some((key, value)) = header.split_once(':')
                {
                    headers.push((key.trim().to_string(), value.trim().to_string()));
                }
            }
            // Data (body or query)
            "-d" | "--data" | "--data-raw" => {
                if let Some(data) = args_iter.next() {
                    data_parts.push(data);
                }
            }
            // Force GET even with data
            "-G" | "--get" => {
                force_get = true;
            }
            // 其他未知选项暂时忽略
            s if s.starts_with('-') => {
                // 如果是 -X=POST 这种形式
                if let Some((opt, val)) = s.split_once('=') {
                    match opt {
                        "-X" | "--request" => method = val.to_uppercase(),
                        "-d" | "--data" | "--data-raw" => data_parts.push(val.to_string()),
                        unknown => {
                            tracing::warn!("Ignored unsupported curl option: {}", unknown);
                        }
                    }
                } else {
                    tracing::warn!("Ignored unsupported curl option: {}", s);
                    // 已知一些带参数的 curl 选项，跳过它们的值，防止值污染真正的 URL 字段
                    let has_value = [
                        "-u",
                        "--user",
                        "-o",
                        "--output",
                        "-m",
                        "--max-time",
                        "--connect-timeout",
                        "-A",
                        "--user-agent",
                        "-e",
                        "--referer",
                        "-b",
                        "--cookie",
                        "-c",
                        "--cookie-jar",
                        "--data-urlencode",
                        "--data-binary",
                        "-F",
                        "--form",
                    ]
                    .contains(&s);
                    if has_value {
                        let _skipped_val = args_iter.next();
                        tracing::debug!(
                            "Skipped value for unsupported option {}: {:?}",
                            s,
                            _skipped_val
                        );
                    }
                }
            }
            // URL (位置参数)
            _ => {
                if url.is_empty() {
                    url = arg;
                }
            }
        }
    }

    // 如果有 data 且没有强制 GET，默认使用 POST
    if !data_parts.is_empty() && method == "GET" && !force_get {
        method = String::from("POST");
    }

    if url.is_empty() {
        return Err(RupostError::ParseError("URL is required".to_string()));
    }

    // Construct ParsedRequest
    let mut parsed = ParsedRequest::new(0); // Line number 0 for CLI
    parsed.method = Some(method);
    parsed.url = url;
    parsed.headers = headers;

    // 处理 data
    if force_get && !data_parts.is_empty() {
        let query_string = data_parts.join("&");
        if parsed.url.contains('?') {
            parsed.url.push('&');
        } else {
            parsed.url.push('?');
        }
        parsed.url.push_str(&query_string);
    } else if !data_parts.is_empty() {
        let body = data_parts.join("&");
        parsed.body = Some(body);
    }

    // 默认 content type 如果有 body
    if parsed.body.is_some()
        && !parsed
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("Content-Type"))
    {
        parsed.headers.push((
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ));
    }

    Ok(parsed)
}

fn is_key_value_param(arg: &str) -> bool {
    // 先排除 URL 模式
    // 1. http:// 或 https://
    if arg.starts_with("http://") || arg.starts_with("https://") {
        return false;
    }
    // 2. 所有以 : 开头的参数均判定为冒号本地快捷键（URL），不是键值对参数
    if arg.starts_with(':') {
        return false;
    }
    // 3. 包含 :// 的 URL（其他协议）
    if arg.contains("://") {
        return false;
    }
    // 4. 域名:端口 格式 (如 example.com:8080)
    if let Some((host, port)) = arg.rsplit_once(':')
        && !host.is_empty()
        && port.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }

    // 按优先级检查键值对分隔符：== :=  = :
    arg.contains("==") || arg.contains(":=") || arg.contains('=') || arg.contains(':')
}

fn parse_httpie(args: Vec<String>) -> Result<ParsedRequest> {
    let mut method = String::from("GET"); // Default method
    let mut url = String::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut query_params: Vec<(String, String)> = Vec::new();
    let mut body_parts = serde_json::Map::new();

    let mut args_iter = args.into_iter().peekable();

    // Step 1: 检查第一个参数是否为 HTTP Method
    if let Some(first) = args_iter.peek() {
        let is_method = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"]
            .contains(&first.to_uppercase().as_str());
        if is_method {
            method = args_iter.next().unwrap().to_uppercase();
        }
    }

    // Step 2: 下一个非键值对参数即为 URL
    if let Some(next) = args_iter.peek()
        && !is_key_value_param(next)
    {
        url = args_iter.next().unwrap();
    }

    // Step 3: 处理剩余的键值对参数
    for arg in args_iter {
        if let Some((key, value)) = arg.split_once("==") {
            query_params.push((key.to_string(), value.to_string()));
        } else if let Some((key, value)) = arg.split_once(":=") {
            let val = serde_json::from_str::<serde_json::Value>(value)
                .map_err(|e| RupostError::ParseError(format!(
                    "Invalid JSON value for key '{}': {}. If it's a string, use '=' instead of ':='.",
                    key, e
                )))?;
            body_parts.insert(key.to_string(), val);
        } else if let Some((key, value)) = arg.split_once('=') {
            body_parts.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        } else if let Some((key, value)) = arg.split_once(':') {
            headers.push((key.to_string(), value.to_string()));
        }
    }

    // If body_parts is not empty, method implicitly becomes POST if it was GET
    if !body_parts.is_empty() && method == "GET" {
        method = String::from("POST");
    }

    if url.is_empty() {
        return Err(RupostError::ParseError("URL is required".to_string()));
    }

    // Append query params to URL manually if needed
    if !query_params.is_empty() {
        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        let qs: Vec<String> = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        url.push_str(&qs.join("&"));
    }

    debug!(method = %method, url = %url, "Parsed httpie arguments");

    // Construct ParsedRequest
    let mut parsed = ParsedRequest::new(0);
    parsed.method = Some(method);
    parsed.url = url;
    parsed.headers = headers;

    // 添加 body (JSON)
    if !body_parts.is_empty() {
        let json_body = serde_json::to_string(&body_parts)
            .map_err(|e| RupostError::ParseError(e.to_string()))?;
        parsed.body = Some(json_body);
        if !parsed
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("Content-Type"))
        {
            parsed
                .headers
                .push(("Content-Type".to_string(), "application/json".to_string()));
        }
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_httpie() {
        // Test case: POST example.com id:=1 name=foo token:123 q==search
        let args = vec![
            "POST".to_string(),
            "example.com".to_string(),
            "id:=1".to_string(),
            "name=foo".to_string(),
            "token:123".to_string(),
            "q==search".to_string(),
        ];
        let req = parse_httpie(args).unwrap();
        assert_eq!(req.method_or_default(), "POST");

        // Test case: Implicit POST (because body present)
        let args2 = vec!["example.com".to_string(), "name=foo".to_string()];
        let req2 = parse_httpie(args2).unwrap();
        assert_eq!(req2.method_or_default(), "POST");

        // Test case: GET
        let args3 = vec!["example.com".to_string()];
        let req3 = parse_httpie(args3).unwrap();
        assert_eq!(req3.method_or_default(), "GET");
    }

    #[test]
    fn test_parse_curl() {
        // Test case: curl -X POST -H "Content-Type: application/json" -d '{"name":"foo"}' example.com
        let args = vec![
            "-X".to_string(),
            "POST".to_string(),
            "-H".to_string(),
            "Content-Type: application/json".to_string(),
            "-d".to_string(),
            r#"{"name":"foo"}"#.to_string(),
            "example.com".to_string(),
        ];
        let req = parse_curl(args).unwrap();
        assert_eq!(req.method_or_default(), "POST");

        // Test case: Implicit POST (because -d present)
        let args2 = vec![
            "example.com".to_string(),
            "-d".to_string(),
            "name=foo".to_string(),
        ];
        let req2 = parse_curl(args2).unwrap();
        assert_eq!(req2.method_or_default(), "POST");

        // Test case: GET with -G flag even with data (作为 query 参数)
        let args3 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search".to_string(),
            "example.com".to_string(),
        ];
        let request3 = parse_curl(args3).unwrap();
        assert!(request3.url.contains("q=search"));

        // Test case: Simple GET
        let args4 = vec!["example.com".to_string()];
        let req4 = parse_curl(args4).unwrap();
        assert_eq!(req4.method_or_default(), "GET");

        // Test case: -G with multiple -d flags (多个 query 参数)
        let args5 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search".to_string(),
            "-d".to_string(),
            "page=1".to_string(),
            "example.com".to_string(),
        ];
        let request5 = parse_curl(args5).unwrap();
        assert!(request5.url.contains("q=search"));
        assert!(request5.url.contains("page=1"));

        // Test case: -G with combined data (q=search&page=1 in one -d)
        let args6 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search&page=1".to_string(),
            "example.com".to_string(),
        ];
        let request6 = parse_curl(args6).unwrap();
        assert!(request6.url.contains("q=search"));
        assert!(request6.url.contains("page=1"));
    }

    #[test]
    fn test_is_key_value_param() {
        // URL 格式不应被识别为键值对
        assert!(!is_key_value_param("http://example.com"));
        assert!(!is_key_value_param("https://example.com/api"));
        assert!(!is_key_value_param(":/api/users")); // localhost 简写
        assert!(!is_key_value_param(":3000")); // 端口简写
        assert!(!is_key_value_param(":8080"));
        assert!(!is_key_value_param("localhost:3000"));
        assert!(!is_key_value_param("example.com:8080"));
        assert!(!is_key_value_param("192.168.1.1:9000"));

        // 键值对参数应被正确识别
        assert!(is_key_value_param("key=value")); // body field
        assert!(is_key_value_param("q==search")); // query param
        assert!(is_key_value_param("id:=123")); // JSON field
        assert!(is_key_value_param("Content-Type:application/json")); // header
    }

    #[test]
    fn test_parse_httpie_with_urls() {
        // Test: http:// URL
        let args = vec!["http://example.com".to_string()];
        parse_httpie(args).unwrap();

        // Test: https:// URL with path
        let args2 = vec!["https://api.example.com/users".to_string()];
        parse_httpie(args2).unwrap();

        // Test: :port shorthand (localhost:port)
        let args3 = vec![":3000".to_string()];
        parse_httpie(args3).unwrap();

        // Test: host:port format
        let args4 = vec!["localhost:8080".to_string(), "name=test".to_string()];
        parse_httpie(args4).unwrap();

        // Test: :/ shorthand (localhost/)
        let args5 = vec![":/api/users".to_string()];
        parse_httpie(args5).unwrap();
    }

    #[test]
    fn test_parse_httpie_strict_json_error() {
        // 传入非法 JSON 值
        let args = vec!["example.com".to_string(), "active:=tru".to_string()];
        let result = parse_httpie(args);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("Invalid JSON value"));
    }

    #[test]
    fn test_parse_curl_unsupported_with_value() {
        // 传入带参数的不支持选项 -u myuser，以及不带参数的不支持选项 --compressed
        let args = vec![
            "-u".to_string(),
            "myuser".to_string(),
            "--compressed".to_string(),
            "example.com".to_string(),
        ];
        let request = parse_curl(args).unwrap();
        // 验证没有因 -u 的值而污染真正的 URL 解析
        assert_eq!(request.url, "example.com");
    }
}
