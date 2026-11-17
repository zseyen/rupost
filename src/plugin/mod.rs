//! RuPost 插件体系与微内核扩展基础设施 (Clean Architecture Microkernel)
//!
//! 遵循 DeepSeek Harness 统一注册表哲学，实现零外部运行时依赖的 Stdio IPC 扩展通道。

pub mod driver;
pub mod patch;
pub mod pipeline;
pub mod protocol;
pub mod registry;
pub mod registry_cache;
pub mod types;

pub use driver::invoke_plugin;
pub use patch::PluginPatch;
pub use pipeline::{
    PluginPipeline, PrescriptionFormatter, parse_plugin_env_specs,
    parse_plugin_env_specs_with_warnings,
};
pub use protocol::{PluginFailureKind, PluginInvocationOutcome};
pub use registry::{DiscoverOptions, PluginRegistry, RegistryCacheFile};
pub use types::{
    CommandCapability, HookCapability, PLUGIN_ENV_ALLOWLIST, PluginCacheScope, PluginDirective,
    PluginManifest, PluginStage,
};

/// 从原始 `--plugin-env` 参数列表提取已放行的环境变量名集合（保留未在宿主机设值的裸 `KEY`）。
pub fn extract_authorized_env_keys(specs: &[String]) -> std::collections::HashSet<String> {
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
