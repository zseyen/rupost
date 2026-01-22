use std::path::PathBuf;
use std::time::Duration;

/// 单个解析后的 HTTP 请求
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    /// HTTP 方法，如果缺失则默认为 GET
    pub method: Option<String>,

    /// 请求 URL（必需）
    pub url: String,

    /// Headers 列表，保持原始顺序
    pub headers: Vec<(String, String)>,

    /// 请求体（可选）
    pub body: Option<String>,

    /// 请求元数据
    pub metadata: RequestMetadata,

    /// 请求在文件中的起始行号（用于错误报告）
    pub line_number: usize,
}

impl ParsedRequest {
    /// 创建一个新的空请求
    pub fn new(line_number: usize) -> Self {
        Self {
            method: None,
            url: String::new(),
            headers: Vec::new(),
            body: None,
            metadata: RequestMetadata::default(),
            line_number,
        }
    }

    /// 获取 HTTP 方法，如果未指定则返回 "GET"
    pub fn method_or_default(&self) -> &str {
        self.method.as_deref().unwrap_or("GET")
    }

    /// 检查请求是否应该被跳过
    pub fn should_skip(&self) -> bool {
        self.metadata.skip
    }

    /// 获取请求名称（如果有）
    pub fn name(&self) -> Option<&str> {
        self.metadata.name.as_deref()
    }
}

/// 请求元数据
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestMetadata {
    /// 请求名称（@name）
    pub name: Option<String>,

    /// 是否跳过该请求（@skip）
    pub skip: bool,

    /// 请求超时时间（@timeout，可选）
    pub timeout: Option<Duration>,

    /// 断言列表（@assert）
    pub assertions: Vec<String>,
}

/// 整个文件的解析结果
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFile {
    /// 解析出的所有请求
    pub requests: Vec<ParsedRequest>,

    /// 源文件路径（用于错误报告）
    pub source_path: Option<PathBuf>,
}

impl ParsedFile {
    /// 创建一个新的空文件解析结果
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            source_path: None,
        }
    }

    /// 设置源文件路径
    pub fn with_source_path(mut self, path: PathBuf) -> Self {
        self.source_path = Some(path);
        self
    }

    /// 添加一个请求
    pub fn add_request(&mut self, request: ParsedRequest) {
        self.requests.push(request);
    }

    /// 获取所有未标记为跳过的请求
    pub fn active_requests(&self) -> impl Iterator<Item = &ParsedRequest> {
        self.requests.iter().filter(|r| !r.should_skip())
    }
}

impl Default for ParsedFile {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析错误类型
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// 文件格式错误
    #[error("Invalid format at line {line}: {message}")]
    InvalidFormat { line: usize, message: String },

    /// 缺少必需的 URL
    #[error("Missing URL at line {line}")]
    MissingUrl { line: usize },

    /// 无效的元数据
    #[error("Invalid metadata at line {line}: {message}")]
    InvalidMetadata { line: usize, message: String },

    /// 无效的 HTTP 方法
    #[error("Invalid HTTP method '{method}' at line {line}")]
    InvalidMethod { method: String, line: usize },

    /// 无效的 Header 格式
    #[error("Invalid header format at line {line}: expected 'Key: Value'")]
    InvalidHeader { line: usize },

    /// IO 错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// 空文件或没有找到请求
    #[error("No requests found in file")]
    NoRequests,
}

/// 解析结果类型别名
pub type ParseResult<T> = Result<T, ParseError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_request_new() {
        let req = ParsedRequest::new(1);
        assert_eq!(req.line_number, 1);
        assert_eq!(req.method, None);
        assert_eq!(req.url, "");
        assert_eq!(req.headers.len(), 0);
        assert_eq!(req.body, None);
        assert!(!req.should_skip());
    }

    #[test]
    fn test_method_or_default() {
        let mut req = ParsedRequest::new(1);
        assert_eq!(req.method_or_default(), "GET");

        req.method = Some("POST".to_string());
        assert_eq!(req.method_or_default(), "POST");
    }

    #[test]
    fn test_parsed_file_new() {
        let file = ParsedFile::new();
        assert_eq!(file.requests.len(), 0);
        assert_eq!(file.source_path, None);
    }

    #[test]
    fn test_parsed_file_with_source_path() {
        let file = ParsedFile::new().with_source_path(PathBuf::from("/test/file.http"));
        assert_eq!(file.source_path, Some(PathBuf::from("/test/file.http")));
    }

    #[test]
    fn test_active_requests() {
        let mut file = ParsedFile::new();

        let mut req1 = ParsedRequest::new(1);
        req1.url = "http://example.com".to_string();

        let mut req2 = ParsedRequest::new(5);
        req2.url = "http://example.org".to_string();
        req2.metadata.skip = true;

        file.add_request(req1);
        file.add_request(req2);

        let active: Vec<_> = file.active_requests().collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].url, "http://example.com");
    }
}
