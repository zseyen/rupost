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
pub use pipeline::{PluginPipeline, parse_plugin_env_spec, parse_plugin_env_specs};
pub use protocol::{PluginFailureKind, PluginInvocationOutcome};
pub use registry::{DiscoverOptions, PluginRegistry, RegistryCacheFile};
pub use types::{
    CommandCapability, HookCapability, PLUGIN_ENV_ALLOWLIST, PluginCacheScope, PluginDirective,
    PluginManifest, PluginStage,
};
