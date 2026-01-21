use crate::http::Response;
use anyhow::Result;
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
            let timeing = format!("Time: {}ms", response.duration.as_millis());
            if self.color {
                let colored_timing = timeing.cyan();
                output.push(colored_timing.to_string());
            } else {
                output.push(timeing);
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
            let timeing = format!("Time: {}ms", response.duration.as_millis());
            if self.color {
                let colored_timing = timeing.cyan();
                output.push(colored_timing.to_string());
            } else {
                output.push(timeing);
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
