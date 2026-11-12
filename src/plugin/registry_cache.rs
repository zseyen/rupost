//! 插件注册表磁盘缓存（~/.rupost/registry.json）的指纹校验与原子写回（03 §3.2 / §10.3）。

use super::types::PluginManifest;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static CACHE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

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

/// 缓存有效性校验：拒绝 symlink 投毒 + 校验属主 uid == euid + 权限 ≤ 0600（多用户机器防投毒）
pub fn cache_file_trustworthy(cache_path: &Path) -> bool {
    let Ok(sym_meta) = std::fs::symlink_metadata(cache_path) else {
        return false;
    };
    if sym_meta.file_type().is_symlink() || !sym_meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if sym_meta.uid() != unsafe { libc::geteuid() } {
            return false;
        }
        if sym_meta.permissions().mode() & 0o077 != 0 {
            return false;
        }
        true
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 加载缓存（快路径）。命中条件：非 symlink / 属主 / 权限可信 + mtime + 前 4KB 哈希指纹全部匹配。
pub fn load_cache(cache_path: &Path) -> Option<RegistryCacheFile> {
    if !cache_file_trustworthy(cache_path) {
        return None;
    }
    let content = std::fs::read_to_string(cache_path).ok()?;
    let cache: RegistryCacheFile = serde_json::from_str(&content).ok()?;
    for entry in &cache.manifests {
        let manifest_path = Path::new(&entry.manifest_path);
        match manifest_fingerprint(manifest_path) {
            Some((mtime, hash)) if mtime == entry.mtime_secs && hash == entry.fingerprint_hash => {}
            _ => return None,
        }
    }
    Some(cache)
}

/// 原子化持久化保存到磁盘缓存文件（唯一临时文件 + 强制 0600 + rename）
pub fn save_cache_atomic(cache: &RegistryCacheFile, target_file: &Path) -> Result<()> {
    if let Some(parent) = target_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json_str = serde_json::to_string_pretty(cache)?;
    let seq = CACHE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_file = target_file.with_extension(format!("tmp.{}.{}", std::process::id(), seq));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp_file)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(json_str.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp_file, json_str)?;
    }
    std::fs::rename(&tmp_file, target_file)?;
    Ok(())
}
