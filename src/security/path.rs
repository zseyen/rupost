//! 统一路径安全围栏组件 (Path Sandbox Infrastructure)
//!
//! 提供基于物理沙箱根目录的强安全护栏：
//! 1. 严格锁定在根目录范围内（杜绝 `..` 穿越、绝对路径逃逸、Windows 反斜杠注入）；
//! 2. 隐藏文件与目录准入控制（默认拦截以 `.` 开头的组件，如 `.env`, `.git` 等）；
//! 3. 扩展名白名单安全门禁（大小写不敏感校验）；
//! 4. 符号链接穿透防御（已存在文件全路径规范化，未存在文件向上递归追溯已存在祖先目录）。

use crate::Result;
use crate::error::RupostError;
use std::path::{Component, Path, PathBuf};

/// 路径安全沙箱
#[derive(Debug, Clone)]
pub struct PathSandbox {
    root: PathBuf,
    allowed_extensions: Option<Vec<String>>,
    disallow_hidden: bool,
}

impl PathSandbox {
    /// 以指定目录为物理安全根创建沙箱
    pub fn new(root: impl AsRef<Path>) -> Self {
        let abs_root =
            std::fs::canonicalize(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
        Self {
            root: abs_root,
            allowed_extensions: None,
            disallow_hidden: true,
        }
    }

    /// 设置允许的文件扩展名白名单（大小写不敏感，如 `["md", "http"]`）
    pub fn with_extensions(mut self, exts: &[&str]) -> Self {
        self.allowed_extensions = Some(exts.iter().map(|s| s.to_lowercase()).collect());
        self
    }

    /// 设置是否允许访问隐藏文件（默认拒绝任何以 `.` 开头的组件）
    pub fn allow_hidden(mut self, allow: bool) -> Self {
        self.disallow_hidden = !allow;
        self
    }

    /// 获取规范化后的沙箱物理根目录
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 解析并校验安全路径（彻底阻断各类目录越界与非法准入）
    pub fn resolve_safe_path(&self, requested: &str) -> Result<PathBuf> {
        let clean = requested.trim_start_matches('/');

        // 1. 拒绝空输入、反斜杠与空字节
        if clean.is_empty() || clean.contains('\\') || clean.contains('\0') {
            return Err(RupostError::Other(format!(
                "Invalid path format: {:?}",
                requested
            )));
        }

        let path = Path::new(clean);

        // 2. 隐藏目录 / 隐藏文件拦截
        if self.disallow_hidden {
            for comp in path.components() {
                if let Component::Normal(c) = comp {
                    let s = c.to_string_lossy();
                    if s.starts_with('.') {
                        return Err(RupostError::Other(format!(
                            "Access to hidden file or directory is denied: {}",
                            requested
                        )));
                    }
                }
            }
        }

        // 3. 扩展名白名单安全校验
        if let Some(ref allowed) = self.allowed_extensions {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            if !allowed.contains(&ext) {
                return Err(RupostError::Other(format!(
                    "Invalid extension '{}' for {}. Permitted extensions: {:?}",
                    ext, requested, allowed
                )));
            }
        }

        let candidate = self.root.join(clean);

        // 4. 符号链接与目录穿越深层校验
        // 存在性判定使用 symlink_metadata 而非 exists()：悬空软链接（指向沙箱外不存在目标）
        // 的 exists() 为 false，会被误判为"新建文件"而放行，写入时跟随链接造成逃逸外写
        if candidate.symlink_metadata().is_ok() {
            let canon = match candidate.canonicalize() {
                Ok(c) => c,
                // 悬空软链接（目标不存在）或存在性竞态：规范化失败一律拒绝
                Err(_) => {
                    return Err(RupostError::Other(format!(
                        "Dangling symlink escape rejected: {}",
                        requested
                    )));
                }
            };
            if canon.starts_with(&self.root) {
                Ok(canon)
            } else {
                Err(RupostError::Other(format!(
                    "Path traversal attempt rejected: {}",
                    requested
                )))
            }
        } else {
            // 对尚不存在的新建文件：向上追溯真实存在的最近祖先目录并规范化，防止软链接穿透
            let mut ancestor = candidate.as_path();
            while !ancestor.exists() {
                match ancestor.parent() {
                    Some(p) => ancestor = p,
                    None => break,
                }
            }

            let canon_ancestor = ancestor.canonicalize().map_err(RupostError::IoError)?;
            if !canon_ancestor.starts_with(&self.root) {
                return Err(RupostError::Other(format!(
                    "Symlink directory traversal rejected: {}",
                    requested
                )));
            }

            // 检查相对路径组件中是否包含 ParentDir (..) 或 RootDir
            for component in path.components() {
                if matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                ) {
                    return Err(RupostError::Other(format!(
                        "Path traversal attempt rejected: {}",
                        requested
                    )));
                }
            }

            Ok(candidate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_path_sandbox_basic_and_traversal() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let sandbox = PathSandbox::new(root)
            .with_extensions(&["md", "http"])
            .allow_hidden(false);

        // 1. 合法正常文件
        let safe = sandbox.resolve_safe_path("docs/api.md").unwrap();
        assert!(safe.starts_with(sandbox.root()));

        // 2. 大小写扩展名不敏感
        let safe_upper = sandbox.resolve_safe_path("docs/SPEC.HTTP").unwrap();
        assert!(safe_upper.starts_with(sandbox.root()));

        // 3. 拒绝跨越扩展名
        assert!(sandbox.resolve_safe_path("script.sh").is_err());
        assert!(sandbox.resolve_safe_path("Cargo.toml").is_err());

        // 4. 拒绝目录穿越
        assert!(sandbox.resolve_safe_path("../escape.md").is_err());
        assert!(sandbox.resolve_safe_path("sub/../../escape.md").is_err());
        assert!(sandbox.resolve_safe_path("..\\escape.md").is_err());

        // 5. 拒绝隐藏文件与隐藏目录
        assert!(sandbox.resolve_safe_path(".env.http").is_err());
        assert!(sandbox.resolve_safe_path(".git/hooks/spec.md").is_err());
        assert!(sandbox.resolve_safe_path("sub/.hidden/test.http").is_err());
    }

    #[test]
    fn test_path_sandbox_existing_file_symlink() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let real_file = root.join("valid.md");
        std::fs::write(&real_file, "content").unwrap();

        let sandbox = PathSandbox::new(root).with_extensions(&["md"]);
        let safe = sandbox.resolve_safe_path("valid.md").unwrap();
        assert_eq!(safe, real_file.canonicalize().unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn test_path_sandbox_dangling_symlink_escape_blocked() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let outside = tempdir().unwrap();

        // 悬空软链接：指向沙箱外"尚不存在"的目标文件
        // 旧实现 exists() 判 false → 走"新建文件"分支放行 → 写入时跟随链接逃逸到沙箱外
        let link = root.join("link.md");
        std::os::unix::fs::symlink(outside.path().join("evil.md"), &link).unwrap();

        let sandbox = PathSandbox::new(root).with_extensions(&["md"]);
        let resolved = sandbox.resolve_safe_path("link.md");
        assert!(
            resolved.is_err(),
            "dangling symlink pointing outside sandbox must be rejected, got {:?}",
            resolved
        );

        // 沙箱外目标目录真实存在时（经典软链接穿越）依旧拒绝
        let real_outside_file = outside.path().join("target.md");
        std::fs::write(&real_outside_file, "secret").unwrap();
        let link2 = root.join("link2.md");
        std::os::unix::fs::symlink(&real_outside_file, &link2).unwrap();
        assert!(sandbox.resolve_safe_path("link2.md").is_err());
    }
}
