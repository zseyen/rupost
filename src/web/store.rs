use crate::Result;
use crate::error::RupostError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 目录树中的文件条目
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

/// 活文档存储接口（解耦文件系统与外部存储源）
#[async_trait::async_trait]
#[allow(clippy::double_must_use)]
pub trait DocumentStore: Send + Sync + 'static {
    /// 列出工作区内的活文档（.md, .http）
    async fn list_documents(&self, subpath: Option<&str>) -> Result<Vec<FileEntry>>;

    /// 读取指定文档的内容
    async fn read_document(&self, path: &str) -> Result<String>;

    /// 保存指定文档的内容
    async fn save_document(&self, path: &str, content: &str) -> Result<()>;
}

/// 基于本地磁盘的标准实现（严格内置防目录越界沙箱检查）
#[derive(Debug, Clone)]
pub struct LocalFsDocumentStore {
    root_dir: PathBuf,
}

impl LocalFsDocumentStore {
    pub fn new(root_dir: impl AsRef<Path>) -> Self {
        let abs_root = std::fs::canonicalize(root_dir.as_ref())
            .unwrap_or_else(|_| root_dir.as_ref().to_path_buf());
        Self { root_dir: abs_root }
    }

    /// 防目录穿越安全沙箱校验
    fn resolve_safe_path(&self, requested: &str) -> Result<PathBuf> {
        let trimmed = requested.trim_start_matches('/');
        let candidate = self.root_dir.join(trimmed);

        // 如果文件存在，用 canonicalize；如果文件尚不存在（新建），则校验其已存在的祖先目录
        if candidate.exists() {
            let canon = candidate.canonicalize().map_err(RupostError::IoError)?;
            if canon.starts_with(&self.root_dir) {
                Ok(canon)
            } else {
                Err(RupostError::Other(format!(
                    "Path traversal attempt rejected: {}",
                    requested
                )))
            }
        } else {
            // 路径尚未存在，直接检查文本是否包含向上穿越组件
            for component in Path::new(trimmed).components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(RupostError::Other(format!(
                        "Path traversal attempt rejected: {}",
                        requested
                    )));
                }
            }
            Ok(candidate)
        }
    }
}

#[async_trait::async_trait]
impl DocumentStore for LocalFsDocumentStore {
    async fn list_documents(&self, subpath: Option<&str>) -> Result<Vec<FileEntry>> {
        let target_dir = if let Some(sub) = subpath {
            self.resolve_safe_path(sub)?
        } else {
            self.root_dir.clone()
        };

        if !target_dir.is_dir() {
            return Err(RupostError::Other(format!(
                "Not a directory: {:?}",
                target_dir
            )));
        }

        let mut entries = Vec::new();
        let read_dir = std::fs::read_dir(&target_dir).map_err(RupostError::IoError)?;

        for entry_res in read_dir {
            let entry = entry_res.map_err(RupostError::IoError)?;
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // 忽略隐藏文件（.git, .workspace 等）
            if file_name.starts_with('.') {
                continue;
            }

            let meta = entry.metadata().map_err(RupostError::IoError)?;
            let is_dir = meta.is_dir();

            // 如果是文件，仅纳入 .md 与 .http
            if !is_dir && !file_name.ends_with(".md") && !file_name.ends_with(".http") {
                continue;
            }

            let rel_path = path
                .strip_prefix(&self.root_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| file_name.clone());

            let modified_at = meta.modified().ok().map(|t| {
                let datetime: chrono::DateTime<chrono::Local> = t.into();
                datetime.format("%Y-%m-%d %H:%M:%S").to_string()
            });

            entries.push(FileEntry {
                path: rel_path,
                name: file_name,
                is_dir,
                size_bytes: meta.len(),
                modified_at,
            });
        }

        // 排序：目录优先，其次按字母顺序
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        Ok(entries)
    }

    async fn read_document(&self, path: &str) -> Result<String> {
        let safe_path = self.resolve_safe_path(path)?;
        if !safe_path.exists() {
            return Err(RupostError::Other(format!("File not found: {}", path)));
        }
        std::fs::read_to_string(&safe_path).map_err(RupostError::IoError)
    }

    async fn save_document(&self, path: &str, content: &str) -> Result<()> {
        let safe_path = self.resolve_safe_path(path)?;
        if let Some(parent) = safe_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(RupostError::IoError)?;
        }
        std::fs::write(&safe_path, content).map_err(RupostError::IoError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_fs_document_store_basic_and_sandbox() {
        let temp = TempDir::new().unwrap();
        let store = LocalFsDocumentStore::new(temp.path());

        // 写入正常文件
        store
            .save_document("api/spec.md", "# Test API Spec")
            .await
            .unwrap();
        let content = store.read_document("api/spec.md").await.unwrap();
        assert_eq!(content, "# Test API Spec");

        // 目录遍历列表
        let files = store.list_documents(Some("api")).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "spec.md");
        assert_eq!(files[0].path, "api/spec.md");

        // 验证目录穿越拦截
        let attack = store.read_document("../../../etc/passwd").await;
        assert!(attack.is_err(), "目录穿越必须被安全拦截");
        let attack_save = store
            .save_document("../../evil.md", "malicious payload")
            .await;
        assert!(attack_save.is_err(), "跨根目录保存必须被拦截");
    }
}
