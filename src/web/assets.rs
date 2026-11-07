use crate::security::PathSandbox;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// 静态资产提供者接口（开闭原则：支持内嵌资产与外部物理目录）
pub trait AssetProvider: Send + Sync + 'static {
    /// 根据相对 URL 路径获取资产的字节流与 MIME Content-Type
    fn get_asset(&self, path: &str) -> Option<(Cow<'static, [u8]>, &'static str)>;
}

/// 允许投递的静态资产扩展名白名单（防误挂载导致源码/敏感文件外泄）
const ASSET_EXTENSIONS: &[&str] = &[
    "html", "htm", "css", "js", "mjs", "json", "svg", "png", "jpg", "jpeg", "ico", "woff",
    "woff2",
];

/// 外部物理目录托管实现（用于 --static-dir 自定义前端接入，内置统一沙箱）
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
        let target_rel = if clean_path.is_empty() {
            "index.html"
        } else {
            clean_path
        };

        // 复用统一安全沙箱组件：拒绝访问隐藏文件与跨目录逃逸，并收敛可投递的扩展名白名单
        let sandbox = PathSandbox::new(&self.base_dir)
            .allow_hidden(false)
            .with_extensions(ASSET_EXTENSIONS);
        let safe_target = sandbox.resolve_safe_path(target_rel).ok()?;

        if safe_target.is_file()
            && let Ok(bytes) = std::fs::read(&safe_target)
        {
            let mime = guess_mime_type(&safe_target);
            return Some((Cow::Owned(bytes), mime));
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
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
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
        let asset = provider
            .get_asset("/")
            .expect("Root should map to index.html");
        assert_eq!(asset.1, "text/html; charset=utf-8");
        assert_eq!(String::from_utf8_lossy(&asset.0), "<h1>Custom UI</h1>");

        // 测试不存在的文件
        assert!(provider.get_asset("/missing.png").is_none());

        // 验证目录穿越攻击被成功拦截
        assert!(
            provider
                .get_asset("/../../../../../../etc/passwd")
                .is_none()
        );
        assert!(provider.get_asset("/../secret.txt").is_none());
    }

    #[test]
    fn test_local_dir_asset_provider_extension_whitelist() {
        let temp = TempDir::new().unwrap();
        // 白名单内的正常资产
        std::fs::write(temp.path().join("index.html"), "<h1>ok</h1>").unwrap();
        std::fs::write(temp.path().join("app.js"), "console.log(1)").unwrap();
        std::fs::write(temp.path().join("logo.svg"), "<svg/>").unwrap();
        // 白名单外的敏感/源码文件：即便语法合法路径合法也绝不投递
        std::fs::write(temp.path().join("secret.txt"), "top-secret").unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("backup.http"), "GET http://x").unwrap();

        let provider = LocalDirAssetProvider::new(temp.path());

        assert!(provider.get_asset("/index.html").is_some());
        assert!(provider.get_asset("/app.js").is_some());
        assert!(provider.get_asset("/logo.svg").is_some());

        assert!(
            provider.get_asset("/secret.txt").is_none(),
            "non-whitelisted extension must never be served"
        );
        assert!(provider.get_asset("/main.rs").is_none());
        assert!(provider.get_asset("/backup.http").is_none());
    }
}
