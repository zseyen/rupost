//! 插件注册表磁盘缓存（~/.rupost/registry.json）的指纹校验与原子写回（03 §3.2 / §10.3）。

use super::types::PluginManifest;
use crate::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

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

/// 全量指纹：纳秒级 mtime + 完整 manifest 文件 SHA-256 哈希（03 §10.3 防幽灵缓存命中）
pub fn manifest_fingerprint(path: &Path) -> Option<(u64, String)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let mtime_secs = modified.as_secs();
    let mtime_nanos = modified.subsec_nanos();

    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Some((mtime_secs, format!("{mtime_nanos:09}:{hex}")))
}

/// 加载缓存（快路径）。命中条件：非 symlink / 属主 / 权限可信 + 纳秒级 mtime + 全量 SHA-256 指纹全部匹配。
pub fn load_cache(cache_path: &Path) -> Option<RegistryCacheFile> {
    if !crate::security::fs::is_owner_only_regular_file(cache_path) {
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

/// 原子化持久化保存到磁盘缓存文件（0700 私有目录 + 唯一临时文件 + 强制 0600 + rename）
pub fn save_cache_atomic(cache: &RegistryCacheFile, target_file: &Path) -> Result<()> {
    if let Some(parent) = target_file.parent()
        && !parent.as_os_str().is_empty()
    {
        crate::security::fs::ensure_private_dir(parent)?;
    }
    let json_str = serde_json::to_string_pretty(cache)?;
    crate::security::fs::write_private_file_atomic(target_file, json_str.as_bytes())
}
