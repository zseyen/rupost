//! `rupost ai bootstrap` 端到端集成测试：离线 dogfooding 闭环。
//!
//! 历史经 `RUPOST_HISTORY_DIR` 注入（Rust 侧构造条目后序列化，格式精确）；
//! AI 后端为 wiremock；`RUPOST_AI_PROBE_URL` 死口保证无 Key 路径确定性。

use std::process::Command;

use chrono::Utc;
use reqwest::header::HeaderMap;
use rupost::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

/// 运行 `ai bootstrap`：注入历史目录 + 清除宿主机 AI 变量 + 追加自定义环境
fn run_bootstrap(
    history_dir: &std::path::Path,
    extra_env: &[(&str, String)],
    args: &[&str],
) -> (i32, String) {
    let mut cmd = Command::new(rupost_bin());
    cmd.args(["ai", "bootstrap"])
        .args(args)
        .env("RUPOST_HISTORY_DIR", history_dir)
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RUPOST_AI_BASE_URL")
        .env_remove("RUPOST_AI_MODEL")
        // 测试钩子：默认探测死口，保证无配置路径确定性
        .env("RUPOST_AI_PROBE_URL", "http://127.0.0.1:1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run rupost ai bootstrap");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

/// 构造两条历史（GET 200，响应含 token 字段）
fn history_jsonl() -> String {
    let make = |id: &str, num: u32| {
        let entry = HistoryEntry {
            id: id.to_string(),
            timestamp: Utc::now(),
            duration_ms: 10,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: format!("http://127.0.0.1:9008/v1/users/{num}"),
                headers: HeaderMap::new(),
                body: None,
                timeout: None,
            },
            source: Some("test".to_string()),
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
                body: Some(r#"{"token":"t","role":"admin"}"#.to_string()),
            },
        };
        serde_json::to_string(&entry).expect("serialize history entry")
    };
    format!("{}\n{}\n", make("h1", 1), make("h2", 2))
}

/// B1 降级：AI 不可用时输出机械基线（status 断言 + @name），exit 0
#[test]
fn test_bootstrap_no_ai_emits_mechanical_baseline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_dir = dir.path().join("hist");
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::write(history_dir.join("history.jsonl"), history_jsonl()).unwrap();

    let out = dir.path().join("baseline.http");
    let (code, output) = run_bootstrap(
        &history_dir,
        &[],
        &["--last", "10", "-o", out.to_str().unwrap(), "--no-ai"],
    );

    assert_eq!(code, 0, "B1 降级必须 exit 0: {output}");
    assert!(out.exists(), "基线文件应落盘");
    let content = std::fs::read_to_string(&out).unwrap();
    assert!(content.contains("GET http://127.0.0.1:9008/v1/users/1"));
    assert!(content.contains("# @assert status == 200"));
    assert!(content.contains("# @name "), "机械基线应含 @name");
    assert!(output.contains("机械还原"), "{output}");
}

/// 自足闭环（GAP-5 同型）：AI 建议过 lint 合并进产物（capture + AI 断言）
#[tokio::test]
async fn test_bootstrap_ai_enhanced_merges_linted_advice() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let dir = tempfile::tempdir().expect("tempdir");
    let history_dir = dir.path().join("hist");
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::write(history_dir.join("history.jsonl"), history_jsonl()).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": r#"[{"index":0,"assertions":["body.token exists"],"captures":["token from body.token"]},{"index":1,"assertions":["body.token exists"]}]"#
                }
            }]
        })))
        .mount(&server)
        .await;

    let out = dir.path().join("enhanced.http");
    let (code, output) = run_bootstrap(
        &history_dir,
        &[
            ("RUPOST_AI_BASE_URL", format!("{}/v1", server.uri())),
            ("RUPOST_AI_API_KEY", "mock-key".to_string()),
        ],
        &["--last", "10", "-o", out.to_str().unwrap()],
    );

    assert_eq!(code, 0, "output: {output}");
    let content = std::fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("# @assert body.token exists"),
        "AI 断言应合并: {content}"
    );
    assert!(
        content.contains("# @assert status == 200"),
        "机械断言应保留: {content}"
    );
    assert!(
        content.contains("# @capture token from body.token"),
        "capture 应合并: {content}"
    );
}

/// 防覆盖：输出已存在 → 拒绝且原文件不变
#[test]
fn test_bootstrap_refuses_overwrite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_dir = dir.path().join("hist");
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::write(history_dir.join("history.jsonl"), history_jsonl()).unwrap();
    let out = dir.path().join("exists.http");
    std::fs::write(&out, "original").expect("seed output");

    let (code, output) = run_bootstrap(
        &history_dir,
        &[],
        &["--last", "10", "-o", out.to_str().unwrap(), "--no-ai"],
    );
    assert_ne!(code, 0, "防覆盖应失败: {output}");
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "original",
        "原文件不得变化"
    );
}

/// 参数校验：--last 0 → 报错含 --last
#[test]
fn test_bootstrap_rejects_invalid_last() {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_dir = dir.path().join("hist");
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::write(history_dir.join("history.jsonl"), "").unwrap();
    let out = dir.path().join("x.http");

    let (code, output) = run_bootstrap(
        &history_dir,
        &[],
        &["--last", "0", "-o", out.to_str().unwrap(), "--no-ai"],
    );
    assert_ne!(code, 0, "{output}");
    assert!(output.contains("--last"), "{output}");
}
