//! 并行/串行模式 × `--cookie-file` 落盘契约（§5.6 缺口）。
//!
//! 锁定的行为契约：
//! 1. 串行模式：cookie 经内存罐跨文件共享，且每个文件执行完后落盘一次
//!    （崩溃丢失窗口 = 单文件，而非整个批次）；
//! 2. 并行模式：worker 执行器均为临时仓库（Ephemeral，Drop 也不落盘），
//!    worker 运行期间获得的新 cookie **绝不**出现在 cookie 文件中——
//!    并发写盘会互相截断，宁可不持久化；主执行器 Drop 兜底只保存主 store。

// 进程级静音历史落盘（tests/common/mod.rs：pre-main 执行，勿删）
mod common;

use rupost::runner::TestExecutor;
use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest};
use rupost::variable::VariableContext;
use std::collections::HashMap;
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_cookie_endpoints(server: &MockServer, cookie_name: &str, cookie_value: &str) {
    Mock::given(method("GET"))
        .and(path("/set"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "Set-Cookie",
                    format!("{cookie_name}={cookie_value}; Path=/"),
                )
                .set_body_string("cookie set"),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("echo"))
        .mount(server)
        .await;
}

fn write_file(dir: &std::path::Path, name: &str, content: String) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// 串行模式：跨文件会话共享 + 逐文件落盘
#[tokio::test]
async fn test_serial_mode_shares_session_and_persists_per_file() {
    let server = MockServer::start().await;
    mount_cookie_endpoints(&server, "serial_sid", "s-123").await;

    let temp = tempfile::TempDir::new().unwrap();
    let cookie_file = temp.path().join("cookies.json");
    let file_a = write_file(
        temp.path(),
        "a.http",
        format!("### Set\nGET {}/set\n", server.uri()),
    );
    let file_b = write_file(
        temp.path(),
        "b.http",
        format!("### Echo\nGET {}/echo\n", server.uri()),
    );

    let mut files_map = HashMap::new();
    let parsed_a = rupost::parser::HttpFileParser::parse_file(&file_a).unwrap();
    let parsed_b = rupost::parser::HttpFileParser::parse_file(&file_b).unwrap();
    files_map.insert(file_a.clone(), parsed_a);
    files_map.insert(file_b.clone(), parsed_b);

    // with_cookies = AutoPersist 模式（Load from file, save on update/drop）
    let executor = TestExecutor::with_cookies(cookie_file.clone()).unwrap();
    let batch = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![file_a.clone(), file_b.clone()],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Serial,
            concurrency: 1,
            fail_fast: false,
        })
        .await
        .unwrap();
    assert!(results.iter().all(|(_, rs)| rs.iter().all(|r| r.success)));

    // 契约 1a：串行会话跨文件共享——文件 B 的请求必须携带文件 A 收到的 cookie
    let received = server.received_requests().await.expect("requests recorded");
    let echo_req = received
        .iter()
        .find(|r| r.url.path() == "/echo")
        .expect("echo request must exist");
    let cookie_header = echo_req
        .headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        cookie_header.contains("serial_sid=s-123"),
        "file B must carry the session cookie, got: {cookie_header:?}"
    );

    // 契约 1b：run 返回时（executor 仍存活）文件已落盘——逐文件持久化，
    // 而非只依赖进程退出时的 Drop 兜底
    assert!(
        cookie_file.exists(),
        "serial mode must persist cookies per file, not only on drop"
    );
    let on_disk = std::fs::read_to_string(&cookie_file).unwrap();
    assert!(
        on_disk.contains("serial_sid"),
        "cookie file must contain the session cookie"
    );
}

/// 并行模式：worker 新 cookie 不落盘；Drop 兜底只保存主 store
#[tokio::test]
async fn test_parallel_mode_never_persists_worker_cookies() {
    let server = MockServer::start().await;
    mount_cookie_endpoints(&server, "unused", "unused").await;
    // 两个文件各拿各的 cookie，互为独立节点（无依赖 → 并行同时跑）
    Mock::given(method("GET"))
        .and(path("/set_a"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "par_a=a-1; Path=/")
                .set_body_string("a"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/set_b"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "par_b=b-2; Path=/")
                .set_body_string("b"),
        )
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    let cookie_file = temp.path().join("cookies.json");
    let file_a = write_file(
        temp.path(),
        "a.http",
        format!("### A\nGET {}/set_a\n", server.uri()),
    );
    let file_b = write_file(
        temp.path(),
        "b.http",
        format!("### B\nGET {}/set_b\n", server.uri()),
    );

    let mut files_map = HashMap::new();
    files_map.insert(
        file_a.clone(),
        rupost::parser::HttpFileParser::parse_file(&file_a).unwrap(),
    );
    files_map.insert(
        file_b.clone(),
        rupost::parser::HttpFileParser::parse_file(&file_b).unwrap(),
    );

    let executor = TestExecutor::with_cookies(cookie_file.clone()).unwrap();
    let batch = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![file_a, file_b],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();
    assert!(results.iter().all(|(_, rs)| rs.iter().all(|r| r.success)));

    // 契约 2a：run 返回时（主执行器仍存活）cookie 文件不存在——
    // 并行分支从不调用 persist_cookies，worker 也不落盘
    assert!(
        !cookie_file.exists(),
        "parallel workers must never persist cookies during the run"
    );

    // 契约 2b：主执行器 Drop 兜底保存的是主 store（未发过请求）——
    // 文件可能出现，但绝不包含 worker 运行期间获得的新 cookie
    drop(batch);
    let on_disk = std::fs::read_to_string(&cookie_file).unwrap_or_default();
    assert!(
        !on_disk.contains("par_a") && !on_disk.contains("par_b"),
        "worker cookies must not leak into the cookie file, got: {on_disk}"
    );
}
