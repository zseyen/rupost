//! Step 2 TDD 守护与迁移测试：Stdio IPC 驱动载荷上限、单 JSON、超时清剿、stderr 排水与环境变量白名单。

use rupost::plugin::pipeline::MAX_PLUGIN_PAYLOAD_BYTES;
use rupost::plugin::{PluginFailureKind, invoke_plugin};
use std::time::Duration;

fn mock_plugin(dir: &tempfile::TempDir, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, body).unwrap();
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
async fn test_invoke_rejects_oversized_stdin_payload_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = mock_plugin(
        &dir,
        "ok.sh",
        "#!/bin/sh\necho '{\"protocol_version\":\"1.0\"}'\n",
    );
    let oversized = "x".repeat(MAX_PLUGIN_PAYLOAD_BYTES + 1024);
    let outcome = invoke_plugin(&plugin, &oversized, Duration::from_secs(5), &[])
        .await
        .unwrap();
    assert!(
        matches!(outcome.failure, Some(PluginFailureKind::InvalidOutput(_))),
        "超过 10MB 输入载荷必须显式返回 InvalidOutput: {:?}",
        outcome.failure
    );
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
echo "debug: hello"
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
        std::env::set_var("AWS_SECRET_ACCESS_KEY_DRIVER_TEST", "must-not-leak");
    }
    let plugin = mock_plugin(
        &dir,
        "env.sh",
        "#!/bin/sh\ncat > /dev/null\nif [ -n \"$AWS_SECRET_ACCESS_KEY_DRIVER_TEST\" ]; then echo 'leaked' >&2; exit 1; fi\necho '{\"protocol_version\":\"1.0\"}'\n",
    );
    let outcome = invoke_plugin(&plugin, payload().as_str(), Duration::from_secs(5), &[])
        .await
        .unwrap();
    unsafe {
        std::env::remove_var("AWS_SECRET_ACCESS_KEY_DRIVER_TEST");
    }
    assert!(outcome.failure.is_none(), "父级凭据不得进入插件环境");
}
