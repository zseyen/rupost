//! Cookie Middleware for automatic cookie management.
//!
//! Provides cookie persistence and injection capabilities.

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;
use cookie_store::CookieStore;
use cookie_store::serde::json::{load_all, save_incl_expired_and_nonpersistent};
use reqwest_cookie_store::CookieStoreMutex;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 根据环境名自动重构 Cookie 文件名，实现环境间隔离
pub fn resolve_cookie_path(base_path: PathBuf, env_name: Option<&str>) -> PathBuf {
    if let Some(env) = env_name {
        let parent = base_path.parent().unwrap_or_else(|| Path::new(""));
        let file_stem = base_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cookies");
        let extension = base_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json");
        parent.join(format!("{}_{}.{}", file_stem, env, extension))
    } else {
        base_path
    }
}

/// Cookie operation mode.
#[derive(Debug, Clone, Default)]
pub enum CookieMode {
    /// Automatic persistence: Load from file, save on update.
    #[default]
    AutoPersist,
    /// Ephemeral: In-memory only, not saved to disk.
    Ephemeral,
    /// Disabled: No cookie handling.
    Disabled,
}

/// Cookie Middleware for managing cookies across requests.
pub struct CookieMiddleware {
    /// The cookie store (shared with reqwest client).
    store: Arc<CookieStoreMutex>,
    /// Path to the cookie file (None for ephemeral mode).
    file_path: Option<PathBuf>,
    /// Operation mode.
    mode: CookieMode,
}

impl CookieMiddleware {
    /// Create a new CookieMiddleware with automatic persistence.
    ///
    /// # Arguments
    /// * `file_path` - Path to the cookie storage file.
    pub fn new_with_persistence(file_path: PathBuf) -> Result<Self> {
        let store = Self::load_or_create(&file_path)?;
        Ok(Self {
            store: Arc::new(CookieStoreMutex::new(store)),
            file_path: Some(file_path),
            mode: CookieMode::AutoPersist,
        })
    }

    /// Create a new CookieMiddleware with ephemeral (in-memory only) storage.
    pub fn new_ephemeral() -> Self {
        Self {
            store: Arc::new(CookieStoreMutex::new(CookieStore::default())),
            file_path: None,
            mode: CookieMode::Ephemeral,
        }
    }

    /// Get a reference to the cookie store for use with reqwest client.
    pub fn cookie_store(&self) -> Arc<CookieStoreMutex> {
        Arc::clone(&self.store)
    }

    /// Load cookies from file or create a new empty store.
    fn load_or_create(path: &PathBuf) -> Result<CookieStore> {
        if path.exists() {
            let file = File::open(path)?;
            file.lock_shared()?;

            let reader = BufReader::new(&file);
            // Load as JSON using load_all to include session cookies
            let store = match load_all(reader) {
                Ok(store) => {
                    tracing::debug!("Loaded cookies from: {:?}", path);
                    store
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse cookie file ({:?}), falling back to empty store. Error: {}",
                        path,
                        e
                    );
                    CookieStore::default()
                }
            };
            file.unlock()?;
            Ok(store)
        } else {
            tracing::debug!("Cookie file not found, creating new store");
            Ok(CookieStore::default())
        }
    }

    /// 将当前 Cookie 罐安全落盘（仅持久化模式；临时模式为 no-op）。
    ///
    /// 安全写盘序列：打开不截断 → 拿排他锁 → 收紧权限 → 截断 + 回卷 → 写入 →
    /// 显式 flush → 解锁。两处顺序是刻意的：
    ///
    /// - 截断发生在拿锁**之后**：`File::create` 会立即截断，并发读者会看到空文件
    ///   并静默回退空仓库丢会话；
    /// - flush 在解锁**之前**：BufWriter 的隐式 flush 发生在 Drop（本函数 unlock
    ///   之后且忽略错误），会留下"他进程拿到锁却读到半截 JSON"的窗口。
    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.file_path else {
            return Ok(()); // Ephemeral mode, nothing to save
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 打开但明确不截断（截断延迟到拿锁之后）
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;

        // 0600 仅作用于新建文件（受 umask 影响的旧文件在持锁后显式收紧）：
        // cookie 文件含会话级凭据，不能是组/其他用户可读
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = file.metadata()?.permissions();
            if perm.mode() & 0o777 != 0o600 {
                perm.set_mode(0o600);
                file.set_permissions(perm)?;
            }
        }

        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());

        tracing::debug!(
            "Cookies in store before save: {:?}",
            store.iter_any().collect::<Vec<_>>()
        );

        // 截断并回卷到起始位置（set_len 不移动游标）
        file.set_len(0)?;
        let mut file = file;
        file.seek(SeekFrom::Start(0))?;

        {
            let mut writer = std::io::BufWriter::new(&file);
            save_incl_expired_and_nonpersistent(&store, &mut writer)
                .map_err(|e| anyhow::anyhow!("Failed to save cookies: {}", e))?;
            writer.flush()?;
        }

        file.unlock()?;
        tracing::debug!("Saved cookies to: {:?}", path);
        Ok(())
    }

    /// 导出当前 CookieStore 的序列化状态为 JSON Value (包含 Session Cookies)
    pub fn export_cookie_state(&self) -> Result<serde_json::Value> {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut buffer = Vec::new();
        save_incl_expired_and_nonpersistent(&store, &mut buffer)
            .map_err(|e| anyhow::anyhow!("Failed to serialize cookie store: {}", e))?;
        let value = serde_json::from_slice(&buffer)?;
        Ok(value)
    }

    /// 从 JSON Value 中导入 CookieStore 状态 (包含 Session Cookies)
    pub fn import_cookie_state(&self, state: serde_json::Value) -> Result<()> {
        let bytes = serde_json::to_vec(&state)?;
        let reader = std::io::BufReader::new(&bytes[..]);
        let new_store = load_all(reader)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize cookie store: {}", e))?;
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        *store = new_store;
        Ok(())
    }
}

impl Middleware for CookieMiddleware {
    async fn before_request(&self, _req: &mut Request) -> Result<()> {
        // Cookie injection is handled by reqwest's cookie_provider.
        // This hook is reserved for future manual header manipulation if needed.
        Ok(())
    }

    async fn after_response(&self, _resp: &Response) -> Result<()> {
        // Cookie 注入/提取由 reqwest 的 cookie_provider 完成；落盘不再在此处
        // 逐响应触发（每个响应全量重写文件且阻塞 async 线程），改由
        // BatchExecutor 在每个测试文件执行完后调用 `persist` 统一落盘，
        // Drop 时兜底。并行模式的 worker 全部使用临时仓库，不落盘。
        Ok(())
    }
}

impl Drop for CookieMiddleware {
    fn drop(&mut self) {
        // Best effort save on drop
        if matches!(self.mode, CookieMode::AutoPersist) {
            let _ = self.save().map_err(|e| {
                tracing::warn!("Failed to save cookies on drop: {}", e);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ephemeral_mode() {
        let middleware = CookieMiddleware::new_ephemeral();
        assert!(middleware.file_path.is_none());
    }

    #[test]
    fn test_persistence_mode() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cookies.json");

        let middleware = CookieMiddleware::new_with_persistence(path.clone()).unwrap();
        assert_eq!(middleware.file_path, Some(path));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cookies.json");

        // Create and save
        {
            let middleware = CookieMiddleware::new_with_persistence(path.clone()).unwrap();
            middleware.save().unwrap();
        }

        // Load again
        let middleware = CookieMiddleware::new_with_persistence(path).unwrap();
        assert!(
            middleware
                .store
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter_any()
                .count()
                == 0
        );
    }

    #[test]
    fn test_export_and_import_cookie_state() {
        let middleware = CookieMiddleware::new_ephemeral();

        // 往里面塞一个 cookie 测试
        {
            let mut store = middleware.store.lock().unwrap_or_else(|e| e.into_inner());
            let cookie = cookie::Cookie::build(("session", "123456"))
                .domain("example.com")
                .path("/")
                .build();
            let url = url::Url::parse("https://example.com/").unwrap();
            store.insert_raw(&cookie, &url).unwrap();
        }

        // 导出
        let state = middleware.export_cookie_state().unwrap();

        // 创建一个新的，导入状态
        let middleware2 = CookieMiddleware::new_ephemeral();
        middleware2.import_cookie_state(state).unwrap();

        // 验证导入成功
        let store2 = middleware.store.lock().unwrap_or_else(|e| e.into_inner());
        let cookies: Vec<_> = store2.iter_any().collect();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name(), "session");
        assert_eq!(cookies[0].value(), "123456");
    }

    #[cfg(unix)]
    #[test]
    fn test_save_permissions_are_0600() {
        // 回归：cookie 文件含会话级凭据，此前 File::create 受 umask 影响
        // 落盘为 0644（组/其他用户可读）
        let dir = tempdir().unwrap();
        let path = dir.path().join("cookies.json");

        let middleware = CookieMiddleware::new_with_persistence(path.clone()).unwrap();
        middleware.save().unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "cookie 文件权限必须为 0600，实际 {:o}", mode);

        // 旧的宽松权限文件在重新保存时被收紧
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        middleware.save().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "已存在的宽松权限文件应被收紧为 0600");
    }

    #[test]
    fn test_save_truncates_stale_longer_content() {
        // 回归：旧实现先截断后拿锁，且 BufWriter 在 unlock 后才隐式 flush。
        // 新序列（锁内 set_len + seek + 显式 flush）必须保证：残留的更长旧内容
        // 不出现在新文件尾部，且解锁前数据已完整落盘。
        let dir = tempdir().unwrap();
        let path = dir.path().join("cookies.json");
        std::fs::write(&path, "X".repeat(4096)).unwrap();

        let middleware = CookieMiddleware::new_with_persistence(path.clone()).unwrap();
        {
            let mut store = middleware.store.lock().unwrap_or_else(|e| e.into_inner());
            let cookie = cookie::Cookie::build(("session", "abcdef"))
                .domain("example.com")
                .path("/")
                .build();
            let url = url::Url::parse("https://example.com/").unwrap();
            store.insert_raw(&cookie, &url).unwrap();
        }
        middleware.save().unwrap();

        // 解锁后文件立即可解析（不依赖 BufWriter 的 Drop 时机）且无残留 "X"
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains('X'), "截断失败：旧内容残留在新文件中");
        let reloaded = CookieMiddleware::new_with_persistence(path).unwrap();
        let count = reloaded
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_any()
            .count();
        assert_eq!(count, 1, "重载后应恰好有 1 条 cookie");
    }
}
