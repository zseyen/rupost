use std::path::PathBuf;

/// 标准请求只读中间表示 (IR)
///
/// 已经完成变量替换、URL 补全、隐式 Header 补齐与可选脱敏
#[derive(Debug, Clone, PartialEq)]
pub struct ExportableRequest {
    /// 请求名称（来自 @name）
    pub name: Option<String>,
    /// 源文件位置与行号（如 "examples/basic.http:3"）
    pub source_location: Option<String>,
    /// 大写 HTTP 方法（如 GET, POST, HEAD）
    pub method: String,
    /// 完整绝对 URL
    pub url: String,
    /// 规范化 Headers（已过滤自计算头，已补齐 JSON Content-Type / User-Agent / Cookie）
    pub headers: Vec<(String, String)>,
    /// 请求体原文
    pub body: Option<String>,
    /// 超时秒数（普通请求取 @timeout 向上取整秒；SSE 请求取 @sse_timeout）
    pub timeout_secs: Option<u64>,
    /// 是否为 SSE 流式请求
    pub is_sse: bool,
    /// 非致命构建/解析警告（输出到 stderr，不污染 stdout）
    pub warnings: Vec<String>,
}

/// 被跳过不导出的请求说明
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedItem {
    pub name: Option<String>,
    pub line_number: usize,
    pub reason: String,
}

/// 单个导出器插件的配置选项
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// 是否单行紧凑输出（默认 false，使用 ` \` 多行美化折行）
    pub compact: bool,
    /// 是否对敏感 Header 与 URL Query 参数脱敏
    pub redact_secrets: bool,
}

/// CLI 顶层传入的高阶导出参数
#[derive(Debug, Clone, Default)]
pub struct ExportRunOptions {
    pub target: Option<String>,
    pub paths: Vec<String>,
    pub env: Option<String>,
    pub var: Vec<String>,
    pub env_file: Option<String>,
    pub cookie_file: Option<String>,
    pub default_scheme: String,
    pub name: Option<String>,
    pub compact: bool,
    pub redact: bool,
    pub output: Option<PathBuf>,
    pub force: bool,
    pub list: bool,
}
