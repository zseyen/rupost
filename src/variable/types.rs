use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
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
    /// Wave 2.1：`rupost.toml [paths]` 自定义路径段（仅允许工作区沙箱内相对路径）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<WorkspacePathsToml>,
}

/// `rupost.toml [paths]` 原始配置段（全部字段可选，向后兼容）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePathsToml {
    #[serde(default)]
    pub plugins_dir: Option<String>,
    #[serde(default)]
    pub history_file: Option<String>,
    #[serde(default)]
    pub logs_dir: Option<String>,
    #[serde(default)]
    pub cookie_dir: Option<String>,
    #[serde(default)]
    pub reports_dir: Option<String>,
    #[serde(default)]
    pub snapshots_dir: Option<String>,
}

/// 解析与沙箱校验后的强类型工作区路径集合（全部锚定显式 `project_root`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePathsConfig {
    pub plugins_dir: PathBuf,
    pub history_file: PathBuf,
    pub logs_dir: PathBuf,
    pub cookie_dir: PathBuf,
    pub reports_dir: PathBuf,
    pub snapshots_dir: PathBuf,
}

impl WorkspacePathsConfig {
    /// 基于显式 `project_root` 构建默认路径（兼容 `RUPOST_HISTORY_DIR` 覆盖）。
    pub fn default_for_root(project_root: &Path) -> Self {
        let rupost_root = project_root.join(crate::platform::DEFAULT_RUPOST_DIR);
        let history_root = std::env::var_os("RUPOST_HISTORY_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| rupost_root.clone());
        Self {
            plugins_dir: rupost_root.join("plugins"),
            history_file: history_root.join("history.jsonl"),
            logs_dir: history_root.join("logs"),
            cookie_dir: rupost_root.join("cookies"),
            reports_dir: rupost_root.join("reports"),
            snapshots_dir: rupost_root.join("snapshots"),
        }
    }
}

/// 统一工作区配置快照（CLI / TUI / Web 三端同源只读消费）。
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub project_root: PathBuf,
    pub paths: WorkspacePathsConfig,
    pub plugins: PluginTomlConfig,
}

impl WorkspaceConfig {
    /// 纯无状态加载：严格锚定传入的 `project_root`，绝不隐式读取 `current_dir()`。
    pub fn load(project_root: &Path, sink: &dyn crate::diagnostics::WarningSink) -> Self {
        let mut paths = WorkspacePathsConfig::default_for_root(project_root);
        let toml_path = project_root.join("rupost.toml");
        if !toml_path.exists() {
            return Self {
                project_root: project_root.to_path_buf(),
                paths,
                plugins: PluginTomlConfig::default(),
            };
        }

        let content = match std::fs::read_to_string(&toml_path) {
            Ok(c) => c,
            Err(e) => {
                sink.emit(
                    crate::diagnostics::WarningSeverity::Warn,
                    "workspace_config",
                    &format!("读取 {} 失败，回退默认配置: {e}", toml_path.display()),
                );
                return Self {
                    project_root: project_root.to_path_buf(),
                    paths,
                    plugins: PluginTomlConfig::default(),
                };
            }
        };

        let parsed: VariableConfig = match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                sink.emit(
                    crate::diagnostics::WarningSeverity::Warn,
                    "workspace_config",
                    &format!("解析 {} 失败，回退默认配置: {e}", toml_path.display()),
                );
                return Self {
                    project_root: project_root.to_path_buf(),
                    paths,
                    plugins: PluginTomlConfig::default(),
                };
            }
        };

        if let Some(p) = parsed.paths {
            let resolve = |field: &str, raw: Option<String>, fallback: PathBuf| -> PathBuf {
                let Some(val) = raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
                    return fallback;
                };
                match resolve_sandboxed_workspace_path(project_root, &val) {
                    Ok(safe) => safe,
                    Err(reason) => {
                        sink.emit(
                            crate::diagnostics::WarningSeverity::Security,
                            "workspace_config",
                            &format!(
                                "[paths].{field} = {:?} 被工作区沙箱拒绝（{reason}），已回退默认路径",
                                val
                            ),
                        );
                        fallback
                    }
                }
            };
            paths.plugins_dir = resolve("plugins_dir", p.plugins_dir, paths.plugins_dir);
            paths.history_file = resolve("history_file", p.history_file, paths.history_file);
            paths.logs_dir = resolve("logs_dir", p.logs_dir, paths.logs_dir);
            paths.cookie_dir = resolve("cookie_dir", p.cookie_dir, paths.cookie_dir);
            paths.reports_dir = resolve("reports_dir", p.reports_dir, paths.reports_dir);
            paths.snapshots_dir = resolve("snapshots_dir", p.snapshots_dir, paths.snapshots_dir);
        }

        Self {
            project_root: project_root.to_path_buf(),
            paths,
            plugins: parsed.plugins.unwrap_or_default(),
        }
    }
}

/// 校验 `[paths]` 配置的相对路径是否严格囚禁在 `project_root` 沙箱内：
/// 拒绝绝对路径、`..` 父级回溯与指向工作区外的符号链接。
pub fn resolve_sandboxed_workspace_path(
    project_root: &Path,
    raw_rel: &str,
) -> std::result::Result<PathBuf, String> {
    let rel = Path::new(raw_rel);
    if rel.is_absolute() {
        return Err("禁止绝对路径".to_string());
    }
    for comp in rel.components() {
        if matches!(comp, Component::ParentDir | Component::Prefix(_)) {
            return Err("禁止父级回溯 `..` 或盘符前缀".to_string());
        }
    }
    let joined = project_root.join(rel);
    if joined.exists() || std::fs::symlink_metadata(&joined).is_ok() {
        let roots = vec![project_root.to_path_buf()];
        if !crate::utils::path_guard::is_within_roots(&joined, &roots) {
            return Err("目标路径或符号链接指向工作区之外".to_string());
        }
    }
    Ok(joined)
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

    #[test]
    fn test_workspace_config_load_compatibility_and_sandbox_guards() {
        use crate::diagnostics::BufferedWarningSink;
        let dir = tempfile::tempdir().unwrap();
        let sink = BufferedWarningSink::new();

        // 1. 无 rupost.toml 时 100% 兼容默认路径
        let cfg_default = WorkspaceConfig::load(dir.path(), &sink);
        assert_eq!(
            cfg_default.paths.plugins_dir,
            dir.path().join(".rupost").join("plugins")
        );
        assert!(sink.is_empty());

        // 2. 合法相对路径放行，越界绝对路径与 `..` 被沙箱拒绝并经 WarningSink 告警
        std::fs::write(
            dir.path().join("rupost.toml"),
            r#"
[paths]
plugins_dir = "custom/plugins"
reports_dir = "../outside"
snapshots_dir = "/etc/evil"
"#,
        )
        .unwrap();
        let cfg = WorkspaceConfig::load(dir.path(), &sink);
        assert_eq!(cfg.paths.plugins_dir, dir.path().join("custom/plugins"));
        assert_eq!(
            cfg.paths.reports_dir,
            dir.path().join(".rupost").join("reports")
        );
        assert_eq!(
            cfg.paths.snapshots_dir,
            dir.path().join(".rupost").join("snapshots")
        );
        let msgs = sink.messages();
        assert_eq!(msgs.len(), 2, "两处逃逸路径均应触发安全告警: {msgs:?}");
    }
}
