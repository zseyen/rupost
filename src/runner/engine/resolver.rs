use crate::Result;
use crate::error::RupostError;
use crate::parser::{HttpFileParser, MarkdownFileParser, ParsedFile};
use crate::runner::path::{display_path, is_supported_file, resolve_dep_path};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 递归依赖解析器
pub struct DependencyResolver;

impl DependencyResolver {
    /// 从用户显式提供的输入路径推导依赖解析的允许根：目录取自身，文件取其父目录。
    /// 采用多根 allowlist 而非最小公共祖先——不相交路径（/a/x.http 与 /b/y.http）
    /// 的 LCA 会退化为 `/`，校验形同虚设。
    pub fn sandbox_roots_from_inputs<P: AsRef<Path>>(inputs: &[P]) -> Vec<PathBuf> {
        inputs
            .iter()
            .filter_map(|p| {
                let p = p.as_ref();
                let root = if p.is_dir() {
                    p.to_path_buf()
                } else {
                    p.parent()?.to_path_buf()
                };
                let root = if root.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    root
                };
                root.canonicalize().ok()
            })
            .collect()
    }

    /// 递归解析所有物理依赖文件。
    ///
    /// 用户显式传入的 `initial_paths` 完全信任（此前以 cwd 为根校验，导致
    /// `cd /tmp && rupost t /abs/path.http` 被误拒）；`@depends-on` 递归解析出的
    /// 依赖必须落在 `sandbox_roots` 任一根之下。这不是安全边界（对网络零限制），
    /// 而是依赖路径写错/越界时的快速失败校验。
    pub fn resolve_and_parse(
        initial_paths: &[PathBuf],
        sandbox_roots: &[PathBuf],
    ) -> Result<HashMap<PathBuf, ParsedFile>> {
        let mut files_map = HashMap::new();
        let mut visited = HashSet::new();
        let mut pending = Vec::new();

        for path in initial_paths {
            let canonical_path = path.canonicalize()?;
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

                // 依赖路径合理性校验
                let canonical_dep_file = canonical_dep.canonicalize()?;
                Self::validate_sandbox(&canonical_dep_file, sandbox_roots)?;
                Self::validate_extension(&canonical_dep_file)?;

                if !visited.contains(&canonical_dep_file) {
                    pending.push(canonical_dep_file);
                }
            }

            files_map.insert(path, parsed);
        }

        Ok(files_map)
    }

    /// 校验依赖落在任一允许根之下，防止 `@depends-on ../../..` 式越界引用
    fn validate_sandbox(path: &Path, sandbox_roots: &[PathBuf]) -> Result<()> {
        if !crate::utils::path_guard::is_within_roots(path, sandbox_roots) {
            return Err(RupostError::Other(format!(
                "依赖路径越界：@depends-on 引用的文件不在任何输入路径的目录之下，拒绝加载！文件: {}",
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

    /// 根据扩展名选择解析器解析文件（大小写不敏感，与 `is_supported_file` 的准入判断一致）
    fn parse_file(path: &Path) -> Result<ParsedFile> {
        let is_markdown = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        let parsed = if is_markdown {
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
    fn test_uppercase_md_routes_to_markdown_parser() {
        // 回归：准入检查大小写不敏感但解析器路由区分大小写，
        // CASE.MD 被送进 HTTP 解析器后把 "```http" 当 URL 执行。
        let dir = tempdir().unwrap();
        let md_path = dir.path().join("CASE.MD");
        fs::write(
            &md_path,
            "# Doc\n\n```http\nGET https://api.example.com/upper\n```\n",
        )
        .unwrap();

        let parsed = DependencyResolver::parse_file(&md_path).unwrap();
        assert_eq!(parsed.requests.len(), 1);
        assert_eq!(parsed.requests[0].url, "https://api.example.com/upper");
    }

    #[test]
    fn test_validate_sandbox_ok() {
        let roots = vec![PathBuf::from("/tmp/sandbox")];
        let path = Path::new("/tmp/sandbox/sub/file.http");
        assert!(DependencyResolver::validate_sandbox(path, &roots).is_ok());
    }

    #[test]
    fn test_validate_sandbox_escape() {
        let roots = vec![PathBuf::from("/tmp/sandbox")];
        let path = Path::new("/tmp/other/file.http");
        assert!(DependencyResolver::validate_sandbox(path, &roots).is_err());
    }

    #[test]
    fn test_validate_sandbox_multi_root_does_not_degrade() {
        // 多根 allowlist：不相交的根各自生效，且不会退化为 "/" 放行一切
        let roots = vec![PathBuf::from("/a/tests"), PathBuf::from("/b/tests")];
        assert!(DependencyResolver::validate_sandbox(Path::new("/a/tests/x.http"), &roots).is_ok());
        assert!(DependencyResolver::validate_sandbox(Path::new("/b/tests/y.http"), &roots).is_ok());
        assert!(
            DependencyResolver::validate_sandbox(Path::new("/etc/passwd.http"), &roots).is_err()
        );
        // 前缀必须按路径分量匹配：/a/tests-evil 不在 /a/tests 之下
        assert!(
            DependencyResolver::validate_sandbox(Path::new("/a/tests-evil/z.http"), &roots)
                .is_err()
        );
    }

    #[test]
    fn test_explicit_file_outside_cwd_is_trusted() {
        // 回归：此前以 current_dir() 为沙箱根，`cd /tmp && rupost t /abs/file.http`
        // 被当成"安全违规"拒绝。显式传入的文件必须信任。
        let dir = tempdir().unwrap();
        let file = dir.path().join("remote.http");
        fs::write(&file, "GET https://api.example.com/remote").unwrap();

        // 根从输入推导（文件 → 父目录），与 cwd 无关
        let roots = DependencyResolver::sandbox_roots_from_inputs(std::slice::from_ref(&file));
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], dir.path().canonicalize().unwrap());

        let resolved =
            DependencyResolver::resolve_and_parse(std::slice::from_ref(&file), &roots).unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(resolved.contains_key(&file.canonicalize().unwrap()));
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
        let resolved =
            DependencyResolver::resolve_and_parse(&initial, std::slice::from_ref(&sandbox_path))
                .unwrap();

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
        let roots = vec![dir_sandbox.path().canonicalize().unwrap()];
        let result = DependencyResolver::resolve_and_parse(&initial, &roots);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("依赖路径越界"));
    }
}
