pub mod http_file;
pub mod markdown_file;
pub mod metadata;
pub mod mock_variant;
pub mod types;

// Re-export commonly used types
pub use http_file::HttpFileParser;
pub use markdown_file::MarkdownFileParser;
pub use mock_variant::MockVariantParser;
pub use types::{
    FileMetadata, ParseError, ParseResult, ParsedFile, ParsedMockVariant, ParsedRequest,
    RequestMetadata,
};

/// 依据文件扩展名统一解析活文档（.md, .markdown 或 .http）
pub fn parse_document_by_path<P: AsRef<std::path::Path>>(
    path: P,
    content: &str,
) -> ParseResult<ParsedFile> {
    let ext = path
        .as_ref()
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown") {
        MarkdownFileParser::parse_content(content)
    } else {
        HttpFileParser::parse_content(content)
    }
}

/// 从文件路径解析 HTTP 文件
pub fn parse_file<P: AsRef<std::path::Path>>(path: P) -> ParseResult<ParsedFile> {
    HttpFileParser::parse_file(path)
}

/// 从字符串内容解析 HTTP 请求
pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
    HttpFileParser::parse_content(content)
}

pub mod scanner;
pub use scanner::DirectoryScanner;
