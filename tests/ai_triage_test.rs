//! `rupost ai triage` / `test --explain` (E5/E2) 端到端集成测试。
//!
//! 离线 dogfooding 闭环：`rupost mock` 充当 OpenAI 兼容后端与被测 401 服务，
//! 全程零真实 API Key；报告消费走真实 `--report-file` 产物（P-4 路径）。

use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

/// 后台拉起 mock 服务进程（同时充当被测 401 服务与 OpenAI 兼容 AI 后端）
fn spawn_mock(config_path: &str, port: u16) -> std::process::Child {
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

/// 运行 `rupost` 子命令，显式清除宿主机 AI 环境变量（CR-6）
fn run_rupost_clean_env(args: &[&str]) -> (i32, String) {
    let output = Command::new(rupost_bin())
        .args(args)
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RUPOST_AI_BASE_URL")
        .env_remove("RUPOST_AI_MODEL")
        // B4 上线后无配置会探测本地 Ollama；测试注入死地址保证确定性
        .env("RUPOST_AI_PROBE_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to run rupost");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

/// 内存构造的最小失败报告（两个同签名 401 失败）
fn sample_failed_reports() -> Vec<rupost::reporter::JsonFileReport> {
    vec![rupost::reporter::JsonFileReport {
        file_path: "auth_suite.http".to_string(),
        results: vec![
            rupost::reporter::JsonTestReport {
                request_number: 1,
                name: Some("GetProfile".to_string()),
                method: "GET".to_string(),
                url: "http://127.0.0.1:18080/v1/users/10".to_string(),
                status: Some(401),
                duration_ms: 5,
                success: false,
                error: None,
                skipped: false,
                assertions: vec![rupost::reporter::JsonAssertionReport {
                    raw: "status == 200".to_string(),
                    passed: false,
                    message: Some("Status mismatch".to_string()),
                    expected: Some("200".to_string()),
                    actual: Some("401".to_string()),
                    stream_event_index: None,
                }],
                warnings: vec![],
                diagnose_report: None,
                response_body_snippet: Some(r#"{"error":"unauthorized"}"#.to_string()),
            },
            rupost::reporter::JsonTestReport {
                request_number: 2,
                name: Some("UpdateProfile".to_string()),
                method: "POST".to_string(),
                url: "http://127.0.0.1:18080/v1/users/20".to_string(),
                status: Some(401),
                duration_ms: 6,
                success: false,
                error: None,
                skipped: false,
                assertions: vec![rupost::reporter::JsonAssertionReport {
                    raw: "status == 200".to_string(),
                    passed: false,
                    message: Some("Status mismatch".to_string()),
                    expected: Some("200".to_string()),
                    actual: Some("401".to_string()),
                    stream_event_index: None,
                }],
                warnings: vec![],
                diagnose_report: None,
                response_body_snippet: Some(r#"{"error":"unauthorized"}"#.to_string()),
            },
        ],
    }]
}

#[tokio::test]
async fn test_deterministic_triage_groups_two_401_requests_into_single_cluster() {
    let reports = sample_failed_reports();
    // 显式传入 None，100% 免疫宿主机环境变量干扰（CR-6）
    let triage = rupost::ai::triage::build_triage_report_with_client(&reports, None).await;
    assert!(!triage.ai_enhanced);
    assert_eq!(triage.total_requests, 2);
    assert_eq!(triage.failed_requests, 2);
    assert_eq!(triage.cluster_count, 1);
    assert_eq!(triage.clusters[0].count, 2);

    let md = rupost::ai::triage::format_triage_markdown(&triage);
    assert!(md.contains("Auth"));
    assert!(md.contains("auth_suite.http#1"));
    assert!(md.contains("auth_suite.http#2"));
}

#[tokio::test]
async fn test_ai_triage_with_mock_server_enhances_clusters() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": r#"[{"category":"Auth","title":"Expired Session Token","bucket_ids":[0],"root_cause":"All 401 endpoints failed due to expired JWT","fix_suggestion":"Run login.http first"}]"#
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = rupost_ai::OpenAiClient::new(
        "mock-key".to_string(),
        Some(format!("{}/v1", server.uri())),
        Some("mock-model".to_string()),
    );

    let reports = sample_failed_reports();
    let triage: rupost_ai::TriageReport =
        rupost::ai::triage::build_triage_report_with_client(&reports, Some(&client)).await;
    assert!(triage.ai_enhanced);
    assert_eq!(triage.cluster_count, 1);
    assert_eq!(triage.clusters[0].title, "Expired Session Token");
    assert_eq!(triage.clusters[0].count, 2);
}

/// E2（GAP-4）：`--explain` 无 Key 时输出规则版基线、无 AI 段落、退出码 1
#[test]
fn test_explain_without_key_prints_rule_baseline_and_exits_1() {
    let port = 9009; // 与 ai_assert_test 的 9002 隔离（cargo 并行运行不同测试二进制）
    let mut child = spawn_mock("examples/ai_triage/mock_ai.json", port);
    assert!(wait_for_port(port), "mock server did not start on {port}");

    let (code, output) = run_rupost_clean_env(&[
        "test",
        "examples/ai_triage/failing_suite.http",
        "--var",
        &format!("base_url=http://127.0.0.1:{port}"),
        "--explain",
    ]);
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        code, 1,
        "失败测试必须以退出码 1 结束（AI 不改变判定）: {output}"
    );
    assert!(
        output.contains("[Possible Causes]"),
        "规则版基线必须输出: {output}"
    );
    assert!(
        !output.contains("[AI Explanation]"),
        "无 Key 时不得出现 AI 归因段落: {output}"
    );
    assert!(
        output.contains("AI 归因已跳过"),
        "PRD US1 要求输出无 Key 黄色提示: {output}"
    );
    assert!(
        output.contains("401") || output.contains("鉴权"),
        "规则版应给出 401 归因: {output}"
    );
}

/// E5 dogfooding 闭环（GAP-5）：真实 test --report-file → ai triage 消费产物
#[test]
fn test_report_file_then_ai_triage_dogfooding_loop() {
    let port = 9004; // 每用例独立端口，避免并行 harness 争抢
    let mut child = spawn_mock("examples/ai_triage/mock_ai.json", port);
    assert!(wait_for_port(port), "mock server did not start on {port}");

    let report_path = std::env::temp_dir().join(format!(
        "rupost_triage_dogfood_report_{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&report_path);

    // 1. 真实测试产生真实失败与真实报告（exit 1 由失败断言决定）
    let (test_code, test_out) = run_rupost_clean_env(&[
        "test",
        "examples/ai_triage/failing_suite.http",
        "--var",
        &format!("base_url=http://127.0.0.1:{port}"),
        "--report-file",
        report_path.to_str().expect("utf8 path"),
    ]);
    assert_eq!(test_code, 1, "测试应失败: {test_out}");
    assert!(report_path.exists(), "--report-file 应落盘报告");

    // 2. ai triage 消费真实报告（无 Key → 确定性降级，exit 0）
    let (triage_code, triage_out) = run_rupost_clean_env(&[
        "ai",
        "triage",
        report_path.to_str().expect("utf8 path"),
        "--json",
    ]);
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(triage_code, 0, "B1 降级路径必须 exit 0: {triage_out}");
    assert!(
        triage_out.contains("\"category\": \"Auth\"")
            || triage_out.contains("\"category\":\"Auth\""),
        "分诊应识别 Auth 根因簇: {triage_out}"
    );
    assert!(triage_out.contains("\"ai_enhanced\": false"));
    // 失败请求的敏感字段不得泄漏到分诊输出
    assert!(
        !triage_out.contains("expired-jwt-should-not-leak"),
        "响应片段必须已脱敏: {triage_out}"
    );
    let _ = std::fs::remove_file(&report_path);
}

/// E2 签名去重（GAP-2/DoD）：同签名失败只消耗一次 AI 调用（wiremock expect(1) + verify 锁定）
#[tokio::test]
async fn test_explain_signature_dedup_calls_ai_at_most_once_per_signature() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // 1. AI 后端：精确期望恰好 1 次 chat completion 调用
    let ai_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": r#"{"root_cause":"JWT expired","fix_suggestion":"re-login"}"#
                }
            }]
        })))
        .expect(1)
        .mount(&ai_server)
        .await;

    // 2. 被测 401 服务（rupost mock，两个请求同签名）
    // 端口 9010：同一测试二进制内与 :161 的 9009 mock 并行隔离（cargo 对同二进制
    // 的多个测试默认多线程并行，复用端口会因 bind 失败被对方 mock 满足而随机互踩）
    let port = 9010;
    let mut child = spawn_mock("examples/ai_triage/mock_ai.json", port);
    assert!(wait_for_port(port), "mock server did not start on {port}");

    let suite =
        std::env::temp_dir().join(format!("rupost_dedup_suite_{}.http", std::process::id()));
    std::fs::write(
        &suite,
        format!(
            "### r1\nGET http://127.0.0.1:{port}/v1/users/1\n\n# @assert status == 200\n\n### r2\nGET http://127.0.0.1:{port}/v1/users/2\n\n# @assert status == 200\n"
        ),
    )
    .expect("write fixture");

    // 3. 指向 wiremock 的 AI 配置 + 两个同签名失败
    let output = Command::new(rupost_bin())
        .args(["test", suite.to_str().expect("utf8"), "--explain"])
        .env("RUPOST_AI_BASE_URL", format!("{}/v1", ai_server.uri()))
        .env("RUPOST_AI_API_KEY", "mock-key")
        .output()
        .expect("failed to run rupost test --explain");
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&suite);

    // wiremock 在 drop 时验证 expect(1)：若去重失效（调用 2 次）此处 panic
    drop(ai_server);

    assert_eq!(output.status.code(), Some(1));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 两个失败都打印规则版；AI 段落恰好出现一次（首次），第二次被去重跳过
    assert_eq!(combined.matches("[AI Explanation]").count(), 1);
    assert_eq!(combined.matches("[Possible Causes]").count(), 2);
}
