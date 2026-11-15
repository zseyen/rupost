//! M6-7 端到端故障归因流水线测试 (Suite 3)：
//! 无插件调试态运行 `--debug --save-snapshot` -> 命中延迟 504 mock 与 `@sign ?optional` 降级 ->
//! 校验 `ExecutionSnapshotFile` v1.0 契约 -> 离线运行 `rupost ai triage --snapshot <file> --json` ->
//! 验证零密钥泄露、响应体片段聚类、脱敏提示、TTFB 诊断与 exit 0。

use std::process::Command;
use std::time::Duration;

use rupost::history::model::{ExecutionOutcome, MaskingMode};
use rupost::history::read_execution_snapshot;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_e2e_suite3_debug_snapshot_degraded_504_to_offline_ai_triage() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gateway-timeout"))
        .respond_with(
            ResponseTemplate::new(504)
                .set_delay(Duration::from_millis(550))
                .set_body_json(serde_json::json!({
                    "error": "upstream gateway timeout",
                    "leaked_token": "Bearer super_secret_e2e_token_xyz"
                })),
        )
        .expect(1..=2)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();

    let http_file = dir.path().join("suite3.http");
    std::fs::write(
        &http_file,
        format!(
            "### Delayed Gateway Timeout\n# @sign missing_signer ?optional\nGET {}/gateway-timeout\nAuthorization: Bearer req_secret_token_abc\n",
            server.uri()
        ),
    )
    .unwrap();
    let snap_path = dir.path().join("suite3.snapshot.json");

    // 1. 运行 `rupost test --debug --allow-missing-plugins --save-snapshot`（测试预期 504 失败 -> exit 1）
    let test_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args([
            "test",
            http_file.to_str().unwrap(),
            "--debug",
            "--allow-missing-plugins",
            "--save-snapshot",
            snap_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        test_out.status.code(),
        Some(1),
        "504 失败套件应返回 exit 1: stdout={} stderr={}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
    assert!(snap_path.exists(), "快照文件必须已生成");

    // 2. 校验 ExecutionSnapshotFile v1.0 契约
    let snap = read_execution_snapshot(&snap_path).unwrap();
    assert_eq!(snap.schema_version, "1.0");
    assert_eq!(snap.masking, MaskingMode::None);
    assert_eq!(snap.entries.len(), 1);
    let entry = &snap.entries[0];
    assert_eq!(entry.status, Some(504));
    assert!(!entry.success);
    assert_eq!(entry.outcome, ExecutionOutcome::Fail);
    assert!(
        entry
            .warnings
            .iter()
            .any(|w| w.contains("missing_signer") || w.contains("降级")),
        "@sign ?optional 缺失必须在 warnings 中留下可见降级诊断: {:?}",
        entry.warnings
    );
    let ttfb_us = entry
        .timing_waterfall
        .as_ref()
        .expect("debug 模式必须记录 timing_waterfall")
        .ttfb_us;
    assert!(
        ttfb_us >= 500_000,
        "延迟 550ms 响应的 ttfb_us 应 >= 500_000，实际 {ttfb_us}"
    );

    // 3. 离线运行 `rupost ai triage --snapshot <file> --json`
    let triage_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RUPOST_AI_BASE_URL")
        .env_remove("RUPOST_AI_MODEL")
        .env("RUPOST_AI_PROBE_URL", "http://127.0.0.1:1")
        .args([
            "ai",
            "triage",
            "--snapshot",
            snap_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        triage_out.status.code(),
        Some(0),
        "ai triage --snapshot 必须 exit 0: stderr={}",
        String::from_utf8_lossy(&triage_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&triage_out.stdout);
    let stderr = String::from_utf8_lossy(&triage_out.stderr);
    assert!(
        !stdout.contains("super_secret_e2e_token_xyz")
            && !stderr.contains("super_secret_e2e_token_xyz"),
        "triage 输出不得泄露响应体密钥: stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("req_secret_token_abc") && !stderr.contains("req_secret_token_abc"),
        "triage 输出不得泄露请求头密钥: stdout={stdout} stderr={stderr}"
    );

    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("triage 必须输出合法 JSON");
    assert_eq!(report["failed_requests"], 1);
    assert_eq!(report["cluster_count"], 1);
    assert_eq!(report["clusters"].as_array().map(|c| c.len()), Some(1));
    assert!(
        report["clusters"][0]["title"]
            .as_str()
            .unwrap_or("")
            .contains("504"),
        "必须包含 504 归因聚类: {report}"
    );
    let adapted = rupost::ai::snapshot_adapter::snapshot_to_json_reports(&snap);
    let snippet = adapted[0].results[0]
        .response_body_snippet
        .as_deref()
        .unwrap_or("");
    assert!(
        snippet.contains("upstream gateway timeout"),
        "适配器必须提取 response_body_snippet: {snippet}"
    );
    assert!(
        !snippet.contains("super_secret_e2e_token_xyz"),
        "response_body_snippet 必须已脱敏: {snippet}"
    );
    assert!(
        stderr.contains("masking=None"),
        "stderr 应包含快照脱敏形态指纹: {stderr}"
    );

    server.verify().await;
}
