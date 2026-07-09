use rand::Rng;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::info;

pub struct LogGc;

impl LogGc {
    /// 懒清理触发器：1% 概率在后台线程中执行异步非阻塞清理
    pub fn try_trigger_lazy_gc(log_dir: &Path) {
        let log_dir_buf = log_dir.to_path_buf();
        // 1% 概率命中
        if rand::rng().random_ratio(1, 100) {
            info!("Log GC: Probabilistic GC trigger hit! Launching background cleanup task.");
            tokio::task::spawn_blocking(move || {
                // 默认 7 天过期，100MB 上限
                let _ = perform_prune(&log_dir_buf, 7, 100);
            });
        }
    }
}

/// 手动/自动修剪算法：删除指定天数前的日志文件，并对超限容量执行基于 mtime 的 LRU 截断
pub fn perform_prune(
    log_dir: &Path,
    days: u64,
    max_size_mb: usize,
) -> Result<(usize, u64), std::io::Error> {
    if !log_dir.exists() {
        return Ok((0, 0));
    }

    let limit_duration = Duration::from_secs(days * 24 * 3600);
    let now = SystemTime::now();
    let max_bytes = max_size_mb * 1024 * 1024;

    let mut deleted_count = 0;
    let mut deleted_bytes = 0;

    let read_dir = std::fs::read_dir(log_dir)?;
    let mut remaining_files = Vec::new();
    let mut current_total_bytes = 0;

    // 阶段一：基于时间的清理
    for entry in read_dir.flatten() {
        let path = entry.path();
        let metadata = match path.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let size = metadata.len();
        let is_expired = if let Ok(modified) = metadata.modified() {
            if let Ok(age) = now.duration_since(modified) {
                age > limit_duration
            } else {
                false
            }
        } else {
            false
        };

        if is_expired {
            if std::fs::remove_file(&path).is_ok() {
                deleted_count += 1;
                deleted_bytes += size;
            }
        } else {
            let modified_time = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            remaining_files.push((path, modified_time, size));
            current_total_bytes += size;
        }
    }

    // 阶段二：基于容量 (LRU) 的清理 (超限 100MB 裁剪到 80%)
    if current_total_bytes > max_bytes as u64 {
        let target_bytes = (max_bytes as f64 * 0.8) as u64;
        // 按修改时间从旧到新（小到大）排序
        remaining_files.sort_by_key(|x| x.1);

        for (path, _, size) in remaining_files {
            if current_total_bytes <= target_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                deleted_count += 1;
                deleted_bytes += size;
                current_total_bytes = current_total_bytes.saturating_sub(size);
            }
        }
    }

    if deleted_count > 0 {
        info!(
            "Log GC Prune complete: Removed {} files, freed {} bytes.",
            deleted_count, deleted_bytes
        );
    }

    Ok((deleted_count, deleted_bytes))
}

/// 物理清空日志文件夹
pub fn perform_clear(log_dir: &Path) -> Result<(usize, u64), std::io::Error> {
    if !log_dir.exists() {
        return Ok((0, 0));
    }

    let mut deleted_count = 0;
    let mut deleted_bytes = 0;

    let read_dir = std::fs::read_dir(log_dir)?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        let metadata = match path.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let size = metadata.len();
        if std::fs::remove_file(&path).is_ok() {
            deleted_count += 1;
            deleted_bytes += size;
        }
    }

    Ok((deleted_count, deleted_bytes))
}
