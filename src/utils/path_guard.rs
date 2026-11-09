//! 路径围栏判定：从 runner 的 DAG 沙箱提取的共用实现（DRY）。
//! 词法前缀比较——调用方必须先 canonicalize，否则 `..` 段会逃逸判定。

use std::path::{Path, PathBuf};

/// 判定 path 是否落在任一根之下。这是正确性护栏而非安全边界（对网络零限制）。
pub fn is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

/// 将文件路径或相对路径统一规范化为跨平台标准的 POSIX 正斜杠字符串（彻底抹平 Windows 反斜杠差异）。
pub fn normalize_posix_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

/// 支持的活文档扩展名白名单（大小写不敏感）
pub const LIVE_DOC_EXTENSIONS: &[&str] = &["http", "md"];

/// 判定给定路径或文件名是否为受支持的活文档（.http / .md，大小写不敏感）。
pub fn is_live_doc(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            LIVE_DOC_EXTENSIONS
                .iter()
                .any(|&e| e.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

/// 判定给定路径是否为 .http 文件（大小写不敏感）。
pub fn is_http_doc(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("http"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_within_roots() {
        let roots = vec![PathBuf::from("/w/a"), PathBuf::from("/w/b")];
        assert!(is_within_roots(Path::new("/w/a/x.http"), &roots));
        assert!(is_within_roots(Path::new("/w/b/sub/y.http"), &roots));
        assert!(!is_within_roots(Path::new("/w/c/z.http"), &roots));
        assert!(!is_within_roots(Path::new("/w"), &roots)); // 根自身不算
    }

    #[test]
    fn test_normalize_posix_path_table() {
        let cases = [
            ("api/spec.md", "api/spec.md"),
            ("api\\spec.md", "api/spec.md"),
            ("a\\b/c\\d.http", "a/b/c/d.http"),
            ("C:\\project\\test.http", "C:/project/test.http"),
            ("", ""),
            ("simple.md", "simple.md"),
        ];

        for (input, expected) in cases {
            assert_eq!(
                normalize_posix_path(Path::new(input)),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn test_is_live_doc_table() {
        let cases = [
            ("test.http", true),
            ("test.HTTP", true),
            ("test.md", true),
            ("test.Md", true),
            ("spec.MD", true),
            ("sub/dir/api.http", true),
            ("test.json", false),
            ("test.txt", false),
            ("test", false),
            ("", false),
        ];

        for (input, expected) in cases {
            assert_eq!(is_live_doc(Path::new(input)), expected, "case: {input}");
        }
    }
}
