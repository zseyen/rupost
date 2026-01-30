use clap::{Parser, Subcommand};
use rupost::http::Response;
use rupost::parser::types::ParsedRequest;
use rupost::runner::TestExecutor;
use rupost::utils::{ResponseFormat, ResponseFormatter};
use rupost::variable::VariableContext;
use rupost::{Result, RupostError};
use tracing::{debug, error, info};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 可选参数用于默认运行(curl/httpie 风格)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run requests from a file
    #[command(alias = "t")]
    Test {
        /// Path to the .http file
        path: String,

        /// Environment name (e.g., dev, staging, prod)
        #[arg(short, long)]
        env: Option<String>,

        /// Variable overrides (key=value)
        #[arg(long, value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// Show detailed request/response information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Manage request history
    #[command(alias = "h")]
    History {
        #[command(subcommand)]
        command: HistoryCommands,
    },

    /// Generate test file from history
    #[command(alias = "g")]
    Generate(GenerateArgs),
}

#[derive(Subcommand)]
pub enum HistoryCommands {
    /// List request history
    #[command(alias = "l")]
    List {
        /// Limit the number of entries
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Show latest entries first (Effective mainly for UI display)
        #[arg(short, long)]
        reverse: bool,
    },
}

#[derive(Parser, Debug)]
pub struct GenerateArgs {
    /// Output file path
    pub output_file: String,

    /// Number of recent requests to include
    #[arg(short, long, default_value = "1")]
    pub last: usize,

    /// Interactive selection mode
    #[arg(short, long)]
    pub interactive: bool,
}

struct CliRunner {
    formatter: ResponseFormatter,
    executor: TestExecutor,
}

impl CliRunner {
    fn new() -> Self {
        Self {
            formatter: ResponseFormatter::new(ResponseFormat::Verbose),
            executor: TestExecutor::new(),
        }
    }

    async fn run(&self, args: Vec<String>) -> Result<()> {
        info!("Parsing command line arguments");
        let parsed_request = self.parse_args(args)?;

        // Setup empty context for CLI run
        let mut context = VariableContext::new();

        info!(url = %parsed_request.url, method = ?parsed_request.method_or_default(), "Executing HTTP request");

        // Execute with source="cli"
        let result = self
            .executor
            .execute_one(parsed_request, 1, &mut context, Some("cli".to_string()))
            .await;

        if result.success {
            if let Some(response) = result.response {
                self.format_response(response);
            }
        } else {
            error!("Request failed: {}", result.error.unwrap_or_default());
        }
        Ok(())
    }

    fn format_response(&self, response: Response) {
        match self.formatter.format(&response) {
            Ok(output) => println!("{}", output),
            Err(e) => error!("Failed to format response: {}", e),
        }
    }

    fn parse_args(&self, args: Vec<String>) -> Result<ParsedRequest> {
        let args = if args.first().map(|s| s == "curl").unwrap_or(false) {
            debug!("Detected curl-style command");
            args[1..].to_vec()
        } else if args.first().map(|s| s == "http").unwrap_or(false) {
            debug!("Detected httpie-style command");
            args[1..].to_vec()
        } else {
            args
        };

        // 根据参数特征判断是 curl 风格还是 httpie 风格
        let is_curl = args.iter().any(|a| a.starts_with('-'));

        if is_curl {
            debug!("Using curl parser");
            self.parse_curl(args)
        } else {
            debug!("Using httpie parser");
            self.parse_httpie(args)
        }
    }
    fn parse_curl(&self, args: Vec<String>) -> Result<ParsedRequest> {
        let mut method = String::from("GET");
        let mut url = String::new();
        let mut headers: Vec<(String, String)> = Vec::new(); // Explicit type annotation
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
                            _ => {} // 忽略其他选项
                        }
                    }
                    // 其他带参数的选项，跳过下一个参数
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
            // -G 模式: 处理为 query params，这需要修改 url
            // 为了简单，直接拼接到 url (有点 naive 但能用)
            let query_string = data_parts.join("&");
            if parsed.url.contains('?') {
                parsed.url.push('&');
            } else {
                parsed.url.push('?');
            }
            parsed.url.push_str(&query_string);
        } else if !data_parts.is_empty() {
            // 非 -G 模式: 作为 body
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
    /// 判断参数是否为键值对参数（headers, query, body）
    /// URL 格式不算键值对：http://, https://, :/, :port
    fn is_key_value_param(arg: &str) -> bool {
        // 先排除 URL 模式
        // 1. http:// 或 https://
        if arg.starts_with("http://") || arg.starts_with("https://") {
            return false;
        }
        // 2. :/ 开头的本地路径简写 (如 :/api -> localhost/api)
        if arg.starts_with(":/") {
            return false;
        }
        // 3. :port 格式 (如 :3000 -> localhost:3000)
        if arg.starts_with(':') && arg[1..].chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        // 4. 包含 :// 的 URL（其他协议）
        if arg.contains("://") {
            return false;
        }
        // 5. 域名:端口 格式 (如 example.com:8080)
        if let Some((host, port)) = arg.rsplit_once(':') {
            // 如果冒号后面全是数字，且前面不为空，认为是 host:port
            if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }

        // 按优先级检查键值对分隔符：== :=  = :
        // 注意：需要先检查多字符分隔符
        arg.contains("==") || arg.contains(":=") || arg.contains('=') || arg.contains(':')
    }

    fn parse_httpie(&self, args: Vec<String>) -> Result<ParsedRequest> {
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
            && !Self::is_key_value_param(next)
        {
            url = args_iter.next().unwrap();
        }

        // Step 3: 处理剩余的键值对参数
        for arg in args_iter {
            if let Some((key, value)) = arg.split_once("==") {
                // Query parameter
                query_params.push((key.to_string(), value.to_string()));
            } else if let Some((key, value)) = arg.split_once(":=") {
                // Raw JSON field
                body_parts.insert(
                    key.to_string(),
                    serde_json::from_str::<serde_json::Value>(value)
                        .unwrap_or(serde_json::Value::String(value.to_string())),
                );
            } else if let Some((key, value)) = arg.split_once('=') {
                // String data field
                body_parts.insert(
                    key.to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            } else if let Some((key, value)) = arg.split_once(':') {
                // Header
                headers.push((key.to_string(), value.to_string()));
            }
            // 非键值对参数在 Step 2 之后应该不存在，忽略
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
}

pub async fn run(args: Vec<String>) -> Result<()> {
    let runner = CliRunner::new();
    runner.run(args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_httpie() {
        let runner = CliRunner::new();
        // Test case: POST example.com id:=1 name=foo token:123 q==search
        let args = vec![
            "POST".to_string(),
            "example.com".to_string(),
            "id:=1".to_string(),
            "name=foo".to_string(),
            "token:123".to_string(),
            "q==search".to_string(),
        ];
        runner.parse_httpie(args).unwrap();

        // Test case: Implicit POST (because body present)
        let args2 = vec!["example.com".to_string(), "name=foo".to_string()];
        runner.parse_httpie(args2).unwrap();

        // Test case: GET
        let args3 = vec!["example.com".to_string()];
        runner.parse_httpie(args3).unwrap();
    }

    #[test]
    fn test_parse_curl() {
        let runner = CliRunner::new();

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
        runner.parse_curl(args).unwrap();

        // Test case: Implicit POST (because -d present)
        let args2 = vec![
            "example.com".to_string(),
            "-d".to_string(),
            "name=foo".to_string(),
        ];
        runner.parse_curl(args2).unwrap();

        // Test case: GET with -G flag even with data (作为 query 参数)
        let args3 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search".to_string(),
            "example.com".to_string(),
        ];
        let request3 = runner.parse_curl(args3).unwrap();
        // Since query_params are merged into URL, check URL
        assert!(request3.url.contains("q=search"));

        // Test case: Simple GET
        let args4 = vec!["example.com".to_string()];
        runner.parse_curl(args4).unwrap();

        // Test case: -G with multiple -d flags (多个 query 参数)
        let args5 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search".to_string(),
            "-d".to_string(),
            "page=1".to_string(),
            "example.com".to_string(),
        ];
        let request5 = runner.parse_curl(args5).unwrap();
        // Check URL for multiple params
        assert!(request5.url.contains("q=search"));
        assert!(request5.url.contains("page=1"));

        // Test case: -G with combined data (q=search&page=1 in one -d)
        let args6 = vec![
            "-G".to_string(),
            "-d".to_string(),
            "q=search&page=1".to_string(),
            "example.com".to_string(),
        ];
        let request6 = runner.parse_curl(args6).unwrap();
        assert!(request6.url.contains("q=search"));
        assert!(request6.url.contains("page=1"));
    }

    #[test]
    fn test_is_key_value_param() {
        // URL 格式不应被识别为键值对
        assert!(!CliRunner::is_key_value_param("http://example.com"));
        assert!(!CliRunner::is_key_value_param("https://example.com/api"));
        assert!(!CliRunner::is_key_value_param(":/api/users")); // localhost 简写
        assert!(!CliRunner::is_key_value_param(":3000")); // 端口简写
        assert!(!CliRunner::is_key_value_param(":8080"));
        assert!(!CliRunner::is_key_value_param("localhost:3000"));
        assert!(!CliRunner::is_key_value_param("example.com:8080"));
        assert!(!CliRunner::is_key_value_param("192.168.1.1:9000"));

        // 键值对参数应被正确识别
        assert!(CliRunner::is_key_value_param("key=value")); // body field
        assert!(CliRunner::is_key_value_param("q==search")); // query param
        assert!(CliRunner::is_key_value_param("id:=123")); // JSON field
        assert!(CliRunner::is_key_value_param(
            "Content-Type:application/json"
        )); // header
    }

    #[test]
    fn test_parse_httpie_with_urls() {
        let runner = CliRunner::new();

        // Test: http:// URL
        let args = vec!["http://example.com".to_string()];
        runner.parse_httpie(args).unwrap();

        // Test: https:// URL with path
        let args2 = vec!["https://api.example.com/users".to_string()];
        runner.parse_httpie(args2).unwrap();

        // Test: :port shorthand (localhost:port)
        let args3 = vec![":3000".to_string()];
        runner.parse_httpie(args3).unwrap();

        // Test: host:port format
        let args4 = vec!["localhost:8080".to_string(), "name=test".to_string()];
        runner.parse_httpie(args4).unwrap();

        // Test: :/ shorthand (localhost/)
        let args5 = vec![":/api/users".to_string()];
        runner.parse_httpie(args5).unwrap();
    }
}
