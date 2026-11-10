use crate::variable::resolver::VariableResolver;
use crate::variable::types::{VariableConfig, VariableContext};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 配置文件加载器
pub struct ConfigLoader;

impl ConfigLoader {
    /// 配置文件名
    const CONFIG_FILE: &'static str = "rupost.toml";

    /// 从指定路径加载配置文件
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<VariableConfig, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))
    }

    /// 查找配置文件物理路径（查找顺序：当前目录 -> 父目录递归 -> ~/.config/rupost/）
    pub fn find_config_path() -> Option<std::path::PathBuf> {
        if let Ok(mut current) = std::env::current_dir() {
            loop {
                let config_path = current.join(Self::CONFIG_FILE);
                if config_path.exists() {
                    return Some(config_path);
                }
                if !current.pop() {
                    break;
                }
            }
        }

        let home = dirs::home_dir()?;
        let user_path = home.join(".config").join("rupost").join(Self::CONFIG_FILE);
        if user_path.exists() {
            Some(user_path)
        } else {
            None
        }
    }

    /// 查找并加载脱敏策略配置 [export.redaction]（三级级联查找，未配置时回退到内置默认策略）
    pub fn find_and_load_redaction_policy() -> crate::exporter::redact::RedactionPolicy {
        if let Some(path) = Self::find_config_path()
            && let Ok(content) = fs::read_to_string(path)
            && let Ok(policy) = crate::exporter::redact::RedactionPolicy::from_toml_str(&content)
        {
            policy
        } else {
            crate::exporter::redact::RedactionPolicy::builtin()
        }
    }

    /// 单个 toml 路径的分类加载（走查缺口 #6c：存在但损坏必须区分于不存在——
    /// 此前 `.ok()` 把解析失败与"没有配置"混为一谈，环境变量层被静默清空）。
    /// 不存在 → Ok(None)（继续下一查找位）；损坏 → Err(根因)（调用方停止搜索，
    /// 坏文件优先于静默回退，否则用户会误用更高层级目录的错误配置）。
    fn try_load_config_file(path: &Path) -> Result<Option<VariableConfig>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let content =
            fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
        toml::from_str(&content).map(Some).map_err(|e| {
            format!(
                "解析 {} 失败: {e} —— 环境变量配置未加载，请修复 rupost.toml 后重试",
                path.display()
            )
        })
    }

    /// 查找并加载配置文件（路径发现复用 [`Self::find_config_path`]：
    /// 当前目录 -> 父目录递归 -> 用户配置目录 ~/.config/rupost/）。
    ///
    /// 返回 `(配置, 告警)`：配置损坏时 `config` 为 None 且 `告警` 携带根因
    /// （坏文件优先于静默回退——首个存在的配置损坏即停止，不再搜索其他位置）。
    /// 渲染决策留给调用方（CLI stderr / Web tracing / TUI 状态栏各不相同）。
    pub fn find_and_load() -> (Option<VariableConfig>, Vec<String>) {
        let mut warnings = Vec::new();
        let config =
            Self::find_config_path().and_then(|path| match Self::try_load_config_file(&path) {
                Ok(config) => config,
                Err(e) => {
                    warnings.push(e);
                    None
                }
            });
        (config, warnings)
    }

    /// 构建变量上下文
    /// env_name: 环境名称（如 "dev", "prod"）
    /// cli_vars: CLI 传入的变量覆盖（--var key=value）
    /// env_file: `(路径, 是否显式指定)`——显式指定的失败必须告警（`--env-file` /
    ///   `.env.<env>`），默认探测 `.env` 的缺失是常态须静默；为 None 时不加载任何 `.env` 文件
    ///
    /// 返回 `(上下文, 告警)`：告警文本由调用方渲染（stderr / tracing / 状态栏）。
    pub fn build_context(
        config: &VariableConfig,
        env_name: Option<&str>,
        cli_vars: &[(String, String)],
        env_file: Option<(&str, bool)>,
    ) -> (VariableContext, Vec<String>) {
        let mut context = VariableContext::new();
        let mut vars = HashMap::new();
        let mut warnings = Vec::new();

        // 1. 从共享 toml 配置文件加载环境变量
        let env_matched = env_name.and_then(|name| config.get_environment(name));
        if let Some(env) = env_matched {
            for (key, value) in &env.variables {
                // 解析系统环境变量 ${VAR}
                let resolved_value = VariableResolver::resolve_env_vars(value);
                vars.insert(key.clone(), resolved_value);
            }
        }

        // 2. 加载本地局部 `.env` 文件（覆盖 toml）——决策统一走 load_env_file
        if let Some((path, explicit)) = env_file {
            apply_env_file(
                crate::variable::env_file::load_env_file(Path::new(path), explicit),
                &mut vars,
                &mut warnings,
            );
        }

        // 如果指定了具体环境名称，如 prod，也可以尝试加载特定环境的本地文件
        // `.env.prod`（可选覆盖文件：缺失是常态静默，存在但损坏仍告警）
        let mut env_specific_missed = false;
        if let Some(name) = env_name {
            let env_file_name = format!(".env.{name}");
            let outcome =
                crate::variable::env_file::load_env_file(Path::new(&env_file_name), false);
            env_specific_missed =
                matches!(outcome, crate::variable::env_file::EnvFileOutcome::Silent);
            apply_env_file(outcome, &mut vars, &mut warnings);
        }

        // 走查缺口 #6a 强信号裁决（审查精度修正）：仅在"两层同时落空"时告警——
        // ① toml 存在其他环境段但不含目标名（拼写错误的最强信号）；
        // ② `.env.<name>` 也未找到（Silent；损坏/目录形态已有专属告警，不重复）。
        // toml 无任何环境段时不告警：此时 --env 可能仅用于 cookie 隔离或
        // env 文件模式，逐次告警会造成告警疲劳，反而回归变相静默。
        if let Some(name) = env_name
            && env_matched.is_none()
            && env_specific_missed
            && !config.environments.is_empty()
        {
            warnings.push(format!(
                "--env {name}: rupost.toml 中不存在 [environments.{name}] 且未找到 .env.{name}，环境切换未生效"
            ));
        }

        // 3. 应用当前进程的系统环境变量进行同名覆盖 (覆盖 .env & toml)
        for key in vars.keys().cloned().collect::<Vec<String>>() {
            if let Ok(sys_val) = std::env::var(&key) {
                vars.insert(key, sys_val);
            }
        }

        // 4. 应用 CLI 覆盖（优先级最高，覆盖一切）
        for (key, value) in cli_vars {
            vars.insert(key.clone(), value.clone());
        }

        context.extend(vars);
        (context, warnings)
    }

    /// 解析 CLI 变量参数 "key=value"
    pub fn parse_cli_var(s: &str) -> Option<(String, String)> {
        s.split_once('=')
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
    }
}

/// 将 env 文件加载决策合并进变量表（`Warn` 文本转出站 warning）。
/// `.env` / `--env-file` / `.env.<env>` 三个加载点共用，DRY 归口。
fn apply_env_file(
    outcome: crate::variable::env_file::EnvFileOutcome,
    vars: &mut HashMap<String, String>,
    warnings: &mut Vec<String>,
) {
    match outcome {
        crate::variable::env_file::EnvFileOutcome::Loaded(env_vars) => {
            for (key, value) in env_vars {
                vars.insert(key, value);
            }
        }
        crate::variable::env_file::EnvFileOutcome::Silent => {}
        crate::variable::env_file::EnvFileOutcome::Warn(msg) => warnings.push(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_path() {
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
api_key = "dev-key"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = ConfigLoader::load_from_path(temp_file.path()).unwrap();
        assert_eq!(config.environments.len(), 1);
        assert!(config.get_environment("dev").is_some());
    }

    #[test]
    fn test_build_context() {
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
token = "dev-token"

[environments.prod]
base_url = "https://api.example.com"
token = "${PROD_TOKEN}"
"#;

        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 测试 dev 环境
        let (context, warnings) = ConfigLoader::build_context(&config, Some("dev"), &[], None);
        assert!(
            warnings.is_empty(),
            "None env_file 不得产生告警: {warnings:?}"
        );
        assert_eq!(
            context.get("base_url").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(context.get("token").as_deref(), Some("dev-token"));

        // 测试 CLI 覆盖
        let cli_vars = vec![("token".to_string(), "custom-token".to_string())];
        let (context, _) = ConfigLoader::build_context(&config, Some("dev"), &cli_vars, None);
        assert_eq!(context.get("token").as_deref(), Some("custom-token"));
    }

    #[test]
    fn test_build_context_cascading_priority() {
        let config_content = r#"
[environments.dev]
base_url = "http://dev-gateway.internal:8080"
api_key = "global-key"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 1. 使用 tempfile 创建隔离的临时 `.env` 文件
        let env_content = r#"
base_url = http://localhost:8081
api_key = local-key
sys_var = local-sys
"#;
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(env_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        let temp_path = temp_file.path().to_string_lossy().to_string();

        // 2. 注入系统环境变量（测试其覆盖 .env）
        unsafe {
            std::env::set_var("sys_var", "system-override-val");
        }

        // 3. 定义 CLI 覆盖
        let cli_vars = vec![("api_key".to_string(), "cli-override-key".to_string())];

        // 4. 执行 build_context 并验证 (显式传入 tempfile 路径)
        let (context, warnings) =
            ConfigLoader::build_context(&config, Some("dev"), &cli_vars, Some((&temp_path, true)));
        assert!(warnings.is_empty(), "存在的显式文件不得告警: {warnings:?}");

        // 5. 清理临时系统环境变量
        unsafe {
            std::env::remove_var("sys_var");
        }

        // 6. 验证级联优先级：
        // CLI > 系统变量 > .env > toml
        assert_eq!(context.get("api_key").as_deref(), Some("cli-override-key")); // CLI 覆盖了 toml 与 .env
        assert_eq!(
            context.get("sys_var").as_deref(),
            Some("system-override-val")
        ); // 系统变量覆盖了 .env
        assert_eq!(
            context.get("base_url").as_deref(),
            Some("http://localhost:8081")
        ); // .env 覆盖了 toml 中的 base_url
    }

    #[test]
    fn test_build_context_with_specific_env_file() {
        let config_content = r#"
[environments.prod]
base_url = "https://api.production.com"
api_key = "prod-global-key"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 创建临时 .env.prod 文件
        let env_content = "base_url = https://api.local-prod-proxy.com";
        fs::write(".env.prod", env_content).unwrap();

        // 执行加载 (env_name = "prod"；.env.prod 存在 → Loaded，零告警)
        let (context, warnings) = ConfigLoader::build_context(&config, Some("prod"), &[], None);
        assert!(
            warnings.is_empty(),
            "存在的 .env.prod 不得告警: {warnings:?}"
        );

        // 清理临时文件
        let _ = fs::remove_file(".env.prod");

        // 验证特定环境文件级联覆盖成功
        assert_eq!(
            context.get("base_url").as_deref(),
            Some("https://api.local-prod-proxy.com")
        );
        assert_eq!(context.get("api_key").as_deref(), Some("prod-global-key")); // 未在 .env.prod 覆盖的依然保持 toml 中定义
    }

    #[test]
    fn test_build_context_env_name_missing_in_toml_warns() {
        // 走查缺口 #6a 完整形态：--env 点名的环境在 toml 中不存在 → 告警
        // （toml 层与 .env.<name> 层同时落空，"以为切了环境实际没切"的最响信号）
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 拼错的环境名 → 恰好一条指名道姓的告警
        let (_context, warnings) =
            ConfigLoader::build_context(&config, Some("stagging"), &[], None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("[environments.stagging]")
                && warnings[0].contains(".env.stagging"),
            "告警应同时指明两层落空: {warnings:?}"
        );
        // 环境名命中 → 零告警
        let (_, warnings) = ConfigLoader::build_context(&config, Some("dev"), &[], None);
        assert!(warnings.is_empty(), "命中环境名不得告警: {warnings:?}");
        // 不指定环境 → 零告警
        let (_, warnings) = ConfigLoader::build_context(&config, None, &[], None);
        assert!(warnings.is_empty(), "{warnings:?}");

        // 反向守护①：toml 无任何环境段（未使用环境机制——cookie 隔离/env 文件
        // 模式）→ --env 不告警（审查精度修正：防逐次告警疲劳）
        let empty_config = VariableConfig::default();
        let (_, warnings) = ConfigLoader::build_context(&empty_config, Some("anything"), &[], None);
        assert!(
            warnings.is_empty(),
            "无环境段的配置不得因 --env 告警: {warnings:?}"
        );

        // 反向守护②：toml 未命中但 .env.<name> 命中 → 只落空一层不算，不告警
        let probe = ".env.gap6_review_probe";
        fs::write(probe, "probe_key = probe_val").unwrap();
        let (context, warnings) =
            ConfigLoader::build_context(&config, Some("gap6_review_probe"), &[], None);
        let _ = fs::remove_file(probe);
        assert!(warnings.is_empty(), "env 文件命中时不得告警: {warnings:?}");
        assert_eq!(context.get("probe_key").as_deref(), Some("probe_val"));
    }

    #[test]
    fn test_build_context_explicit_env_file_missing_warns() {
        // 走查缺口 #6b：显式指定的 env 文件不存在必须告警
        // （反转原 test_build_context_missing_env_file_tolerance 锁定的静默契约）
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        let (context, warnings) = ConfigLoader::build_context(
            &config,
            Some("dev"),
            &[],
            Some(("non_existent_file_path_123.env", true)),
        );

        // toml 变量不受影响，且恰好一条指名道姓的告警
        assert_eq!(
            context.get("base_url").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(warnings.len(), 1, "显式缺失应恰好一条告警: {warnings:?}");
        assert!(warnings[0].contains("不存在"), "{warnings:?}");
        assert!(
            warnings[0].contains("non_existent_file_path_123.env"),
            "{warnings:?}"
        );
    }

    #[test]
    fn test_try_load_config_file_classifies_missing_vs_corrupt() {
        // 走查缺口 #6c：不存在 → Ok(None)（继续查找位），损坏 → Err(根因)
        use tempfile::NamedTempFile;

        // 1. 不存在 → Ok(None)
        let missing = std::path::Path::new("/no/such/rupost_toml_for_gap6.toml");
        assert!(
            ConfigLoader::try_load_config_file(missing)
                .unwrap()
                .is_none()
        );

        // 2. 合法 → Ok(Some)
        let mut ok = NamedTempFile::new().unwrap();
        ok.write_all(b"[environments.dev]\nbase_url = \"http://x\"\n")
            .unwrap();
        ok.flush().unwrap();
        assert!(
            ConfigLoader::try_load_config_file(ok.path())
                .unwrap()
                .is_some()
        );

        // 3. 损坏 → Err 且根因含"解析失败"（不再与"缺失"混同）
        let mut corrupt = NamedTempFile::new().unwrap();
        corrupt.write_all(b"not valid toml [[[\n").unwrap();
        corrupt.flush().unwrap();
        let err = ConfigLoader::try_load_config_file(corrupt.path()).unwrap_err();
        assert!(err.contains("失败"), "{err}");
        assert!(
            err.contains("环境变量配置未加载") && err.contains("请修复"),
            "损坏告警应说明后果并携带修复指引: {err}"
        );
    }

    #[test]
    fn test_parse_cli_var() {
        assert_eq!(
            ConfigLoader::parse_cli_var("key=value"),
            Some(("key".to_string(), "value".to_string()))
        );

        assert_eq!(
            ConfigLoader::parse_cli_var("url=https://example.com"),
            Some(("url".to_string(), "https://example.com".to_string()))
        );

        assert_eq!(ConfigLoader::parse_cli_var("invalid"), None);
    }

    #[test]
    fn test_find_and_load_redaction_policy_fallback() {
        // 未提供配置文件时，应回退至内置默认策略，且游标默认豁免
        let policy = ConfigLoader::find_and_load_redaction_policy();
        assert!(policy.is_sensitive("Authorization"));
        assert!(!policy.is_sensitive("next_token"));
    }
}
