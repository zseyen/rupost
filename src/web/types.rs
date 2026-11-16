use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// 通用 API 包装结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// 保存文件入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFileRequest {
    pub path: String,
    pub content: String,
}

/// 保存文件出参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFileResponse {
    pub path: String,
    pub bytes_written: usize,
    pub verified_blocks: usize,
}

/// 单块原位调试执行入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBlockRequest {
    pub file_path: String,
    pub raw_block: String,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub extra_variables: Option<HashMap<String, String>>,
    /// B3（免重填主线）：请求级插件/门禁覆盖（> toml [plugins] > 默认）
    #[serde(default)]
    pub plugin_overrides: Option<PluginOverrides>,
}

/// B3：请求级插件/门禁覆盖（部分字段可选，None 字段沿用 toml/默认）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PluginOverrides {
    #[serde(default)]
    pub allow_missing_plugins: Option<bool>,
    #[serde(default)]
    pub plugin_timeout_secs: Option<u64>,
    #[serde(default)]
    pub plugin_env: Vec<String>,
    #[serde(default)]
    pub strict_vars: Option<bool>,
    #[serde(default)]
    pub strict_plugins: Option<bool>,
    #[serde(default)]
    pub allow_project_plugins: Option<bool>,
}

/// 单块原位调试执行响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBlockResponse {
    pub success: bool,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub assertions: Vec<ReportAssertionItem>,
    pub timing_waterfall: Option<TimingWaterfall>,
    pub response_headers: HashMap<String, String>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// 多格式导入类型（下沉至通用领域层 `crate::importer`，此处保持重导出兼容）
pub use crate::importer::{ImportFormat, ImportRequest, ImportResponse, default_import_format};

/// Mock 规则仿真探针入参
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MockSimulateRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub query: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    /// 待测试的 Markdown 代码块或变体规则描述
    pub raw_block: String,
    #[serde(default)]
    pub file_path: Option<String>,
}

/// 工作台元信息（前端只读态感知）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchMeta {
    pub readonly: bool,
}

/// 整篇文档套件批量执行入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSuiteRequest {
    pub path: String,
    #[serde(default)]
    pub env: Option<String>,
    /// WP-R2：失败时产出故障复现包（布尔开关；输出位置由服务端决定，
    /// **绝不接受客户端路径**——浏览器可控的文件系统路径是注入/滥用面）
    #[serde(default)]
    pub incident: Option<bool>,
    /// B3：请求级插件/门禁覆盖（> toml [plugins] > 默认）
    #[serde(default)]
    pub plugin_overrides: Option<PluginOverrides>,
}

/// 套件单条请求执行摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteItemResult {
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub success: bool,
    pub skipped: bool,
    pub error: Option<String>,
}

/// 整篇文档套件批量执行响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSuiteResponse {
    pub path: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub items: Vec<SuiteItemResult>,
    /// WP-R2：故障复现包目录（仅失败 + incident 开关时为 Some；路径由服务端管辖）
    #[serde(default)]
    pub incident_dir: Option<String>,
}

/// 单条规则匹配诊断
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionEvaluation {
    pub condition_str: String,
    pub passed: bool,
    pub expected: String,
    pub actual: Option<String>,
}

/// Mock 仿真结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockSimulateResponse {
    pub matched_variant_index: Option<usize>,
    pub matched_variant_name: String,
    pub expected_status: u16,
    pub evaluations: Vec<ConditionEvaluation>,
    #[serde(default)]
    pub matched_pattern: Option<String>,
    #[serde(default)]
    pub captured_params: HashMap<String, String>,
    #[serde(default)]
    pub response_headers: HashMap<String, String>,
    #[serde(default)]
    pub response_body: Option<String>,
}

/// Markdown 渲染请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderMarkdownRequest {
    pub content: String,
}

/// Markdown 渲染响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderMarkdownResponse {
    pub html: String,
    pub executable_blocks: Vec<String>,
    #[serde(default)]
    pub executable_cards: Vec<HttpCardDto>,
}

/// `.http` 文档结构化解析请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseHttpRequest {
    pub content: String,
}

/// 标准化请求头 DTO
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpHeaderDto {
    pub key: String,
    pub value: String,
}

/// 统一 HTTP 请求卡片 DTO（`.http` 与 `.md` 活文档共用，SSOT）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpCardDto {
    pub title: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<HttpHeaderDto>,
    pub body: Option<String>,
    pub directives: Vec<String>,
    pub has_mock: bool,
    #[serde(default)]
    pub mock_variant_count: usize,
    pub is_websocket: bool,
    pub raw_block: String,
}

/// `.http` 文档结构化解析响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseHttpResponse {
    pub variables: BTreeMap<String, String>,
    pub cards: Vec<HttpCardDto>,
}

/// 工作区发现的 Mock 路由条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MockRouteItemDto {
    pub file_path: String,
    pub title: String,
    pub method: String,
    pub path: String,
    pub raw_block: String,
    #[serde(default)]
    pub variant_count: usize,
}

/// 运行中的 Web 托管 Mock 实例信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MockServerInfo {
    pub id: String,
    pub file_path: String,
    pub port: u16,
    pub url: String,
    pub route_count: usize,
}

/// 启动托管 Mock 服务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMockRequest {
    pub file_path: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub enable_cors: Option<bool>,
}

/// 停止托管 Mock 服务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopMockRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

/// 原地重载托管 Mock 服务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadMockRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

/// 新建活文档请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFileRequest {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
}

/// 新建活文档响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFileResponse {
    pub path: String,
    pub bytes_written: usize,
}

/// 工作区可用环境列表响应（仅返回环境名，不暴露变量值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvsResponse {
    pub environments: Vec<String>,
    pub default_env: Option<String>,
}

/// 单块导出请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBlockRequest {
    pub raw_block: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub compact: Option<bool>,
    #[serde(default)]
    pub redact_secrets: Option<bool>,
    #[serde(default)]
    pub extra_variables: Option<HashMap<String, String>>,
}

/// 单块导出响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBlockResponse {
    pub format: String,
    pub command: String,
    pub warnings: Vec<String>,
}

/// 当前浏览器会话状态摘要（仅限当前 rupost_session）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateResponse {
    pub captured_variables: BTreeMap<String, String>,
    pub has_cookie_state: bool,
}

/// 工作区隔离后的历史记录摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItemSummary {
    pub id: String,
    pub timestamp: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration_ms: u64,
    pub source: Option<String>,
}

/// 链路诊断请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnoseRequest {
    pub url: String,
}
