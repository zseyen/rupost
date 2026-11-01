//! `rupost ai resolve` 集成测试：`@ai-assert` 原地展开（B2：AI 失败文件原样）。

use std::process::Command;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn run_resolve(
    file: &std::path::Path,
    extra_env: &[(&str, String)],
    dry_run: bool,
) -> (i32, String) {
    let mut cmd = Command::new(rupost_bin());
    cmd.args(["ai", "resolve", file.to_str().expect("utf8")]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("RUPOST_AI_BASE_URL")
        .env_remove("RUPOST_AI_MODEL")
        .env("RUPOST_AI_PROBE_URL", "http://127.0.0.1:1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run rupost ai resolve");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

fn write_fixture(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        "### login\nPOST {{base_url}}/auth/login\n\n# @ai-assert 登录成功且返回管理员\n",
    )
    .expect("write fixture");
    path
}

const AI_ADVICE: &str = r#"["status == 200", "body.role == \"admin\"", "body.token exists"]"#;

/// 正常展开：AI 返回合法断言 → 原地替换为 @assert 行（保留原指令注释形态）
#[tokio::test]
async fn test_resolve_expands_directive_in_place() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": AI_ADVICE}}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let file = write_fixture(dir.path(), "login.http");
    let (code, output) = run_resolve(
        &file,
        &[
            ("RUPOST_AI_BASE_URL", format!("{}/v1", server.uri())),
            ("RUPOST_AI_API_KEY", "k".to_string()),
        ],
        false,
    );

    assert_eq!(code, 0, "{output}");
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("# @assert status == 200"), "{content}");
    assert!(
        content.contains(r#"# @assert body.role == "admin""#),
        "{content}"
    );
    assert!(content.contains("# @assert body.token exists"), "{content}");
    assert!(
        !content.contains("@ai-assert"),
        "指令应被展开替换: {content}"
    );
}

/// dry-run：不落盘、文件原样
#[tokio::test]
async fn test_resolve_dry_run_leaves_file_untouched() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": AI_ADVICE}}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let file = write_fixture(dir.path(), "login.http");
    let original = std::fs::read_to_string(&file).unwrap();

    let (code, output) = run_resolve(
        &file,
        &[
            ("RUPOST_AI_BASE_URL", format!("{}/v1", server.uri())),
            ("RUPOST_AI_API_KEY", "k".to_string()),
        ],
        true,
    );
    assert_eq!(code, 0, "{output}");
    assert!(output.contains("dry-run"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        original,
        "dry-run 不得落盘"
    );
}

/// B2：AI 全局失败 → 文件原样 + 指引 + exit 1
#[tokio::test]
async fn test_resolve_ai_failure_leaves_file_untouched() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let file = write_fixture(dir.path(), "login.http");
    let original = std::fs::read_to_string(&file).unwrap();

    let (code, output) = run_resolve(
        &file,
        &[
            ("RUPOST_AI_BASE_URL", format!("{}/v1", server.uri())),
            ("RUPOST_AI_API_KEY", "k".to_string()),
        ],
        false,
    );
    assert_eq!(code, 1, "B2 全局失败必须 exit 1: {output}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        original,
        "AI 失败文件必须原样"
    );
    assert!(
        output.contains("文件未修改"),
        "错误应说明文件未修改: {output}"
    );
}

/// 无 Key → 指引 + exit 1（B2）
#[test]
fn test_resolve_without_key_prints_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_fixture(dir.path(), "login.http");

    let (code, output) = run_resolve(&file, &[], false);
    assert_eq!(code, 1);
    assert!(output.contains("未配置 AI"), "{output}");
}

/// 部分成功（GAP P1-1 回归锁定）：第一条展开成功、第二条 AI 失败 → exit 0 +
/// 成功断言落盘 + 失败行保留原指令与警告
#[tokio::test]
async fn test_resolve_partial_failure_exits_zero_with_both_effects() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    // 单 mock + 响应闭包按请求体分流（挂载顺序无关，条件互斥）
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|req: &wiremock::Request| {
            let body = String::from_utf8_lossy(&req.body).to_string();
            let hit = body.contains("登录成功");
            if hit {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": r#"["status == 200", "body.role == \"admin\"", "body.token exists"]"#}}]
                }))
            } else {
                ResponseTemplate::new(500)
            }
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mixed.http");
    std::fs::write(
        &file,
        "### login\nPOST {{base_url}}/login\n\n# @ai-assert 登录成功\n\n### order\nPOST {{base_url}}/order\n\n# @ai-assert 订单失败\n",
    )
    .expect("write fixture");

    let (code, output) = run_resolve(
        &file,
        &[
            ("RUPOST_AI_BASE_URL", format!("{}/v1", server.uri())),
            ("RUPOST_AI_API_KEY", "k".to_string()),
        ],
        false,
    );
    assert_eq!(code, 0, "部分成功必须 exit 0: {output}");
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("# @assert status == 200"),
        "成功断言应落盘: {content}"
    );
    assert!(
        content.contains("# @ai-assert 订单失败"),
        "失败指令应保留: {content}"
    );
    assert!(content.contains("展开失败"), "应有行内警告: {content}");
}
