//! 路径围栏判定：从 runner 的 DAG 沙箱提取的共用实现（DRY）。
//! 词法前缀比较——调用方必须先 canonicalize，否则 `..` 段会逃逸判定。

use std::path::{Path, PathBuf};

/// 判定 path 是否落在任一根之下。这是正确性护栏而非安全边界（对网络零限制）。
pub fn is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
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
}
