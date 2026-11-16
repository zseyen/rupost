//! WP-P2b 守护：--contract-baseline 首跑建基线、复跑报漂移并更新基线；
//! 基线文件 schema 精确（无 actual 值）。
mod common;

use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

/// 反假守护：把 contract.rs 的 diff_contracts 临时改为恒返回 default(),
/// 第二段"契约漂移"断言必须稳定变红。
#[tokio::test]
async fn test_contract_baseline_creates_then_reports_drift() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 ### / 2 assert / 3 GET（id 稳定为 ok.http#L3）
    std::fs::write(
        dir.path().join("ok.http"),
        format!("### ok\n# @assert status == 200\nGET {}/ok\n", server.uri()),
    )
    .unwrap();

    // 首跑：创建基线
    let out = run(
        dir.path(),
        history.path(),
        &["t", "ok.http", "--contract-baseline", "base.json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("契约基线已创建"), "{stderr}");

    // 基线 schema 精确：顶层两键、条目三键、无 actual
    let raw = std::fs::read(dir.path().join("base.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 2, "{obj:?}");
    assert!(
        obj.contains_key("schema_version") && obj.contains_key("contracts"),
        "{obj:?}"
    );
    assert_eq!(v["schema_version"], "1.0");
    let entry = v["contracts"][0].as_object().unwrap();
    assert_eq!(entry.len(), 3);
    assert!(
        entry.contains_key("id") && entry.contains_key("assertion") && entry.contains_key("passed"),
        "{entry:?}"
    );
    assert_eq!(v["contracts"][0]["id"], "ok.http#L3");
    assert_eq!(v["contracts"][0]["passed"], true);

    // 复跑：同文件名重写为 500 端点（id 保持 ok.http#L3 不变）→ 漂移可见 + 基线更新
    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server2)
        .await;
    std::fs::write(
        dir.path().join("ok.http"),
        format!(
            "### ok\n# @assert status == 200\nGET {}/ok\n",
            server2.uri()
        ),
    )
    .unwrap();
    let out = run(
        dir.path(),
        history.path(),
        &["t", "ok.http", "--contract-baseline", "base.json"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("契约漂移"), "复跑必须报告漂移: {stderr}");
    let raw = std::fs::read(dir.path().join("base.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v["contracts"][0]["passed"], false, "基线必须随复跑更新");
}
