//! 交互入口（TUI/Web）共享执行器工厂（B-1 组合根收口，AGENTS §6）。
//!
//! 断点回顾：TUI/Web 此前各自裸建 `TestExecutor::with_ephemeral_cookies()`，
//! 插件注册表从未装配——本机已安装的非 optional 插件指令在两入口假性报
//! "插件未安装"阻断。本工厂把注册表装配收口为表现层"即调即走"的单点。

use crate::plugin::pipeline::PluginRuntimeOptions;
use crate::plugin::{DiscoverOptions, PluginRegistry};
use crate::runner::TestExecutor;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

/// 插件注册表发现（与套件路径 `assemble_plugin_runtime` 共用同一发现与告警链路）：
/// readdir 零子进程 + 缓存闭环；发现失败回退空注册表并反静默告警（门禁不旁路）。
pub fn discover_plugin_registry(
    discover: &DiscoverOptions,
    authorized_env_keys: &HashSet<String>,
) -> Arc<PluginRegistry> {
    let cache_path = dirs::home_dir()
        .map(|h| h.join(".rupost").join("registry.json"))
        .unwrap_or_else(|| PathBuf::from(".rupost-registry.json"));
    match PluginRegistry::load_or_discover(discover, &cache_path) {
        Ok(reg) => {
            for warning in &reg.warnings {
                eprintln!("[!] {warning}");
            }
            let mut directives: Vec<String> = reg.hooks.keys().cloned().collect();
            directives.sort();
            directives.dedup();
            for warning in reg.collect_missing_env_warnings(&directives, authorized_env_keys) {
                eprintln!("[WARN] {warning}");
            }
            Arc::new(reg)
        }
        Err(e) => {
            // 反静默：发现失败必须可见；回退空注册表确保插件管线与严格门禁不旁路
            eprintln!("[!] 插件注册表发现失败（插件注解将按缺失门禁/降级处理）: {e}");
            Arc::new(PluginRegistry::new())
        }
    }
}

/// 交互入口共享执行器：本机插件注册表（项目级 `./.rupost/plugins` 默认隔离，
/// 与 CLI 闸门一致；`RUPOST_ALLOW_PROJECT_PLUGINS` 环境通道同样生效）+
/// 内存隔离 Cookie 会话 + 默认插件运行时选项。
pub fn build_interactive_executor() -> TestExecutor {
    build_interactive_executor_with(&DiscoverOptions {
        allow_project_plugins: crate::security::fs::env_flag_enabled(
            "RUPOST_ALLOW_PROJECT_PLUGINS",
        ),
        project_root: None,
    })
}

/// 允许显式注入发现选项的变体（测试/嵌入场景无全局 env 副作用）。
pub fn build_interactive_executor_with(discover: &DiscoverOptions) -> TestExecutor {
    let registry = discover_plugin_registry(discover, &HashSet::new());
    TestExecutor::with_ephemeral_cookies().with_plugins(registry, PluginRuntimeOptions::default())
}

/// B3（免重填主线）：toml `[plugins]` 段 → 交互执行器。
/// PluginTomlConfig → PluginRuntimeOptions 映射 + strict_vars 接线（B4）+
/// `plugin_env` 经 `extract_authorized_env_keys` 进入 missing-env 告警豁免（T5）。
pub fn build_interactive_executor_with_options(
    discover: &DiscoverOptions,
    plugin_cfg: &crate::variable::PluginTomlConfig,
    strict_vars: bool,
) -> TestExecutor {
    let plugin_runtime = PluginRuntimeOptions {
        allow_missing_plugins: plugin_cfg.allow_missing_plugins,
        strict_plugins: plugin_cfg.strict_plugins,
        plugin_timeout: std::time::Duration::from_secs(
            plugin_cfg
                .plugin_timeout_secs
                .unwrap_or(crate::plugin::pipeline::DEFAULT_PLUGIN_TIMEOUT_SECS),
        ),
        plugin_env: crate::plugin::pipeline::parse_plugin_env_specs(&plugin_cfg.plugin_env),
    };
    let registry = discover_plugin_registry(
        discover,
        &crate::runner::suite_setup::extract_authorized_env_keys(&plugin_cfg.plugin_env),
    );
    TestExecutor::with_ephemeral_cookies()
        .with_strict_vars(strict_vars)
        .with_plugins(registry, plugin_runtime)
}

#[cfg(test)]
mod tests {

    /// T3/T4/T5（WP-2026-11-14-01）：with_options 映射 + zero-diff 防假守护 + env 豁免。
    #[test]
    fn test_build_interactive_executor_with_options_mapping_and_zero_diff() {
        use crate::variable::PluginTomlConfig;

        // T3：toml 字段 → PluginRuntimeOptions 映射
        let cfg = PluginTomlConfig {
            allow_missing_plugins: true,
            strict_plugins: true,
            plugin_timeout_secs: Some(15),
            plugin_env: vec!["VAULT_TOKEN=abc".to_string()],
            allow_project_plugins: true,
            strict_vars: true,
        };
        let executor = build_interactive_executor_with_options(
            &DiscoverOptions {
                allow_project_plugins: true,
                project_root: None,
            },
            &cfg,
            true,
        );
        let opts = &executor.plugin_options;
        assert!(opts.allow_missing_plugins);
        assert!(opts.strict_plugins);
        assert_eq!(opts.plugin_timeout, std::time::Duration::from_secs(15));
        assert_eq!(
            opts.plugin_env,
            vec![("VAULT_TOKEN".to_string(), "abc".to_string())]
        );
        assert!(executor.strict_vars, "B4：strict_vars 必须经工厂接线");

        // T4 zero-diff：Default 配置 → 与 PluginRuntimeOptions::default() 全等
        let executor_default = build_interactive_executor_with_options(
            &DiscoverOptions::default(),
            &PluginTomlConfig::default(),
            false,
        );
        let d = &executor_default.plugin_options;
        let base = PluginRuntimeOptions::default();
        // zero-diff：逐字段断言（PluginRuntimeOptions 无 PartialEq，防 derive 膨胀）
        assert!(!d.allow_missing_plugins);
        assert!(!d.strict_plugins);
        assert_eq!(d.plugin_timeout, base.plugin_timeout);
        assert!(d.plugin_env.is_empty());
        assert!(!executor_default.strict_vars);
    }

    /// T5：plugin_env 经 extract_authorized_env_keys 豁免 missing-env 告警。
    #[test]
    fn test_plugin_env_authorized_keys_exempt_from_missing_env_warnings() {
        let cfg = crate::variable::PluginTomlConfig {
            plugin_env: vec!["VAULT_TOKEN=abc".to_string(), "BARE_KEY".to_string()],
            ..Default::default()
        };
        let authorized = crate::runner::suite_setup::extract_authorized_env_keys(&cfg.plugin_env);
        assert!(authorized.contains("VAULT_TOKEN") && authorized.contains("BARE_KEY"));
    }
    use super::*;

    /// 工厂必须真实装配注册表（非空钩子索引），而非退化空注册表。
    #[test]
    fn test_discover_plugin_registry_loads_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join(".rupost").join("plugins");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rupost-plugin-factory-probe"),
            "#!/bin/sh\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.join("rupost-plugin-factory-probe"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("rupost-plugin-factory-probe.json"),
            r#"{"manifest_version":"1.0","name":"factory-probe","version":"0.1.0","description":"probe","commands":[],"hooks":[{"directive":"auth","alias":"factory-probe","stage":"prepare","cache_scope":"none","requires_env":[],"parameters_schema":{}}]}"#,
        )
        .unwrap();
        let reg = discover_plugin_registry(
            &DiscoverOptions {
                allow_project_plugins: true,
                project_root: Some(dir.parent().unwrap().parent().unwrap().to_path_buf()),
            },
            &HashSet::new(),
        );
        assert!(
            reg.find_hook("factory-probe").is_some(),
            "工厂发现必须能装载项目级插件 manifest"
        );
    }
}
