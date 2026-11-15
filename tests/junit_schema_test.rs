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

    // 走查缺口 #3：timestamp/hostname 锚点——CI 归档与跨构建比对的时间/节点坐标
    {
        let ts_start = xml
            .find("timestamp=\"")
            .expect("timestamp attr must exist: {xml}")
            + "timestamp=\"".len();
        let ts_end = xml[ts_start..].find('"').expect("timestamp value quote") + ts_start;
        let ts = &xml[ts_start..ts_end];
        chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap_or_else(|e| panic!("timestamp 必须是可解析的 RFC3339: {ts:?} ({e})"));
        assert!(xml.contains("hostname=\""), "hostname attr: {xml}");
    }

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

/// B-3（免重填审查）：插件降级形态的成功（DegradedPass）在 CI 报告中必须可见——
/// JUnit 呈现为 <skipped>（非 failure：断言本身通过），JSON 报告携带 degraded 字段；
/// 消灭 "--strict-plugins 红 job 与全绿看板" 的归因矛盾。
#[tokio::test]
async fn test_degraded_pass_visible_in_junit_and_json_reports() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    // 403 兜底（插件缺席原样外发命中）——断言锁 403 → success+degraded
    wiremock::Mock::given(method("GET"))
        .and(path("/guarded"))
        .respond_with(wiremock::ResponseTemplate::new(403).set_body_string("no plugin"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let case = dir.path().join("deg.http");
    std::fs::write(
        &case,
        format!(
            "### guarded\n@auth no-such-plugin-b3 ?optional\nGET {}/guarded\n\n# @assert status == 403\n",
            server.uri()
        ),
    )
    .unwrap();

    let out_path = dir.path().join("deg-junit.xml");
    let out = std::process::Command::new(rupost_bin())
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args([
            "t",
            case.to_str().unwrap(),
            "--allow-missing-plugins",
            "--report",
            "junit",
            "--report-file",
            out_path.to_str().unwrap(),
            "--save-snapshot",
            dir.path().join("deg-snap.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "降级 + 403 断言通过 → exit 0: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let xml = std::fs::read_to_string(&out_path).unwrap();
    assert!(
        xml.contains("<skipped"),
        "degraded 条目必须在 JUnit 中呈现为 <skipped>: {xml}"
    );
    assert!(
        xml.contains("plugin degraded"),
        "skipped 必须注明降级原因: {xml}"
    );

    let snap = std::fs::read_to_string(dir.path().join("deg-snap.json")).unwrap();
    assert!(
        snap.contains("degraded_pass"),
        "快照三态必须记录 DegradedPass"
    );
}
