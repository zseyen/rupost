//! 对抗性回归测试
//!
//! 沉淀 2026-10 对抗性审查中实测复现的 bug（见
//! `doc/plans/2026-10-18-quality-hardening-execution-plan.md`）。每个用例注释内嵌
//! 当时的原始复现方式，保证修复可被重放验证。需要真实网络栈的用例用 wiremock 承载。

use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 域：timeout_enforcement
///
/// 原复现：`printf '@timeout 500ms\nGET http://127.0.0.1:18081/slow\n' > t.http`
/// 对 3 秒延迟端点运行 → 修复前耗时 3012ms 且 ✓ 通过（@timeout 被解析但从未应用）。
#[tokio::test]
async fn timeout_is_enforced_for_plain_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_string("slow"),
        )
        .mount(&server)
        .await;

    let content = format!("@timeout 500ms\nGET {}/slow\n", server.uri());
    let parsed = HttpFileParser::parse_content(&content).unwrap();

    let executor = TestExecutor::new();
    let mut ctx = VariableContext::new();
    let start = Instant::now();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 1);
    assert!(!results[0].success, "超时请求必须判定为失败");
    assert!(
        elapsed < Duration::from_millis(2000),
        "@timeout 500ms 应在约 0.5s 内中断，实际耗时 {:?}",
        elapsed
    );
    let err = results[0].error.as_deref().unwrap_or("");
    assert!(
        err.to_lowercase().contains("timed out") || err.contains("超时") || err.contains("timeout"),
        "错误信息应提示超时，实际: {}",
        err
    );
}

/// 域：status_code_edge
///
/// 原复现：本地 Python 服务对 /seven 返回 `HTTP 700` →
/// `panicked at http_runner.rs:159: ParseError("Invalid HTTP status code: 700")`。
#[tokio::test]
async fn nonstandard_status_code_yields_result_not_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/seven"))
        .respond_with(ResponseTemplate::new(700).set_body_string("custom status"))
        .mount(&server)
        .await;

    // 无断言：700 非 2xx，应为失败结果而非 panic
    let content = format!("GET {}/seven\n", server.uri());
    let parsed = HttpFileParser::parse_content(&content).unwrap();
    let executor = TestExecutor::new();
    let mut ctx = VariableContext::new();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, Some(700));
    assert!(!results[0].success);

    // 显式断言 700 应可通过
    let content = format!("GET {}/seven\n@assert status == 700\n", server.uri());
    let parsed = HttpFileParser::parse_content(&content).unwrap();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();
    assert!(results[0].success, "@assert status == 700 应通过");
    assert!(results[0].assertions.iter().all(|a| a.passed));
}

/// 域：header_validation
///
/// 原复现：`printf 'GET http://127.0.0.1:18080/h\nX Bad Name: x\n'` →
/// `panicked at http/request.rs:32: invalid header name`。
#[tokio::test]
async fn invalid_header_name_yields_error_result_not_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let content = format!("GET {}/h\nX Bad Name: x\n", server.uri());
    let parsed = HttpFileParser::parse_content(&content).unwrap();
    let executor = TestExecutor::new();
    let mut ctx = VariableContext::new();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    let err = results[0].error.as_deref().unwrap_or("");
    assert!(
        err.contains("Invalid header name"),
        "应为 header 构建错误，实际: {}",
        err
    );
    // 前缀不应重复（此前二次包装导致 "构建请求失败: 构建请求失败: ..."）
    assert_eq!(err.matches("构建请求失败").count(), 1);
}

/// 域：capture_and_vars
///
/// 原复现：一坏一好两个 @capture → 修复前两个变量全部丢失，两个请求都显示 ✓，
/// 下游 URL 带字面量 `{{good_id}}` 发出。现在：好的生效、坏的进 warnings、
/// 未定义占位符也进 warnings。
#[tokio::test]
async fn capture_failure_is_isolated_and_surfaced_as_warning() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_string(r#"{"id": 42}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let content = format!(
        "@capture bad_var from body.missing.path\n@capture good_id from body.id\nGET {u}/data\n\n###\nGET {u}/echo/{{{{good_id}}}}/{{{{never_defined}}}}\n",
        u = server.uri()
    );
    let parsed = HttpFileParser::parse_content(&content).unwrap();
    let executor = TestExecutor::new();
    let mut ctx = VariableContext::new();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();

    assert_eq!(results.len(), 2);
    // 第一个请求：坏 capture 进 warnings，好 capture 生效
    assert!(
        results[0].warnings.iter().any(|w| w.contains("bad_var")),
        "坏 capture 应出现在 warnings: {:?}",
        results[0].warnings
    );
    assert_eq!(ctx.get("good_id").as_deref(), Some("42"));
    // 第二个请求：good_id 已替换，never_defined 作为未定义变量进 warnings
    assert!(results[1].url.contains("/echo/42/"));
    assert!(
        results[1]
            .warnings
            .iter()
            .any(|w| w.contains("{{never_defined}}")),
        "未定义变量应出现在 warnings: {:?}",
        results[1].warnings
    );
}

/// 域：cookie_hardening
///
/// 原复现：`rupost --cookie-file ck.json --env dev http://.../login`（内联路径此前
/// 绕过 resolve_cookie_path，无视环境隔离读写同一个文件）；cookie 文件此前以
/// 0644 落盘（session 凭据组/其他用户可读）。
#[tokio::test]
async fn inline_request_cookie_file_is_env_isolated_and_0600() {
    use wiremock::matchers::method;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "session=DEV1; Path=/; Max-Age=3600"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cookie_file = dir.path().join("ck.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        // 子进程的请求历史写入临时目录，不污染仓库的 .rupost/
        .env("RUPOST_HISTORY_DIR", dir.path())
        .arg("--cookie-file")
        .arg(&cookie_file)
        .arg("--env")
        .arg("dev")
        .arg(format!("{}/login", server.uri()))
        .output()
        .expect("failed to spawn rupost binary");
    assert!(output.status.success(), "inline request should succeed");

    // 环境隔离：文件名带 _dev 后缀
    let env_file = dir.path().join("ck_dev.json");
    assert!(env_file.exists(), "内联请求 + --env dev 应生成 ck_dev.json");
    let content = std::fs::read_to_string(&env_file).unwrap();
    assert!(content.contains("session=DEV1"));

    // 权限收紧：0600（unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&env_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "cookie 文件权限必须为 0600");
    }
}

/// 域：streaming_protocol
///
/// 原复现（探针实测）：chunked SSE 响应在发出无空行结尾的 `data: tail` 后直接断开，
/// reqwest 报 `Err("error decoding response body")`；修复前 Err 分支直接 break，
/// parser 内尚未完结的尾帧被静默丢弃。wiremock 无法模拟中途断流，故用裸 TcpListener。
#[tokio::test]
async fn sse_stream_error_still_flushes_pending_tail_event() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = conn.read(&mut buf);
        let body = b"id: 1\ndata: hello\n\ndata: tail";
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            body.len()
        );
        conn.write_all(head.as_bytes()).unwrap();
        conn.write_all(body).unwrap();
        conn.write_all(b"\r\n").unwrap();
        conn.flush().unwrap();
        // 不发送终止 chunk（0\r\n\r\n）直接断开 → 客户端进入 Some(Err) 分支
        drop(conn);
    });

    let content = format!("@sse\nGET http://{}/stream\n", addr);
    let parsed = HttpFileParser::parse_content(&content).unwrap();
    let executor = TestExecutor::new();
    let mut ctx = VariableContext::new();
    let results = executor.execute_all(parsed, &mut ctx).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(
        results[0].success,
        "断流不应判定为请求失败: {:?}",
        results[0].error
    );
    // process_event 把每个事件的 data 累积进最终响应 body：尾帧被 flush 处理过的
    // 直接证据就是 body 含 "tail"
    let body = &results[0]
        .response
        .as_ref()
        .expect("should have response")
        .body;
    assert!(body.contains("hello"), "首个完整事件应已处理: {body:?}");
    assert!(
        body.contains("tail"),
        "传输错误后的尾帧必须被 flush 并处理: {body:?}"
    );
}
