//! DBG-R7 失败钩子与 trace 锚点协议集成测试（DX-Debug PRD US-R7）。
//!
//! 锁定契约：失败请求触发钩子并注入 `RUPOST_FAILED_URL/STATUS/TRACE_ID` 锚点、
//! 无 trace 头时空串 + warnings 可见、钩子超时/非零退出不影响测试退出码、
//! 成功请求零触发；JSON 报告模式下钩子仍触发且警告进入报告。

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rupost_bin())
        .current_dir(dir)
        .env("RUPOST_HISTORY_DIR", dir.join("history_dir"))
        .args(args)
        .output()
        .expect("Failed to execute rupost binary")
}

/// 单请求用例（断言 status == 200）：`fail` 返回 401，`ok` 返回 200，
/// 可选携带 trace 响应头。返回用例路径与 MockServer（调用方持有以保活）。
async fn setup(
    dir: &Path,
    endpoint: &str,
    status: u16,
    trace_header: Option<&str>,
) -> (String, MockServer) {
    let server = MockServer::start().await;
    let mut tpl = ResponseTemplate::new(status).set_body_string(r#"{"ok":true}"#);
    if let Some(th) = trace_header {
        tpl = tpl.append_header("Traceparent", th);
    }
    Mock::given(method("GET"))
        .and(path(endpoint))
        .respond_with(tpl)
        .mount(&server)
        .await;

    let case = dir.join("case.http");
    fs::write(
        &case,
        format!(
            "### check\n# @name check\nGET {}/api\n\n# @assert status == 200\n",
            server.uri()
        ),
    )
    .unwrap();
    (case.to_string_lossy().into_owned(), server)
}

const TRACE: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

/// US-R7 主链路：失败 + Traceparent 头 → 钩子拿到完整锚点三元组
#[cfg(unix)]
#[tokio::test]
async fn test_hook_receives_trace_anchor_env_vars() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 401, Some(TRACE)).await;
    let out_path = dir.path().join("captured.txt");
    let hook = format!(
        "printf 'url=%s\\nstatus=%s\\ntrace=%s' \"$RUPOST_FAILED_URL\" \"$RUPOST_FAILED_STATUS\" \"$RUPOST_FAILED_TRACE_ID\" > {}",
        out_path.display()
    );

    let out = run(dir.path(), &["t", &case, "--on-failure", &hook]);

    // 测试本身失败（401），退出码由测试决定
    assert_eq!(out.status.code(), Some(1));
    let captured = fs::read_to_string(&out_path).unwrap();
    assert!(
        captured.contains(&format!("trace={TRACE}")),
        "captured: {captured}"
    );
    assert!(captured.contains("status=401"), "captured: {captured}");
    // url 经脱敏纪律（本用例无敏感 query，应原样保留路径）
    assert!(captured.contains("/api"), "captured: {captured}");
}

/// US-R7 And：无 trace 头 → 变量空串 + warnings 可见
#[cfg(unix)]
#[tokio::test]
async fn test_hook_without_trace_headers_warns_and_passes_empty_env() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 500, None).await;
    let out_path = dir.path().join("captured.txt");
    let hook = format!(
        "printf 'trace=[%s]' \"$RUPOST_FAILED_TRACE_ID\" > {}",
        out_path.display()
    );

    let out = run(dir.path(), &["t", &case, "--on-failure", &hook]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&out_path).unwrap(), "trace=[]");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stderr.contains("未检测到 trace 头") || stdout.contains("未检测到 trace 头"),
        "warning not visible; stderr: {stderr}"
    );
}

/// US-R7 And：钩子非零退出/报错 → stderr 透传、批次收尾完整、退出码不被钩子改写
#[cfg(unix)]
#[tokio::test]
async fn test_hook_failure_does_not_affect_exit_code_or_summary() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 401, Some(TRACE)).await;

    let out = run(
        dir.path(),
        &[
            "t",
            &case,
            "--on-failure",
            "echo hook-side-effect >&2; exit 9",
        ],
    );

    // 退出码仍由测试失败决定（1），而非钩子的 9
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("hook-side-effect"),
        "hook stderr not passed through: {stderr}"
    );
    // 批次汇总正常收尾（钩子未中断渲染链）
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("check"), "batch summary missing: {stdout}");
}

/// 零噪音：请求成功时钩子不触发
#[cfg(unix)]
#[tokio::test]
async fn test_hook_not_fired_on_success() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 200, Some(TRACE)).await;
    let out_path = dir.path().join("should-not-exist.txt");
    let hook = format!("touch {}", out_path.display());

    let out = run(dir.path(), &["t", &case, "--on-failure", &hook]);

    assert_eq!(out.status.code(), Some(0));
    assert!(!out_path.exists(), "hook must not fire on success");
}

/// JSON 报告模式下钩子仍触发，且"无 trace 头"警告进入报告 warnings 数组
#[cfg(unix)]
#[tokio::test]
async fn test_hook_fires_in_json_mode_and_warning_lands_in_report() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 500, None).await;
    let out_path = dir.path().join("fired.txt");
    let report_path = dir.path().join("report.json");
    let hook = format!("touch {}", out_path.display());

    let out = run(
        dir.path(),
        &[
            "t",
            &case,
            "--report",
            "json",
            "--report-file",
            report_path.to_str().unwrap(),
            "--on-failure",
            &hook,
        ],
    );

    assert_eq!(out.status.code(), Some(1));
    assert!(out_path.exists(), "hook must fire in json report mode");
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(
        report.contains("未检测到 trace 头"),
        "warning must be visible in json report: {report}"
    );
}

/// JSON 纯净性契约（审查轮 P1 修复）：钩子 stdout/stderr 一律并入 rupost 的
/// stderr——`--report json` 的 stdout 保持机器可读，任何钩子输出不得混入。
#[cfg(unix)]
#[tokio::test]
async fn test_hook_output_never_pollutes_json_stdout() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), "/api", 500, None).await;

    let out = run(
        dir.path(),
        &[
            "t",
            &case,
            "--report",
            "json",
            "--on-failure",
            "echo HOOK-STDOUT-MARKER; echo HOOK-STDERR-MARKER >&2",
        ],
    );

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim_start().starts_with('['),
        "json stdout must start with report array: {stdout}"
    );
    assert!(
        !stdout.contains("HOOK-STDOUT-MARKER") && !stdout.contains("HOOK-STDERR-MARKER"),
        "hook output leaked into json stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HOOK-STDOUT-MARKER") && stderr.contains("HOOK-STDERR-MARKER"),
        "hook output must be transparent on stderr: {stderr}"
    );
}
