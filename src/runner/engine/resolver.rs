use crate::Result;
use crate::error::RupostError;
use crate::parser::{HttpFileParser, MarkdownFileParser, ParsedFile};
use crate::runner::path::{display_path, is_supported_file, resolve_dep_path};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 递归依赖解析器
pub struct DependencyResolver;

impl DependencyResolver {
    /// 递归解析所有物理依赖文件，并实施安全沙箱检查
    pub fn resolve_and_parse(
        initial_paths: &[PathBuf],
        sandbox_root: &Path,
    ) -> Result<HashMap<PathBuf, ParsedFile>> {
        let mut files_map = HashMap::new();
        let mut visited = HashSet::new();
        let mut pending = Vec::new();

        // 规范化 sandbox_root
        let canonical_sandbox = sandbox_root
            .canonicalize()
            .unwrap_or_else(|_| sandbox_root.to_path_buf());

        for path in initial_paths {
            let canonical_path = path.canonicalize()?;
            Self::validate_sandbox(&canonical_path, &canonical_sandbox)?;
            Self::validate_extension(&canonical_path)?;
            pending.push(canonical_path);
        }

        while let Some(path) = pending.pop() {
            if visited.contains(&path) {
                continue;
            }
            visited.insert(path.clone());

            // 1. 解析文件
            let parsed = Self::parse_file(&path)?;

            // 2. 遍历依赖，并验证它们
            for dep in &parsed.dependencies {
                let canonical_dep = resolve_dep_path(&path, dep);

                // 验证依赖物理存在，若不存在，报错
                if !canonical_dep.exists() {
                    return Err(RupostError::DependencyNotFound {
                        file: display_path(&path),
                        missing_dep: display_path(&canonical_dep),
                    });
                }

                // 安全沙箱校验
                let canonical_dep_file = canonical_dep.canonicalize()?;
                Self::validate_sandbox(&canonical_dep_file, &canonical_sandbox)?;
                Self::validate_extension(&canonical_dep_file)?;

                if !visited.contains(&canonical_dep_file) {
                    pending.push(canonical_dep_file);
                }
            }

            files_map.insert(path, parsed);
        }

        Ok(files_map)
    }

    /// 校验沙箱路径范围，防止路径穿越 (Path Traversal)
    fn validate_sandbox(path: &Path, sandbox_root: &Path) -> Result<()> {
        if !path.starts_with(sandbox_root) {
            return Err(RupostError::Other(format!(
                "安全违规：依赖文件不在沙箱目录内，拒绝加载！文件: {}",
                display_path(path)
            )));
        }
        Ok(())
    }

    /// 校验受支持的后缀名
    fn validate_extension(path: &Path) -> Result<()> {
        if !is_supported_file(path) {
            return Err(RupostError::Other(format!(
                "不受支持的测试文件类型，拒绝加载！文件: {}",
                display_path(path)
            )));
        }
        Ok(())
    }

    /// 根据扩展名选择解析器解析文件
    fn parse_file(path: &Path) -> Result<ParsedFile> {
        let parsed = if path.extension().and_then(|s| s.to_str()) == Some("md") {
            MarkdownFileParser::parse_file(path)?
        } else {
            HttpFileParser::parse_file(path)?
        };
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_sandbox_ok() {
        let root = Path::new("/tmp/sandbox");
        let path = Path::new("/tmp/sandbox/sub/file.http");
        assert!(DependencyResolver::validate_sandbox(path, root).is_ok());
    }

    #[test]
    fn test_validate_sandbox_escape() {
        let root = Path::new("/tmp/sandbox");
        let path = Path::new("/tmp/other/file.http");
        assert!(DependencyResolver::validate_sandbox(path, root).is_err());
    }

    #[test]
    fn test_validate_extension_ok() {
        assert!(DependencyResolver::validate_extension(Path::new("test.http")).is_ok());
        assert!(DependencyResolver::validate_extension(Path::new("test.md")).is_ok());
    }

    #[test]
    fn test_validate_extension_err() {
        assert!(DependencyResolver::validate_extension(Path::new("test.json")).is_err());
        assert!(DependencyResolver::validate_extension(Path::new("test.rs")).is_err());
    }

    #[test]
    fn test_recursive_dependency_and_sandbox() {
        let dir = tempdir().unwrap();
        let sandbox_path = dir.path().to_path_buf().canonicalize().unwrap();

        let a_path = sandbox_path.join("a.http");
        let b_path = sandbox_path.join("b.http");

        // 创建 a.http 并依赖 b.http
        fs::write(
            &a_path,
            "### @depends-on b.http\nGET https://api.example.com/a",
        )
        .unwrap();
        // 创建 b.http
        fs::write(&b_path, "GET https://api.example.com/b").unwrap();

        let a_canonical = a_path.canonicalize().unwrap();
        let b_canonical = b_path.canonicalize().unwrap();

        let initial = vec![a_path];
        let resolved = DependencyResolver::resolve_and_parse(&initial, &sandbox_path).unwrap();

        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains_key(&a_canonical));
        assert!(resolved.contains_key(&b_canonical));
    }

    #[test]
    fn test_sandbox_escape_dependency_fails() {
        let dir_sandbox = tempdir().unwrap();
        let dir_external = tempdir().unwrap();

        let a_path = dir_sandbox.path().join("a.http");
        let external_path = dir_external.path().join("external.http");

        // a.http 试图依赖外部的 external.http
        let dep_str = external_path.to_string_lossy();
        fs::write(
            &a_path,
            format!("### @depends-on {}\nGET https://api.example.com/a", dep_str),
        )
        .unwrap();
        fs::write(&external_path, "GET https://api.example.com/external").unwrap();

        let initial = vec![a_path];
        let result = DependencyResolver::resolve_and_parse(&initial, dir_sandbox.path());
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("安全违规"));
    }
}
