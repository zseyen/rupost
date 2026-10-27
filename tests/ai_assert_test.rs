//! `rupost ai assert` 端到端集成测试。
//!
//! 离线 dogfooding 闭环：用 `rupost mock` 自身充当 OpenAI 兼容后端，
//! 全程不需要真实 API Key。

use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

/// 后台拉起 mock 服务进程
fn spawn_mock(config_path: &str, port: u16) -> Child {
    Command::new(rupost_bin())
        .args(["mock", config_path, "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn rupost mock")
}

/// 轮询端口直至 mock 服务就绪
fn wait_for_port(port: u16) -> bool {
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 运行 `rupost ai assert`，显式隔离宿主机的 AI 环境变量
fn run_ai_assert(base_port: Option<u16>, api_key: Option<&str>, text: &str) -> (i32, String) {
    let mut cmd = Command::new(rupost_bin());
    cmd.args(["ai", "assert", text])
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RUPOST_AI_BASE_URL")
        .env_remove("RUPOST_AI_MODEL");
    if let Some(port) = base_port {
        cmd.env("RUPOST_AI_BASE_URL", format!("http://127.0.0.1:{port}/v1"));
    }
    if let Some(key) = api_key {
        cmd.env("RUPOST_AI_API_KEY", key);
    }
    let output = cmd.output().expect("failed to run rupost ai assert");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

#[test]
fn test_ai_assert_offline_dogfooding_loop() {
    let port = 19110;
    let mut child = spawn_mock("examples/ai_assert/mock_ai.json", port);
    assert!(wait_for_port(port), "mock server did not start on {port}");

    let (code, output) = run_ai_assert(Some(port), Some("demo"), "登录成功并且响应里包含token");
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(code, 0, "output: {output}");
    assert!(
        output.contains("# @assert status == 200"),
        "stdout should contain status assertion, got: {output}"
    );
    assert!(
        output.contains("# @assert body.token exists"),
        "stdout should contain token assertion, got: {output}"
    );
    // 密钥与 prompt 原文不得出现在输出中
    assert!(!output.contains("demo-key"), "output must not leak api key");
}

#[test]
fn test_ai_assert_missing_key_fails_with_guidance() {
    let (code, output) = run_ai_assert(None, None, "status is 200");

    assert_eq!(code, 1, "output: {output}");
    assert!(
        output.contains("RUPOST_AI_API_KEY"),
        "error should contain executable guidance, got: {output}"
    );
}

#[test]
fn test_ai_assert_backend_401_reports_status() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let config = temp.path().join("mock_401.json");
    std::fs::write(
        &config,
        r#"[{
        "method": "POST",
        "path": "/v1/chat/completions",
        "variants": [{
            "condition": null,
            "status": 401,
            "headers": {"Content-Type": "application/json"},
            "response_body": "{\"error\":\"invalid api key\"}"
        }]
    }]"#,
    )
    .expect("write mock config");

    let port = 19111;
    let mut child = spawn_mock(config.to_str().expect("utf8 path"), port);
    assert!(wait_for_port(port), "mock server did not start on {port}");

    let (code, output) = run_ai_assert(Some(port), Some("demo"), "status is 200");
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(code, 1, "output: {output}");
    assert!(
        output.contains("401"),
        "error should contain the backend status code, got: {output}"
    );
}
