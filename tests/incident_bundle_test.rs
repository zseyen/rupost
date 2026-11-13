//! 故障复现包 MVP 守护（差异化执行计划 A3/A4，Phase 1 核心闭环）。
//!
//! 契约：失败运行 `rupost test --incident-out <不存在目录>` 产出三文件工件
//! （manifest.json / report.json / snapshot.json），强制脱敏（无原始凭据）、
//! snapshot 可被既有 `rupost replay` 直接消费；成功运行不建目录；
//! 目标路径已存在或为 symlink 时拒绝（exit 2）且不改动既有内容。
//!
//! 反假守护说明：实现落地后，删除"目标必须不存在"检查会使守护 3/4 变红
//! （目录被工件覆盖）；删除强制脱敏会使守护 1 的无泄漏断言变红。

mod common;

use std::process::{Command, Output};
use tempfile::tempdir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 失败套件 fixture：携带 Bearer 凭据的请求，断言 status == 200 必然失败
const SECRET_TOKEN: &str = "sekrit-payment-token-abc123";

fn write_failing_fixture(dir: &std::path::Path, base: &str) {
    let suite = format!(
        r#"### charge payment
# @assert status == 200
POST {base}/charge
Authorization: Bearer {SECRET_TOKEN}
Content-Type: application/json

{{"amount":100,"currency":"USD"}}
"#
    );
    std::fs::write(dir.join("payments.http"), suite).unwrap();
}

fn write_green_fixture(dir: &std::path::Path, base: &str) {
    let suite = format!(
        r#"### health check
# @assert status == 200
GET {base}/ok
"#
    );
    std::fs::write(dir.join("ok.http"), suite).unwrap();
}

/// 挂载失败链路：精确 Bearer 匹配的 500（body 打标 declined-with-token，
/// 证明 token 确实随请求发出，使无泄漏断言非空转）+ 兜底 500
async fn mount_failing_charge(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/charge"))
        .and(header(
            "Authorization",
            format!("Bearer {SECRET_TOKEN}").as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(500).set_body_string("payment-gateway: declined-with-token"),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/charge"))
        .respond_with(ResponseTemplate::new(500).set_body_string("unauthorized-request"))
        .mount(server)
        .await;
}

fn run_rupost(cwd: &std::path::Path, history: &std::path::Path, args: &[String]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rupost"));
    cmd.env("RUPOST_HISTORY_DIR", history)
        .current_dir(cwd)
        .args(args);
    cmd.output().expect("spawn rupost subprocess")
}

/// 主契约（A3/A4）：失败运行产出三文件复现包——退出 1、manifest 契约字段精确、
/// 任一文件无原始凭据（且请求确实携带过凭据）、snapshot 可被 replay 直接消费。
#[tokio::test]
async fn test_failed_run_emits_redacted_replayable_incident_bundle() {
    let server = MockServer::start().await;
    mount_failing_charge(&server).await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_failing_fixture(workdir.path(), &server.uri());

    let incident_path = artifact_root.path().join("payment-failure");
    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "payments.http".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            SECRET_TOKEN.into(),
        ],
    );

    assert_eq!(
        out.status.code(),
        Some(1),
        "failing suite must exit 1, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 三文件齐全
    assert!(
        incident_path.is_dir(),
        "incident bundle dir must be created"
    );
    let manifest_path = incident_path.join("manifest.json");
    let report_path = incident_path.join("report.json");
    let snapshot_path = incident_path.join("snapshot.json");
    for f in [&manifest_path, &report_path, &snapshot_path] {
        assert!(f.is_file(), "incident artifact missing: {}", f.display());
    }

    // manifest 契约字段精确匹配（Incident schema 1.0；不写绝对工作目录）
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], "1.0", "manifest: {manifest}");
    assert_eq!(manifest["masking"], "secrets", "manifest: {manifest}");
    assert_eq!(
        manifest["artifacts"],
        serde_json::json!(["manifest.json", "report.json", "snapshot.json"]),
        "manifest: {manifest}"
    );
    assert_eq!(manifest["total"], 1, "manifest: {manifest}");
    assert_eq!(manifest["failed"], 1, "manifest: {manifest}");
    assert_eq!(manifest["skipped"], 0, "manifest: {manifest}");
    assert!(manifest["environment"].is_null(), "manifest: {manifest}");
    assert!(manifest["created_at"].is_string(), "manifest: {manifest}");
    assert_eq!(
        manifest["source_paths"],
        serde_json::json!(["payments.http"]),
        "manifest source_paths must be relative, no absolute workdir: {manifest}"
    );
    assert!(
        manifest["max_snapshot_body_bytes"].is_u64(),
        "manifest: {manifest}"
    );

    // 非空转前提：响应片段证明请求确实携带过 token（精确 Bearer mock 命中标记）
    let report_bytes = std::fs::read(&report_path).unwrap();
    let report_str = String::from_utf8_lossy(&report_bytes);
    assert!(
        report_str.contains("declined-with-token"),
        "report must prove the request carried the token (exact-Bearer mock hit): {report_str}"
    );

    // A3：任一工件文件都不得含原始凭据
    for f in [&manifest_path, &report_path, &snapshot_path] {
        let content = std::fs::read(f).unwrap();
        let content = String::from_utf8_lossy(&content);
        assert!(
            !content.contains(SECRET_TOKEN),
            "raw secret leaked into {}: {content}",
            f.display()
        );
    }

    // A4：snapshot 可被既有 rupost replay 直接消费（同类失败：掩码值使精确 mock
    // 不再命中，重放得到同类失败 → exit 1；stdout 必须出现快照加载行）
    let replay = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "r".into(),
            snapshot_path.to_string_lossy().into_owned(),
            "--target".into(),
            server.uri(),
        ],
    );
    let replay_stdout = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay_stdout.contains("Loaded snapshot containing"),
        "replay must consume the incident snapshot directly: {replay_stdout}"
    );
    assert_eq!(
        replay.status.code(),
        Some(1),
        "replay against target must reproduce the same-class failure, stdout: {replay_stdout}"
    );
}

/// 成功对照：全绿运行即使给了 --incident-out 也不创建目标目录（exit 0）。
#[tokio::test]
async fn test_green_run_creates_no_incident_directory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_green_fixture(workdir.path(), &server.uri());

    let incident_path = artifact_root.path().join("should-not-exist");
    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "ok.http".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
        ],
    );

    assert!(
        out.status.success(),
        "green suite must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !incident_path.exists(),
        "green run must not create the incident directory"
    );
}

/// 路径保护：目标目录已存在时命令必须失败（exit 2），且既有内容一个字节都不能动。
#[tokio::test]
async fn test_incident_out_refuses_existing_directory_and_preserves_content() {
    let server = MockServer::start().await;
    mount_failing_charge(&server).await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_failing_fixture(workdir.path(), &server.uri());

    let incident_path = artifact_root.path().join("existing");
    std::fs::create_dir(&incident_path).unwrap();
    let sentinel = incident_path.join("sentinel.txt");
    std::fs::write(&sentinel, "do-not-touch").unwrap();

    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "payments.http".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            SECRET_TOKEN.into(),
        ],
    );

    assert_eq!(
        out.status.code(),
        Some(2),
        "refusal to overwrite an existing incident dir must exit 2 (artifact contract), stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&sentinel).unwrap(),
        "do-not-touch",
        "existing dir content must be preserved byte-for-byte"
    );
    for f in ["manifest.json", "report.json", "snapshot.json"] {
        assert!(
            !incident_path.join(f).exists(),
            "no artifact may be written into the refused target"
        );
    }
}

/// 路径保护（Unix）：目标为符号链接时必须拒绝，链接背后的真实目录不得被改动。
#[cfg(unix)]
#[tokio::test]
async fn test_incident_out_refuses_symlink_target() {
    let server = MockServer::start().await;
    mount_failing_charge(&server).await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_failing_fixture(workdir.path(), &server.uri());

    let real_dir = artifact_root.path().join("real-dir");
    std::fs::create_dir(&real_dir).unwrap();
    let sentinel = real_dir.join("sentinel.txt");
    std::fs::write(&sentinel, "do-not-touch").unwrap();

    let link_path = artifact_root.path().join("incident-link");
    std::os::unix::fs::symlink(&real_dir, &link_path).unwrap();

    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "payments.http".into(),
            "--incident-out".into(),
            link_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            SECRET_TOKEN.into(),
        ],
    );

    assert_eq!(
        out.status.code(),
        Some(2),
        "symlink target must be refused with exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&sentinel).unwrap(),
        "do-not-touch",
        "symlinked real dir content must be preserved"
    );
    for f in ["manifest.json", "report.json", "snapshot.json"] {
        assert!(
            !real_dir.join(f).exists(),
            "no artifact may be written through the symlink"
        );
    }
}

/// 回归（问题 1）：当失败响应体的非敏感键、错误信息或断言实际值中回显了
/// `--mask-secret-value` 指定的凭据时，manifest/report/snapshot 三文件均不得泄漏原文。
#[tokio::test]
async fn test_incident_bundle_redacts_explicit_secret_value_in_report_body_and_assertions() {
    let server = MockServer::start().await;
    let echoed_secret = "custom-opaque-secret-998877";
    Mock::given(method("POST"))
        .and(path("/echo-fail"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_string(format!(r#"{{"gateway_echo":"{echoed_secret}"}}"#)),
        )
        .mount(&server)
        .await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    let suite = format!(
        r#"### echo failure
# @assert status == 200
# @assert body.gateway_echo == "ok"
POST {}/echo-fail
Content-Type: application/json

{{"payload":"{echoed_secret}"}}
"#,
        server.uri()
    );
    std::fs::write(workdir.path().join("echo.http"), suite).unwrap();

    let incident_path = artifact_root.path().join("echo-failure");
    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "echo.http".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            echoed_secret.into(),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "failing suite must exit 1, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for name in ["manifest.json", "report.json", "snapshot.json"] {
        let content = std::fs::read_to_string(incident_path.join(name)).unwrap();
        assert!(
            !content.contains(echoed_secret),
            "explicit secret value leaked into {name}: {content}"
        );
    }
}

/// WP-R3（缺口 3）：--env 必须贯通到 manifest.environment 与 snapshot.environment。
/// 反假守护：把 src/cli/suite_presenter.rs 中传给 create_incident_bundle 的
/// options.env.as_deref() 临时改为 None，本测试必须稳定变红；验证后恢复。
#[tokio::test]
async fn test_incident_manifest_carries_env_from_cli_flag() {
    let server = MockServer::start().await;
    mount_failing_charge(&server).await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_failing_fixture(workdir.path(), &server.uri());
    // --env 需要可解析的环境名：最小 rupost.toml 声明 staging
    std::fs::write(
        workdir.path().join("rupost.toml"),
        format!("[environments.staging]\nbase_url = \"{}\"\n", server.uri()),
    )
    .unwrap();

    let incident_path = artifact_root.path().join("with-env");
    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "payments.http".into(),
            "--env".into(),
            "staging".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            SECRET_TOKEN.into(),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let manifest_bytes = std::fs::read(incident_path.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["environment"], "staging", "manifest: {manifest}");

    let snap_bytes = std::fs::read(incident_path.join("snapshot.json")).unwrap();
    let snap: serde_json::Value = serde_json::from_slice(&snap_bytes).unwrap();
    assert_eq!(snap["environment"], "staging");
}
