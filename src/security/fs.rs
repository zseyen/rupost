//! 文件系统安全原语 SSOT（属主/权限校验、私有目录、原子写入与布尔环境变量解析）。

use crate::Result;
use crate::error::RupostError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ATOMIC_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 校验路径是否为当前用户独占的普通文件（拒绝 symlink、目录及他人可读/写/执行权限）。
pub fn is_owner_only_regular_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != unsafe { libc::geteuid() } {
            return false;
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    true
}

/// 校验目录是否可信（存在、非 symlink、为目录；Unix 下属主为当前用户或 root，
/// 不可全局可写，且属主为当前用户时不可组/全局可写）。
pub fn is_trusted_dir(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let euid = unsafe { libc::geteuid() };
        let uid = meta.uid();
        if uid != euid && uid != 0 {
            return false;
        }
        let mode = meta.permissions().mode();
        if mode & 0o002 != 0 {
            return false;
        }
        if uid == euid && (mode & 0o022 != 0) {
            return false;
        }
    }
    true
}

/// 确保私有目录存在且属主与权限受控（非 symlink、属主为当前用户、Unix 下强制 `0700`）。
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(RupostError::SecurityViolation(
            "私有目录路径不能为空".to_string(),
        ));
    }
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return Err(RupostError::SecurityViolation(format!(
            "拒绝使用符号链接或非目录作为私有目录: {}",
            path.display()
        )));
    }
    std::fs::create_dir_all(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(RupostError::SecurityViolation(format!(
            "私有目录校验失败（非普通目录）: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(RupostError::SecurityViolation(format!(
                "私有目录属主与当前用户不一致: {}",
                path.display()
            )));
        }
        if meta.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// 在同目录创建唯一临时文件（`create_new(true)` + Unix `0600`）
fn create_unique_private_temp_file(
    parent: &Path,
    base_name: &str,
) -> Result<(PathBuf, std::fs::File)> {
    const MAX_ATTEMPTS: usize = 16;
    for _ in 0..MAX_ATTEMPTS {
        let seq = ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp_path = parent.join(format!(".{base_name}.tmp.{}.{seq}", std::process::id()));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&tmp_path) {
            Ok(file) => return Ok((tmp_path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(RupostError::IoError(e)),
        }
    }
    Err(RupostError::IoError(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "无法生成唯一的原子写入临时文件",
    )))
}

/// 原子写入私有文件：在同目录创建唯一临时文件（`create_new` + `0600`）写入后 `rename` 覆盖目标，
/// 失败时清理临时文件；绝不跟随 symlink，绝不原地截断目标文件。
pub fn write_private_file_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let Some(base_name_os) = target.file_name() else {
        return Err(RupostError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("无效的目标文件路径: {}", target.display()),
        )));
    };
    let base_name = base_name_os
        .to_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("rupost");
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let (tmp_path, mut file) = create_unique_private_temp_file(parent, base_name)?;
    let result = (|| -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.flush()?;
        drop(file);
        std::fs::rename(&tmp_path, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

/// 统一布尔环境变量解析（去除首尾空白、大小写不敏感）：
/// 仅 `"1" | "true" | "yes" | "on"` 返回 `true`，其余（包括 `"0"` / `"false"` / `""` / 未设置）均返回 `false`。
pub fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|val| {
        matches!(
            val.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// 在 `parent` 下创建唯一命名的私有 staging 目录（Unix `0700`，`create_new` 语义不冲突）。
/// 仅服务于 [`publish_private_dir_atomically`] 的整目录原子发布。
fn create_unique_private_staging_dir(parent: &Path) -> Result<PathBuf> {
    const MAX_ATTEMPTS: usize = 16;
    for _ in 0..MAX_ATTEMPTS {
        let seq = ATOMIC_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".rupost_staging.{}.{seq}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(RupostError::IoError(e)),
        }
    }
    Err(RupostError::IoError(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "无法生成唯一的私有 staging 目录",
    )))
}

/// 以"staging 目录 + rename 发布"整目录原子地创建私有目录工件集（WP-3 复现包原语）。
///
/// - `target` 必须不存在：已存在（含符号链接）一律返回 `SecurityViolation`，
///   绝不覆盖既有内容；后续 `rename` 的 fail-if-exists 语义提供第二道防线；
/// - `populate` 在 0700 私有 staging 目录内写入全部文件（建议逐文件走
///   [`write_private_file_atomic`]，得到 0600 文件）；
/// - `populate` 或最终 rename 任一步失败：staging 目录被清理、`target` 保持不存在、
///   无任何临时残留；
/// - 成功后 `target` 为 0700 私有目录。
pub fn publish_private_dir_atomically<F>(target: &Path, populate: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if target.as_os_str().is_empty() {
        return Err(RupostError::SecurityViolation(
            "私有目录工件目标路径不能为空".to_string(),
        ));
    }
    if let Ok(meta) = std::fs::symlink_metadata(target) {
        return Err(RupostError::SecurityViolation(format!(
            "目标路径已存在，拒绝覆盖（符号链接同样拒绝）: {} ({:?})",
            target.display(),
            meta.file_type()
        )));
    }
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent)?;

    let staging = create_unique_private_staging_dir(parent)?;
    let result = (|| -> Result<()> {
        populate(&staging)?;
        std::fs::rename(&staging, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_env_flag_enabled_truth_table() {
        let cases = [
            ("1", true),
            ("true", true),
            ("TRUE", true),
            (" yes ", true),
            ("On", true),
            ("0", false),
            ("false", false),
            ("FALSE", false),
            ("no", false),
            ("off", false),
            ("", false),
            ("   ", false),
            ("invalid", false),
        ];

        for (idx, (raw, expected)) in cases.iter().enumerate() {
            let key = format!("RUPOST_TEST_SECURITY_FS_FLAG_{idx}_{}", std::process::id());
            // 测试使用唯一进程内环境变量键，避免并发测试竞争
            unsafe {
                std::env::set_var(&key, raw);
            }
            let actual = env_flag_enabled(&key);
            unsafe {
                std::env::remove_var(&key);
            }
            assert_eq!(actual, *expected, "raw={raw:?}");
        }

        let unset_key = format!("RUPOST_TEST_SECURITY_FS_UNSET_{}", std::process::id());
        unsafe {
            std::env::remove_var(&unset_key);
        }
        assert!(!env_flag_enabled(&unset_key));
    }

    #[test]
    fn test_write_private_file_atomic_creates_and_overwrites() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("nested").join("secret.json");

        write_private_file_atomic(&target, b"first").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        write_private_file_atomic(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");

        // rename 目标为已有目录时必须失败并清理临时文件
        let target_dir = dir.path().join("existing_dir");
        std::fs::create_dir(&target_dir).unwrap();
        assert!(write_private_file_atomic(&target_dir, b"fail").is_err());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "失败后不应残留临时文件: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_write_private_file_atomic_does_not_follow_symlink_and_tightens_0644() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"do-not-touch").unwrap();

        let link = dir.path().join("snap.json");
        symlink(&victim, &link).unwrap();

        write_private_file_atomic(&link, b"atomic-payload").unwrap();

        // 目标 symlink 被原子替换为普通 0600 文件，原 symlink 指向的 victim 完好无损
        assert_eq!(std::fs::read(&victim).unwrap(), b"do-not-touch");
        assert!(
            !std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&link).unwrap(), b"atomic-payload");
        let mode = std::fs::metadata(&link).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // 覆盖已有 0644 文件后也必须收紧到 0600
        let existing = dir.path().join("existing.json");
        std::fs::write(&existing, b"old").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_file_atomic(&existing, b"new").unwrap();
        let mode2 = std::fs::metadata(&existing).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn test_is_owner_only_regular_file_checks() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = TempDir::new().unwrap();
        let file = dir.path().join("private.txt");
        std::fs::write(&file, b"ok").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(is_owner_only_regular_file(&file));

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_owner_only_regular_file(&file));

        let link = dir.path().join("link.txt");
        symlink(&file, &link).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!is_owner_only_regular_file(&link));

        assert!(!is_owner_only_regular_file(dir.path()));
        assert!(!is_owner_only_regular_file(&dir.path().join("missing")));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_trusted_dir_and_ensure_private_dir() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = TempDir::new().unwrap();
        let priv_dir = dir.path().join("rupost_home");
        ensure_private_dir(&priv_dir).unwrap();
        assert!(is_trusted_dir(&priv_dir));
        let mode = std::fs::metadata(&priv_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);

        // 组可写或全局可写目录应不可信
        let group_writable = dir.path().join("gw");
        std::fs::create_dir(&group_writable).unwrap();
        std::fs::set_permissions(&group_writable, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(!is_trusted_dir(&group_writable));

        // 符号链接目录应拒绝
        let link_dir = dir.path().join("link_dir");
        symlink(&priv_dir, &link_dir).unwrap();
        assert!(!is_trusted_dir(&link_dir));
        assert!(ensure_private_dir(&link_dir).is_err());

        // 普通文件伪装目录应拒绝
        let regular = dir.path().join("not_a_dir");
        std::fs::write(&regular, b"x").unwrap();
        assert!(!is_trusted_dir(&regular));
        assert!(ensure_private_dir(&regular).is_err());
    }

    /// 发布原语 happy path：populate 写入 → 目标 0700、文件 0600、无 staging 残留
    #[test]
    fn test_publish_private_dir_atomically_success() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("bundle");
        publish_private_dir_atomically(&target, |staging| {
            write_private_file_atomic(&staging.join("a.json"), b"{}")?;
            write_private_file_atomic(&staging.join("b.json"), b"[]")?;
            Ok(())
        })
        .unwrap();

        assert!(target.is_dir());
        let names: Vec<_> = std::fs::read_dir(&target)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.json".to_string()));
        assert!(names.contains(&"b.json".to_string()));
        assert_eq!(std::fs::read(target.join("a.json")).unwrap(), b"{}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
            let file_mode = std::fs::metadata(target.join("a.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
        }

        // 父目录内不得残留 staging 临时目录
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".rupost_staging")
            })
            .collect();
        assert!(leftovers.is_empty(), "staging leftover: {leftovers:?}");
    }

    /// 目标已存在：拒绝覆盖（SecurityViolation），既有内容一个字节不动
    #[test]
    fn test_publish_refuses_existing_target_and_preserves_content() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("existing");
        std::fs::create_dir(&target).unwrap();
        let sentinel = target.join("sentinel.txt");
        std::fs::write(&sentinel, b"do-not-touch").unwrap();

        let err = publish_private_dir_atomically(&target, |_| Ok(())).unwrap_err();
        assert!(
            matches!(err, RupostError::SecurityViolation(_)),
            "must be SecurityViolation: {err:?}"
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"do-not-touch");
    }

    #[cfg(unix)]
    #[test]
    fn test_publish_refuses_symlink_target() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        let sentinel = victim.join("sentinel.txt");
        std::fs::write(&sentinel, b"do-not-touch").unwrap();

        let link = dir.path().join("link");
        symlink(&victim, &link).unwrap();

        let err = publish_private_dir_atomically(&link, |_| Ok(())).unwrap_err();
        assert!(matches!(err, RupostError::SecurityViolation(_)), "{err:?}");
        // symlink 未被替换、victim 内容原样
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"do-not-touch");
    }

    /// populate 失败：目标不存在、staging 无残留（失败不留半成品）
    #[test]
    fn test_publish_failure_leaves_no_staging_nor_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("bundle");
        let result = publish_private_dir_atomically(&target, |staging| {
            write_private_file_atomic(&staging.join("partial.json"), b"{}")?;
            Err(RupostError::SecurityViolation("boom".to_string()))
        });
        assert!(result.is_err());
        assert!(!target.exists(), "failed publish must not create target");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".rupost_staging")
            })
            .collect();
        assert!(leftovers.is_empty(), "staging leftover: {leftovers:?}");
    }

    /// 父目录不存在时自动建立（嵌套路径一步到位）
    #[test]
    fn test_publish_creates_missing_parents() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("level1").join("level2").join("bundle");
        publish_private_dir_atomically(&target, |staging| {
            write_private_file_atomic(&staging.join("m.json"), b"{}")?;
            Ok(())
        })
        .unwrap();
        assert!(target.join("m.json").is_file());
    }
}
