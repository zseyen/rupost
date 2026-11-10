use crate::Result;
use crate::http::Response;
use colored::*;
pub enum ResponseFormat {
    Compact,
    Verbose,
}

pub struct ResponseFormatter {
    format: ResponseFormat,
    color: bool,
    show_body: bool,
    show_headers: bool,
    show_timing: bool,
}

impl ResponseFormatter {
    pub fn new(format: ResponseFormat) -> Self {
        Self {
            format,
            color: true,
            show_body: true,
            show_headers: true,
            show_timing: true,
        }
    }

    pub fn with_color(mut self, enable: bool) -> Self {
        self.color = enable;
        self
    }

    pub fn with_body(mut self, enable: bool) -> Self {
        self.show_body = enable;
        self
    }

    pub fn with_headers(mut self, enable: bool) -> Self {
        self.show_headers = enable;
        self
    }

    pub fn with_timing(mut self, enable: bool) -> Self {
        self.show_timing = enable;
        self
    }

    pub fn format(&self, response: &Response) -> Result<String> {
        match self.format {
            ResponseFormat::Compact => self.format_compact(response),
            ResponseFormat::Verbose => self.format_verbose(response),
        }
    }

    fn format_compact(&self, response: &Response) -> Result<String> {
        let mut output = Vec::new();
        let status_line = format!(
            "HTTP {} {}",
            response.status.code(),
            response.status.reason_phrase()
        );
        if self.color {
            let colored_status_line = if response.is_success() {
                status_line.green()
            } else if response.is_client_error() {
                status_line.yellow()
            } else {
                status_line.red()
            };
            output.push(colored_status_line.to_string());
        } else {
            output.push(status_line);
        }
        if self.show_timing {
            let timing = format!("Time: {}ms", response.duration.as_millis());
            if self.color {
                let colored_timing = timing.cyan();
                output.push(colored_timing.to_string());
            } else {
                output.push(timing);
            }
        }

        if self.show_body {
            let body = &response.body;
            if !body.is_empty() && body.len() < 200 {
                // 尝试格式化 JSON，失败则显示原始内容
                let formatted_body = self
                    .try_format_json(body)
                    .unwrap_or_else(|_| body.to_string());
                output.push(formatted_body);
            } else if !body.is_empty() {
                output.push(format!("Body: {} bytes", body.len()));
            }
        }

        Ok(output.join("\n"))
    }

    fn format_verbose(&self, response: &Response) -> Result<String> {
        let mut output = Vec::new();
        let status_line = format!(
            "HTTP {} {}",
            response.status.code(),
            response.status.reason_phrase()
        );

        if self.color {
            let colored_status_line = if response.is_success() {
                status_line.green().bold()
            } else if response.is_client_error() {
                status_line.yellow().bold()
            } else {
                status_line.red().bold()
            };
            output.push(colored_status_line.to_string());
        } else {
            output.push(status_line);
        }
        if self.show_timing {
            let timing = format!("Time: {}ms", response.duration.as_millis());
            if self.color {
                let colored_timing = timing.cyan();
                output.push(colored_timing.to_string());
            } else {
                output.push(timing);
            }
        }
        if self.show_headers {
            output.push("".to_string());
            if self.color {
                output.push("Headers:".blue().bold().to_string());
            } else {
                output.push("Headers:".to_string());
            }
            for (key, value) in response.headers.iter() {
                let value_str = value.to_str().unwrap_or("<invalid utf-8>");
                if self.color {
                    output.push(format!("   {}: {}", key, value_str).blue().to_string());
                } else {
                    output.push(format!("   {}: {}", key, value_str));
                }
            }
        }

        if self.show_body {
            let body = &response.body;
            if !body.is_empty() {
                output.push("".to_string());
                if self.color {
                    output.push("Body:".blue().bold().to_string());
                } else {
                    output.push("Body:".to_string());
                }
                // 尝试格式化 JSON，失败则显示原始内容
                let formatted_body = self
                    .try_format_json(body)
                    .unwrap_or_else(|_| body.to_string());
                output.push(formatted_body);
            }
        }

        Ok(output.join("\n"))
    }

    /// 尝试将 body 格式化为漂亮的 JSON
    /// 如果不是有效的 JSON，返回错误
    fn try_format_json(&self, body: &str) -> Result<String> {
        let value: serde_json::Value = serde_json::from_str(body)?;
        serde_json::to_string_pretty(&value).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;
    use std::time::Duration;

    fn make_test_response(status: u16, body: &str, headers: HeaderMap) -> Response {
        Response::new(
            status,
            headers,
            body.to_string(),
            Duration::from_millis(125),
            Duration::ZERO,
            Duration::ZERO,
        )
        .unwrap()
    }

    #[test]
    fn test_format_compact_table_driven() {
        struct Case {
            name: &'static str,
            status: u16,
            body: String,
            color: bool,
            expected_fragments: Vec<&'static str>,
            unexpected_fragments: Vec<&'static str>,
        }

        let cases = vec![
            Case {
                name: "200 OK 短 JSON 美化展开 (无色)",
                status: 200,
                body: r#"{"status":"ready"}"#.to_string(),
                color: false,
                expected_fragments: vec![
                    "HTTP 200 OK",
                    "Time: 125ms",
                    "\"status\": \"ready\"",
                ],
                unexpected_fragments: vec!["Body:", "Headers:"],
            },
            Case {
                name: "404 超过 200 字节长文本截断 (无色)",
                status: 404,
                body: "A".repeat(250),
                color: false,
                expected_fragments: vec![
                    "HTTP 404 Not Found",
                    "Time: 125ms",
                    "Body: 250 bytes",
                ],
                unexpected_fragments: vec!["AAAAA"],
            },
            Case {
                name: "500 畸形 JSON 原样输出",
                status: 500,
                body: "Invalid {JSON} syntax".to_string(),
                color: false,
                expected_fragments: vec![
                    "HTTP 500 Internal Server Error",
                    "Invalid {JSON} syntax",
                ],
                unexpected_fragments: vec!["Body:"],
            },
        ];

        for c in cases {
            let res = make_test_response(c.status, &c.body, HeaderMap::new());
            let formatter = ResponseFormatter::new(ResponseFormat::Compact).with_color(c.color);
            let output = formatter.format(&res).unwrap();

            for exp in c.expected_fragments {
                assert!(
                    output.contains(exp),
                    "[{}] 缺少预期片段: {}\n实际输出:\n{}",
                    c.name,
                    exp,
                    output
                );
            }
            for unexp in c.unexpected_fragments {
                assert!(
                    !output.contains(unexp),
                    "[{}] 包含了非预期片段: {}\n实际输出:\n{}",
                    c.name,
                    unexp,
                    output
                );
            }
        }
    }

    #[test]
    fn test_format_verbose_headers_and_body_indentation() {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("X-Request-Id", "req-12345".parse().unwrap());

        let res = make_test_response(201, r#"{"id":1}"#, headers);
        let formatter = ResponseFormatter::new(ResponseFormat::Verbose).with_color(false);
        let output = formatter.format(&res).unwrap();

        // 验证 Verbose 模式包含明确的 Headers 标题与 3 空格缩进
        assert!(output.contains("HTTP 201 Created"));
        assert!(output.contains("Time: 125ms"));
        assert!(output.contains("Headers:"));
        assert!(output.contains("   content-type: application/json"));
        assert!(output.contains("   x-request-id: req-12345"));
        assert!(output.contains("Body:"));
        assert!(output.contains("{\n  \"id\": 1\n}"));
    }

    #[test]
    fn test_formatter_flags_toggle() {
        let res = make_test_response(200, "hello", HeaderMap::new());
        let formatter = ResponseFormatter::new(ResponseFormat::Compact)
            .with_color(false)
            .with_timing(false)
            .with_body(false);
        let output = formatter.format(&res).unwrap();

        assert_eq!(output.trim(), "HTTP 200 OK");
    }
}
