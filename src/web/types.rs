use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// 多格式导入类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    Auto,
    Curl,
    Httpie,
    RawHttp,
    Har,
    OpenApi,
    Postman,
}

fn default_import_format() -> ImportFormat {
    ImportFormat::Auto
}

/// 统一导入请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    #[serde(default = "default_import_format")]
    pub format: ImportFormat,
    pub content: String,
    #[serde(default)]
    pub templatize_host: bool,
    #[serde(default)]
    pub generate_mock: bool,
}

/// 统一导入响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResponse {
    pub detected_format: ImportFormat,
    pub markdown_blocks: Vec<String>,
    pub total_imported: usize,
    pub warnings: Vec<String>,
}

/// Mock 规则仿真探针入参
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
