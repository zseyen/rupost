//! 留存域：长连接日志的限额落盘与过期清理。
//! 注：此处保留直接 std::fs 写（设计文档第四轮审查登记的预存债务），
//! 待端口化需求触发时一并接入 FileRepository。

/// 通用限额落盘函数：如果日志大小超出限制，则停止追加并写入警告
pub fn write_log_with_limit(
    file_path: &std::path::Path,
    data: &str,
    max_bytes: usize,
) -> Result<(), std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let file_len = file_path.metadata().map(|m| m.len() as usize).unwrap_or(0);
    if file_len >= max_bytes {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    let new_len = file_len + data.len();
    if new_len > max_bytes {
        file.write_all(b"\n[SYSTEM] Log truncated due to size limit\n")?;
    } else {
        file.write_all(data.as_bytes())?;
    }
    file.flush()?;
    Ok(())
}

/// 自动清理 7 天前过期的日志目录文件
pub fn cleanup_old_logs_dir(dir_path: &std::path::Path, days: u64) -> Result<(), std::io::Error> {
    crate::history::gc::perform_prune(dir_path, days, 999999).map(|_| ())
}
