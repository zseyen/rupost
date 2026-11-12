use super::registry_cache::{self};
pub use super::registry_cache::{CachedPluginEntry, RegistryCacheFile, manifest_fingerprint};
use super::types::{
    CommandCapability, FailedPluginRecord, HookCapability, PLUGIN_ENV_ALLOWLIST,
    PluginHealthStatus, PluginManifest, PluginRuntime, PluginStage, PluginStatusEntry,
};
use crate::Result;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// 统一插件注册表 (PluginRegistry - DeepSeek Harness 架构模式)
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    /// CLI 子命令路由索引: 命令名称 -> (所属插件路径, 命令能力描述)
    pub commands: HashMap<String, (PathBuf, CommandCapability)>,
    /// HTTP 钩子路由索引（兼容性扁平视图）: 指令/别名 -> (所属插件路径, 钩子能力描述)
    pub hooks: HashMap<String, (PathBuf, HookCapability)>,
    /// HTTP 钩子复合键索引（03 §3.2 #1 SSOT）: (指令/别名, 阶段) -> (所属插件路径, 钩子能力描述)
    hooks_by_stage: HashMap<(String, PluginStage), (PathBuf, HookCapability)>,
    /// 已发现插件的可执行路径 -> (原始静态 manifest 路径, 已归一化 Manifest 实体)
    discovered_manifests: HashMap<PathBuf, (PathBuf, PluginManifest)>,
    /// 直接通过 `register_manifest` 注册的 Manifest 实体（测试/内联构造路径）
    registered_manifests: HashMap<PathBuf, PluginManifest>,
    /// 发现期解析失败或被安全策略拒绝的插件清单记录（仅内存诊断，绝不写入缓存）
    failed_plugins: Vec<FailedPluginRecord>,
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
/// `rupost-plugin-<name>`；兼容 `rupost-<name>`；Windows 额外探测 `.exe`/`.cmd`/`.bat`）
fn sibling_executables(manifest_path: &Path) -> Vec<PathBuf> {
    let stem = manifest_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let mut bases = vec![manifest_path.with_file_name(stem)];
    if let Some(name) = stem.strip_prefix("rupost-plugin-") {
        bases.push(manifest_path.with_file_name(format!("rupost-{name}")));
    }
    #[cfg(windows)]
    {
        let mut out = Vec::with_capacity(bases.len() * 4);
        for base in &bases {
            for ext in ["exe", "cmd", "bat"] {
                let mut os = base.as_os_str().to_os_string();
                os.push(format!(".{ext}"));
                out.push(PathBuf::from(os));
            }
            out.push(base.clone());
        }
        out
    }
    #[cfg(not(windows))]
    {
        bases
    }
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

struct ValidatedManifest {
    exe_canon: PathBuf,
    manifest_canon: PathBuf,
    manifest: PluginManifest,
    warnings: Vec<String>,
}

fn has_reserved_stage(manifest: &PluginManifest) -> bool {
    manifest
        .hooks
        .iter()
        .any(|h| matches!(h.stage, PluginStage::PostResponse | PluginStage::OnFailure))
}

fn parse_and_validate_manifest(
    root_canon: &Path,
    manifest_path: &Path,
) -> std::result::Result<ValidatedManifest, (Option<String>, String)> {
    let content =
        std::fs::read_to_string(manifest_path).map_err(|e| (None, format!("读取失败: {e}")))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| (None, format!("JSON 解析失败: {e}")))?;
    let parsed_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut warnings = Vec::new();
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
                warnings.push(format!(
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
                return Err((
                    parsed_name,
                    format!(
                        "hook '{directive_label}' 声明了未放行的保留字阶段 '{stage_str}'（P2 保留字，MVP 注册面仅 prepare/transform/sign）"
                    ),
                ));
            }
        }
    }
    let manifest: PluginManifest = serde_json::from_value(value)
        .map_err(|e| (parsed_name.clone(), format!("JSON 解析失败: {e}")))?;
    if manifest.manifest_version != "1.0" {
        return Err((
            Some(manifest.name),
            format!("不支持的 manifest_version: {}", manifest.manifest_version),
        ));
    }
    if has_reserved_stage(&manifest) {
        return Err((
            Some(manifest.name),
            "manifest 包含未放行的 P2 保留字阶段钩子".to_string(),
        ));
    }

    let candidates = sibling_executables(manifest_path);
    let executable = candidates.iter().find(|c| c.exists()).ok_or_else(|| {
        (
            Some(manifest.name.clone()),
            "同目录插件二进制不存在".to_string(),
        )
    })?;
    let exe_canon = executable.canonicalize().map_err(|e| {
        (
            Some(manifest.name.clone()),
            format!("canonicalize 失败: {e}"),
        )
    })?;
    if !exe_canon.is_file() {
        return Err((
            Some(manifest.name.clone()),
            "插件可执行入口不是普通文件".to_string(),
        ));
    }
    if !exe_canon.starts_with(root_canon) {
        return Err((
            Some(manifest.name.clone()),
            "可执行路径逃逸插件根（symlink/.. 攻击拒绝）".to_string(),
        ));
    }

    let manifest_canon = manifest_path
        .canonicalize()
        .unwrap_or_else(|_| manifest_path.to_path_buf());
    Ok(ValidatedManifest {
        exe_canon,
        manifest_canon,
        manifest,
        warnings,
    })
}

fn validate_cached_entry(entry: &CachedPluginEntry) -> bool {
    if entry.manifest.manifest_version != "1.0" || has_reserved_stage(&entry.manifest) {
        return false;
    }
    let manifest_path = Path::new(&entry.manifest_path);
    let Some(manifest_parent) = manifest_path.parent() else {
        return false;
    };
    if !crate::security::fs::is_trusted_dir(manifest_parent) {
        return false;
    }
    let Ok(root_canon) = manifest_parent.canonicalize() else {
        return false;
    };
    let Ok(exe_canon) = Path::new(&entry.executable_path).canonicalize() else {
        return false;
    };
    exe_canon.is_file() && exe_canon.starts_with(&root_canon)
}

impl PluginRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个插件 Manifest 实体到内存索引中（后注册覆盖先注册 + 冲突告警）
    pub fn register_manifest(&mut self, executable_path: PathBuf, manifest: PluginManifest) {
        self.registered_manifests
            .insert(executable_path.clone(), manifest.clone());
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
                let stage_key = (alias.clone(), hook.stage);
                if let Some(prev) = self.hooks_by_stage.get(&stage_key) {
                    self.warnings.push(format!(
                        "插件别名 '{}' (stage={:?}) 冲突：'{}' 覆盖 '{}' 的注册",
                        alias,
                        hook.stage,
                        plugin_name,
                        prev.0.display()
                    ));
                }
                self.hooks_by_stage
                    .insert(stage_key, (executable_path.clone(), hook.clone()));
                self.hooks
                    .insert(alias.clone(), (executable_path.clone(), hook.clone()));
            }
            self.hooks_by_stage.insert(
                (hook.directive.clone(), hook.stage),
                (executable_path.clone(), hook.clone()),
            );
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
        self.discovered_manifests
            .insert(executable_path.clone(), (manifest_path, manifest.clone()));
        self.register_manifest(executable_path, manifest);
    }

    /// 根据 CLI 子命令名查找插件入口
    pub fn find_command(&self, name: &str) -> Option<&(PathBuf, CommandCapability)> {
        self.commands.get(name)
    }

    /// 根据指令名或插件别名查找 Hook 入口（无阶段限定时的兼容入口）
    pub fn find_hook(&self, directive_or_alias: &str) -> Option<&(PathBuf, HookCapability)> {
        self.hooks.get(directive_or_alias)
    }

    /// 根据指令名/别名与生命周期阶段精确查找 Hook 入口（消除多阶段钩子键冲突）
    pub fn find_hook_for_stage(
        &self,
        directive_or_alias: &str,
        stage: PluginStage,
    ) -> Option<&(PathBuf, HookCapability)> {
        self.hooks_by_stage
            .get(&(directive_or_alias.to_string(), stage))
    }

    /// 静态 manifest 发现（03 §3.0 安全裁决：readdir + JSON 解析，**零子进程**）。
    pub fn discover(options: &DiscoverOptions) -> Result<Self> {
        Self::discover_with_roots(options, None)
    }

    /// 带完整诊断信息的全量扫描发现（绕过缓存，收集 `failed_plugins` 供 `plugin list` 展示三态）。
    pub fn discover_with_diagnostics(options: &DiscoverOptions) -> Result<Self> {
        Self::discover_with_roots(options, None)
    }

    /// 返回发现期解析失败或被拒绝的插件清单记录
    pub fn failed_plugins(&self) -> &[FailedPluginRecord] {
        &self.failed_plugins
    }

    /// 查询指定可执行路径对应的插件运行时声明
    pub fn runtime_for_executable(&self, exe_path: &Path) -> Option<&PluginRuntime> {
        self.discovered_manifests
            .get(exe_path)
            .map(|(_, m)| m)
            .or_else(|| self.registered_manifests.get(exe_path))
            .and_then(|m| m.runtime.as_ref())
    }

    /// 允许显式注入基础搜索根目录的发现入口（供测试无全局 env 竞态隔离使用）。
    pub fn discover_with_roots(
        options: &DiscoverOptions,
        custom_path_roots: Option<&[PathBuf]>,
    ) -> Result<Self> {
        let mut registry = Self::new();
        for root in collect_plugin_roots(options, custom_path_roots) {
            if !root.exists() || !root.is_dir() {
                continue;
            }
            if !crate::security::fs::is_trusted_dir(&root) {
                registry.warnings.push(format!(
                    "跳过不可信插件目录 {}: 属主或权限不安全",
                    root.display()
                ));
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
                match parse_and_validate_manifest(&root_canon, &path) {
                    Ok(validated) => {
                        registry.warnings.extend(validated.warnings);
                        registry.register_manifest_with_source(
                            validated.exe_canon,
                            validated.manifest_canon,
                            validated.manifest,
                        );
                    }
                    Err((name, reason)) => {
                        registry
                            .warnings
                            .push(format!("跳过 manifest {}: {reason}", path.display()));
                        registry.failed_plugins.push(FailedPluginRecord {
                            manifest_path: path.canonicalize().unwrap_or(path),
                            name,
                            reason,
                        });
                    }
                }
            }
        }
        Ok(registry)
    }

    /// 响应式计算所有已发现/失败插件的三态健康状态（Ready / Pending / Failed），绝不持久化。
    pub fn evaluate_statuses(&self, path_env: Option<&OsStr>) -> Vec<PluginStatusEntry> {
        let mut entries = Vec::new();
        for (exe_path, (manifest_path, manifest)) in &self.discovered_manifests {
            entries.push(build_status_entry(
                exe_path.clone(),
                manifest_path.clone(),
                manifest,
                path_env,
            ));
        }
        for (exe_path, manifest) in &self.registered_manifests {
            if self.discovered_manifests.contains_key(exe_path) {
                continue;
            }
            entries.push(build_status_entry(
                exe_path.clone(),
                exe_path.with_extension("json"),
                manifest,
                path_env,
            ));
        }
        for rec in &self.failed_plugins {
            let fallback_name = rec
                .manifest_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            entries.push(PluginStatusEntry {
                name: rec.name.clone().unwrap_or(fallback_name),
                version: String::new(),
                description: String::new(),
                manifest_path: rec.manifest_path.clone(),
                executable_path: None,
                status: PluginHealthStatus::Failed {
                    reason: rec.reason.clone(),
                },
            });
        }
        entries.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.manifest_path.cmp(&b.manifest_path))
        });
        entries
    }

    /// 收集已使用插件指令/命令的 `requires_env` 未放行告警（去重；放行 `RUPOST_*`、`PLUGIN_ENV_ALLOWLIST` 与 `--plugin-env`）。
    pub fn collect_missing_env_warnings(
        &self,
        used_directives: &[String],
        authorized_env_keys: &HashSet<String>,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut seen_pairs: HashSet<(String, String)> = HashSet::new();

        for directive in used_directives {
            let mut hook_matches: Vec<(PluginStage, &[String])> = self
                .hooks_by_stage
                .iter()
                .filter(|((key, _), _)| key == directive)
                .map(|((_, stage), (_, hook))| (*stage, hook.requires_env.as_slice()))
                .collect();
            hook_matches.sort_by_key(|(stage, _)| *stage);

            let cmd_reqs: &[String] = self
                .commands
                .get(directive)
                .map(|(_, cmd)| cmd.requires_env.as_slice())
                .unwrap_or(&[]);

            for key in hook_matches
                .into_iter()
                .flat_map(|(_, reqs)| reqs.iter())
                .chain(cmd_reqs.iter())
            {
                let key = key.trim();
                if key.is_empty() {
                    continue;
                }
                if key.starts_with("RUPOST_")
                    || PLUGIN_ENV_ALLOWLIST.contains(&key)
                    || authorized_env_keys.contains(key)
                {
                    continue;
                }
                if seen_pairs.insert((directive.clone(), key.to_string())) {
                    warnings.push(format!(
                        "插件 '{directive}' 声明需要环境变量 '{key}'，但运行时未通过 --plugin-env {key} 显式放行，子进程将无法读取该变量"
                    ));
                }
            }
        }
        warnings
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
            // 已知取舍（C-2 回滚登记）：缓存清单仅含合法 manifest——新增坏 manifest
            // 会使磁盘态≠缓存态而触发重发现+告警（可见），但下次起缓存指纹不含它
            // 而静默命中；`t` 路径不可见属已知取舍，排查请用 `plugin list`（绕缓存）
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
            let inventory = Self::scan_manifest_inventory(options, custom_path_roots, &cache_set);
            let disk_set: BTreeSet<(String, u64, String)> = inventory
                .into_iter()
                .map(|(path, (mtime, hash))| (path.to_string_lossy().into_owned(), mtime, hash))
                .collect();
            if cache_set == disk_set && cache.manifests.iter().all(validate_cached_entry) {
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

    /// 插件根静态 manifest 清单扫描（仅纳入合法可发现的 manifest，防止孤儿/非法 manifest 引发永久缓存抖动）
    fn scan_manifest_inventory(
        options: &DiscoverOptions,
        custom_path_roots: Option<&[PathBuf]>,
        cached_entries: &BTreeSet<(String, u64, String)>,
    ) -> Vec<(PathBuf, (u64, String))> {
        let mut inventory = Vec::new();
        for root in collect_plugin_roots(options, custom_path_roots) {
            if !root.exists() || !root.is_dir() || !crate::security::fs::is_trusted_dir(&root) {
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
                let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                let Some(fp) = manifest_fingerprint(&canon) else {
                    continue;
                };
                let key = (canon.to_string_lossy().into_owned(), fp.0, fp.1.clone());
                if cached_entries.contains(&key)
                    || parse_and_validate_manifest(&root_canon, &path).is_ok()
                {
                    inventory.push((canon, fp));
                }
            }
        }
        inventory
    }

    /// 将当前注册表内容静默写回缓存（持久化内存已归一化的 Manifest，失败仅登记告警，绝不阻塞主流程）
    pub fn persist_cache(&mut self, cache_path: &Path) {
        let mut manifests: Vec<CachedPluginEntry> = Vec::new();
        for (exe_path, (manifest_path, manifest)) in &self.discovered_manifests {
            if let Some((mtime, hash)) = manifest_fingerprint(manifest_path) {
                manifests.push(CachedPluginEntry {
                    executable_path: exe_path.to_string_lossy().into_owned(),
                    manifest_path: manifest_path.to_string_lossy().into_owned(),
                    mtime_secs: mtime,
                    fingerprint_hash: hash,
                    manifest: manifest.clone(),
                });
            }
        }
        manifests.sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
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

fn build_status_entry(
    exe_path: PathBuf,
    manifest_path: PathBuf,
    manifest: &PluginManifest,
    path_env: Option<&OsStr>,
) -> PluginStatusEntry {
    let status = match preflight_runtime(manifest.runtime.as_ref(), path_env) {
        Ok(()) => PluginHealthStatus::Ready,
        Err((interpreter, searched_dirs)) => {
            let rt = manifest.runtime.as_ref();
            PluginHealthStatus::Pending {
                interpreter,
                min_version: rt.and_then(|r| r.min_version.clone()),
                modules: rt.map(|r| r.modules.clone()).unwrap_or_default(),
                searched_dirs,
            }
        }
    };
    PluginStatusEntry {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        manifest_path,
        executable_path: Some(exe_path),
        status,
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 零子进程运行时解释器预检（03 §3.1.1：仅对 PATH 目录做 stat + Unix `mode & 0o111 != 0` 校验，绝不 spawn `--version`）。
pub fn preflight_runtime(
    runtime: Option<&PluginRuntime>,
    path_env: Option<&OsStr>,
) -> std::result::Result<(), (String, usize)> {
    let Some(interp) = runtime
        .and_then(|r| r.interpreter.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };

    if interp.contains('/') || interp.contains('\\') || Path::new(interp).components().count() > 1 {
        return if is_executable_file(Path::new(interp)) {
            Ok(())
        } else {
            Err((interp.to_string(), 1))
        };
    }

    let Some(raw_path) = path_env.filter(|p| !p.is_empty()) else {
        return Err((interp.to_string(), 0));
    };

    let mut searched_dirs = 0usize;
    for dir in std::env::split_paths(raw_path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        searched_dirs += 1;
        if is_executable_file(&dir.join(interp)) {
            return Ok(());
        }
    }
    Err((interp.to_string(), searched_dirs))
}
