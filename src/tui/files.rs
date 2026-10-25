//! 快速新建用例的领域逻辑：输入推导（纯函数）+ 工作区围栏内创建（经端口落盘）。

use super::ports::FileRepository;
use crate::utils::path_guard::is_within_roots;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BASE: usize = 80;

pub struct DerivedTest {
    pub filename: String,
    pub content: String,
}

/// 输入 → (文件名, 模板内容)。URL 判定语义与 CLI 保持一致（行为锁定，commit 3 统一为共用函数）。
pub fn derive_test_file(input: &str) -> DerivedTest {
    let is_url = input.starts_with("http://")
        || input.starts_with("https://")
        || (input.contains('.')
            && !input.ends_with(".http")
            && !input.ends_with(".md")
            && !input.contains(' '));

    if is_url {
        let normalized_url = if !input.starts_with("http://") && !input.starts_with("https://") {
            format!("https://{input}")
        } else {
            input.to_string()
        };

        let file_base = if let Ok(parsed) = url::Url::parse(&normalized_url) {
            let host = parsed.host_str().unwrap_or("quick_test");
            let path = parsed.path().trim_matches('/');
            let mut combined = host.to_string();
            if !path.is_empty() {
                combined.push('_');
                combined.push_str(path);
            }
            sanitize(&combined)
        } else {
            "quick_test".to_string()
        };

        DerivedTest {
            filename: format!("{file_base}.http"),
            content: format!("GET {normalized_url}\n"),
        }
    } else {
        let mut file_base = input.to_string();
        if !file_base.ends_with(".http") && !file_base.ends_with(".md") {
            file_base.push_str(".http");
        }
        DerivedTest {
            filename: file_base,
            // 债务：默认模板硬编码外网地址，离线即失败（设计文档债务登记）
            content: "GET http://httpbingo.org/get\n".to_string(),
        }
    }
}

/// URL 片段 → 合法文件名：非字母数字转 '_'；超过 80 字符截断并追加 8 位短哈希，
/// 保证总长远离各文件系统 255 字节上限。
fn sanitize(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if mapped.chars().count() <= MAX_FILE_BASE {
        return mapped;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut hasher = DefaultHasher::new();
    hasher.write(raw.as_bytes());
    let truncated: String = mapped.chars().take(MAX_FILE_BASE).collect();
    format!("{truncated}_{:08x}", hasher.finish() as u32)
}

/// 拒绝携带路径语义的文件名：`..`/`.` 段、绝对路径、Windows 盘符、反斜杠。
/// 防止快速输入把文件写到工作区之外（与 cookie 拒绝路径穿越 --env 同一防线思路）。
fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.contains('\0')
        && !Path::new(name).components().any(|c| {
            matches!(
                c,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
}

#[derive(Debug)]
pub enum CreateOutcome {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

/// 在工作区围栏内原子创建测试文件：
/// 1. 文件名不得携带路径语义；2. 规范化父目录后必须仍在工作区内；
/// 3. create_exclusive 原子创建，已存在不覆盖，其余错误原样浮出。
pub fn create_test_in_workspace(
    repo: &dyn FileRepository,
    dir: &Path,
    workspace_root: &Path,
    derived: &DerivedTest,
) -> Result<CreateOutcome, String> {
    if !is_safe_filename(&derived.filename) {
        return Err(format!("invalid file name: {:?}", derived.filename));
    }
    let canon_root = repo
        .canonicalize(workspace_root)
        .map_err(|e| format!("resolve workspace failed: {e}"))?;
    let canon_dir = repo
        .canonicalize(dir)
        .map_err(|e| format!("resolve target dir failed: {e}"))?;
    let target = canon_dir.join(&derived.filename);
    if !is_within_roots(&target, &[canon_root]) {
        return Err(format!("path escapes workspace: {}", derived.filename));
    }
    match repo.create_exclusive(&target, &derived.content) {
        Ok(()) => Ok(CreateOutcome::Created(target)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(CreateOutcome::AlreadyExists(target))
        }
        Err(e) => Err(format!("create {} failed: {}", derived.filename, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_url_to_filename() {
        let d = derive_test_file("google.com");
        assert_eq!(d.filename, "google_com.http");
        assert_eq!(d.content, "GET https://google.com\n");

        let d = derive_test_file("api.github.com/users");
        assert_eq!(d.filename, "api_github_com_users.http");
        assert_eq!(d.content, "GET https://api.github.com/users\n");

        let d = derive_test_file("http://example.com");
        assert_eq!(d.filename, "example_com.http");
        assert_eq!(d.content, "GET http://example.com\n");
    }

    #[test]
    fn test_derive_plain_filename() {
        let d = derive_test_file("auth");
        assert_eq!(d.filename, "auth.http");
        let d = derive_test_file("tests/auth.http");
        assert_eq!(d.filename, "tests/auth.http");
        let d = derive_test_file("my notes");
        assert_eq!(d.filename, "my notes.http");
    }

    #[test]
    fn test_sanitize_truncates_and_is_deterministic() {
        let long_url = format!("https://example.com/{}", "a".repeat(300));
        let d = derive_test_file(&long_url);
        let base = d.filename.strip_suffix(".http").unwrap();
        assert!(base.chars().count() <= MAX_FILE_BASE + 1 + 8, "{base}");

        // 相同输入确定性；不同输入哈希后缀不同
        let again = derive_test_file(&long_url);
        assert_eq!(d.filename, again.filename);
        let other = derive_test_file(&format!("https://example.com/{}", "b".repeat(300)));
        assert_ne!(d.filename, other.filename);
    }

    #[test]
    fn test_is_safe_filename_rejects_path_semantics() {
        for bad in [
            "../escape.http",
            "a/../../b.http",
            "/abs.http",
            "..\\escape.http",
            "",
        ] {
            assert!(!is_safe_filename(bad), "must reject {bad:?}");
        }
        for good in [
            "x.http",
            "tests/auth.http",
            "my notes.http",
            "google_com.http",
        ] {
            assert!(is_safe_filename(good), "must accept {good:?}");
        }
    }

    #[test]
    fn test_create_in_workspace_outcomes() {
        let repo = super::super::ports::InMemoryRepository::default();
        let root = Path::new("/w");
        let derived = derive_test_file("google.com");

        let created = create_test_in_workspace(&repo, root, root, &derived).unwrap();
        assert!(
            matches!(created, CreateOutcome::Created(p) if p == Path::new("/w/google_com.http"))
        );

        let exists = create_test_in_workspace(&repo, root, root, &derived).unwrap();
        assert!(matches!(exists, CreateOutcome::AlreadyExists(_)));

        // 文件名携带路径语义 → 拒绝
        let evil = DerivedTest {
            filename: "../escape.http".into(),
            content: String::new(),
        };
        let err = create_test_in_workspace(&repo, root, root, &evil).unwrap_err();
        assert!(err.contains("invalid file name"));

        // 词法围栏：dir 不在 root 之下 → 拒绝（InMemory 的 canonicalize 为身份映射，
        // 不存在目录的探测由 FsRepository 集成测试与只读目录用例覆盖）
        let err =
            create_test_in_workspace(&repo, Path::new("/elsewhere"), root, &derived).unwrap_err();
        assert!(err.contains("path escapes workspace"));
    }
}
