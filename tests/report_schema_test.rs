//! JSON 报告 schema 契约测试（§5.18 缺口）。
//!
//! 字段清单逐项存在性锁定：CI 消费者（jq/脚本/看板）依赖这些键名，
//! 手写字段清单漏字段、或未来重构悄悄改名/删字段时，测试先行失败。
//! 采用**精确键集合**断言：新增字段也必须显式更新契约（schema 语义）。

// 进程级静音历史落盘（tests/common/mod.rs：pre-main 执行，勿删）
mod common;

use rupost::http::diagnose_url;
use rupost::parser::HttpFileParser;
use rupost::reporter::TestReporter;
use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest, TestExecutor};
use rupost::variable::VariableContext;
use std::collections::HashMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 批量测试 JSON 报告：顶层与逐请求字段的精确键集合
#[tokio::test]
async fn test_batch_json_report_schema_exact_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    let file_a = temp.path().join("a.http");
    // 未定义变量 {{undef_x}} 放在 query：按字面量发送（触发 warnings）且请求正常发出
    std::fs::write(
        &file_a,
        format!(
            "### Req\n@assert status == 200\nGET {}/ok?trace={{{{undef_x}}}}\n",
            server.uri()
        ),
    )
    .unwrap();
    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(file_a.clone(), parsed_a);

    let executor = TestExecutor::new();
    let batch = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![file_a],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Serial,
            concurrency: 1,
            fail_fast: false,
        })
        .await
        .unwrap();

    let mut buf: Vec<u8> = Vec::new();
    TestReporter::report_json(&results, &mut buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&buf).unwrap();

    // 顶层：文件数组
    let top = parsed.as_array().expect("report root must be an array");
    assert_eq!(top.len(), 1);
    let file_obj = &top[0];
    let file_keys: Vec<&str> = file_obj
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        file_keys,
        vec!["file_path", "results"],
        "file-level schema keys changed — update CI consumers or restore the contract"
    );

    // 逐请求字段（精确集合，含 warnings/diagnose_report 两个易漏字段）
    let result_obj = &file_obj["results"][0];
    let result_keys: Vec<&str> = result_obj
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut sorted = result_keys.clone();
    sorted.sort_unstable();
    let mut expected = vec![
        "assertions",
        "diagnose_report",
        "duration_ms",
        "error",
        "method",
        "name",
        "request_number",
        "skipped",
        "status",
        "success",
        "url",
        "warnings",
    ];
    expected.sort_unstable();
    assert_eq!(sorted, expected, "per-request schema keys changed");

    // 字段类型与形态抽样
    assert!(result_obj["success"].is_boolean());
    assert_eq!(result_obj["status"], 200);
    assert_eq!(result_obj["method"], "GET");
    assert!(result_obj["duration_ms"].is_number());
    assert!(
        !result_obj["warnings"].as_array().unwrap().is_empty(),
        "undefined-variable warning must be serialized into the report"
    );

    // 断言子对象字段
    let assertion = &result_obj["assertions"][0];
    let a_keys: Vec<&str> = assertion
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut a_sorted = a_keys.clone();
    a_sorted.sort_unstable();
    assert_eq!(a_sorted, vec!["message", "passed", "raw"]);
}

/// diagnose JSON 报告：顶层 13 字段精确键集合 + http 场景的空值形态
#[tokio::test]
async fn test_diagnose_json_schema_exact_fields() {
    // 本地裸 TCP HTTP 服务：无 TLS、无证书 → 锁定 cert_info/tls 相关字段的空值形态
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        }
    });

    let report = diagnose_url(&format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap();
    server.await.unwrap();

    let value = serde_json::to_value(&report).unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    let mut expected = vec![
        "cert_info",
        "dns_lookup_duration",
        "http_status",
        "http_version",
        "is_https",
        "is_websocket",
        "resolved_ips",
        "tcp_connect_duration",
        "tls_handshake_duration",
        "total_duration",
        "ttfb",
        "url",
        "ws_upgrade_success",
    ];
    expected.sort_unstable();
    assert_eq!(sorted, expected, "diagnose schema keys changed");

    // http 明文场景的空值形态（CI 侧按此判空）
    assert_eq!(value["is_https"], false);
    assert_eq!(value["cert_info"], serde_json::Value::Null);
    assert_eq!(value["tls_handshake_duration"], serde_json::Value::Null);
    assert_eq!(value["http_status"], 200);
    // Duration 序列化为 {secs, nanos} 对象
    let dns = &value["dns_lookup_duration"];
    let dns_keys: Vec<&str> = dns
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(dns_keys, vec!["nanos", "secs"]);
}

/// 失败/错误形态的 schema 一致性：断言败（HTTP 有响应）与连接错误
/// （无响应）两种结果的金字段集合与成功形态完全相同——CI 看板按同一
/// 结构消费，不因结果形态而改变。
#[tokio::test]
async fn test_batch_json_report_schema_failed_result_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    // 已关端口 → 连接拒绝 → error Some、status null
    let dead_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let file_a = temp.path().join("a.http");
    std::fs::write(
        &file_a,
        format!(
            "### AssertFail\n@assert status == 599\nGET {}/ok\n\n### ConnFail\nGET http://127.0.0.1:{dead_port}/\n",
            server.uri()
        ),
    )
    .unwrap();
    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(file_a.clone(), parsed_a);
    let batch = BatchExecutor::new(TestExecutor::new());
    let mut context = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![file_a],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Serial,
            concurrency: 1,
            fail_fast: false,
        })
        .await
        .unwrap();

    let mut buf: Vec<u8> = Vec::new();
    TestReporter::report_json(&results, &mut buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&buf).unwrap();

    let reqs = parsed[0]["results"].as_array().unwrap();
    assert_eq!(reqs.len(), 2);

    // 形态 1：断言败——HTTP 完成但 success false，error 仍为 null
    let assert_fail = &reqs[0];
    assert_eq!(assert_fail["success"], false);
    assert_eq!(assert_fail["status"], 200);
    assert_eq!(assert_fail["error"], serde_json::Value::Null);
    assert_eq!(
        assert_fail["assertions"][0]["passed"], false,
        "failed assertion must be visible in the report"
    );

    // 形态 2：连接错误——status null、error 非 null
    let conn_fail = &reqs[1];
    assert_eq!(conn_fail["success"], false);
    assert_eq!(conn_fail["status"], serde_json::Value::Null);
    assert!(
        conn_fail["error"].is_string(),
        "connection error must surface"
    );

    // 两种失败形态的键集合彼此一致（成功形态由既有测试锁定；同源于
    // reporter.rs 的同一 json! 字面量，此断言防未来按形态拆分序列化路径）
    let keys = |v: &serde_json::Value| {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(assert_fail), keys(conn_fail));
}
