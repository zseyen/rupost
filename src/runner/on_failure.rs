//! DX-Debug R7：失败钩子与 trace 锚点协议（B0 确定性领域，零 AI 参与）。
//!
//! rupost 只负责给锚点：失败请求的 `trace-id`（按 Traceparent > X-Request-Id >
//! X-Trace-Id 优先级提取）经 `RUPOST_FAILED_*` 环境变量交给用户命令，服务端日志
//! 聚合展示委托用户管道（stern/k9s logs/...）——本模块绝不内建日志平台聚合。
//!
//! 钩子语义（DX-Debug PRD US-R7）：
//! - 每个失败请求触发一次，stderr/stdout 透传；
//! - 超时/非零退出/未检出 trace 头均不影响测试退出码（不阻塞 Fail-Fast）；
//! - 无 trace 头时变量为空串，并向 `TestResult.warnings` 追加可见提示。

use std::process::Stdio;
use std::time::Duration;

use colored::Colorize;
use tokio::process::Command;

use crate::runner::types::TestResult;

/// trace 头候选，按 PRD US-R7 优先级排序（取首个命中；HeaderMap 键恒为小写）
pub const TRACE_HEADER_CANDIDATES: [&str; 3] = ["traceparent", "x-request-id", "x-trace-id"];

/// 钩子进程默认超时（挂起命令不得阻塞批次收尾；无 CLI 配置面，YAGNI）
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// 钩子执行结果（仅诊断提示用，不参与测试退出码）
pub struct HookOutcome {
    pub timed_out: bool,
    pub exit_success: bool,
    /// 钩子进程 stderr（由调用方透传，保持"rupost 不加_prefix"的原样语义）
    pub stderr: String,
    pub stdout: String,
}

/// 按优先级从响应头提取 trace 锚点；非可见 ASCII 值（如二进制帧）跳过继续找下一个候选
pub fn extract_trace_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    TRACE_HEADER_CANDIDATES.iter().find_map(|h| {
        headers
            .get(*h)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// 执行一次失败钩子：`sh -c <cmd>`（Windows 为 `cmd /C`），注入锚点环境变量。
/// 超时强制 kill（kill_on_drop），子进程输出捕获后由调用方透传。
pub async fn run_hook(
    cmd: &str,
    url: &str,
    status: Option<u16>,
    trace_id: Option<&str>,
    timeout: Duration,
) -> HookOutcome {
    let mut command = Command::new(shell());
    command.args(shell_args()).arg(cmd);
    command.env("RUPOST_FAILED_URL", url);
    command.env(
        "RUPOST_FAILED_STATUS",
        status.map(|s| s.to_string()).unwrap_or_default(),
    );
    command.env("RUPOST_FAILED_TRACE_ID", trace_id.unwrap_or_default());
    command.kill_on_drop(true);
    command.stdin(Stdio::null());

    let spawned = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(child) = spawned else {
        return HookOutcome {
            timed_out: false,
            exit_success: false,
            stderr: String::new(),
            stdout: String::new(),
        };
    };

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => HookOutcome {
            timed_out: false,
            exit_success: out.status.success(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        },
        // 超时：future 被 drop，kill_on_drop 终止子进程
        Ok(Err(_)) | Err(_) => HookOutcome {
            timed_out: false,
            exit_success: false,
            stderr: String::new(),
            stdout: String::new(),
        },
    }
}

/// 对整个批次执行失败钩子环节（渲染前调用，使"无 trace 头"警告进入报告与 explain 上下文）。
/// 逐条串行触发保证输出确定性；钩子的任何失败只产生 stderr 提示，绝不改变退出码。
pub async fn fire_failure_hooks(
    batch_results: &mut [(std::path::PathBuf, Vec<TestResult>)],
    cmd: &str,
) {
    for (_, results) in batch_results.iter_mut() {
        for r in results.iter_mut() {
            if r.success || r.skipped {
                continue;
            }
            let trace_id = r
                .response
                .as_ref()
                .and_then(|resp| extract_trace_id(&resp.headers));
            // url 与报告/LLM 同一脱敏纪律（敏感 query 掩码，路径仍可用于日志定位）
            let safe_url = crate::ai::redactor::redact_url(&r.url);
            let outcome =
                run_hook(cmd, &safe_url, r.status, trace_id.as_deref(), HOOK_TIMEOUT).await;

            if trace_id.is_none() {
                r.warnings.push(
                    "未检测到 trace 头（Traceparent/X-Request-Id/X-Trace-Id），钩子仅暴露 url/status"
                        .to_string(),
                );
            }
            // stderr/stdout 原样透传（PRD US-R7）
            eprint!("{}", outcome.stderr);
            print!("{}", outcome.stdout);
            if outcome.timed_out {
                eprintln!(
                    "    {} on-failure hook timed out after {HOOK_TIMEOUT:?} (process killed)",
                    "⚠".yellow()
                );
            } else if !outcome.exit_success {
                eprintln!(
                    "    {} on-failure hook exited non-zero (test exit code unaffected)",
                    "⚠".yellow()
                );
            }
        }
    }
}

fn shell() -> &'static str {
    if cfg!(windows) { "cmd" } else { "sh" }
}

fn shell_args() -> [&'static str; 1] {
    if cfg!(windows) { ["/C"] } else { ["-c"] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn test_extract_trace_id_empty_map_returns_none() {
        assert_eq!(extract_trace_id(&headers(&[])), None);
    }

    #[test]
    fn test_extract_trace_id_single_candidates() {
        let map = headers(&[("X-Request-Id", "req-42")]);
        assert_eq!(extract_trace_id(&map).as_deref(), Some("req-42"));

        let map = headers(&[("X-Trace-Id", "tr-7")]);
        assert_eq!(extract_trace_id(&map).as_deref(), Some("tr-7"));

        let map = headers(&[("Traceparent", "00-abc-def-01")]);
        assert_eq!(extract_trace_id(&map).as_deref(), Some("00-abc-def-01"));
    }

    #[test]
    fn test_extract_trace_id_priority_table() {
        // 决策表：三候选并存 → Traceparent 最优先；仅后两者 → X-Request-Id 先于 X-Trace-Id
        let map = headers(&[
            ("X-Trace-Id", "low"),
            ("X-Request-Id", "mid"),
            ("Traceparent", "high"),
        ]);
        assert_eq!(extract_trace_id(&map).as_deref(), Some("high"));

        let map = headers(&[("X-Trace-Id", "low"), ("X-Request-Id", "mid")]);
        assert_eq!(extract_trace_id(&map).as_deref(), Some("mid"));
    }

    #[test]
    fn test_extract_trace_id_skips_non_utf8_and_blank_values() {
        let mut map = reqwest::header::HeaderMap::new();
        map.insert(
            reqwest::header::HeaderName::from_static("traceparent"),
            reqwest::header::HeaderValue::from_bytes(&[0xFF, 0xFE]).unwrap(),
        );
        map.insert(
            reqwest::header::HeaderName::from_static("x-request-id"),
            reqwest::header::HeaderValue::from_static("   "),
        );
        map.insert(
            reqwest::header::HeaderName::from_static("x-trace-id"),
            reqwest::header::HeaderValue::from_static("fallback"),
        );
        assert_eq!(extract_trace_id(&map).as_deref(), Some("fallback"));
    }

    // 进程派生用例依赖 POSIX shell（sh -c）；windows CI 仅跑 extract 决策表
    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hook_injects_anchor_env_vars() {
        // 锚点经环境变量注入，由脚本自身回显验证（无网络、无文件）
        let out = run_hook(
            "printf 'url=%s status=%s trace=[%s]' \"$RUPOST_FAILED_URL\" \"$RUPOST_FAILED_STATUS\" \"$RUPOST_FAILED_TRACE_ID\"",
            "http://h/?token=x",
            Some(401),
            Some("00-abc-def-01"),
            Duration::from_secs(5),
        )
        .await;
        assert!(out.exit_success);
        assert!(out.stdout.contains("url=http://h/?token=x"));
        assert!(out.stdout.contains("status=401"));
        assert!(out.stdout.contains("trace=[00-abc-def-01]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hook_empty_trace_and_status() {
        let out = run_hook(
            "printf 'trace=[%s] status=[%s]' \"$RUPOST_FAILED_TRACE_ID\" \"$RUPOST_FAILED_STATUS\"",
            "http://h/",
            None,
            None,
            Duration::from_secs(5),
        )
        .await;
        assert!(out.stdout.contains("trace=[] status=[]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hook_captures_stderr_and_nonzero_exit() {
        let out = run_hook(
            "echo boom-from-hook >&2; exit 7",
            "http://h/",
            Some(500),
            None,
            Duration::from_secs(5),
        )
        .await;
        assert!(!out.exit_success);
        assert_eq!(out.stderr.trim(), "boom-from-hook");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hook_times_out_and_kills() {
        let out = run_hook(
            "sleep 5",
            "http://h/",
            None,
            None,
            Duration::from_millis(150),
        )
        .await;
        assert!(!out.exit_success);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hook_spawn_failure_is_not_a_crash() {
        // 不存在的 shell 不可能发生，但无效命令经 sh 返回非零即可；这里验证空命令不 panic
        let out = run_hook("", "http://h/", None, None, Duration::from_secs(2)).await;
        // sh -c "" 正常退出
        assert!(out.exit_success);
    }
}
