use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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

/// 解析自 .http 文本的单个插件语义注解
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDirective {
    /// 注解标识符：auth, transform, sign, plugin 等
    pub directive: String,
    /// 插件唯一名称，如 aws-sigv4, echo-sign
    pub name: String,
    /// 归属生命周期阶段
    pub stage: PluginStage,
    /// 是否为可选插件 (缺失时是否允许优雅自愈降级)
    pub optional: bool,
    /// 传递给插件的参数键值对
    pub params: HashMap<String, String>,
}

impl PluginDirective {
    pub fn new(directive: impl Into<String>, name: impl Into<String>, stage: PluginStage) -> Self {
        Self {
            directive: directive.into(),
            name: name.into(),
            stage,
            optional: false,
            params: HashMap::new(),
        }
    }
}

/// 插件跨语言运行时依赖声明（03 §3.1 v2.1.1：零额外探针，仅 PATH stat 预检）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginRuntime {
    pub interpreter: Option<String>,
    pub min_version: Option<String>,
    #[serde(default)]
    pub modules: Vec<String>,
}

/// 插件元数据清单 (Plugin Manifest)，遵循 DeepSeek Harness 规范
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub manifest_version: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub commands: Vec<CommandCapability>,
    #[serde(default)]
    pub hooks: Vec<HookCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PluginRuntime>,
}

/// 插件暴露给系统的 CLI 子命令扩展能力
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CommandCapability {
    pub name: String,
    pub usage: String,
    pub description: String,
    #[serde(default)]
    pub requires_tty: bool,
    #[serde(default)]
    pub requires_env: Vec<String>,
}

/// 插件暴露给系统的 HTTP 请求生命周期钩子能力
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookCapability {
    pub directive: String,
    pub alias: Option<String>,
    pub stage: PluginStage,
    /// Hook 结果缓存作用域 (03 §3.1)：`none` = 每请求执行；`request` = 请求内复用；
    /// `suite` = 全套件复用（OAuth2 取 Token 类插件声明，消除子进程风暴）。
    /// 缺省 `none`；`ttl` 为 P2 保留值，解析到即按 none + 告警处理。
    #[serde(default)]
    pub cache_scope: PluginCacheScope,
    /// 环境变量白名单声明（03 §6.2）：子进程默认仅继承 PATH/HOME/LANG/TERM/TMPDIR +
    /// RUPOST_*，声明外的变量需 manifest 声明或 --plugin-env 显式放行。
    #[serde(default)]
    pub requires_env: Vec<String>,
    #[serde(default)]
    pub parameters_schema: serde_json::Value,
}

/// Hook 结果缓存作用域（03 §3.1 唯一权威；`ttl` 为 P2 保留）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginCacheScope {
    /// 每次请求都执行插件（默认）
    #[default]
    None,
    /// 请求内复用（同一请求多个同 alias 指令共享一次执行）
    Request,
    /// 全套件复用（OAuth2 类插件声明，跨请求复用 Patch 结果）
    Suite,
}

/// 插件执行时子进程的环境继承策略（03 §6.2 白名单裁决）
#[cfg(not(windows))]
pub const PLUGIN_ENV_ALLOWLIST: [&str; 6] = ["PATH", "HOME", "LANG", "TERM", "TMPDIR", "TZ"];

/// Windows 子进程额外需要系统根路径与临时目录变量，否则 CreateProcess/Runtime 初始化可能失败
#[cfg(windows)]
pub const PLUGIN_ENV_ALLOWLIST: [&str; 13] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "LANG",
    "TERM",
    "TMPDIR",
    "TEMP",
    "TMP",
    "TZ",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
];

/// 插件三态健康状态（03 §3.1.1：响应性重算，绝不持久化进 registry.json）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginHealthStatus {
    Ready,
    Pending {
        interpreter: String,
        min_version: Option<String>,
        modules: Vec<String>,
        searched_dirs: usize,
    },
    Failed {
        reason: String,
    },
}

/// 单个插件的状态视图（供 `plugin list` 与诊断消费）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatusEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub manifest_path: PathBuf,
    pub executable_path: Option<PathBuf>,
    pub status: PluginHealthStatus,
}

/// 发现期被拒绝或解析失败的插件清单记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPluginRecord {
    pub manifest_path: PathBuf,
    pub name: Option<String>,
    pub reason: String,
}
