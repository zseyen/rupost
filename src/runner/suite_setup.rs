use crate::Result;
use crate::middleware::resolve_cookie_path;
use crate::plugin::pipeline::PluginRuntimeOptions;
use crate::plugin::{DiscoverOptions, PluginRegistry, parse_plugin_env_specs};
use crate::runner::{TestExecutor, TestSuiteOptions};
use crate::variable::{ConfigLoader, VariableContext};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
        allow_project_plugins: options.allow_project_plugins,
        project_root: None,
    };
    let cache_path = dirs::home_dir()
        .map(|h| h.join(".rupost").join("registry.json"))
        .unwrap_or_else(|| PathBuf::from(".rupost-registry.json"));

    let registry = match PluginRegistry::load_or_discover(&discover_options, &cache_path) {
        Ok(reg) => {
            for warning in &reg.warnings {
                eprintln!("[!] {warning}");
            }
            Arc::new(reg)
        }
        Err(e) => {
            // 反静默：发现失败必须可见；回退空注册表确保插件管线与严格门禁不旁路
            eprintln!("[!] 插件注册表发现失败（插件注解将按缺失门禁/降级处理）: {e}");
            Arc::new(PluginRegistry::new())
        }
    };
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
