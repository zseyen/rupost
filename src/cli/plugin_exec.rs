//! CLI 插件命令执行器与 TOFU 可信校验（03 §6.2 交互直通与环境变量隔离）。

use std::path::Path;
use std::process::Stdio;

/// 校验 TOFU 缓存文件属主与权限（防软链劫持与同机篡改）
pub(crate) fn tofu_file_trustworthy(path: &Path) -> bool {
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

fn check_tofu_seen(tofu_cache: &Path, fingerprint: &str) -> bool {
    if !tofu_file_trustworthy(tofu_cache) {
        return false;
    }
    std::fs::read_to_string(tofu_cache)
        .map(|content| content.lines().any(|line| line.trim() == fingerprint))
        .unwrap_or(false)
}

fn record_tofu_approval(tofu_cache: &Path, fingerprint: &str) {
    use std::io::Write;
    if let Some(parent) = tofu_cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(meta) = std::fs::symlink_metadata(tofu_cache)
        && meta.file_type().is_symlink()
    {
        eprintln!("[!] 拒绝写入 symlink TOFU 缓存: {}", tofu_cache.display());
        return;
    }
    #[cfg(unix)]
    let open_res = {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let res = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(tofu_cache);
        if let Ok(ref file) = res {
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        res
    };
    #[cfg(not(unix))]
    let open_res = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(tofu_cache);
    if let Ok(mut file) = open_res {
        let _ = writeln!(file, "{fingerprint}");
    }
}

/// 拉起插件命令子进程（03 §6.2）：
/// - TTY 独占直通（inherit 三向）
/// - 来源横幅（路径 + sha256 前 16 位）+ 首次 TOFU 确认
/// - 环境白名单：基础允许集 + RUPOST_* + 显式 `--plugin-env`
/// - 退出码信号感知穿透
pub async fn launch_plugin_command(exe_path: &Path, args: &[String], allow_env: &[String]) -> i32 {
    use std::io::Write;

    let exe_hash = sha256_prefix(exe_path);
    let hash_short = exe_hash.get(..16).unwrap_or(&exe_hash);
    let tofu_cache = cache_dir().join("tofu-commands.txt");
    let fingerprint = format!("{} {}", exe_path.display(), exe_hash);
    if !check_tofu_seen(&tofu_cache, &fingerprint) {
        println!(
            "[plugin] 首次执行外部命令：{} sha256:{hash_short}",
            exe_path.display()
        );
        print!("确认执行？[y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        let answer = answer.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("[!] 已取消执行插件命令");
            return 1;
        }
        record_tofu_approval(&tofu_cache, &fingerprint);
    } else {
        println!("[plugin] {} sha256:{hash_short}", exe_path.display());
    }

    let mut cmd = tokio::process::Command::new(exe_path);
    cmd.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    cmd.env_clear();
    for key in rupost::plugin::PLUGIN_ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    for (key, value) in std::env::vars().filter(|(k, _)| k.starts_with("RUPOST_")) {
        cmd.env(key, value);
    }
    for (key, value) in rupost::plugin::parse_plugin_env_specs(allow_env) {
        cmd.env(key, value);
    }
    cmd.env(
        "RUPOST_WORKSPACE",
        std::env::current_dir().unwrap_or_default(),
    );
    if let Some(home) = dirs::home_dir() {
        cmd.env("RUPOST_CONFIG_DIR", home.join(".rupost"));
    }

    match cmd.status().await {
        Ok(status) => status.code().unwrap_or_else(|| exit_signal_code(status)),
        Err(e) => {
            eprintln!("[!] 插件命令启动失败: {e}");
            1
        }
    }
}

fn exit_signal_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|s| 128 + s).unwrap_or(1)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        1
    }
}

fn cache_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".rupost"))
        .unwrap_or_else(|| std::path::PathBuf::from(".rupost"))
}

fn sha256_prefix(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let Ok(bytes) = std::fs::read(path) else {
        return "unreadable".repeat(2);
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_tofu_store_enforces_0600_and_rejects_symlink_or_loose_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tofu_path = dir.path().join("tofu-commands.txt");

        record_tofu_approval(&tofu_path, "/bin/demo abcdef");
        let mode = std::fs::metadata(&tofu_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "TOFU 缓存必须强制 0600");
        assert!(check_tofu_seen(&tofu_path, "/bin/demo abcdef"));

        std::fs::set_permissions(&tofu_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            !check_tofu_seen(&tofu_path, "/bin/demo abcdef"),
            "权限过宽的 TOFU 文件不得采信"
        );

        let sym = dir.path().join("sym-tofu.txt");
        std::os::unix::fs::symlink(&tofu_path, &sym).unwrap();
        assert!(!tofu_file_trustworthy(&sym), "symlink TOFU 不得采信");
    }
}
