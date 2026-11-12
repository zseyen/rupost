//! 旧版重放快照套件 (`SnapshotSuite`) 读写、双格式兼容与历史导出。

use crate::Result;
use crate::error::RupostError;
use crate::history::model::{ExecutionSnapshotFile, SnapshotEntry, SnapshotSuite};
use crate::history::selector::{SelectionStrategy, select_entries};
use crate::history::snapshot_mask::mask_snapshot_entry;
use crate::history::storage::get_storage;
use std::fs;
use std::path::Path;

/// 将 SnapshotSuite 写入到指定路径（Unix 下权限强制 0600，原子写入且不跟随符号链接）。
pub fn write_snapshot_suite<P: AsRef<Path>>(suite: &SnapshotSuite, path: P) -> Result<()> {
    let json = serde_json::to_string_pretty(suite)?;
    crate::security::fs::write_private_file_atomic(path.as_ref(), json.as_bytes())
}

/// 从指定路径读取 SnapshotSuite
pub fn read_snapshot_suite<P: AsRef<Path>>(path: P) -> Result<SnapshotSuite> {
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    let suite = serde_json::from_str(&content)?;
    Ok(suite)
}

/// B-5：replay 快照体积上限（02 §3，与 `ai triage` 64MB 对齐）
pub const MAX_REPLAY_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// 双格式兼容读取（`replay` 消费入口）：
/// 优先按黑匣子契约 ExecutionSnapshotFile (Schema v1.0, `--save-snapshot` 产物)
/// 解析并转换为重放套件；失败回退旧 SnapshotSuite（`history export` 产物）。
pub fn read_snapshot_suite_compat<P: AsRef<Path>>(path: P) -> Result<SnapshotSuite> {
    // B-5（02 §3）：离线快照视为不可信输入——消费前体积上限（与 triage 64MB 对齐）
    let path = path.as_ref();
    let meta = fs::metadata(path).map_err(RupostError::IoError)?;
    if meta.len() > MAX_REPLAY_SNAPSHOT_BYTES {
        return Err(RupostError::Other(format!(
            "快照文件过大（{} bytes，上限 64MB）：拒绝消费不可信超大输入",
            meta.len()
        )));
    }
    let content = fs::read_to_string(path).map_err(RupostError::IoError)?;
    if let Ok(snapshot) = serde_json::from_str::<ExecutionSnapshotFile>(&content) {
        // B-5：schema 主版本门（"1.x 均可消费"；2.0+ 显式拒绝防静默错读）
        let major = snapshot.schema_version.split('.').next().unwrap_or("");
        if major != "1" {
            return Err(RupostError::Other(format!(
                "快照 schema_version '{}' 不兼容（当前支持 1.x）：拒绝静默错读未来格式",
                snapshot.schema_version
            )));
        }
        return Ok(snapshot.to_replay_suite());
    }
    let suite: SnapshotSuite = serde_json::from_str(&content).map_err(|e| {
        RupostError::Other(format!("快照既非 Schema v1.0 黑匣子也非历史导出格式: {e}"))
    })?;
    if suite.version > 2 {
        return Err(RupostError::Other(format!(
            "快照 version {} 超出支持范围（≤2）：拒绝静默错读未来格式",
            suite.version
        )));
    }
    Ok(suite)
}

/// 将最近 N 条历史条目导出为可重放快照套件文件。
pub fn export_history_snapshot(last: usize, output: &str, mask_secrets: bool) -> Result<()> {
    let storage = get_storage();
    let entries = select_entries(storage, SelectionStrategy::Last(last))?;
    if entries.is_empty() {
        tracing::warn!("No history entries to export.");
        return Ok(());
    }

    let mut suite = SnapshotSuite {
        version: 2,
        timestamp: chrono::Utc::now(),
        source_file: Some("history".to_string()),
        entries: entries.into_iter().map(SnapshotEntry::from).collect(),
        masking: if mask_secrets {
            Some("secrets".to_string())
        } else {
            None
        },
    };
    if mask_secrets {
        for e in suite.entries.iter_mut() {
            mask_snapshot_entry(e);
        }
    }
    let count = suite.entries.len();
    write_snapshot_suite(&suite, output)?;
    println!("Exported snapshot: {} ({} requests)", output, count);
    Ok(())
}
