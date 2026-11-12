use crate::Result;
use crate::middleware::resolve_cookie_path;
use crate::plugin::pipeline::PluginRuntimeOptions;
use crate::plugin::{DiscoverOptions, PluginRegistry, parse_plugin_env_specs};
use crate::runner::{TestExecutor, TestSuiteOptions};
use crate::variable::{ConfigLoader, VariableContext};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// 从原始 `--plugin-env` 参数列表提取已放行的环境变量名集合（保留未在宿主机设值的裸 `KEY`）。
pub(crate) fn extract_authorized_env_keys(specs: &[String]) -> HashSet<String> {
    specs
        .iter()
        .filter_map(|spec| {
            let key = match spec.split_once('=') {
                Some((k, _)) => k.trim(),
                None => spec.trim(),
            };
            (!key.is_empty()).then(|| key.to_string())
        })
        .collect()
}

/// 加载工作区配置、环境变量文件与 `--var` 覆盖，构造套件变量上下文。
pub(crate) fn load_suite_context(options: &TestSuiteOptions) -> VariableContext {
    let (mut var_context, warnings) = ConfigLoader::load_context_with_var_specs(
        options.env.as_deref(),
        &options.var,
        options.env_file.as_deref(),
    );
    for warning in &warnings {
        eprintln!("[!] {warning}");
    }
    var_context.insert("__default_scheme", &options.default_scheme);
    // R-2 免重填：--var-state 回灌为最低优先级层（只补缺；缺失静默、损坏指名告警）
    if let Some(state_path) = &options.var_state {
        match crate::variable::var_state::load_var_state(Path::new(state_path)) {
            None => {}
            Some(Ok(vars)) => {
                let filled = crate::variable::var_state::backfill_missing(&mut var_context, &vars);
                if filled > 0 {
                    eprintln!(
                        "[i] 已从 {state_path} 回灌 {filled} 个捕获变量（仅补缺，显式值优先）"
                    );
                }
            }
            Some(Err(e)) => {
                eprintln!("[!] --var-state 文件损坏，已忽略回灌: {state_path} ({e})");
            }
        }
    }
    var_context
}

/// 装配插件注册表与运行时降级/门禁选项；即使发现失败也返回空注册表而非 None，
/// 以保障 `--strict-plugins` 与必需插件报错门禁始终生效。
pub(crate) fn assemble_plugin_runtime(
    options: &TestSuiteOptions,
) -> (Arc<PluginRegistry>, PluginRuntimeOptions) {
    let plugin_runtime = PluginRuntimeOptions {
        allow_missing_plugins: options.allow_missing_plugins,
        strict_plugins: options.strict_plugins,
        plugin_timeout: Duration::from_secs(options.plugin_timeout_secs),
        plugin_env: parse_plugin_env_specs(&options.plugin_env),
    };
    let discover_options = DiscoverOptions {
        allow_project_plugins: options.allow_project_plugins
            || crate::security::fs::env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
        project_root: None,
    };
    let authorized_env_keys = extract_authorized_env_keys(&options.plugin_env);
    let registry = crate::runner::executor_factory::discover_plugin_registry(
        &discover_options,
        &authorized_env_keys,
    );
    (registry, plugin_runtime)
}

/// 基于选项构建测试套件的主 `TestExecutor`（含 Cookie 策略、调试模式与插件装配）。
pub(crate) fn build_suite_executor(options: &TestSuiteOptions) -> Result<TestExecutor> {
    let executor = if options.no_cookies {
        TestExecutor::new()
    } else if let Some(cookie_path) = &options.cookie_file {
        let resolved = resolve_cookie_path(PathBuf::from(cookie_path), options.env.as_deref())?;
        TestExecutor::with_cookies(resolved)?
    } else {
        if options.verbose {
            println!(
                "[i] 会话 Cookie 为临时模式（仅本次运行内共享）；加 --cookie-file <FILE> 可跨运行持久化"
            );
        }
        tracing::debug!("Cookies are ephemeral for this run; pass --cookie-file to persist");
        TestExecutor::with_ephemeral_cookies()
    };

    let (registry, plugin_runtime) = assemble_plugin_runtime(options);
    Ok(executor
        .with_debug(options.debug)
        .with_debug_on_failure(options.debug_on_failure)
        .with_plugins(registry, plugin_runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::types::{HookCapability, PluginManifest, PluginStage};

    #[test]
    fn test_extract_authorized_env_keys_preserves_bare_and_kv_specs() {
        let specs = vec![
            "UNSET_BARE_KEY".to_string(),
            "  TRIMMED_KEY = value=with=eq ".to_string(),
            "   ".to_string(),
            "=empty_key".to_string(),
        ];
        let keys = extract_authorized_env_keys(&specs);
        assert!(keys.contains("UNSET_BARE_KEY"));
        assert!(keys.contains("TRIMMED_KEY"));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_registry_missing_env_warnings_respects_bare_plugin_env_key() {
        let mut reg = PluginRegistry::new();
        reg.register_manifest(
            PathBuf::from("/tmp/vault/rupost-plugin-vault"),
            PluginManifest {
                manifest_version: "1.0".to_string(),
                name: "vault-plugin".to_string(),
                version: "1.0.0".to_string(),
                description: "vault".to_string(),
                runtime: None,
                hooks: vec![HookCapability {
                    directive: "vault".to_string(),
                    alias: None,
                    stage: PluginStage::Prepare,
                    cache_scope: Default::default(),
                    requires_env: vec!["VAULT_TOKEN".to_string()],
                    parameters_schema: serde_json::json!({}),
                }],
                commands: vec![],
            },
        );

        let directives = vec!["vault".to_string()];
        let warnings_missing = reg.collect_missing_env_warnings(&directives, &HashSet::new());
        assert_eq!(warnings_missing.len(), 1);
        assert!(warnings_missing[0].contains("VAULT_TOKEN"));

        let authorized = extract_authorized_env_keys(&["VAULT_TOKEN".to_string()]);
        let warnings_allowed = reg.collect_missing_env_warnings(&directives, &authorized);
        assert!(warnings_allowed.is_empty());
    }
}
