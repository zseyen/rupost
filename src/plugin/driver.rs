//! Stdio IPC 驱动器（03 §8/§10 唯一权威实现）：
//! - 异步双工：stdin 写入与 stdout/stderr 排水拆独立并发 Task（大包防死锁）
//! - 载荷硬上限 10MB（出站先验校验 + 入站 `MAX_PLUGIN_PAYLOAD_BYTES` 限读）
//! - stdin 写毕显式 `shutdown()` + drop 发送 EOF
//! - 超时进程组 SIGKILL（Unix PGID / Windows kill_on_drop 兜底）
//! - 时延隔离：`plugin_duration_us` 单独计时，绝不混入物理网络秒表

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use super::protocol::{
    MAX_PLUGIN_PAYLOAD_BYTES, PluginFailureKind, PluginInvocationOutcome, parse_plugin_output,
};
use crate::Result;

const MAX_STDERR_TAIL_BYTES: usize = 64 * 1024;

/// 以独立进程组启动插件子进程（03 §10.2 进程树清剿）
#[cfg(unix)]
fn configure_process_group(std_cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    std_cmd.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_std_cmd: &mut std::process::Command) {}

/// 杀死整个进程组（Unix PGID 组级 SIGKILL；Windows 由 kill_on_drop 兜底）
#[cfg(unix)]
fn kill_process_group(child: &mut tokio::process::Child) {
    if let Some(id) = child.id().filter(|&id| id > 0) {
        unsafe {
            libc::kill(-(id as i32), libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
}

/// 取消/早退/异常路径清理守卫：确保子进程组与三个并发 IO 任务绝不泄漏
struct PluginInvocationGuard {
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
        if let Some(mut child) = self.child.take() {
            kill_process_group(&mut child);
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
    let mut cmd = tokio::process::Command::new(executable);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(cmd.as_std_mut());

    // 环境白名单（03 §6.2）：仅 allowlist + RUPOST_* + requires_env/--plugin-env 显式放行
    cmd.env_clear();
    for key in super::types::PLUGIN_ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    for (key, value) in std::env::vars().filter(|(k, _)| k.starts_with("RUPOST_")) {
        cmd.env(key, value);
    }
    for (key, value) in env_overrides {
        cmd.env(key, value);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let interpreter = infer_missing_interpreter(executable);
            return Ok(PluginInvocationOutcome {
                output: None,
                failure: Some(PluginFailureKind::RuntimeMissing {
                    interpreter,
                    detail: format!("启动失败（ENOENT）: {e}"),
                }),
                duration_us: started.elapsed().as_micros() as u64,
                stderr_tail: String::new(),
            });
        }
        Err(e) => {
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
        child: Some(child),
        stdin_task: Some(stdin_task),
        stdout_task: Some(stdout_task),
        stderr_task: Some(stderr_task),
    };

    // 超时硬熔断：整个进程组 SIGKILL（03 §8.3）
    let wait_result = match guard.child.as_mut() {
        Some(child_ref) => tokio::time::timeout(timeout, child_ref.wait()).await,
        None => unreachable!(),
    };
    let duration_us = started.elapsed().as_micros() as u64;

    let status = match wait_result {
        Err(_elapsed) => {
            if let Some(mut child_owned) = guard.child.take() {
                kill_process_group(&mut child_owned);
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
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(executable) {
        let mut buf = [0u8; 256];
        if let Ok(n) = file.read(&mut buf) {
            let head = String::from_utf8_lossy(&buf[..n]);
            if let Some(first_line) = head.lines().next()
                && let Some(rest) = first_line.strip_prefix("#!")
            {
                let mut parts = rest.split_whitespace();
                if let Some(first) = parts.next() {
                    let is_env = first == "env" || first.ends_with("/env");
                    let candidate = if is_env {
                        parts.find(|p| !p.starts_with('-')).unwrap_or(first)
                    } else {
                        std::path::Path::new(first)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(first)
                    };
                    if let Some(tok) = sanitize_extracted_token(candidate) {
                        return tok;
                    }
                }
            }
        }
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
