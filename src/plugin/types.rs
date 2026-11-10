use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件执行所属的不可逆生命周期阶段 (Stage)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginStage {
    /// 阶段 1: 准备与认证 (如 OAuth Token 获取、动态 STS 凭据拉取)
    Prepare = 10,
    /// 阶段 2: 请求变形与加密 (如 AES 载荷加密、压缩、数据转换)
    #[default]
    Transform = 20,
    /// 阶段 3: 报文防篡改签名 (如 AWS SigV4、HMAC 摘要)
    Sign = 30,
    /// 阶段 4: 响应后处理 (如解密响应报文、提取安全凭据)
    PostResponse = 40,
    /// 阶段 5: 故障时现场归因 (如拉取网关诊断日志、增强快照错误)
    OnFailure = 50,
}

impl PluginStage {
    /// 获取当前阶段默认的优先级权重 (默认区间 10~90)
    pub fn default_priority(&self) -> u32 {
        match self {
            Self::Prepare => 20,
            Self::Transform => 50,
            Self::Sign => 80,
            Self::PostResponse => 110,
            Self::OnFailure => 140,
        }
    }
}

/// 解析自 .http 文本的单个插件语义注解
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirective {
    /// 注解标识符：auth, transform, sign, plugin 等
    pub directive: String,
    /// 插件唯一名称，如 aws-sigv4, echo-sign
    pub name: String,
    /// 归属生命周期阶段
    pub stage: PluginStage,
    /// 阶段内的执行优先级 (数值越小越先执行)
    pub priority: u32,
    /// 是否为可选插件 (缺失时是否允许优雅自愈降级)
    pub optional: bool,
    /// 传递给插件的参数键值对
    pub params: HashMap<String, String>,
}

impl PluginDirective {
    pub fn new(directive: impl Into<String>, name: impl Into<String>, stage: PluginStage) -> Self {
        let directive_str = directive.into();
        let priority = stage.default_priority();
        Self {
            directive: directive_str,
            name: name.into(),
            stage,
            priority,
            optional: false,
            params: HashMap::new(),
        }
    }
}

/// 插件元数据清单 (Plugin Manifest)，遵循 DeepSeek Harness 规范
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub manifest_version: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub commands: Vec<CommandCapability>,
    #[serde(default)]
    pub hooks: Vec<HookCapability>,
}

/// 插件暴露给系统的 CLI 子命令扩展能力
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandCapability {
    pub name: String,
    pub usage: String,
    pub description: String,
    #[serde(default)]
    pub requires_tty: bool,
}

/// 插件暴露给系统的 HTTP 请求生命周期钩子能力
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookCapability {
    pub directive: String,
    pub alias: Option<String>,
    pub stage: PluginStage,
    pub default_priority: u32,
    #[serde(default)]
    pub parameters_schema: serde_json::Value,
}
