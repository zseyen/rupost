//! R-2/R-3（免重填主线）：捕获变量跨运行持久化 e2e 守护。
//!
//! 契约：第一跑 `--var-state` 落盘 @capture 产物；第二跑（不再执行登录）
//! 回灌补缺使下游请求免重填成功；显式 `--var` 恒优先于持久化层。

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir)
        .env("RUPOST_HISTORY_DIR", dir.join("history_dir"))
        .args(args)
        .output()
        .expect("Failed to execute rupost binary")
}

/// R-2 端到端：跑一次登录（落盘）→ 删掉登录文件模拟"第二次运行"→
/// 下游请求仅凭 var-state 回灌的 token 免重填成功。
#[tokio::test]
async fn test_var_state_persists_captured_token_across_runs() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token":"tok-cross"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer tok-cross",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("orders-ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let login = dir.path().join("login.http");
    fs::write(
        &login,
        format!(
            "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
            server.uri()
        ),
    )
    .unwrap();
    let orders = dir.path().join("orders.http");
    fs::write(
        &orders,
        format!(
            "### list\nGET {}/orders\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
    )
    .unwrap();

    // 第一跑：登录 + 捕获落盘
    let out1 = run(
        dir.path(),
        &[
            "t",
            login.to_str().unwrap(),
            "--var-state",
            "state.json",
            "--no-history",
        ],
    );
    assert!(
        out1.status.success(),
        "第一跑（登录）必须成功: stderr={}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let state_raw = fs::read_to_string(dir.path().join("state.json")).unwrap();
    assert!(
        state_raw.contains("tok-cross"),
        "捕获的 token 必须进 var-state 文件: {state_raw}"
    );
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(dir.path().join("state.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "var-state 必须落盘 0600");

    // 第二跑：移除登录文件（模拟不重跑登录），仅靠回灌
    fs::remove_file(&login).unwrap();
    let out2 = run(
        dir.path(),
        &[
            "t",
            orders.to_str().unwrap(),
            "--var-state",
            "state.json",
            "--no-history",
        ],
    );
    assert!(
        out2.status.success(),
        "第二跑必须凭回灌 token 免重填成功: stdout={} stderr={}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out2.stderr).contains("回灌"),
        "回灌必须可见提示"
    );
}

/// R-2 优先级契约：显式 `--var` 恒优先于 var-state 回灌（陈旧 token 不得覆盖新值）。
#[tokio::test]
async fn test_var_state_never_overrides_explicit_var() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/probe"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer explicit",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/probe"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;
    fs::write(
        dir.path().join("state.json"),
        r#"{"auth_token": "stale-from-state"}"#,
    )
    .unwrap();
    let probe = dir.path().join("probe.http");
    fs::write(
        &probe,
        format!(
            "### probe\nGET {}/probe\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = run(
        dir.path(),
        &[
            "t",
            probe.to_str().unwrap(),
            "--var-state",
            "state.json",
            "--var",
            "auth_token=explicit",
            "--no-history",
        ],
    );
    assert!(
        out.status.success(),
        "显式 --var 必须覆盖 var-state 陈旧值: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// R-3（TUI 语义在 lib 层锁定）：响应到达即承接捕获，断言失败不影响变量承接。
#[test]
fn test_capture_merge_survives_failed_assertions() {
    use rupost::tui::state::AppState;
    // 构造：登录请求断言失败（断言写错）但响应到达且捕获了 token
    let mut state = AppState::default();
    let mut captured = std::collections::HashMap::new();
    captured.insert("auth_token".to_string(), "tok-keep".to_string());
    let resp = rupost::http::Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        r#"{"token":"tok-keep"}"#.to_string(),
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(5),
        std::time::Duration::ZERO,
    )
    .unwrap();
    let failed_assertions = vec![rupost::assertion::AssertionResult {
        raw: "status == 999".to_string(),
        passed: false,
        message: Some("expected 999".to_string()),
        expected: "999".to_string(),
        actual: Some("200".to_string()),
        stream_event_index: None,
    }];
    state.handle_request_finished(Ok(resp), captured, failed_assertions, vec![]);
    assert_eq!(
        state.variables.get("auth_token").as_deref(),
        Some("tok-keep"),
        "断言红不得丢弃本次捕获的 token（R-3 收口）"
    );
}
