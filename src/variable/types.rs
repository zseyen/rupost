use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// 变量上下文，存储所有可用变量
#[derive(Debug, Clone)]
pub struct VariableContext {
    /// 局部变量映射表
    variables: HashMap<String, String>,
    /// 全局共享变量映射表
    global: Arc<RwLock<HashMap<String, String>>>,
}

impl Default for VariableContext {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            global: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl VariableContext {
    /// 创建新的空变量上下文
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入变量
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key_str = key.into();
        let val_str = value.into();
        if key_str.starts_with("global.") {
            if let Ok(mut g) = self.global.write() {
                g.insert(key_str, val_str);
            }
        } else {
            self.variables.insert(key_str, val_str);
        }
    }

    /// 设置变量 (insert 的别名)
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.insert(key, value);
    }

    /// 获取变量值
    pub fn get(&self, key: &str) -> Option<String> {
        if key.starts_with("global.") {
            self.global.read().ok()?.get(key).cloned()
        } else if let Some(env_name) = key.strip_prefix("env.") {
            std::env::var(env_name)
                .ok()
                .or_else(|| self.variables.get(env_name).cloned())
        } else {
            self.variables.get(key).cloned()
        }
    }

    /// 批量插入变量
    pub fn extend(&mut self, vars: HashMap<String, String>) {
        for (k, v) in vars {
            self.insert(k, v);
        }
    }

    /// 变量数量
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    /// 获取局部变量映射表引用
    pub fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }
}

/// 环境配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Environment {
    /// 变量映射
    #[serde(flatten)]
    pub variables: HashMap<String, String>,
}

/// 完整的变量配置文件
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VariableConfig {
    /// 所有环境配置
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
    /// B3（免重填主线）：TUI/Web 交互入口的插件与门禁选项（rupost.toml [plugins] 段）。
    /// 仅交互入口消费——CLI 套件路径的旗标面不变（assemble_plugin_runtime 不读此段）。
    /// 旧 toml 无该段 → None（追加式演进，零破坏）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginTomlConfig>,
}

/// B3：`rupost.toml [plugins]` 段——TUI/Web 交互入口的插件运行时与门禁选项。
/// 优先级：Web 请求覆盖 > toml [plugins] > `PluginRuntimeOptions::default()`；
/// CLI 套件路径不受影响（旗标面完整）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginTomlConfig {
    /// 插件缺失/失败时降级为警告跳过（默认 false：阻断 + 三选一处方）
    #[serde(default)]
    pub allow_missing_plugins: bool,
    /// 严格门禁：发生任何降级 → 整体判失败（TUI run_results / Web 响应 success）
    #[serde(default)]
    pub strict_plugins: bool,
    /// 单次插件调用超时秒数（None = 默认 5s）
    #[serde(default)]
    pub plugin_timeout_secs: Option<u64>,
    /// 显式放行插件子进程环境变量（KEY 或 KEY=VALUE，语义同 --plugin-env）
    #[serde(default)]
    pub plugin_env: Vec<String>,
    /// 放行工作区 ./.rupost/plugins（默认隔离；RUPOST_ALLOW_PROJECT_PLUGINS 通道仍生效）
    #[serde(default)]
    pub allow_project_plugins: bool,
    /// E-4：未解析 {{var}} 拒绝外发
    #[serde(default)]
    pub strict_vars: bool,
}

impl VariableConfig {
    /// 获取指定环境的变量
    pub fn get_environment(&self, env_name: &str) -> Option<&Environment> {
        self.environments.get(env_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_context_basic() {
        let mut ctx = VariableContext::new();
        assert!(ctx.is_empty());

        ctx.insert("key", "value");
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx.get("key").as_deref(), Some("value"));
        assert_eq!(ctx.get("missing").as_deref(), None);
    }

    #[test]
    fn test_variable_context_extend() {
        let mut ctx = VariableContext::new();
        let mut vars = HashMap::new();
        vars.insert("key1".to_string(), "value1".to_string());
        vars.insert("key2".to_string(), "value2".to_string());

        ctx.extend(vars);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx.get("key1").as_deref(), Some("value1"));
        assert_eq!(ctx.get("key2").as_deref(), Some("value2"));
    }

    #[test]
    fn test_variable_config_parse() {
        let toml_str = r#"
[environments.dev]
base_url = "http://localhost:8080"
api_key = "dev-key"

[environments.prod]
base_url = "https://api.example.com"
api_key = "${PROD_KEY}"
"#;

        let config: VariableConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.environments.len(), 2);

        let dev = config.get_environment("dev").unwrap();
        assert_eq!(
            dev.variables.get("base_url"),
            Some(&"http://localhost:8080".to_string())
        );
    }
}
