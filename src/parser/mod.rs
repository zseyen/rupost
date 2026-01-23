pub mod converter;
pub mod http_file;
pub mod markdown_file;
pub mod metadata;
pub mod types;

// Re-export commonly used types
pub use converter::to_request;
pub use http_file::HttpFileParser;
pub use markdown_file::MarkdownFileParser;
pub use types::{ParseError, ParseResult, ParsedFile, ParsedRequest, RequestMetadata};

/// 从文件路径解析 HTTP 文件
pub fn parse_file<P: AsRef<std::path::Path>>(path: P) -> ParseResult<ParsedFile> {
    HttpFileParser::parse_file(path)
}

/// 从字符串内容解析 HTTP 请求
pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
    HttpFileParser::parse_content(content)
}
