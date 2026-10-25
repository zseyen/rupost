//! TUI 写路径端口：依赖倒置，让 ViewModel 只依赖抽象。
//! 端口只覆盖写路径（用例文件的创建）；读路径 I/O 见设计文档债务登记。

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub trait FileRepository: Send + Sync {
    /// 原子创建并写入；文件已存在时返回 `ErrorKind::AlreadyExists`（不覆盖）。
    /// 权限/磁盘等错误原样浮出，由调用方决定如何呈现。
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()>;

    /// 规范化已存在的路径（workspace_root、已存在的父目录）。
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf>;
}

/// Adapter：真实文件系统
pub struct FsRepository;

impl FileRepository for FsRepository {
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(content.as_bytes())
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

/// 测试替身：内存文件系统。`canonicalize` 为身份映射（不做 symlink 解析），
/// 因此基于它的围栏检查只能验证词法合法性，symlink 逃逸须由 FsRepository 集成测试覆盖。
#[derive(Default)]
pub struct InMemoryRepository(Mutex<HashMap<PathBuf, String>>);

impl FileRepository for InMemoryRepository {
    fn create_exclusive(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let mut map = self.0.lock().expect("in-memory repo poisoned");
        if map.contains_key(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
        map.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// `Arc<dyn FileRepository>` 不实现 Debug，而 AppState 派生 Debug——用 newtype 包一层。
#[derive(Clone)]
pub struct Repo(Arc<dyn FileRepository>);

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Repo(..)")
    }
}

impl Repo {
    pub fn new<R: FileRepository + 'static>(repo: R) -> Self {
        Self(Arc::new(repo))
    }

    pub fn file_repo(&self) -> &dyn FileRepository {
        self.0.as_ref()
    }
}

impl Default for Repo {
    fn default() -> Self {
        Self::new(FsRepository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_create_and_already_exists() {
        let repo = InMemoryRepository::default();
        let path = Path::new("/w/google_com.http");

        repo.create_exclusive(path, "GET https://google.com\n")
            .unwrap();
        let err = repo.create_exclusive(path, "x").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        // 已存在文件未被覆盖
        let map = repo.0.lock().unwrap();
        assert_eq!(map.get(path).unwrap(), "GET https://google.com\n");
    }

    #[test]
    fn test_in_memory_canonicalize_is_identity() {
        let repo = InMemoryRepository::default();
        assert_eq!(
            repo.canonicalize(Path::new("/w/a")).unwrap(),
            PathBuf::from("/w/a")
        );
    }

    #[test]
    fn test_fs_repository_create_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.http");

        FsRepository
            .create_exclusive(&path, "GET http://x\n")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "GET http://x\n");

        let err = FsRepository.create_exclusive(&path, "y").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "GET http://x\n");
    }

    #[test]
    fn test_fs_repository_permission_error_surfaces() {
        // 只读目录下的创建必须以 io::Error 浮出，而不是被吞掉
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        let mut perms = std::fs::metadata(&ro).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o555);
        }
        std::fs::set_permissions(&ro, perms).unwrap();

        let err = FsRepository
            .create_exclusive(&ro.join("a.http"), "x")
            .unwrap_err();
        assert!(!matches!(err.kind(), std::io::ErrorKind::AlreadyExists));
    }
}
