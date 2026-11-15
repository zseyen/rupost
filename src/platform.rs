//! 跨平台差异统一适配层（Home 目录、POSIX 文件权限、进程信号退出码）。
//!
//! 全仓业务与测试代码应通过本模块访问平台相关能力，避免散落 `#[cfg(unix)]`
//! 或在 Windows 下因 `dirs::home_dir()` 忽略 `HOME` 环境变量而导致隔离失效。

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// 解析当前用户主目录：优先读取非空 `HOME` 环境变量，未设置时回退到 `dirs::home_dir()`。
///
/// 统一三端（Linux/macOS/Windows）行为，确保测试与隔离场景通过 `HOME` 覆盖时在 Windows 上同样生效。
pub fn home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    dirs::home_dir()
}

/// 返回用户级配置根目录（`$HOME/.rupost`）。
pub fn rupost_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".rupost"))
}

/// 返回 `$HOME/.rupost/<rel>` 路径。
pub fn rupost_path(rel: impl AsRef<Path>) -> Option<PathBuf> {
    rupost_dir().map(|dir| dir.join(rel))
}

/// 读取路径的 POSIX 权限位（`mode & 0o777`）；非 Unix 平台返回 `Ok(None)`。
pub fn unix_mode(path: &Path) -> std::io::Result<Option<u32>> {
    let meta = std::fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(Some(meta.permissions().mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Ok(None)
    }
}

/// 设置路径的 POSIX 权限位；非 Unix 平台安全忽略并返回 `Ok(())`。
pub fn set_unix_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// 将子进程异常终止（如 Unix 信号）映射为规范退出码（`128 + signal`，非 Unix 回退 `1`）。
pub fn exit_signal_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|sig| 128 + sig).unwrap_or(1)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        1
    }
}

/// 为 `ExitStatus` 提供链式退出码提取扩展，避免调用方手写信号回退闭包。
pub trait ExitStatusExt {
    fn code_or_signal(self) -> i32;
}

impl ExitStatusExt for ExitStatus {
    fn code_or_signal(self) -> i32 {
        self.code().unwrap_or_else(|| exit_signal_code(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_unix_mode_and_set_unix_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("perm.txt");
        std::fs::write(&file, b"ok").unwrap();
        set_unix_mode(&file, 0o600).unwrap();
        if let Some(mode) = unix_mode(&file).unwrap() {
            assert_eq!(mode, 0o600);
        }
        set_unix_mode(&file, 0o755).unwrap();
        if let Some(mode) = unix_mode(&file).unwrap() {
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn test_home_dir_prefers_non_empty_env_home() {
        // 仅在进程内串行测试无污染场景：验证当前 HOME 非空时返回 Some(HOME)
        if let Some(home) = std::env::var_os("HOME")
            && !home.is_empty()
        {
            let expected = PathBuf::from(&home);
            assert_eq!(home_dir(), Some(expected.clone()));
            assert_eq!(
                rupost_path("ai.toml"),
                Some(expected.join(".rupost").join("ai.toml"))
            );
        }
    }
}
