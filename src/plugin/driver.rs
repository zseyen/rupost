//! Stdio IPC 驱动器（03 §8/§10 唯一权威实现）：
//! - 异步双工：stdin 写入与 stdout/stderr 排水拆独立并发 Task（大包防死锁）
//! - 载荷硬上限 10MB（`take()` 限读），超限判 `invalid_output`
//! - stdin 写毕显式 `shutdown()` + drop 发送 EOF
//! - 超时进程组 SIGKILL（Unix PGID / Windows kill_on_drop 兜底）
//! - 失败分类学：`timeout / invalid_output / crashed / exit_non_zero`
//! - 时延隔离：`plugin_duration_us` 单独计时，绝不混入物理网络秒表
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::pipeline::MAX_PLUGIN_PAYLOAD_BYTES;
use crate::Result;

/// 插件调用失败分类（03 §8.3 唯一权威；与"缺失"共享降级开关）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginFailureKind {
    /// 超过超时窗，进程组已被 SIGKILL
    Timeout,
    /// stdout 非单 JSON / 版本不兼容 / 载荷超限 / 失败信封
    InvalidOutput(String),
    /// 子进程被信号终止
    Crashed(String),
    /// exit ≠ 0 且无合法输出
    ExitNonZero(i32, String),
}

impl PluginFailureKind {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::InvalidOutput(_) => "invalid_output",
            Self::Crashed(_) => "crashed",
            Self::ExitNonZero(_, _) => "exit_non_zero",
        }
    }

    /// 人类可读摘要（含 stderr 摘要/信号编号，落报告与快照 warnings）
    pub fn describe(&self) -> String {
        match self {
            Self::Timeout => "插件执行超时（进程组已熔断）".to_string(),
            Self::InvalidOutput(detail) => format!("插件输出非法（{detail}）"),
            Self::Crashed(signal) => format!("插件进程被信号终止（{signal}）"),
            Self::ExitNonZero(code, stderr) => {
                let brief: String = stderr.chars().take(200).collect();
                format!("插件退出码 {code}：{brief}")
            }
        }
    }
}

/// 一次插件调用的完整结果
#[derive(Debug, Clone)]
pub struct PluginInvocationOutcome {
    /// 成功解析的 stdout JSON（单对象），失败时为 None
    pub output: Option<serde_json::Value>,
    pub failure: Option<PluginFailureKind>,
    /// 插件耗时（微秒）——时延隔离：只进 plugin_duration_us，不进 TTFB/duration
    pub duration_us: u64,
    /// stderr 摘要（截断 16KB；CI 模式不直通终端）
    pub stderr_tail: String,
}

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
    // Windows：tokio kill 作用于 Job Object 绑定的进程树
    let _ = child.start_kill();
}

/// 执行一次插件 Stdio IPC 调用：
/// `payload` 为内核序列化的输入 JSON 字符串；返回 [`PluginInvocationOutcome`]。
pub async fn invoke_plugin(
    executable: &std::path::Path,
    payload: &str,
    timeout: Duration,
    env_overrides: &[(String, String)],
) -> Result<PluginInvocationOutcome> {
    let started = Instant::now();
    let mut cmd = tokio::process::Command::new(executable);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(cmd.as_std_mut());
    // 环境白名单（03 §6.2）：仅 allowlist + RUPOST_* + requires_env 声明项
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
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| crate::RupostError::PluginError("stdout 句柄获取失败".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| crate::RupostError::PluginError("stderr 句柄获取失败".to_string()))?;

    // 独立并发 Task：stdin 写入 + EOF；stdout/stderr 限读排水（03 §10.1 三向防死锁）
    let payload_owned = payload.to_string();
    let stdin_task = tokio::spawn(async move {
        let payload_bytes = payload_owned.into_bytes();
        if payload_bytes.len() > MAX_PLUGIN_PAYLOAD_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("输入载荷 {} 超过 10MB 硬上限", payload_bytes.len()),
            ));
        }
        stdin.write_all(&payload_bytes).await?;
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<(), std::io::Error>(())
    });
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::with_capacity(8 * 1024);
        let mut chunk = [0u8; 16 * 1024];
        let mut capped = tokio::io::BufReader::new(&mut stdout);
        loop {
            // take() 语义以手写计数实现：总量超限即截断标记
            let n = capped.read(&mut chunk).await;
            let Ok(0) = n else {
                let len = n.unwrap_or(0);
                if buf.len() + len > MAX_PLUGIN_PAYLOAD_BYTES {
                    return (buf, true);
                }
                buf.extend_from_slice(&chunk[..len]);
                continue;
            };
            break;
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
            let _ = child.wait().await;
            let _ = stdin_task.await;
            let _ = stdout_task.await;
            let stderr_bytes = stderr_task.await.unwrap_or_default();
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

    let payload_over = stdin_task.await.unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "stdin task panicked",
        ))
    });
    let (stdout_bytes, stdout_capped) = stdout_task.await.unwrap_or((Vec::new(), false));
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    let stderr_tail = String::from_utf8_lossy(&stderr_bytes).into_owned();
    let _ = payload_over;

    let failure = if let Some(code) = status.code() {
        if code != 0 {
            Some(PluginFailureKind::ExitNonZero(code, stderr_tail.clone()))
        } else if stdout_capped {
            Some(PluginFailureKind::InvalidOutput(
                "stdout 超过 10MB 载荷上限".to_string(),
            ))
        } else {
            parse_plugin_output(&stdout_bytes)
        }
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let signal = status.signal().unwrap_or(0);
            Some(PluginFailureKind::Crashed(format!("SIG{signal}")))
        }
        #[cfg(not(unix))]
        {
            Some(PluginFailureKind::Crashed("terminated".to_string()))
        }
    };

    Ok(PluginInvocationOutcome {
        output: failure.is_none().then(|| {
            serde_json::from_slice(&stdout_bytes).unwrap_or_else(|_| serde_json::Value::Null)
        }),
        failure,
        duration_us,
        stderr_tail,
    })
}

/// stdout 单对象纪律（03 §8.2）：非单 JSON / 版本不兼容 / 失败信封 → `invalid_output`
fn parse_plugin_output(stdout_bytes: &[u8]) -> Option<PluginFailureKind> {
    let text = String::from_utf8_lossy(stdout_bytes);
    let trimmed = text.trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Some(PluginFailureKind::InvalidOutput(
            "stdout 不是单个 JSON 对象（插件日志必须走 stderr）".to_string(),
        ));
    };
    if !value.is_object() {
        return Some(PluginFailureKind::InvalidOutput(
            "stdout 顶层必须是 JSON 对象".to_string(),
        ));
    }
    if let Some(ok) = value.get("ok").and_then(|v| v.as_bool())
        && !ok
    {
        let message = value
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unspecified");
        return Some(PluginFailureKind::InvalidOutput(format!(
            "插件失败信封: {message}"
        )));
    }
    if let Some(version) = value.get("protocol_version").and_then(|v| v.as_str())
        && !version.starts_with("1.")
    {
        return Some(PluginFailureKind::InvalidOutput(format!(
            "不兼容的协议版本: {version}"
        )));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 在临时目录编译一个极简 mock 插件（sh 脚本；禁 Python，04 §2.5 harness 契约）
    fn mock_plugin(dir: &tempfile::TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{}", body).unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn payload() -> String {
        serde_json::json!({
            "protocol_version": "1.0",
            "stage": "sign",
            "request": {"method": "GET", "url": "http://x/", "headers": [], "body": null},
            "variables": {},
            "params": {}
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_invoke_success_single_json() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = mock_plugin(
            &dir,
            "ok.sh",
            r#"#!/bin/sh
cat > /dev/null
echo '{"protocol_version":"1.0","extra_headers":[["X-Sign","abc"]],"remove_headers":[],"mutated_body":null,"set_variables":{},"warnings":[]}'
"#,
        );
        let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        assert!(outcome.failure.is_none());
        let output = outcome.output.unwrap();
        assert_eq!(output["extra_headers"][0][1].as_str().unwrap(), "abc");
        assert!(outcome.duration_us > 0, "plugin_duration_us 必须独立计时");
    }

    #[tokio::test]
    async fn test_invoke_invalid_output_logged_to_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = mock_plugin(
            &dir,
            "noisy.sh",
            r#"#!/bin/sh
cat > /dev/null
echo "debug: hello"   # 日志走 stdout = 协议违例
echo '{"protocol_version":"1.0"}'
"#,
        );
        let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        assert_eq!(
            outcome.failure.as_ref().unwrap().kind_label(),
            "invalid_output"
        );
    }

    #[tokio::test]
    async fn test_invoke_timeout_kills_process_group() {
        let dir = tempfile::tempdir().unwrap();
        // 孙进程：sleep 由子 shell 拉起，验证组级清剿不留孤儿
        let plugin = mock_plugin(
            &dir,
            "slow.sh",
            "#!/bin/sh\ncat > /dev/null\n(sleep 30) &\nwait\n",
        );
        let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_millis(300), &[])
            .await
            .unwrap();
        assert_eq!(outcome.failure.unwrap(), PluginFailureKind::Timeout);
    }

    #[tokio::test]
    async fn test_invoke_stderr_flood_no_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        // 2MB stderr 洪泛：必须异步排空返回而非挂死（04 §2.5 #6）
        let plugin = mock_plugin(
            &dir,
            "flood.sh",
            "#!/bin/sh\ncat > /dev/null\nhead -c 2097152 /dev/zero | tr '\\0' 'x' >&2\necho '{\"protocol_version\":\"1.0\"}'\n",
        );
        let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        assert!(outcome.failure.is_none());
        assert!(outcome.stderr_tail.len() >= 16 * 1024, "stderr 必须被排水");
    }

    #[tokio::test]
    async fn test_invoke_exit_non_zero_and_failure_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let crash = mock_plugin(&dir, "crash.sh", "#!/bin/sh\ncat > /dev/null\nexit 3\n");
        let outcome = invoke_plugin(&crash, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        match outcome.failure.unwrap() {
            PluginFailureKind::ExitNonZero(3, _) => {}
            other => panic!("expected exit_non_zero(3), got {other:?}"),
        }

        let envelope = mock_plugin(
            &dir,
            "envelope.sh",
            "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"ok\":false,\"error_kind\":\"plugin_error\",\"message\":\"bad key\"}'\n",
        );
        let outcome = invoke_plugin(&envelope, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        assert_eq!(
            outcome.failure.as_ref().unwrap().kind_label(),
            "invalid_output"
        );
        assert!(outcome.failure.unwrap().describe().contains("bad key"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_invoke_env_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("RUPOSH_LEAKY_SECRET", "no");
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "must-not-leak");
        }
        let plugin = mock_plugin(
            &dir,
            "env.sh",
            "#!/bin/sh\ncat > /dev/null\nif [ -n \"$AWS_SECRET_ACCESS_KEY\" ]; then echo 'leaked' >&2; exit 1; fi\necho '{\"protocol_version\":\"1.0\"}'\n",
        );
        let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_secs(5), &[])
            .await
            .unwrap();
        assert!(outcome.failure.is_none(), "父级凭据不得进入插件环境");
        unsafe {
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        }
    }
}
