//! 快速新建用例的领域逻辑：输入推导（纯函数）+ 工作区围栏内创建（经端口落盘）。

use super::ports::FileRepository;
use crate::utils::path_guard::is_within_roots;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BASE: usize = 80;

pub struct DerivedTest {
    pub filename: String,
    pub content: String,
}

use crate::http::url::{NormalizedInput, normalize_input_url};

/// 输入 → (文件名, 模板内容)。URL 判定复用 http::url::normalize_input_url（与 CLI 同一语义）。
pub fn derive_test_file(input: &str) -> DerivedTest {
    match normalize_input_url(input, "https") {
        NormalizedInput::Absolute(normalized_url)
        | NormalizedInput::LocalhostShorthand(normalized_url)
        | NormalizedInput::HostName(normalized_url) => derive_url_test_file(&normalized_url),
        NormalizedInput::NotUrl(name) => derive_filename_test_file(&name),
    }
}

fn derive_url_test_file(normalized_url: &str) -> DerivedTest {
    {
        let file_base = if let Ok(parsed) = url::Url::parse(normalized_url) {
            let host = parsed.host_str().unwrap_or("quick_test");
            let path = parsed.path().trim_matches('/');
            let mut combined = host.to_string();
            // host_str 不含端口；显式端口参与命名（localhost:3000 → localhost_3000_x）
            if let Some(port) = parsed.port() {
                combined.push(':');
                combined.push_str(&port.to_string());
            }
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
    }
}

fn derive_filename_test_file(name: &str) -> DerivedTest {
    let mut file_base = bound_filename(name);
    if !file_base.ends_with(".http") && !file_base.ends_with(".md") {
        file_base.push_str(".http");
    }
    DerivedTest {
        filename: file_base,
        // 债务：默认模板硬编码外网地址，离线即失败（设计文档债务登记）
        content: "GET http://httpbingo.org/get\n".to_string(),
    }
}

/// 输入字符串 → 8 位十六进制短哈希（截断名的区分后缀）。
/// 注意：DefaultHasher 不承诺跨 Rust 版本稳定，工具链升级可能使同一长输入
/// 生成不同后缀（产生新文件而非命中 AlreadyExists）——影响极小，登记债务。
fn short_hash(raw: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut hasher = DefaultHasher::new();
    hasher.write(raw.as_bytes());
    format!("{:08x}", hasher.finish() as u32)
}

/// 按 UTF-8 字符边界截断到 max_chars 个字符
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// URL 片段 → 合法文件名：非字母数字转 '_'；超过 80 字符截断并追加 8 位短哈希，
/// 保证总长远离各文件系统 255 字节上限。输入经 url::Url 解析，实际恒为 ASCII。
fn sanitize(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if mapped.chars().count() <= MAX_FILE_BASE {
        return mapped;
    }
    format!(
        "{}_{short_hash}",
        truncate_chars(&mapped, MAX_FILE_BASE),
        short_hash = short_hash(raw)
    )
}

/// 文件名通道的长度约束：stem 超 80 字节截断 + 短哈希，保留扩展名。
/// （与 URL 通道同等待遇，避免 300 字符名依赖 OS 的 ENAMETOOLONG 才失败）
fn bound_filename(name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if name.len() - i <= 5 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    if stem.len() <= MAX_FILE_BASE {
        return name.to_string();
    }
    let budget = MAX_FILE_BASE - 9; // 预留 '_' + 8 位哈希
    let mut cut = 0usize;
    for (idx, c) in stem.char_indices() {
        if idx + c.len_utf8() > budget {
            break;
        }
        cut = idx + c.len_utf8();
    }
    format!(
        "{}_{short_hash}{ext}",
        &stem[..cut],
        short_hash = short_hash(name)
    )
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
    fn test_derive_localhost_shorthand() {
        // CLI 语义一致：:3000/x 与 localhost:3000/x 均识别为本地 URL（commit 3 统一）
        let d = derive_test_file(":3000/x");
        assert_eq!(d.filename, "localhost_3000_x.http");
        assert_eq!(d.content, "GET http://localhost:3000/x\n");

        let d = derive_test_file("localhost:3000/x");
        assert_eq!(d.filename, "localhost_3000_x.http");
        assert_eq!(d.content, "GET http://localhost:3000/x\n");
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
    fn test_derive_long_filename_bounded() {
        let long = "长".repeat(300); // 多字节 stem，验证字符边界截断
        let d = derive_test_file(&long);
        assert!(d.filename.len() < 255, "{} bytes", d.filename.len());
        assert!(d.filename.ends_with(".http"));
        let d2 = derive_test_file("b".repeat(300).as_str());
        assert!(d2.filename.len() < 255);
        // 短名不受影响
        assert_eq!(derive_test_file("auth").filename, "auth.http");
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
