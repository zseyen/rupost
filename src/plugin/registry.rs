use super::types::{CommandCapability, HookCapability, PluginManifest};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 磁盘轻量注册表高速缓存结构 (位于 ~/.rupost/registry.json)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RegistryCacheFile {
    pub version: String,
    pub updated_at_utc: String,
    pub manifests: Vec<CachedPluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedPluginEntry {
    pub executable_path: String,
    pub mtime_secs: u64,
    pub fingerprint_hash: String,
    pub manifest: PluginManifest,
}

/// 统一插件注册表 (PluginRegistry - DeepSeek Harness 架构模式)
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    /// CLI 子命令路由索引: 命令名称 -> (所属插件路径, 命令能力描述)
    pub commands: HashMap<String, (PathBuf, CommandCapability)>,
    /// HTTP 钩子路由索引: 指令/别名 -> (所属插件路径, 钩子能力描述)
    pub hooks: HashMap<String, (PathBuf, HookCapability)>,
}

impl PluginRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个插件 Manifest 实体到内存索引中
    pub fn register_manifest(&mut self, executable_path: PathBuf, manifest: PluginManifest) {
        for cmd in manifest.commands {
            self.commands
                .insert(cmd.name.clone(), (executable_path.clone(), cmd));
        }
        for hook in manifest.hooks {
            if let Some(alias) = &hook.alias {
                self.hooks
                    .insert(alias.clone(), (executable_path.clone(), hook.clone()));
            }
            self.hooks
                .insert(hook.directive.clone(), (executable_path.clone(), hook));
        }
    }

    /// 根据 CLI 子命令名查找插件入口
    pub fn find_command(&self, name: &str) -> Option<&(PathBuf, CommandCapability)> {
        self.commands.get(name)
    }

    /// 根据指令名或插件别名查找 Hook 入口
    pub fn find_hook(&self, directive_or_alias: &str) -> Option<&(PathBuf, HookCapability)> {
        self.hooks.get(directive_or_alias)
    }

    /// 原子化持久化保存到磁盘缓存文件 (先写临时文件再 rename，杜绝并发损坏)
    pub fn save_cache_atomic(cache: &RegistryCacheFile, target_file: &Path) -> Result<()> {
        if let Some(parent) = target_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(cache)?;
        let tmp_file = target_file.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp_file, json_str)?;
        std::fs::rename(&tmp_file, target_file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::types::PluginStage;

    #[test]
    fn test_registry_registration_and_lookup() {
        let mut registry = PluginRegistry::new();
        let manifest = PluginManifest {
            manifest_version: "1.0".to_string(),
            name: "aws-sigv4".to_string(),
            version: "0.1.0".to_string(),
            description: "AWS Signature plugin".to_string(),
            commands: vec![CommandCapability {
                name: "sigv4".to_string(),
                usage: "rupost sigv4 --inspect".to_string(),
                description: "Inspect credentials".to_string(),
                requires_tty: true,
            }],
            hooks: vec![HookCapability {
                directive: "auth".to_string(),
                alias: Some("aws-sigv4".to_string()),
                stage: PluginStage::Prepare,
                default_priority: 20,
                parameters_schema: serde_json::json!({"type": "object"}),
            }],
        };

        let fake_bin = PathBuf::from("/usr/local/bin/rupost-plugin-aws-sigv4");
        registry.register_manifest(fake_bin.clone(), manifest);

        // 验证命令行自定义子命令索引
        let found_cmd = registry.find_command("sigv4").expect("command found");
        assert_eq!(found_cmd.0, fake_bin);
        assert_eq!(found_cmd.1.name, "sigv4");

        // 验证 Hook 别名索引
        let found_hook = registry.find_hook("aws-sigv4").expect("hook found");
        assert_eq!(found_hook.0, fake_bin);
        assert_eq!(found_hook.1.stage, PluginStage::Prepare);
    }
}
