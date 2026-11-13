//! CLI 插件命令执行器与 TOFU 可信校验（03 §6.2 交互直通与环境变量隔离）。

use std::path::Path;
use std::process::Stdio;

fn validate_tofu_exe_path(exe_path: &Path) -> std::io::Result<()> {
    let has_ctrl_char = exe_path.to_string_lossy().chars().any(|c| c.is_control());
    #[cfg(unix)]
    let has_ctrl_byte = {
        use std::os::unix::ffi::OsStrExt;
        exe_path
            .as_os_str()
            .as_bytes()
            .iter()
            .any(|&b| b < 0x20 || b == 0x7f)
    };
    #[cfg(not(unix))]
    let has_ctrl_byte = false;

    if has_ctrl_char || has_ctrl_byte {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "路径包含换行或控制字符",
        ));
    }
    Ok(())
}

fn check_tofu_seen(tofu_cache: &Path, fingerprint: &str) -> bool {
    if !rupost::security::fs::is_owner_only_regular_file(tofu_cache) {
        return false;
    }
    std::fs::read_to_string(tofu_cache)
        .map(|content| content.lines().any(|line| line.trim() == fingerprint))
        .unwrap_or(false)
}

fn record_tofu_approval(tofu_cache: &Path, fingerprint: &str) -> rupost::Result<()> {
    use std::io::Write;
    if fingerprint.chars().any(|c| c.is_control()) {
        return Err(rupost::RupostError::SecurityViolation(
            "TOFU 指纹包含非法控制字符".to_string(),
        ));
    }
    if let Some(parent) = tofu_cache.parent()
        && !parent.as_os_str().is_empty()
    {
        rupost::security::fs::ensure_private_dir(parent)?;
    }
    if std::fs::symlink_metadata(tofu_cache).is_ok()
        && !rupost::security::fs::is_owner_only_regular_file(tofu_cache)
    {
        return Err(rupost::RupostError::SecurityViolation(format!(
            "拒绝写入不可信 TOFU 缓存（非本人所属常规文件或权限过宽）: {}",
            tofu_cache.display()
        )));
    }
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(tofu_cache)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } {
            return Err(rupost::RupostError::SecurityViolation(format!(
                "TOFU 缓存文件属主或类型校验失败: {}",
                tofu_cache.display()
            )));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(tofu_cache)?;
    writeln!(file, "{fingerprint}")?;
    Ok(())
}

fn verify_or_prompt_tofu(
    exe_path: &Path,
    tofu_cache: &Path,
    is_tty: bool,
) -> std::result::Result<(), String> {
    use std::io::Write;

    validate_tofu_exe_path(exe_path)
        .map_err(|e| format!("拒绝包含控制字符的插件路径 {:?}: {e}", exe_path))?;

    let exe_hash = sha256_prefix(exe_path).map_err(|e| {
        format!(
            "无法读取或校验插件二进制 {} 的 SHA-256: {e}",
            exe_path.display()
        )
    })?;
    let hash_short = exe_hash.get(..16).unwrap_or(&exe_hash);
    let fingerprint = format!("{} {}", exe_path.display(), exe_hash);

    if check_tofu_seen(tofu_cache, &fingerprint) {
        println!("[plugin] {} sha256:{hash_short}", exe_path.display());
        return Ok(());
    }

    if !is_tty {
        return Err(format!(
            "非交互式终端禁止首次执行未确认的插件命令（TOFU）：{} sha256:{hash_short}（请在交互终端运行一次以确认）",
            exe_path.display()
        ));
    }

    println!(
        "[plugin] 首次执行外部命令：{} sha256:{hash_short}",
        exe_path.display()
    );
    print!("确认执行？[y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("读取终端确认失败: {e}"))?;
    let answer = answer.trim().to_ascii_lowercase();
    if answer != "y" && answer != "yes" {
        return Err("已取消执行插件命令".to_string());
    }
    record_tofu_approval(tofu_cache, &fingerprint)
        .map_err(|e| format!("写入 TOFU 缓存失败: {e}"))?;
    Ok(())
}

fn extract_authorized_env_keys(specs: &[String]) -> std::collections::HashSet<String> {
    specs
        .iter()
        .filter_map(|spec| {
            let key = match spec.split_once('=') {
                Some((k, _)) => k.trim(),
                None => spec.trim(),
            };
            (!key.is_empty()).then(|| key.to_string())
        })
        .collect()
}

/// 拉起插件命令子进程（03 §6.2）：
/// - TTY 独占直通（inherit 三向）
/// - 来源横幅（路径 + sha256 前 16 位）+ 首次 TOFU 确认
/// - 环境白名单：基础允许集 + RUPOST_* + 显式 `--plugin-env`
/// - 退出码信号感知穿透
pub async fn launch_plugin_command(
    registry: &rupost::plugin::PluginRegistry,
    command_name: &str,
    exe_path: &Path,
    args: &[String],
    allow_env: &[String],
) -> i32 {
    let exe_path_owned = exe_path.to_path_buf();
    let tofu_cache = cache_dir().join("tofu-commands.txt");
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    match tokio::task::spawn_blocking(move || {
        verify_or_prompt_tofu(&exe_path_owned, &tofu_cache, is_tty)
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(msg)) => {
            eprintln!("[!] {msg}");
            return 1;
        }
        Err(e) => {
            eprintln!("[!] 插件 TOFU 校验任务异常终止: {e}");
            return 1;
        }
    }

    let authorized_env_keys = extract_authorized_env_keys(allow_env);
    for warning in
        registry.collect_missing_env_warnings(&[command_name.to_string()], &authorized_env_keys)
    {
        eprintln!("[WARN] {warning}");
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
    // WP-AST：内核自身路径——插件可经 `$RUPOST_BIN ast <file>` 消费核心解析 AST
    //（取不到 exe 时退化为 PATH 名，不阻断启动）
    cmd.env(
        "RUPOST_BIN",
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("rupost")),
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

fn sha256_prefix(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
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

        record_tofu_approval(&tofu_path, "/bin/demo abcdef").unwrap();
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
        assert!(
            !rupost::security::fs::is_owner_only_regular_file(&sym),
            "symlink TOFU 不得采信"
        );
    }

    #[test]
    fn test_sha256_prefix_streams_and_fails_closed() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        // 1. 不存在的文件必须 fail closed（Err）
        let missing = dir.path().join("missing-bin");
        assert!(sha256_prefix(&missing).is_err(), "缺失文件不得返回假哈希");

        // 2. 目录路径必须 fail closed
        assert!(sha256_prefix(dir.path()).is_err(), "目录不得计算哈希");

        // 3. 超过 64KB 缓冲区的流式哈希正确性
        let big_bin = dir.path().join("big-plugin");
        let payload = vec![0xABu8; 150 * 1024 + 17];
        std::fs::write(&big_bin, &payload).unwrap();
        let expected: String = Sha256::digest(&payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sha256_prefix(&big_bin).unwrap(), expected);

        // 4. 不可读文件（0o000）在非 root 用户下必须 fail closed
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt;
            let exec_only = dir.path().join("exec-only");
            std::fs::write(&exec_only, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&exec_only, std::fs::Permissions::from_mode(0o111)).unwrap();
            assert!(
                sha256_prefix(&exec_only).is_err(),
                "不可读 (--x) 插件二进制必须 fail closed"
            );
        }
    }

    #[test]
    fn test_tofu_rejects_control_chars_and_untrustworthy_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let tofu_path = dir.path().join("private").join("tofu-commands.txt");

        // 1. 控制字符路径（换行/回车/ESC）必须在写 TOFU 前拒绝
        for bad in [
            std::path::PathBuf::from("/tmp/evil\n/bin/ok abc"),
            std::path::PathBuf::from("/tmp/evil\r/bin/ok"),
            std::path::PathBuf::from("/tmp/evil\x1b[31m"),
        ] {
            let err = verify_or_prompt_tofu(&bad, &tofu_path, false)
                .expect_err("包含控制字符的 exe_path 必须被拒绝");
            assert!(err.contains("控制字符"), "unexpected err: {err}");
        }
        assert!(!tofu_path.exists());

        // 2. 已有宽松权限 TOFU 文件（0o644）或 symlink 时 record_tofu_approval 必须拒绝追加
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let loose = dir.path().join("loose-tofu.txt");
            std::fs::write(&loose, "/bin/existing 0000\n").unwrap();
            std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(
                record_tofu_approval(&loose, "/bin/new 1111").is_err(),
                "权限过宽的既有 TOFU 文件不得追加"
            );
            let content = std::fs::read_to_string(&loose).unwrap();
            assert!(!content.contains("/bin/new"), "被拒绝的条目不得写入文件");

            let target = dir.path().join("real.txt");
            std::fs::write(&target, "").unwrap();
            let sym = dir.path().join("sym-append.txt");
            std::os::unix::fs::symlink(&target, &sym).unwrap();
            assert!(
                record_tofu_approval(&sym, "/bin/new 1111").is_err(),
                "symlink TOFU 不得追加"
            );
            assert!(std::fs::read_to_string(&target).unwrap().is_empty());
        }
    }

    #[test]
    fn test_tofu_non_tty_fails_closed_unless_preapproved() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("demo-plugin");
        std::fs::write(&bin, b"#!/bin/sh\necho ok\n").unwrap();
        let tofu_path = dir.path().join("cfg").join("tofu-commands.txt");

        let err = verify_or_prompt_tofu(&bin, &tofu_path, false)
            .expect_err("非 TTY 下首次未确认插件必须 fail closed");
        assert!(err.contains("非交互") || err.contains("TOFU"), "{err}");
        assert!(!tofu_path.exists());

        let hash = sha256_prefix(&bin).unwrap();
        let fp = format!("{} {}", bin.display(), hash);
        record_tofu_approval(&tofu_path, &fp).unwrap();
        verify_or_prompt_tofu(&bin, &tofu_path, false).expect("已预置 TOFU 指纹在非 TTY 下应放行");
    }
}
