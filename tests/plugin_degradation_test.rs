//! 04 套件 2：插件缺失、自愈降级与三态门禁验证（M3 owner）
//! 语义对齐 03 §2 唯一权威：降级 = 插件不注入 + 请求按 .http 原样外发 +
//! 三处 WARNING + 快照 DegradedPass；--strict-plugins 下降级即整体 exit 1。
//! 反假守护：正向对照组（真实插件注入特征 Header）保证"降级成功"与
//! "插件根本没被调用"可区分——反转生产逻辑必红。

mod common;

use std::path::PathBuf;

/// mock 插件 fixture（跨平台原生可执行体，禁 Python——04 §2.5 harness 契约）：
/// 写入静态 manifest + 可执行占位，落 `--allow-project-plugins` 工作区目录
fn install_mock_plugin(root: &std::path::Path, alias: &str, header_value: &str) -> PathBuf {
    let plugin_dir = root.join(".rupost").join("plugins");
    common::mock_plugin::install_mock_executable(
        &plugin_dir,
        &format!("rupost-plugin-{alias}"),
        &format!(
            "READ_STDIN=\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Plugin-Proof\",\"{header_value}\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n"
        ),
    );
    let manifest = plugin_dir.join(format!("rupost-plugin-{alias}.json"));
    std::fs::write(
        &manifest,
        format!(
            r#"{{"manifest_version":"1.0","name":"{alias}","version":"0.1.0","description":"mock","commands":[],"hooks":[{{"directive":"sign","alias":"{alias}","stage":"sign","cache_scope":"none","requires_env":[],"parameters_schema":{{}}}}]}}"#
        ),
    )
    .unwrap();
    manifest
}

fn write_case(dir: &tempfile::TempDir, url: &str, plugin: &str, optional: bool) -> PathBuf {
    let case = dir.path().join("case.http");
    let suffix = if optional { " ?optional" } else { "" };
    std::fs::write(
        &case,
        format!(
            "### t\nPOST {url}\nX-Debug-Token: fixed-value\nX-From-File: always\n@sign {plugin}{suffix}\n\n{{\"k\":1}}\n"
        ),
    )
    .unwrap();
    case
}

/// 正向对照组：真实存在的 mock 插件必须注入特征 Header——
/// 证明插件确实被调用（防"降级成功"与"插件没被调用"混淆的假守护）
#[tokio::test]
async fn test_plugin_invoked_header_reaches_server() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/t"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    install_mock_plugin(dir.path(), "proof", "magic-123");
    let case = write_case(&dir, &format!("{}/t", server.uri()), "proof", false);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args([
            "t",
            case.to_str().unwrap(),
            "--allow-project-plugins",
            "--report",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "正向对照组必须成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let headers = &requests[0].headers;
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "x-plugin-proof" && value == "magic-123"),
        "真实插件的 Patch 必须到达服务端"
    );
    // 降级计数横幅不得出现（非降级路径）
    assert!(!String::from_utf8_lossy(&out.stderr).contains("降级形态发送"));
}

/// 场景 2：?optional 缺失降级 —— 原样外发 + DegradedPass + 快照 WARNING；
/// 兜底 Header 按文件逐字到达服务端（引擎绝不注入魔法值）
#[tokio::test]
async fn test_optional_missing_degrades_verbatim_and_records() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/t"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = write_case(
        &dir,
        &format!("{}/t", server.uri()),
        "non_existent_signer",
        true,
    );
    let snap = dir.path().join("snap.json");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args([
            "t",
            case.to_str().unwrap(),
            "--save-snapshot",
            snap.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "?optional 降级不阻塞排障: {stderr}");
    assert!(
        stderr.contains("插件 'non_existent_signer' 未安装，已降级跳过"),
        "终端 WARNING 反静默: {stderr}"
    );
    assert!(stderr.contains("降级形态发送"), "降级计数汇总: {stderr}");

    // 服务端收到的 Header 与 .http 文件逐字一致（原样外发裁决）
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .headers
            .iter()
            .any(|(name, value)| name == "x-debug-token" && value == "fixed-value"),
        "兜底 Header 按文件原样外发"
    );
    assert!(
        requests[0]
            .headers
            .iter()
            .all(|(name, _)| name != "x-plugin-proof"),
        "降级请求不得携带插件特征 Header"
    );

    // 快照三态 + WARNING
    let snap_json = std::fs::read_to_string(&snap).unwrap();
    assert!(
        snap_json.contains("degraded_pass"),
        "快照 outcome=DegradedPass: {snap_json}"
    );
    assert!(
        snap_json.contains("未安装，已降级跳过"),
        "快照 WARNING 反静默"
    );
}

/// 场景 1：未声明降级（默认安全拦截）—— 非零退出 + 搜索路径清单
#[tokio::test]
async fn test_missing_without_flags_blocks_with_path_listing() {
    let dir = tempfile::tempdir().unwrap();
    let case = write_case(&dir, "http://127.0.0.1:9/t", "non_existent_signer", false);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args(["t", case.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "默认必须安全拦截");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("未安装") && combined.contains("搜索路径"),
        "报错必须列出搜索路径清单: {combined}"
    );
}

/// 场景 2b：全局 --allow-missing-plugins 对非 optional 用例同样自愈
#[tokio::test]
async fn test_allow_missing_flag_heals_non_optional_case() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/t"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = write_case(
        &dir,
        &format!("{}/t", server.uri()),
        "non_existent_signer",
        false,
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args([
            "t",
            case.to_str().unwrap(),
            "--allow-missing-plugins",
            "--save-snapshot",
            dir.path().join("snap.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "全局放行必须自愈: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("降级形态发送"),
        "降级计数横幅必须可见"
    );
}

/// 场景 3：--strict-plugins CI 门禁 —— HTTP 200 也因降级整体 exit 1
#[tokio::test]
async fn test_strict_plugins_fails_suite_on_degradation() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/t"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = write_case(
        &dir,
        &format!("{}/t", server.uri()),
        "non_existent_signer",
        true,
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args(["t", case.to_str().unwrap(), "--strict-plugins"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "strict 门禁下降级必须阻断（即使 HTTP 200）: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--strict-plugins"),
        "门禁原因必须可见"
    );
}

/// 含未解析 {{var}} 的兜底 Header：变量解析失败的兜底视为兜底缺失，
/// 该请求跳过并 WARNING（03 §2.1 禁止字面量外发）
#[tokio::test]
async fn test_unresolved_placeholder_header_skips_request() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/t"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("case.http");
    std::fs::write(
        &case,
        format!(
            "### t\nPOST {}/t\nAuthorization: Bearer {{{{DEBUG_FALLBACK_TOKEN}}}}\n@sign non_existent_signer ?optional\n\n{{\"k\":1}}\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args(["t", case.to_str().unwrap(), "--allow-missing-plugins"])
        .output()
        .unwrap();
    // 请求被跳过：服务端不得收到把字面量当 Bearer 的请求
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.headers.iter().all(|(n, _)| n != "authorization")),
        "未解析 {{var}} 兜底 Header 绝不外发"
    );
    let _ = out;
}
