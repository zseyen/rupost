use super::registry_cache::{self};
pub use super::registry_cache::{CachedPluginEntry, RegistryCacheFile, manifest_fingerprint};
use super::types::{CommandCapability, HookCapability, PluginManifest};
use crate::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// 统一插件注册表 (PluginRegistry - DeepSeek Harness 架构模式)
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    /// CLI 子命令路由索引: 命令名称 -> (所属插件路径, 命令能力描述)
    pub commands: HashMap<String, (PathBuf, CommandCapability)>,
    /// HTTP 钩子路由索引: 指令/别名 -> (所属插件路径, 钩子能力描述)
    pub hooks: HashMap<String, (PathBuf, HookCapability)>,
    /// 已发现插件的可执行路径 -> 原始静态 manifest 路径（供回退名持久化写回缓存）
    manifest_paths: HashMap<PathBuf, PathBuf>,
    /// 发现过程中的非致命告警（alias 冲突、越界路径拒绝等，调用方负责渲染）
    pub warnings: Vec<String>,
}

/// 插件根目录扫描配置（03 §3.0/§3.2 安全裁决）
#[derive(Debug, Clone, Default)]
pub struct DiscoverOptions {
    /// 工作区 `./.rupost/plugins` 放行旗标（默认隔离防恶意仓库 RCE）
    pub allow_project_plugins: bool,
    /// 工作区根目录（默认当前目录）
    pub project_root: Option<PathBuf>,
}

/// 静态 manifest 文件命名契约：`rupost-plugin-<name>.json`（03 §3.0）
pub fn is_manifest_file(file_name: &str) -> bool {
    file_name.starts_with("rupost-plugin-") && file_name.ends_with(".json")
}

/// 由 manifest 文件名推导同目录插件二进制名（`rupost-plugin-<name>.json` →
/// `rupost-plugin-<name>`；兼容 `rupost-<name>`）
fn sibling_executables(manifest_path: &Path) -> Vec<PathBuf> {
    let stem = manifest_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let mut out = vec![manifest_path.with_file_name(stem)];
    if let Some(name) = stem.strip_prefix("rupost-plugin-") {
        out.push(manifest_path.with_file_name(format!("rupost-{name}")));
    }
    out
}

/// 收集有序且去重的插件扫描根目录：
/// `custom_path_roots` 为 `None` 时扫描 `$PATH` + `~/.rupost/plugins`；
/// 再按 `allow_project_plugins` 追加工作区 `./.rupost/plugins`（后者优先级更高）。
fn collect_plugin_roots(
    options: &DiscoverOptions,
    custom_path_roots: Option<&[PathBuf]>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    match custom_path_roots {
        Some(injected) => roots.extend(injected.iter().cloned()),
        None => {
            if let Some(path_env) = std::env::var_os("PATH") {
                roots.extend(std::env::split_paths(&path_env));
            }
            if let Some(home) = dirs::home_dir() {
                roots.push(home.join(".rupost").join("plugins"));
            }
        }
    }
    if options.allow_project_plugins {
        let base = options
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        roots.push(base.join(".rupost").join("plugins"));
    }
    roots
}

impl PluginRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个插件 Manifest 实体到内存索引中（后注册覆盖先注册 + 冲突告警）
    pub fn register_manifest(&mut self, executable_path: PathBuf, manifest: PluginManifest) {
        let plugin_name = manifest.name.clone();
        for cmd in manifest.commands {
            if let Some(prev) = self.commands.get(&cmd.name) {
                self.warnings.push(format!(
                    "插件命令 '{}' 冲突：'{}' 覆盖 '{}' 的注册（内置命令始终优先）",
                    cmd.name,
                    plugin_name,
                    prev.0.display()
                ));
            }
            self.commands
                .insert(cmd.name.clone(), (executable_path.clone(), cmd));
        }
        for hook in manifest.hooks {
            if let Some(alias) = &hook.alias {
                if let Some(prev) = self.hooks.get(alias) {
                    self.warnings.push(format!(
                        "插件别名 '{}' 冲突：'{}' 覆盖 '{}' 的注册",
                        alias,
                        plugin_name,
                        prev.0.display()
                    ));
                }
                self.hooks
                    .insert(alias.clone(), (executable_path.clone(), hook.clone()));
            }
            self.hooks
                .insert(hook.directive.clone(), (executable_path.clone(), hook));
        }
    }

    /// 带原始 manifest 路径注册（确保 `rupost-<name>` 回退二进制或带后缀二进制均可正确写回缓存）
    pub fn register_manifest_with_source(
        &mut self,
        executable_path: PathBuf,
        manifest_path: PathBuf,
        manifest: PluginManifest,
    ) {
        self.manifest_paths
            .insert(executable_path.clone(), manifest_path);
        self.register_manifest(executable_path, manifest);
    }

    /// 根据 CLI 子命令名查找插件入口
    pub fn find_command(&self, name: &str) -> Option<&(PathBuf, CommandCapability)> {
        self.commands.get(name)
    }

    /// 根据指令名或插件别名查找 Hook 入口
    pub fn find_hook(&self, directive_or_alias: &str) -> Option<&(PathBuf, HookCapability)> {
        self.hooks.get(directive_or_alias)
    }

    /// 静态 manifest 发现（03 §3.0 安全裁决：readdir + JSON 解析，**零子进程**）。
    pub fn discover(options: &DiscoverOptions) -> Result<Self> {
        Self::discover_with_roots(options, None)
    }

    /// 允许显式注入基础搜索根目录的发现入口（供测试无全局 env 竞态隔离使用）。
    pub fn discover_with_roots(
        options: &DiscoverOptions,
        custom_path_roots: Option<&[PathBuf]>,
    ) -> Result<Self> {
        let mut registry = Self::new();
        for root in collect_plugin_roots(options, custom_path_roots) {
            if !root.is_dir() {
                continue;
            }
            let Ok(root_canon) = root.canonicalize() else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
                    continue;
                };
                if !is_manifest_file(file_name) {
                    continue;
                }
                if let Err(reason) = Self::register_from_manifest(&mut registry, &root_canon, &path)
                {
                    registry
                        .warnings
                        .push(format!("跳过 manifest {}: {reason}", path.display()));
                }
            }
        }
        Ok(registry)
    }

    /// 注册单个静态 manifest：解析 JSON + 同目录二进制白名单校验
    fn register_from_manifest(
        registry: &mut Self,
        root_canon: &Path,
        manifest_path: &Path,
    ) -> std::result::Result<(), String> {
        let content =
            std::fs::read_to_string(manifest_path).map_err(|e| format!("读取失败: {e}"))?;
        let mut value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("JSON 解析失败: {e}"))?;
        // interface-freeze-m1 §1 裁决：post_response/on_failure/harvest 为 P2 保留字阶段，
        // manifest 中出现即拒绝注册（绝不静默注册永不调用的死钩子）。
        if let Some(hooks) = value.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            for hook in hooks {
                let Some(obj) = hook.as_object_mut() else {
                    continue;
                };
                let directive_label = obj
                    .get("directive")
                    .and_then(|d| d.as_str())
                    .unwrap_or("?")
                    .to_string();
                // types.rs cache_scope 裁决：P2 保留值 ttl → 告警 + 按 none 归位（不解析失败）
                let ttl_declared = obj.contains_key("ttl")
                    || matches!(obj.get("cache_scope"), Some(serde_json::Value::Object(_)));
                if ttl_declared {
                    registry.warnings.push(format!(
                        "manifest hook '{directive_label}' 声明了 P2 保留值 ttl，已忽略并按 cache_scope=none 处理"
                    ));
                    obj.remove("ttl");
                    obj.insert(
                        "cache_scope".to_string(),
                        serde_json::Value::String("none".to_string()),
                    );
                }
                if let Some(stage_str) = obj.get("stage").and_then(|s| s.as_str())
                    && matches!(stage_str, "post_response" | "on_failure" | "harvest")
                {
                    return Err(format!(
                        "hook '{directive_label}' 声明了未放行的保留字阶段 '{stage_str}'（P2 保留字，MVP 注册面仅 prepare/transform/sign）"
                    ));
                }
            }
        }
        let manifest: PluginManifest =
            serde_json::from_value(value).map_err(|e| format!("JSON 解析失败: {e}"))?;
        if manifest.manifest_version != "1.0" {
            return Err(format!(
                "不支持的 manifest_version: {}",
                manifest.manifest_version
            ));
        }

        let candidates = sibling_executables(manifest_path);
        let executable = candidates
            .iter()
            .find(|c| c.exists())
            .ok_or_else(|| "同目录插件二进制不存在".to_string())?;
        let exe_canon = executable
            .canonicalize()
            .map_err(|e| format!("canonicalize 失败: {e}"))?;
        if !exe_canon.starts_with(root_canon) {
            return Err("可执行路径逃逸插件根（symlink/.. 攻击拒绝）".to_string());
        }

        let manifest_canon = manifest_path
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.to_path_buf());
        registry.register_manifest_with_source(exe_canon, manifest_canon, manifest);
        Ok(())
    }

    pub fn load_cache(cache_path: &Path) -> Option<RegistryCacheFile> {
        registry_cache::load_cache(cache_path)
    }

    pub fn save_cache_atomic(cache: &RegistryCacheFile, target_file: &Path) -> Result<()> {
        registry_cache::save_cache_atomic(cache, target_file)
    }

    /// 从磁盘缓存或重新发现构建注册表（`--allow-project-plugins` 由调用方透传）。
    pub fn load_or_discover(options: &DiscoverOptions, cache_path: &Path) -> Result<Self> {
        Self::load_or_discover_with_roots(options, cache_path, None)
    }

    /// 带注入根目录的缓存/发现闭环入口（无全局 `PATH` 副作用）。
    pub fn load_or_discover_with_roots(
        options: &DiscoverOptions,
        cache_path: &Path,
        custom_path_roots: Option<&[PathBuf]>,
    ) -> Result<Self> {
        if let Some(cache) = Self::load_cache(cache_path) {
            let inventory = Self::scan_manifest_inventory(options, custom_path_roots);
            let cache_set: BTreeSet<(String, u64, String)> = cache
                .manifests
                .iter()
                .map(|e| {
                    (
                        e.manifest_path.clone(),
                        e.mtime_secs,
                        e.fingerprint_hash.clone(),
                    )
                })
                .collect();
            let disk_set: BTreeSet<(String, u64, String)> = inventory
                .into_iter()
                .map(|(path, (mtime, hash))| (path.to_string_lossy().into_owned(), mtime, hash))
                .collect();
            if cache_set == disk_set {
                let mut registry = Self::new();
                for entry in cache.manifests {
                    registry.register_manifest_with_source(
                        PathBuf::from(entry.executable_path),
                        PathBuf::from(entry.manifest_path),
                        entry.manifest,
                    );
                }
                return Ok(registry);
            }
        }
        let mut registry = Self::discover_with_roots(options, custom_path_roots)?;
        registry.persist_cache(cache_path);
        Ok(registry)
    }

    /// 插件根静态 manifest 清单扫描（零子进程：readdir + 指纹，供缓存新鲜度对比）
    fn scan_manifest_inventory(
        options: &DiscoverOptions,
        custom_path_roots: Option<&[PathBuf]>,
    ) -> Vec<(PathBuf, (u64, String))> {
        let mut inventory = Vec::new();
        for root in collect_plugin_roots(options, custom_path_roots) {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
                    continue;
                };
                if is_manifest_file(file_name)
                    && let Some(fp) = manifest_fingerprint(&path)
                {
                    let canon = path.canonicalize().unwrap_or(path);
                    inventory.push((canon, fp));
                }
            }
        }
        inventory
    }

    /// 将当前注册表内容静默写回缓存（失败仅登记告警，绝不阻塞主流程）
    pub fn persist_cache(&mut self, cache_path: &Path) {
        let mut manifests: Vec<CachedPluginEntry> = Vec::new();
        let mut seen = BTreeSet::new();
        let command_exes = self.commands.values().map(|(p, _)| p);
        let hook_exes = self.hooks.values().map(|(p, _)| p);
        for exe_path in command_exes.chain(hook_exes) {
            if !seen.insert(exe_path.clone()) {
                continue;
            }
            let manifest_path = self
                .manifest_paths
                .get(exe_path)
                .cloned()
                .unwrap_or_else(|| exe_path.with_extension("json"));
            if let Some((mtime, hash)) = manifest_fingerprint(&manifest_path)
                && let Ok(content) = std::fs::read_to_string(&manifest_path)
                && let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content)
            {
                manifests.push(CachedPluginEntry {
                    executable_path: exe_path.to_string_lossy().into_owned(),
                    manifest_path: manifest_path.to_string_lossy().into_owned(),
                    mtime_secs: mtime,
                    fingerprint_hash: hash,
                    manifest,
                });
            }
        }
        let cache = RegistryCacheFile {
            version: "1.0".to_string(),
            updated_at_utc: chrono::Utc::now().to_rfc3339(),
            manifests,
        };
        if let Err(e) = Self::save_cache_atomic(&cache, cache_path) {
            self.warnings
                .push(format!("注册表缓存写回失败（不影响本次执行）: {e}"));
        }
    }
}
