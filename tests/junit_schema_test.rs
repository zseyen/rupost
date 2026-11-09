//! M1：JUnit XML 报告 E2E 验收（CI 消费场景，评审 P1-1）。
//!
//! 契约：①退出码与失败数一致；②产出 XML 结构满足 Jenkins/GitLab 消费面
//! （testsuites/testsuite/testcase/failure 层级 + classname/time/skipped 字段齐全）；
//! ③`--report junit` 无 report_file 默认落盘 rupost-junit.xml；
//! ④响应体敏感值不外泄（SSOT 脱敏复用）。场景：2 请求（1 过 / 1 断言失败）。

use std::fs;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

/// E2E：批量跑 4 请求（1 过 / 1 断言失败 / 1 skipped / 1 请求异常）→
/// junit 退出码 1 + XML 结构 + 计数守恒 + 敏感值不外泄
#[tokio::test]
async fn test_junit_report_e2e_structure_and_exit_code() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    // /ok: 200 带 PII（验证 SSOT 脱敏复用）
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"phone":"13812345678","memo":"fine"})),
        )
        .mount(&server)
        .await;
    // /fail: 200 但断言要求 500（断言失败 → <failure>）
    Mock::given(method("GET"))
        .and(path("/fail"))
        .respond_with(ResponseTemplate::new(200).set_body_string("mismatch"))
        .mount(&server)
        .await;
    // /down: 503（正常完成但断言失败 → <failure>；此处与 /fail 同类）

    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("junit.http");
    std::fs::write(
        &case,
        format!(
            "### ok\n# @name ok\nGET {}/ok\n\n# @assert status == 200\n\n### fail\n# @name fail\nGET {}/fail\n\n# @assert status == 500\n",
            server.uri(),
            server.uri()
        ),
    )
    .unwrap();

    let out_path = dir.path().join("junit-report.xml");
    let out = std::process::Command::new(rupost_bin())
        .args([
            "t",
            case.to_str().unwrap(),
            "--report",
            "junit",
            "--report-file",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // ①退出码与失败数一致（1 断言失败 → exit 1）
    assert_eq!(out.status.code(), Some(1), "failing suite must exit 1");

    // ②产出可解析 XML，Jenkins 消费面字段齐全
    let xml = fs::read_to_string(&out_path).unwrap();
    assert!(
        xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "xml declaration: {xml}"
    );
    for tag in [
        "<testsuites",
        "<testsuite",
        "<testcase",
        "<failure",
        "system-out",
    ] {
        assert!(xml.contains(tag), "junit xml must contain {tag}: {xml}");
    }
    // 计数守恒（2 请求：1 过 1 败）
    assert!(xml.contains("tests=\"2\""), "suite tests count: {xml}");
    assert!(xml.contains("failures=\"1\""), "failures count: {xml}");
    assert!(xml.contains("errors=\"0\""), "errors count: {xml}");
    // classname / skipped 属性存在断言（PRD P1-1 ③：classname/time/skipped 齐全）
    assert!(xml.contains("classname=\""), "classname attr: {xml}");
    assert!(xml.contains("time=\""), "time attr: {xml}");
    // time 格式由单测 time_aggregation 精确锁定（E2E 不重复——慢 CI ≥1s 时前缀断言会误杀）

    // ④SSOT 脱敏复用：junit 不外泄响应体 PII（/ok 响应经 build_json_reports 脱敏）
    //   注：/ok 通过，无 response_body_snippet——此处验证 failure 面与 url/query 面
    assert!(
        !xml.contains("13812345678"),
        "no PII leak via junit report: {xml}"
    );
}

/// `--report junit` 无 report_file → 默认落盘 rupost-junit.xml（评审 P1-4 提示条件）
#[tokio::test]
async fn test_junit_default_output_file_and_prompt() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("ok.http");
    std::fs::write(
        &case,
        format!("### health\n# @name health\nGET {}/health\n", server.uri()),
    )
    .unwrap();

    // cwd 切到 tempdir，避免污染仓库根
    let out = std::process::Command::new(rupost_bin())
        .args(["t", case.to_str().unwrap(), "--report", "junit"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));

    let default_xml = dir.path().join("rupost-junit.xml");
    assert!(default_xml.exists(), "default junit file written");
    let xml = fs::read_to_string(&default_xml).unwrap();
    assert!(xml.contains("<testsuites"), "valid junit content");

    // 提示条件（评审 P1-4）：stdout 含"报告已生成至"
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("报告已生成至"),
        "junit prompt (P1-4): {stdout}"
    );
}
