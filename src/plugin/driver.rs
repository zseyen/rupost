//! Stdio IPC 驱动器（03 §8/§10 唯一权威实现）：
//! - 异步双工：stdin 写入与 stdout/stderr 排水拆独立并发 Task（大包防死锁）
//! - 载荷硬上限 10MB（出站先验校验 + 入站 `MAX_PLUGIN_PAYLOAD_BYTES` 限读）
//! - stdin 写毕显式 `shutdown()` + drop 发送 EOF
//! - 超时进程组 SIGKILL（Unix PGID / Windows kill_on_drop 兜底）
//! - 时延隔离：`plugin_duration_us` 单独计时，绝不混入物理网络秒表

use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use super::protocol::{
    MAX_PLUGIN_PAYLOAD_BYTES, PluginFailureKind, PluginInvocationOutcome, parse_plugin_output,
};
use crate::Result;

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
    if let Some(id) = child.id() {
        unsafe {
            libc::kill(-(id as i32), libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
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

    let mut child = cmd.spawn().map_err(|e| {
        crate::RupostError::PluginError(format!("插件 {} 启动失败: {e}", executable.display()))
    })?;
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

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(8 * 1024);
        let mut chunk = [0u8; 16 * 1024];
        let mut reader = tokio::io::BufReader::new(stdout);
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > MAX_PLUGIN_PAYLOAD_BYTES {
                        return (buf, true);
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        (buf, false)
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(4 * 1024);
        let mut chunk = [0u8; 8 * 1024];
        loop {
            match stderr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() < 64 * 1024 {
                        buf.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
        buf
    });

    // 超时硬熔断：整个进程组 SIGKILL（03 §8.3）
    let wait_result = tokio::time::timeout(timeout, child.wait()).await;
    let duration_us = started.elapsed().as_micros() as u64;

    let status = match wait_result {
        Err(_elapsed) => {
            kill_process_group(&mut child);
            let _ = tokio::time::timeout(Duration::from_millis(300), child.wait()).await;
            let grace = Duration::from_millis(200);
            stdin_task.abort();
            let _ = join_with_grace(stdout_task, grace).await;
            let stderr_bytes = join_with_grace(stderr_task, grace)
                .await
                .unwrap_or_default();
            return Ok(PluginInvocationOutcome {
                output: None,
                failure: Some(PluginFailureKind::Timeout),
                duration_us,
                stderr_tail: String::from_utf8_lossy(&stderr_bytes).into_owned(),
            });
        }
        Ok(Err(e)) => {
            return Err(crate::RupostError::PluginError(format!(
                "进程等待失败: {e}"
            )));
        }
        Ok(Ok(status)) => status,
    };

    let stdin_res = stdin_task
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(format!("stdin task panicked: {e}"))));
    let (stdout_bytes, stdout_capped) = stdout_task.await.unwrap_or((Vec::new(), false));
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    let stderr_tail = String::from_utf8_lossy(&stderr_bytes).into_owned();

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

    let (output, failure) = match status.code() {
        Some(code) if code != 0 => (
            None,
            Some(PluginFailureKind::ExitNonZero(code, stderr_tail.clone())),
        ),
        Some(_) if stdout_capped => (
            None,
            Some(PluginFailureKind::InvalidOutput(
                "stdout 超过 10MB 载荷上限".to_string(),
            )),
        ),
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
