//! 留存域：长连接日志的限额落盘与过期清理。
//! 决策（log_decision，纯函数）与执行（FileRepository::append）分离。

use crate::tui::ports::FileRepository;
use std::path::Path;

/// 限额决策结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDecision {
    /// 正常追加
    Append,
    /// 本次写入将越限 → 只写截断通告
    AppendTruncated,
    /// 已达上限 → 跳过本次写入
    Skip,
}

/// 限额决策（纯函数）。语义与拆分前 write_log_with_limit 的内联判定逐条一致。
pub fn log_decision(file_len: u64, data_len: usize, max_bytes: usize) -> LogDecision {
    if file_len >= max_bytes as u64 {
        LogDecision::Skip
    } else if file_len + data_len as u64 > max_bytes as u64 {
        LogDecision::AppendTruncated
    } else {
        LogDecision::Append
    }
}

/// 通用限额落盘：决策经纯函数，写经端口。
pub fn write_log_with_limit(
    repo: &dyn FileRepository,
    file_path: &Path,
    data: &str,
    max_bytes: usize,
) -> Result<(), std::io::Error> {
    if let Some(parent) = file_path.parent() {
        if repo.file_len(parent) == 0 {
            let _ = std::fs::create_dir_all(parent); // 目录创建保持原样（登记：属文件系统准备，非数据写）
        }
    }
    match log_decision(repo.file_len(file_path), data.len(), max_bytes) {
        LogDecision::Skip => Ok(()),
        LogDecision::Append => repo.append(file_path, data),
        LogDecision::AppendTruncated => {
            repo.append(file_path, "\n[SYSTEM] Log truncated due to size limit\n")
        }
    }
}

/// 自动清理 7 天前过期的日志目录文件
pub fn cleanup_old_logs_dir(dir_path: &std::path::Path, days: u64) -> Result<(), std::io::Error> {
    crate::history::gc::perform_prune(dir_path, days, 999999).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::ports::InMemoryRepository;

    #[test]
    fn test_log_decision_table() {
        use LogDecision::*;
        let cases = [
            (100u64, 50usize, 200usize, Append), // 未超限追加
            (199, 1, 200, Append),               // 恰好到限
            (200, 1, 200, Skip),                 // 已达上限跳过
            (500, 10, 200, Skip),                // 超限跳过
            (150, 100, 200, AppendTruncated),    // 本次写入将越限 → 截断通告
        ];
        for (fl, dl, max, want) in cases {
            assert_eq!(
                log_decision(fl, dl, max),
                want,
                "file_len={fl} data={dl} max={max}"
            );
        }
    }

    #[test]
    fn test_write_log_via_repository() {
        let repo = InMemoryRepository::default();
        // tempdir 而非绝对路径：InMemory 的 file_len(parent)==0 会触发真实的
        // create_dir_all——用 /w 这类根路径会让 root CI 容器在真实文件系统建目录
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conn.log");

        write_log_with_limit(&repo, &path, "line1\n", 100).unwrap();
        write_log_with_limit(&repo, &path, "line2\n", 100).unwrap();
        assert_eq!(repo.read_file(&path).unwrap(), "line1\nline2\n");

        // 越限（max=60：12B 内容 + 200B 数据 > 60）→ 只写截断通告（43B，总 55B）
        write_log_with_limit(&repo, &path, "x".repeat(200).as_str(), 60).unwrap();
        let content = repo.read_file(&path).unwrap();
        assert!(content.contains("Log truncated"), "{content:?}");

        // 已达上限（max=50 < 55B）→ 跳过，内容不再变化
        let before = content.clone();
        write_log_with_limit(&repo, &path, "y", 50).unwrap();
        assert_eq!(repo.read_file(&path).unwrap(), before);

        // 未达限（max=100 > 55B）→ 继续正常追加
        write_log_with_limit(&repo, &path, "y", 100).unwrap();
        assert_eq!(repo.read_file(&path).unwrap(), format!("{before}y"));
    }
}
