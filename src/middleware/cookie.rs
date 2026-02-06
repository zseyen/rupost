//! Cookie Middleware for automatic cookie management.
//!
//! Provides cookie persistence and injection capabilities.

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;
use async_trait::async_trait;
use cookie_store::CookieStore;
use reqwest_cookie_store::CookieStoreMutex;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

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
            let reader = BufReader::new(file);
            // Load as JSON format
            let store: CookieStore = serde_json::from_reader(reader)
                .map_err(|e| anyhow::anyhow!("Failed to load cookies: {}", e))?;
            tracing::info!("Loaded cookies from: {:?}", path);
            Ok(store)
        } else {
            tracing::debug!("Cookie file not found, creating new store");
            Ok(CookieStore::default())
        }
    }

    /// Save cookies to file (with file lock for safety).
    fn save(&self) -> Result<()> {
        let Some(path) = &self.file_path else {
            return Ok(()); // Ephemeral mode, nothing to save
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Open with file lock
        let file = File::create(path)?;
        use fs2::FileExt;
        file.lock_exclusive()?;

        // Save as JSON
        let store = self.store.lock().unwrap();
        let mut writer = std::io::BufWriter::new(&file);
        serde_json::to_writer_pretty(&mut writer, &*store)
            .map_err(|e| anyhow::anyhow!("Failed to save cookies: {}", e))?;

        file.unlock()?;
        tracing::debug!("Saved cookies to: {:?}", path);
        Ok(())
    }
}

#[async_trait]
impl Middleware for CookieMiddleware {
    async fn before_request(&self, _req: &mut Request) -> Result<()> {
        // Cookie injection is handled by reqwest's cookie_provider.
        // This hook is reserved for future manual header manipulation if needed.
        Ok(())
    }

    async fn after_response(&self, _resp: &Response) -> Result<()> {
        // Cookie extraction is handled by reqwest's cookie_provider.
        // Here we persist to disk if in AutoPersist mode.
        if matches!(self.mode, CookieMode::AutoPersist) {
            self.save()?;
        }
        Ok(())
    }
}

impl Drop for CookieMiddleware {
    fn drop(&mut self) {
        // Best effort save on drop
        if matches!(self.mode, CookieMode::AutoPersist) {
            if let Err(e) = self.save() {
                tracing::warn!("Failed to save cookies on drop: {}", e);
            }
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
        assert!(middleware.store.lock().unwrap().iter_any().count() == 0);
    }
}
