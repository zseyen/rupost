use crate::variable::capture::VariableCapture;
use std::time::Duration;

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

    /// 变量捕获列表（@capture）
    pub captures: Vec<VariableCapture>,
}
