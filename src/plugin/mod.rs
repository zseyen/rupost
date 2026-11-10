//! RuPost 插件体系与微内核扩展基础设施 (Clean Architecture Microkernel)
//!
//! 遵循 DeepSeek Harness 统一注册表哲学，实现零外部运行时依赖的 Stdio IPC 扩展通道。

pub mod patch;
pub mod pipeline;
pub mod registry;
pub mod types;

pub use patch::PluginPatch;
pub use pipeline::PluginPipeline;
pub use registry::PluginRegistry;
pub use types::{CommandCapability, HookCapability, PluginDirective, PluginManifest, PluginStage};
