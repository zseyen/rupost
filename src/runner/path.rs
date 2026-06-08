//! 路径管理工具模块
//!
//! 集中所有路径相关的工具函数，避免在各模块中散落 PathBuf 操作。
//! 职责：
//! - 文件类型判断（是否为受支持的测试文件）
//! - 依赖路径解析（将相对路径 dep 转换为 canonical 绝对路径）
//! - 展示用路径格式化（去除冗余前缀，美化输出）

use std::path::{Path, PathBuf};

// ─── 支持的文件扩展名 ─────────────────────────────────────────────────────────

const SUPPORTED_EXTENSIONS: &[&str] = &["http", "md"];

/// 判断给定路径是否为受支持的测试文件（.http / .md）。
pub fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let ext_lower = ext.to_string_lossy().to_lowercase();
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|&s| s == ext_lower.as_str())
        })
        .unwrap_or(false)
}

// ─── 依赖路径解析 ─────────────────────────────────────────────────────────────

/// 将依赖声明 `dep`（可能是相对路径或绝对路径）解析为 canonical 绝对路径。
///
/// # 参数
/// - `base_file`: 声明该依赖的文件自身的路径（需已 canonical）
/// - `dep`: 依赖声明字符串，例如 `"../auth/login.http"` 或 `/abs/path.http`
///
/// # 返回
/// canonical 后的绝对路径；若 canonicalize 失败则返回拼接后的原始路径（容错）。
pub fn resolve_dep_path(base_file: &Path, dep: &str) -> PathBuf {
    let dep_path = if Path::new(dep).is_absolute() {
        PathBuf::from(dep)
    } else {
        // 相对路径：相对于声明文件的父目录展开
        match base_file.parent() {
            Some(parent) => parent.join(dep),
            None => PathBuf::from(dep),
        }
    };
    dep_path.canonicalize().unwrap_or(dep_path)
}

// ─── 展示用路径格式化 ─────────────────────────────────────────────────────────

/// 将绝对路径格式化为供终端展示的字符串。
///
/// 优先尝试返回相对于当前工作目录的相对路径（更简洁），
/// 若失败则回退为绝对路径字符串。
pub fn display_path(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_supported_file_http() {
        assert!(is_supported_file(Path::new("api.http")));
    }

    #[test]
    fn test_is_supported_file_md() {
        assert!(is_supported_file(Path::new("readme.md")));
    }

    #[test]
    fn test_is_supported_file_case_insensitive() {
        assert!(is_supported_file(Path::new("api.HTTP")));
        assert!(is_supported_file(Path::new("api.MD")));
    }

    #[test]
    fn test_unsupported_extension() {
        assert!(!is_supported_file(Path::new("script.rs")));
        assert!(!is_supported_file(Path::new("no_extension")));
    }

    #[test]
    fn test_resolve_dep_relative() {
        // 使用 /tmp 作为 base，验证相对路径拼接逻辑
        let base = Path::new("/tmp/tests/auth.http");
        let resolved = resolve_dep_path(base, "login.http");
        // canonicalize 在 /tmp/tests/login.http 不存在时会回退到拼接路径
        assert!(resolved.ends_with("login.http"));
    }

    #[test]
    fn test_resolve_dep_absolute() {
        let base = Path::new("/tmp/tests/auth.http");
        let resolved = resolve_dep_path(base, "/abs/path.http");
        // 绝对路径应保持原样（canonicalize 失败时 fallback）
        assert!(resolved.to_string_lossy().contains("abs"));
    }

    #[test]
    fn test_display_path_non_empty() {
        let path = Path::new("/usr/local/bin");
        let displayed = display_path(path);
        assert!(!displayed.is_empty());
    }
}
