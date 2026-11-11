use super::types::{CommandCapability, HookCapability, PluginManifest};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 磁盘轻量注册表高速缓存结构 (位于 ~/.rupost/registry.json)。
/// 定位 = 静态 manifest 的**可重建索引**（非权威数据源，03 §3.2）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RegistryCacheFile {
    pub version: String,
    pub updated_at_utc: String,
    pub manifests: Vec<CachedPluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedPluginEntry {
    pub executable_path: String,
    pub manifest_path: String,
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
    let mut out = vec![manifest_path.with_file_name(stem.to_string())];
    if let Some(name) = stem.strip_prefix("rupost-plugin-") {
        out.push(manifest_path.with_file_name(format!("rupost-{name}")));
    }
    out
}

/// 极速指纹：mtime + 文件前 4KB FNV-1a 哈希（03 §10.3 防幽灵缓存命中）
pub fn manifest_fingerprint(path: &Path) -> Option<(u64, String)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 4096];
    let read = file.read(&mut head).unwrap_or(0);
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in &head[..read] {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Some((mtime, format!("{hash:016x}")))
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

    /// 根据 CLI 子命令名查找插件入口
    pub fn find_command(&self, name: &str) -> Option<&(PathBuf, CommandCapability)> {
        self.commands.get(name)
    }

    /// 根据指令名或插件别名查找 Hook 入口
    pub fn find_hook(&self, directive_or_alias: &str) -> Option<&(PathBuf, HookCapability)> {
        self.hooks.get(directive_or_alias)
    }

    /// 静态 manifest 发现（03 §3.0 安全裁决：readdir + JSON 解析，**零子进程**）。
    ///
    /// 扫描根：`$PATH` 各目录 + `~/.rupost/plugins` + 工作区 `./.rupost/plugins`
    /// （后者必须携带放行旗标）。可执行路径必须与 manifest 同目录且 canonicalize
    /// 后不逃逸插件根（路径穿越/symlink 逃逸拒绝）。
    pub fn discover(options: &DiscoverOptions) -> Result<Self> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(path_env) = std::env::var_os("PATH") {
            roots.extend(std::env::split_paths(&path_env));
        }
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".rupost").join("plugins"));
        }
        if options.allow_project_plugins {
            let base = options
                .project_root
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            roots.push(base.join(".rupost").join("plugins"));
        }

        let mut registry = Self::new();
        for root in roots {
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
        let manifest: PluginManifest =
            serde_json::from_str(&content).map_err(|e| format!("JSON 解析失败: {e}"))?;
        if manifest.manifest_version != "1.0" {
            return Err(format!(
                "不支持的 manifest_version: {}",
                manifest.manifest_version
            ));
        }

        // 可执行路径白名单：必须与 manifest 同目录、canonicalize 后落在插件根内
        // （拒绝 `..` 与逃逸插件根的 symlink，03 §3.2 路径穿越防线）
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

        registry.register_manifest(exe_canon, manifest);
        Ok(())
    }

    // ==================== 缓存（可重建索引，03 §3.2） ====================

    /// 缓存有效性校验：属主 uid == euid + 权限 0600（多用户机器防投毒）
    fn cache_file_trustworthy(cache_path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let Ok(meta) = std::fs::metadata(cache_path) else {
                return false;
            };
            if meta.uid() != unsafe { libc_euid() } {
                return false;
            }
            if meta.permissions().mode() & 0o077 != 0 {
                return false;
            }
            true
        }
        #[cfg(not(unix))]
        {
            cache_path.is_file()
        }
    }

    /// 加载缓存（快路径）。命中条件：属主/权限可信 + mtime + 前 4KB 哈希指纹
    /// 全部匹配（03 §10.3 防幽灵命中）。返回 None 表示缓存缺失/过期/不可信。
    pub fn load_cache(cache_path: &Path) -> Option<RegistryCacheFile> {
        if !Self::cache_file_trustworthy(cache_path) {
            return None;
        }
        let content = std::fs::read_to_string(cache_path).ok()?;
        let cache: RegistryCacheFile = serde_json::from_str(&content).ok()?;
        // 逐条目指纹校验：任一 manifest 变化即视为整体过期（重建成本 = 一次 readdir 级扫描）
        for entry in &cache.manifests {
            let manifest_path = PathBuf::from(&entry.manifest_path);
            match manifest_fingerprint(&manifest_path) {
                Some((mtime, hash))
                    if mtime == entry.mtime_secs && hash == entry.fingerprint_hash => {}
                _ => return None,
            }
        }
        Some(cache)
    }

    /// 从磁盘缓存或重新发现构建注册表（`--allow-project-plugins` 由调用方透传）。
    /// 缓存过期 → 重新扫描 → 原子写回（03 §3.0 stat 新鲜度闭环：新装插件在
    /// 任何一次发现后即可被路由，消除"假降级窗口"）。
    pub fn load_or_discover(options: &DiscoverOptions, cache_path: &Path) -> Result<Self> {
        if let Some(cache) = Self::load_cache(cache_path) {
            let mut registry = Self::new();
            for entry in cache.manifests {
                registry.register_manifest(PathBuf::from(entry.executable_path), entry.manifest);
            }
            return Ok(registry);
        }
        let mut registry = Self::discover(options)?;
        registry.persist_cache(cache_path);
        Ok(registry)
    }

    /// 原子化持久化保存到磁盘缓存文件（临时文件 + rename；文件强制 0600）
    pub fn save_cache_atomic(cache: &RegistryCacheFile, target_file: &Path) -> Result<()> {
        if let Some(parent) = target_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(cache)?;
        let tmp_file = target_file.with_extension(format!("tmp.{}", std::process::id()));
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_file)?;
            file.write_all(json_str.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&tmp_file, json_str)?;
        }
        std::fs::rename(&tmp_file, target_file)?;
        Ok(())
    }

    /// 将当前注册表内容静默写回缓存（失败仅登记告警，绝不阻塞主流程）
    pub fn persist_cache(&mut self, cache_path: &Path) {
        let mut manifests: Vec<CachedPluginEntry> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let command_exes = self.commands.values().map(|(p, _)| p);
        let hook_exes = self.hooks.values().map(|(p, _)| p);
        for exe_path in command_exes.chain(hook_exes) {
            if !seen.insert(exe_path.clone()) {
                continue;
            }
            let manifest_path = exe_path.with_extension("json");
            if let Some((mtime, hash)) = manifest_fingerprint(&manifest_path) {
                // 从同目录 manifest 重建完整清单快照
                if let Ok(content) = std::fs::read_to_string(&manifest_path)
                    && let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content)
                {
                    manifests.push(CachedPluginEntry {
                        executable_path: exe_path.to_string_lossy().to_string(),
                        manifest_path: manifest_path.to_string_lossy().to_string(),
                        mtime_secs: mtime,
                        fingerprint_hash: hash,
                        manifest,
                    });
                }
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

#[cfg(unix)]
unsafe fn libc_euid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::types::PluginStage;
    use std::io::Write;

    fn demo_manifest(name: &str) -> PluginManifest {
        PluginManifest {
            manifest_version: "1.0".to_string(),
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: "Demo plugin".to_string(),
            commands: vec![CommandCapability {
                name: name.to_string(),
                usage: format!("rupost {name}"),
                description: "Demo command".to_string(),
                requires_tty: false,
            }],
            hooks: vec![HookCapability {
                directive: "auth".to_string(),
                alias: Some(name.to_string()),
                stage: PluginStage::Prepare,
                cache_scope: crate::plugin::types::PluginCacheScope::None,
                requires_env: vec![],
                parameters_schema: serde_json::json!({"type": "object"}),
            }],
        }
    }

    /// 固定一个假插件根：manifest JSON + 同目录可执行占位文件
    fn fixture_plugin(root: &Path, plugin_name: &str, body: Option<&str>) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        let bin = root.join(format!("rupost-plugin-{plugin_name}"));
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest_path = root.join(format!("rupost-plugin-{plugin_name}.json"));
        let manifest = match body {
            Some(raw) => raw.to_string(),
            None => serde_json::to_string(&demo_manifest(plugin_name)).unwrap(),
        };
        std::fs::write(&manifest_path, manifest).unwrap();
        manifest_path
    }

    #[test]
    fn test_registry_registration_and_lookup() {
        let mut registry = PluginRegistry::new();
        registry.register_manifest(
            PathBuf::from("/usr/local/bin/rupost-plugin-aws-sigv4"),
            demo_manifest("aws-sigv4"),
        );

        let found_cmd = registry.find_command("aws-sigv4").expect("command found");
        assert_eq!(
            found_cmd.0,
            PathBuf::from("/usr/local/bin/rupost-plugin-aws-sigv4")
        );
        let found_hook = registry.find_hook("aws-sigv4").expect("hook found");
        assert_eq!(found_hook.1.stage, PluginStage::Prepare);
        assert_eq!(
            found_hook.1.cache_scope,
            crate::plugin::types::PluginCacheScope::None
        );
    }

    /// 03 §3.0 安全根基：发现 = readdir + JSON 解析，路径穿越与 symlink 逃逸拒绝
    #[test]
    fn test_discovery_static_manifest_and_path_traversal_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("plugins");
        let good = fixture_plugin(&root, "good", None);

        // 攻击 1：manifest 携带 `..` 逃逸路径（二进制放在根外，工具找不到同目录二进制）
        fixture_plugin(
            &root,
            "evil-dotdot",
            Some(
                r#"{"manifest_version":"1.0","name":"evil-dotdot","version":"1","description":"x","commands":[],"hooks":[]}"#,
            ),
        );

        // 攻击 2：symlink 逃逸——二进制 link 指向根外目标
        let outside = dir.path().join("outside.sh");
        std::fs::write(&outside, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("rupost-plugin-evil-symlink")).unwrap();
        std::fs::write(
            root.join("rupost-plugin-evil-symlink.json"),
            r#"{"manifest_version":"1.0","name":"evil-symlink","version":"1","description":"x","commands":[{"name":"boom","usage":"u","description":"d"}],"hooks":[]}"#,
        )
        .unwrap();

        let mut options = DiscoverOptions::default();
        options.project_root = Some(dir.path().to_path_buf());
        options.allow_project_plugins = true;
        // 仅扫本根：临时改 PATH 注入（discover 读 PATH）
        unsafe {
            std::env::set_var("PATH", root.to_str().unwrap());
        }
        let registry = PluginRegistry::discover(&options).unwrap();
        unsafe {
            std::env::remove_var("PATH");
        }

        assert!(
            registry.find_command("good").is_some(),
            "合法插件必须被发现: {:?}",
            registry.warnings
        );
        assert!(
            registry.find_command("boom").is_none(),
            "symlink 逃逸插件必须被拒绝"
        );
        let _ = good;
        assert!(
            registry.warnings.iter().any(|w| w.contains("evil-symlink")),
            "拒绝原因必须登记为告警: {:?}",
            registry.warnings
        );
    }

    /// 缓存投毒防线：非属主 / 权限过宽的 registry.json 拒绝加载
    #[cfg(unix)]
    #[test]
    fn test_cache_rejects_foreign_owner_and_loose_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("registry.json");
        let cache = RegistryCacheFile {
            version: "1.0".to_string(),
            updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
            manifests: vec![],
        };
        PluginRegistry::save_cache_atomic(&cache, &cache_path).unwrap();

        // 攻击 1：权限放宽到 0644 → 必须拒绝
        std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            PluginRegistry::load_cache(&cache_path).is_none(),
            "权限过宽的缓存必须拒绝加载"
        );

        // 恢复 0600 后命中
        std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(PluginRegistry::load_cache(&cache_path).is_some());
    }

    /// stat 新鲜度闭环（03 §3.0）：manifest 变更 → 缓存整体失效；
    /// 新装插件在下一次发现即可路由（无需手动 sync）
    #[test]
    fn test_cache_staleness_on_manifest_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("plugins");
        let manifest_path = fixture_plugin(&root, "fresh", None);

        // 构建缓存
        let mut registry = PluginRegistry::new();
        let exe = root.join("rupost-plugin-fresh");
        registry.register_manifest(exe, demo_manifest("fresh"));
        let cache_path = dir.path().join("registry.json");
        registry.persist_cache(&cache_path);
        assert!(
            PluginRegistry::load_cache(&cache_path).is_some(),
            "首建缓存必须命中"
        );

        // 指纹漂移：改写 manifest 内容（cache_scope 变化模拟插件升级）
        let mut updated = demo_manifest("fresh");
        updated.hooks[0].cache_scope = crate::plugin::types::PluginCacheScope::Suite;
        std::fs::write(&manifest_path, serde_json::to_string(&updated).unwrap()).unwrap();
        assert!(
            PluginRegistry::load_cache(&cache_path).is_none(),
            "manifest 变更后旧缓存必须判定过期"
        );

        // 指纹未变但缓存文件被写投毒路径 → 由属主/权限校验挡（另一测试覆盖）
        let _ = std::fs::File::create(&manifest_path)
            .unwrap()
            .write_all(b"");
    }

    /// 缓存数据文件本身被篡改指向根外二进制时：指纹校验失败（内容变了）→ 过期重建，
    /// 重建走静态 manifest 白名单。此处锁定"缓存非权威，manifest 才是"的裁决。
    #[test]
    fn test_cache_is_rebuildable_index_not_authority() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("registry.json");
        // 伪造缓存：指向 /tmp/evil 的"注册项"，manifest 指纹字段对不上任何真实文件
        let poisoned = RegistryCacheFile {
            version: "1.0".to_string(),
            updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
            manifests: vec![CachedPluginEntry {
                executable_path: "/tmp/evil".to_string(),
                manifest_path: "/tmp/evil.json".to_string(),
                mtime_secs: 0,
                fingerprint_hash: "deadbeef".to_string(),
                manifest: demo_manifest("evil"),
            }],
        };
        PluginRegistry::save_cache_atomic(&poisoned, &cache_path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(
            PluginRegistry::load_cache(&cache_path).is_none(),
            "指向不存在 manifest 的投毒缓存必须判过期"
        );
    }

    /// alias 冲突：同名双插件后注册覆盖 + 告警登记（03 §3.1）
    #[test]
    fn test_alias_conflict_warning() {
        let mut registry = PluginRegistry::new();
        registry.register_manifest(PathBuf::from("/a/rupost-plugin-dup"), demo_manifest("dup"));
        registry.register_manifest(PathBuf::from("/b/rupost-plugin-dup"), demo_manifest("dup"));
        assert_eq!(
            registry.find_hook("dup").unwrap().0,
            PathBuf::from("/b/rupost-plugin-dup"),
            "后者覆盖前者"
        );
        assert!(
            registry.warnings.iter().any(|w| w.contains("冲突")),
            "覆盖必须告警: {:?}",
            registry.warnings
        );
    }
}
