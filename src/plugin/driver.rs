//! Stdio IPC 驱动器（03-plugin-spi-and-degradation.md §8/§10 唯一权威实现）：
//! - 异步双工：stdin 写入与 stdout/stderr 排水拆独立并发 Task（大包防死锁）
//! - 载荷硬上限 10MB（出站先验校验 + 入站 `MAX_PLUGIN_PAYLOAD_BYTES` 限读）
//! - stdin 写毕显式 `shutdown()` + drop 发送 EOF
//! - 超时进程组 SIGKILL（Unix PGID / Windows kill_on_drop 兜底）
//! - 时延隔离：`plugin_duration_us` 单独计时，绝不混入物理网络秒表

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use super::protocol::{
    MAX_PLUGIN_PAYLOAD_BYTES, PluginFailureKind, PluginInvocationOutcome, parse_plugin_output,
};
use super::registry::resolve_interpreter_in_path;
use crate::Result;

const MAX_STDERR_TAIL_BYTES: usize = 64 * 1024;
const WINDOWS_ERROR_BAD_EXE_FORMAT: i32 = 193;

/// 取消/早退/异常路径清理守卫：确保子进程组与三个并发 IO 任务绝不泄漏
struct PluginInvocationGuard {
    pg_guard: crate::platform::ProcessGroupGuard,
    child: Option<tokio::process::Child>,
    stdin_task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    stdout_task: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
    stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>,
}

impl Drop for PluginInvocationGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.stdin_task.take() {
            handle.abort();
        }
        if let Some(handle) = self.stdout_task.take() {
            handle.abort();
        }
        if let Some(handle) = self.stderr_task.take() {
            handle.abort();
        }
        self.pg_guard.kill_now();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

/// 净化不可信插件 stderr：剥除 ANSI CSI/OSC/DCS/ESC 转义序列与控制字符，仅保留可打印文本与 `\n`/`\t`
fn sanitize_plugin_stderr(bytes: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(bytes);
    let mut cleaned = String::with_capacity(lossy.len());
    let mut chars = lossy.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek().copied() {
                // CSI: ESC [ ... final_byte (@..=~)
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii() && ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] ... (BEL or ST)
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // DCS / SOS / PM / APC: ESC (P|X|^|_) ... ST
                Some('P' | 'X' | '^' | '_') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // nF escape sequence (e.g. ESC ( B) or 2-byte Fe/Fp/Fs
                Some(next) if next.is_ascii() && (' '..='/').contains(&next) => {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii() && ('0'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if c == '\n' || c == '\t' || !c.is_control() {
            cleaned.push(c);
        }
    }
    cleaned
}

/// 带限时的任务收尾：防止遗留后台句柄阻塞超时返回路径
async fn join_with_grace<T>(mut handle: tokio::task::JoinHandle<T>, grace: Duration) -> Option<T> {
    match tokio::time::timeout(grace, &mut handle).await {
        Ok(Ok(val)) => Some(val),
        Ok(Err(_)) => None,
        Err(_) => {
            handle.abort();
            None
        }
    }
}

/// 应对 Linux 多线程并行 `fork()` 期间继承临时写入句柄引发的短暂 `ETXTBSY (os error 26)`：
/// 仅在 `ExecutableFileBusy` 时做极短退避重试，其余错误立即透传。
async fn spawn_with_etxtbsy_retry(
    cmd: &mut tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    const MAX_RETRIES: usize = 8;
    for attempt in 0..=MAX_RETRIES {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e)
                if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < MAX_RETRIES =>
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// 解析出的 Shebang 声明（供 Windows 透明桥接与跨平台 RuntimeMissing 归因共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShebangSpec {
    pub program: String,
    pub display_interpreter: String,
    pub args: Vec<String>,
}

/// 从脚本首行解析 `#!` Shebang 声明（支持 `#!/usr/bin/env [-S] <interp> [args...]` 与显式路径 `#!/path/to/<interp> [args...]`）。
pub fn parse_shebang_line(first_line: &str) -> Option<ShebangSpec> {
    let rest = first_line.trim_end_matches('\r').strip_prefix("#!")?;
    let mut parts = rest.split_whitespace();
    let first = parts.next()?;
    let is_env = first == "env" || first.ends_with("/env");

    let (program_raw, remaining_args): (&str, Vec<String>) = if is_env {
        let mut interp: Option<&str> = None;
        for part in parts.by_ref() {
            if !part.starts_with('-') {
                interp = Some(part);
                break;
            }
        }
        let chosen = interp.unwrap_or(first);
        (chosen, parts.map(str::to_string).collect())
    } else {
        (first, parts.map(str::to_string).collect())
    };

    let program = sanitize_extracted_token(program_raw)?;
    let display_raw = Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&program);
    let display_interpreter =
        sanitize_extracted_token(display_raw).unwrap_or_else(|| program.clone());

    Some(ShebangSpec {
        program,
        display_interpreter,
        args: remaining_args,
    })
}

/// 读取文件首部 512 字节并解析 `#!` Shebang 声明。
pub fn parse_shebang(executable: &Path) -> Option<ShebangSpec> {
    use std::io::Read;
    let mut file = std::fs::File::open(executable).ok()?;
    let mut buf = [0u8; 512];
    let n = file.read(&mut buf).ok()?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.lines().next().and_then(parse_shebang_line)
}

fn is_bad_exe_format(err: &std::io::Error) -> bool {
    cfg!(windows) && err.raw_os_error() == Some(WINDOWS_ERROR_BAD_EXE_FORMAT)
}

/// 插件子进程永不透传的敏感变量（审查 P1-1：`RUPOST_*` 前缀透传是管线变量
/// 契约，但会把 `RUPOST_AI_API_KEY` 送进任意插件子进程——密钥面必须显式经
/// manifest `requires_env` + `--plugin-env` 放行，豁免清单只增不改）。
pub const PLUGIN_ENV_SECRET_DENYLIST: [&str; 1] = ["RUPOST_AI_API_KEY"];

/// 统一裁剪插件子进程环境变量白名单（03-plugin-spi-and-degradation.md §6.2 SSOT）：allowlist + `RUPOST_*`
/// 前缀（除 [`PLUGIN_ENV_SECRET_DENYLIST`]）+ 显式放行覆盖项。
pub fn apply_plugin_env(cmd: &mut tokio::process::Command, env_overrides: &[(String, String)]) {
    cmd.env_clear();
    for key in super::types::PLUGIN_ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    for (key, value) in std::env::vars().filter(|(k, _)| {
        k.starts_with("RUPOST_") && !PLUGIN_ENV_SECRET_DENYLIST.contains(&k.as_str())
    }) {
        cmd.env(key, value);
    }
    for (key, value) in env_overrides {
        cmd.env(key, value);
    }
}

/// 插件进程启动失败类型（用于区分可诊断的运行时缺失与硬性 I/O 故障）
#[derive(Debug)]
pub enum PluginSpawnError {
    RuntimeMissing { interpreter: String, detail: String },
    Io(std::io::Error),
}

fn resolve_shebang_for_bad_exe_format(
    executable: &Path,
    path_env: Option<&OsStr>,
    raw_err: &std::io::Error,
) -> std::result::Result<(PathBuf, ShebangSpec), Option<PluginFailureKind>> {
    let Some(spec) = parse_shebang(executable) else {
        return Err(None);
    };
    match resolve_interpreter_in_path(&spec.program, path_env) {
        Some(interp_exe) => Ok((interp_exe, spec)),
        None => Err(Some(PluginFailureKind::RuntimeMissing {
            interpreter: spec.display_interpreter,
            detail: format!(
                "启动失败（Shebang 解释器 '{}' 未找到）: {raw_err}",
                spec.program
            ),
        })),
    }
}

/// 跨平台插件进程启动器（Hook 管道与 CLI 子命令共用）：
/// 1. 应对 Linux 并发写后即执行的短暂 `ETXTBSY (os error 26)`；
/// 2. `ENOENT` 自动映射到 `RuntimeMissing`；
/// 3. Windows `os error 193`（`ERROR_BAD_EXE_FORMAT`）若检测为 `#!` Shebang 脚本，
///    则透明桥接解释器重启，或在解释器不存在时映射为 `RuntimeMissing`。
pub async fn spawn_plugin_process<F>(
    executable: &Path,
    mut configure_cmd: F,
) -> std::result::Result<tokio::process::Child, PluginSpawnError>
where
    F: FnMut(&mut tokio::process::Command),
{
    let mut cmd = tokio::process::Command::new(executable);
    configure_cmd(&mut cmd);

    match spawn_with_etxtbsy_retry(&mut cmd).await {
        Ok(child) => Ok(child),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(PluginSpawnError::RuntimeMissing {
                interpreter: infer_missing_interpreter(executable),
                detail: format!("启动失败（ENOENT）: {e}"),
            })
        }
        Err(e) if is_bad_exe_format(&e) => {
            let path_env = std::env::var_os("PATH");
            let (interp_exe, spec) =
                match resolve_shebang_for_bad_exe_format(executable, path_env.as_deref(), &e) {
                    Ok(pair) => pair,
                    Err(Some(PluginFailureKind::RuntimeMissing {
                        interpreter,
                        detail,
                    })) => {
                        return Err(PluginSpawnError::RuntimeMissing {
                            interpreter,
                            detail,
                        });
                    }
                    _ => return Err(PluginSpawnError::Io(e)),
                };

            let mut shebang_cmd = tokio::process::Command::new(&interp_exe);
            shebang_cmd.args(&spec.args).arg(executable);
            configure_cmd(&mut shebang_cmd);

            match spawn_with_etxtbsy_retry(&mut shebang_cmd).await {
                Ok(child) => Ok(child),
                Err(e2) if e2.kind() == std::io::ErrorKind::NotFound || is_bad_exe_format(&e2) => {
                    Err(PluginSpawnError::RuntimeMissing {
                        interpreter: spec.display_interpreter,
                        detail: format!(
                            "启动失败（Shebang 解释器 '{}' 不可执行）: {e2}",
                            spec.program
                        ),
                    })
                }
                Err(e2) => Err(PluginSpawnError::Io(e2)),
            }
        }
        Err(e) => Err(PluginSpawnError::Io(e)),
    }
}

/// 执行一次插件 Stdio IPC 调用：
/// `payload` 为内核序列化的输入 JSON 字符串；返回 [`PluginInvocationOutcome`]。
pub async fn invoke_plugin(
    executable: &std::path::Path,
    payload: &str,
    timeout: Duration,
    env_overrides: &[(String, String)],
) -> Result<PluginInvocationOutcome> {
    if payload.len() > MAX_PLUGIN_PAYLOAD_BYTES {
        return Ok(PluginInvocationOutcome {
            output: None,
            failure: Some(PluginFailureKind::InvalidOutput(format!(
                "输入载荷 {} 字节超过 10MB 硬上限",
                payload.len()
            ))),
            duration_us: 0,
            stderr_tail: String::new(),
        });
    }

    let started = Instant::now();
    let mut child = match spawn_plugin_process(executable, |cmd| {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::platform::configure_process_group(cmd.as_std_mut());
        apply_plugin_env(cmd, env_overrides);
    })
    .await
    {
        Ok(child) => child,
        Err(PluginSpawnError::RuntimeMissing {
            interpreter,
            detail,
        }) => {
            return Ok(PluginInvocationOutcome {
                output: None,
                failure: Some(PluginFailureKind::RuntimeMissing {
                    interpreter,
                    detail,
                }),
                duration_us: started.elapsed().as_micros() as u64,
                stderr_tail: String::new(),
            });
        }
        Err(PluginSpawnError::Io(e)) => {
            return Err(crate::RupostError::PluginError(format!(
                "插件 {} 启动失败: {e}",
                executable.display()
            )));
        }
    };
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| crate::RupostError::PluginError("stdin 句柄获取失败".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| crate::RupostError::PluginError("stdout 句柄获取失败".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| crate::RupostError::PluginError("stderr 句柄获取失败".to_string()))?;

    let payload_bytes = payload.as_bytes().to_vec();
    let stdin_task = tokio::spawn(async move {
        if let Err(e) = stdin.write_all(&payload_bytes).await
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(e);
        }
        let _ = stdin.shutdown().await;
        drop(stdin);
        Ok::<(), std::io::Error>(())
    });

    let stdout_capped_flag = Arc::new(AtomicBool::new(false));
    let stdout_capped_writer = Arc::clone(&stdout_capped_flag);
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(8 * 1024);
        let mut chunk = [0u8; 16 * 1024];
        let mut reader = tokio::io::BufReader::new(stdout);
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > MAX_PLUGIN_PAYLOAD_BYTES {
                        stdout_capped_writer.store(true, Ordering::Release);
                        return (buf, true);
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        (buf, false)
    });

    let stderr_task = tokio::spawn(async move {
        let mut ring: VecDeque<u8> = VecDeque::with_capacity(4 * 1024);
        let mut chunk = [0u8; 8 * 1024];
        loop {
            match stderr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if n >= MAX_STDERR_TAIL_BYTES {
                        ring.clear();
                        ring.extend(&chunk[n - MAX_STDERR_TAIL_BYTES..n]);
                    } else {
                        let overflow = (ring.len() + n).saturating_sub(MAX_STDERR_TAIL_BYTES);
                        if overflow > 0 {
                            ring.drain(..overflow);
                        }
                        ring.extend(&chunk[..n]);
                    }
                }
            }
        }
        Vec::from(ring)
    });

    let mut guard = PluginInvocationGuard {
        pg_guard: crate::platform::ProcessGroupGuard::new(child.id()),
        child: Some(child),
        stdin_task: Some(stdin_task),
        stdout_task: Some(stdout_task),
        stderr_task: Some(stderr_task),
    };

    // 超时硬熔断：整个进程组 SIGKILL（03-plugin-spi-and-degradation.md §8.3）
    let wait_result = match guard.child.as_mut() {
        Some(child_ref) => tokio::time::timeout(timeout, child_ref.wait()).await,
        None => unreachable!(),
    };
    let duration_us = started.elapsed().as_micros() as u64;

    let status = match wait_result {
        Err(_elapsed) => {
            guard.pg_guard.kill_now();
            if let Some(mut child_owned) = guard.child.take() {
                let _ = child_owned.start_kill();
                let _ = tokio::time::timeout(Duration::from_millis(300), child_owned.wait()).await;
            }
            let grace = Duration::from_millis(200);
            if let Some(h) = guard.stdin_task.take() {
                h.abort();
            }
            if let Some(h) = guard.stdout_task.take() {
                let _ = join_with_grace(h, grace).await;
            }
            let stderr_bytes = match guard.stderr_task.take() {
                Some(h) => join_with_grace(h, grace).await.unwrap_or_default(),
                None => Vec::new(),
            };
            return Ok(PluginInvocationOutcome {
                output: None,
                failure: Some(PluginFailureKind::Timeout),
                duration_us,
                stderr_tail: sanitize_plugin_stderr(&stderr_bytes),
            });
        }
        Ok(Err(e)) => {
            return Err(crate::RupostError::PluginError(format!(
                "进程等待失败: {e}"
            )));
        }
        Ok(Ok(status)) => {
            guard.pg_guard.disarm();
            guard.child = None;
            status
        }
    };

    let stdin_res = match guard.stdin_task.take() {
        Some(h) => h
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(format!("stdin task panicked: {e}")))),
        None => Ok(()),
    };
    let (stdout_bytes, task_capped) = match guard.stdout_task.take() {
        Some(h) => h.await.unwrap_or((Vec::new(), false)),
        None => (Vec::new(), false),
    };
    let stderr_bytes = match guard.stderr_task.take() {
        Some(h) => h.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let stderr_tail = sanitize_plugin_stderr(&stderr_bytes);

    if let Err(e) = stdin_res {
        return Ok(PluginInvocationOutcome {
            output: None,
            failure: Some(PluginFailureKind::InvalidOutput(format!(
                "stdin 写入失败: {e}"
            ))),
            duration_us,
            stderr_tail,
        });
    }

    if stdout_capped_flag.load(Ordering::Acquire) || task_capped {
        return Ok(PluginInvocationOutcome {
            output: None,
            failure: Some(PluginFailureKind::InvalidOutput(
                "stdout 超过 10MB 载荷上限".to_string(),
            )),
            duration_us,
            stderr_tail,
        });
    }

    let (output, failure) = match status.code() {
        Some(code) if code != 0 => (None, Some(classify_nonzero_exit(code, &stderr_tail))),
        Some(_) => match parse_plugin_output(&stdout_bytes) {
            Ok(val) => (Some(val), None),
            Err(kind) => (None, Some(kind)),
        },
        None => (None, Some(crashed_failure(status))),
    };

    Ok(PluginInvocationOutcome {
        output,
        failure,
        duration_us,
        stderr_tail,
    })
}

fn crashed_failure(status: std::process::ExitStatus) -> PluginFailureKind {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal().unwrap_or(0);
        PluginFailureKind::Crashed(format!("SIG{signal}"))
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        PluginFailureKind::Crashed("terminated".to_string())
    }
}

fn sanitize_extracted_token(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '\'' | '"' | '`' | '‘' | '’' | '“' | '”'))
        .trim();
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn infer_missing_interpreter(executable: &std::path::Path) -> String {
    if let Some(spec) = parse_shebang(executable) {
        return spec.display_interpreter;
    }
    executable
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(sanitize_extracted_token)
        .unwrap_or_else(|| executable.display().to_string())
}

fn is_local_module_path(module: &str) -> bool {
    module.starts_with("./")
        || module.starts_with("../")
        || module.starts_with(".\\")
        || module.starts_with("..\\")
}

fn extract_after_marker(line: &str, marker: &str) -> Option<String> {
    let idx = line.find(marker)?;
    let rest = line[idx + marker.len()..].trim_start();
    let token = if let Some(quote) = rest.chars().next().filter(|&c| c == '\'' || c == '"') {
        rest[1..].split(quote).next().unwrap_or("")
    } else {
        rest.split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(['.', ',', ':', ';', ')'])
    };
    sanitize_extracted_token(token)
}

enum DepPatternKind {
    Python,
    Node,
}

struct DepPattern {
    needle: &'static str,
    extractor_marker: &'static str,
    kind: DepPatternKind,
}

const DEPENDENCY_MISSING_PATTERNS: &[DepPattern] = &[
    DepPattern {
        needle: "ModuleNotFoundError:",
        extractor_marker: "No module named ",
        kind: DepPatternKind::Python,
    },
    DepPattern {
        needle: "ImportError:",
        extractor_marker: "No module named ",
        kind: DepPatternKind::Python,
    },
    DepPattern {
        needle: "Cannot find module ",
        extractor_marker: "Cannot find module ",
        kind: DepPatternKind::Node,
    },
    DepPattern {
        needle: "Cannot find package ",
        extractor_marker: "Cannot find package ",
        kind: DepPatternKind::Node,
    },
];

fn classify_nonzero_exit(code: i32, stderr_tail: &str) -> PluginFailureKind {
    if code == 127 {
        for line in stderr_tail.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("env:")
                && (lower.contains("no such file") || lower.contains("not found"))
                && let Some(idx) = lower.find("env:")
            {
                let after = &line[idx + 4..];
                let raw_interp = after.split(':').next().unwrap_or("");
                let interpreter =
                    sanitize_extracted_token(raw_interp).unwrap_or_else(|| "unknown".to_string());
                let detail: String = line.trim().chars().take(200).collect();
                return PluginFailureKind::RuntimeMissing {
                    interpreter,
                    detail,
                };
            }
        }
    }

    for line in stderr_tail.lines() {
        let trimmed = line.trim();
        for pat in DEPENDENCY_MISSING_PATTERNS {
            if trimmed.contains(pat.needle) {
                let extracted = extract_after_marker(trimmed, pat.extractor_marker);
                if matches!(pat.kind, DepPatternKind::Node)
                    && let Some(ref m) = extracted
                    && is_local_module_path(m)
                {
                    return PluginFailureKind::ExitNonZero(code, stderr_tail.to_string());
                }
                let detail: String = trimmed.chars().take(200).collect();
                return PluginFailureKind::DependencyMissing {
                    module: extracted,
                    detail,
                };
            }
        }
        if trimmed.contains("ERR_MODULE_NOT_FOUND") {
            let detail: String = trimmed.chars().take(200).collect();
            return PluginFailureKind::DependencyMissing {
                module: None,
                detail,
            };
        }
    }

    PluginFailureKind::ExitNonZero(code, stderr_tail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shebang_line_variants() {
        let env_py = parse_shebang_line("#!/usr/bin/env python3\r").unwrap();
        assert_eq!(env_py.program, "python3");
        assert_eq!(env_py.display_interpreter, "python3");
        assert!(env_py.args.is_empty());

        let env_s = parse_shebang_line("#!/usr/bin/env -S node --no-warnings").unwrap();
        assert_eq!(env_s.program, "node");
        assert_eq!(env_s.display_interpreter, "node");
        assert_eq!(env_s.args, vec!["--no-warnings"]);

        let explicit = parse_shebang_line("#!/nonexistent/bin/fake_python_xyz -u").unwrap();
        assert_eq!(explicit.program, "/nonexistent/bin/fake_python_xyz");
        assert_eq!(explicit.display_interpreter, "fake_python_xyz");
        assert_eq!(explicit.args, vec!["-u"]);

        assert!(parse_shebang_line("MZ\u{0090}\x00not a shebang").is_none());
    }

    #[test]
    fn test_resolve_shebang_for_bad_exe_format_distinguishes_non_shebang_missing_and_valid() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let fake_node = bin_dir.join(format!("node{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&fake_node, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_node, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path_env = std::env::join_paths([&bin_dir]).unwrap();
        let dummy_err = std::io::Error::from_raw_os_error(WINDOWS_ERROR_BAD_EXE_FORMAT);

        // 1. 非 Shebang 损坏文件：返回 Err(None)（透传硬启动失败，不伪装成 RuntimeMissing）
        let corrupt_bin = dir.path().join("corrupt.exe");
        std::fs::write(&corrupt_bin, b"MZ-broken").unwrap();
        assert_eq!(
            resolve_shebang_for_bad_exe_format(
                &corrupt_bin,
                Some(path_env.as_os_str()),
                &dummy_err
            ),
            Err(None)
        );

        // 2. 显式不存在解释器路径的 Shebang：映射为 RuntimeMissing（绝不回退 PATH）
        let bad_shebang = dir.path().join("bad_shebang.py");
        std::fs::write(
            &bad_shebang,
            "#!/nonexistent/bin/fake_python_xyz\nprint('{}')\n",
        )
        .unwrap();
        assert!(matches!(
            resolve_shebang_for_bad_exe_format(
                &bad_shebang,
                Some(path_env.as_os_str()),
                &dummy_err
            ),
            Err(Some(PluginFailureKind::RuntimeMissing { ref interpreter, .. }))
                if interpreter == "fake_python_xyz"
        ));

        // 3. `#!/usr/bin/env -S node --no-warnings` 且 PATH 命中 node：返回解析后的解释器与参数
        let node_script = dir.path().join("plugin_node");
        std::fs::write(&node_script, "#!/usr/bin/env -S node --no-warnings\n").unwrap();
        let (resolved, spec) = resolve_shebang_for_bad_exe_format(
            &node_script,
            Some(path_env.as_os_str()),
            &dummy_err,
        )
        .expect("env shebang in PATH must resolve");
        assert_eq!(resolved, fake_node);
        assert_eq!(spec.args, vec!["--no-warnings"]);
    }
}
