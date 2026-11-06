use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// 静态资产提供者接口（开闭原则：支持内嵌资产与外部物理目录）
pub trait AssetProvider: Send + Sync + 'static {
    /// 根据相对 URL 路径获取资产的字节流与 MIME Content-Type
    fn get_asset(&self, path: &str) -> Option<(Cow<'static, [u8]>, &'static str)>;
}

/// 外部物理目录托管实现（用于 --static-dir 自定义前端接入）
pub struct LocalDirAssetProvider {
    base_dir: PathBuf,
}

impl LocalDirAssetProvider {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }
}

impl AssetProvider for LocalDirAssetProvider {
    fn get_asset(&self, path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
        let clean_path = path.trim_start_matches('/');
        let target = if clean_path.is_empty() {
            self.base_dir.join("index.html")
        } else {
            self.base_dir.join(clean_path)
        };

        if target.is_file() {
            if let Ok(bytes) = std::fs::read(&target) {
                let mime = guess_mime_type(&target);
                return Some((Cow::Owned(bytes), mime));
            }
        }
        None
    }
}

/// 官方默认内置资产提供者（极简高质感内嵌前端，零外部文件与 node 依赖）
pub struct EmbeddedAssetProvider;

impl AssetProvider for EmbeddedAssetProvider {
    fn get_asset(&self, path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
        let clean_path = path.trim_start_matches('/');
        match clean_path {
            "" | "index.html" => {
                let html = include_str!("assets/index.html");
                Some((Cow::Borrowed(html.as_bytes()), "text/html; charset=utf-8"))
            }
            "app.js" => {
                let js = include_str!("assets/app.js");
                Some((
                    Cow::Borrowed(js.as_bytes()),
                    "application/javascript; charset=utf-8",
                ))
            }
            "style.css" => {
                let css = include_str!("assets/style.css");
                Some((Cow::Borrowed(css.as_bytes()), "text/css; charset=utf-8"))
            }
            _ => None,
        }
    }
}

/// 简单的 MIME 类型推导
pub fn guess_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_local_dir_asset_provider() {
        let temp = TempDir::new().unwrap();
        let index_path = temp.path().join("index.html");
        std::fs::write(&index_path, "<h1>Custom UI</h1>").unwrap();

        let provider = LocalDirAssetProvider::new(temp.path());

        // 测试根路径自动映射 index.html
        let asset = provider.get_asset("/").expect("Root should map to index.html");
        assert_eq!(asset.1, "text/html; charset=utf-8");
        assert_eq!(String::from_utf8_lossy(&asset.0), "<h1>Custom UI</h1>");

        // 测试不存在的文件
        assert!(provider.get_asset("/missing.png").is_none());
    }
}
