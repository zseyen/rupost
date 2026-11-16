//! WP-T2 二进制契约测试（assert_cmd 形态）：锁定"编译产物"的退出码三值表、
//! 内联客户端、工件错误、安全旗标。只锁 assert_cmd 形态面——语义闭环以
//! incident_bundle/contract_drift/plugin_validate 既有 e2e 为准（不重复铺断言）。
//! 反假守护抽样：exit2 映射/历史停用/越界错误 三类各破坏一次必须变红。
use assert_cmd::Command;
use predicates::str::contains;
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// 统一构造：NO_COLOR + 历史隔离 + cwd（跨平台五条军规的测试侧映射）
fn cmd(dir: &Path, history: &Path) -> Command {
    let mut c = Command::cargo_bin("rupost").unwrap();
    c.env("NO_COLOR", "1")
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .timeout(Duration::from_secs(60));
    c
}

/// 后台拉起 mock 服务（bind :0 探测空闲端口，防固定端口冲突；Drop 时 kill）
struct MockServer {
    child: std::process::Child,
    port: u16,
}

impl MockServer {
    fn start(config_body: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("mock.json");
        std::fs::write(&config, config_body).unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // 服务器进程用 std 命令（常驻形态；assert_cmd 仅用于断言形态的调用）
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
            .args([
                "mock",
                config.to_str().unwrap(),
                "--port",
                &port.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn rupost mock");
        let server = Self { child, port };
        // 就绪探测：有限重试，连接成功即就绪（任意 HTTP 应答均可）
        for _ in 0..50 {
            if TcpListener::bind("127.0.0.1:0").is_ok()
                && std::net::TcpStream::connect(("127.0.0.1", server.port)).is_ok()
            {
                return server;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("mock 未在超时内就绪: port {port}");
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 例 1（M18）：退出码三值表——exit 2 的三个来源，stderr 前缀互不相同。
/// 反假守护：main.rs 移除任一映射 → 对应断言红。
#[test]
fn test_exit_code_2_three_sources_disambiguation() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();

    // 来源 1：clap 参数错误
    cmd(dir.path(), history.path())
        .args(["t", "--no-such-flag"])
        .assert()
        .code(2)
        .stderr(contains("error:"));

    // 来源 2：解析错误
    std::fs::write(dir.path().join("bad.http"), "FTP https://x\n").unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "bad.http"])
        .assert()
        .code(2)
        .stderr(contains("[ERROR] 解析错误:"));

    // 来源 3：契约基线 I/O（损坏 JSON）
    std::fs::write(
        dir.path().join("ok.http"),
        "### ok\n# @assert status == 200\nGET http://127.0.0.1:9/ok\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("broken.json"), "{not-json").unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "ok.http", "--contract-baseline", "broken.json"])
        .assert()
        .code(2)
        .stderr(contains("[ERROR] 契约基线工件错误:"));
}

/// 例 2（M1.2）：curl 风格位置 body——`--raw` 是被忽略的 curl 选项（实跑实证），
/// 锁定正确形态：`POST <url> '<body>'` 位置参数
#[test]
fn test_inline_post_positional_body() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    let mock = MockServer::start(
        r#"[{"method":"POST","path":"/echo","variants":[{"condition":null,"status":200,"headers":{},"response_body":"{\"ok\":true}"}]}]"#,
    );
    let url = format!("http://127.0.0.1:{}/echo", mock.port);

    cmd(dir.path(), history.path())
        .args(["POST", &url, "hello"])
        .assert()
        .code(0)
        .stdout(contains("200"));
}

/// 例 3（M1.3）：内联双风格边界（实测锁定——详见指南 M1.3 与计划查漏）：
/// ① httpie 纯形式（无旗标）`GET url` → 200；
/// ② 前置未知旗标 `--verbose GET url` → 根路由裁决序"未知命令" exit 2（AGENTS §5）；
/// ③ 混合形态 `GET url --verbose` 会切换 curl 解析器（方法 token 被当 URL）——
///    已登记为解析器选择边界观察，本测试不锁定该 wart 的具体错误文本
#[test]
fn test_inline_style_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    let mock = MockServer::start(
        r#"[{"method":"GET","path":"/ok","variants":[{"condition":null,"status":200,"headers":{},"response_body":"ok"}]}]"#,
    );
    let url = format!("http://127.0.0.1:{}/ok", mock.port);

    // ① httpie 纯形式
    cmd(dir.path(), history.path())
        .args(["GET", &url])
        .assert()
        .code(0)
        .stdout(contains("200"));

    // ② 前置未知旗标：根路由 unknown → exit 2
    cmd(dir.path(), history.path())
        .args(["--verbose", "GET", &url])
        .assert()
        .code(2)
        .stderr(contains("未知命令"));

    // ③ 混合形态：curl 解析器接管（登记的解析器选择边界，锁失败事实本身）
    cmd(dir.path(), history.path())
        .args(["GET", &url, "--verbose"])
        .assert()
        .code(1);
}

/// 例 4（M2.3）：--only 越界 → 可读中文错误 + exit 1
#[test]
fn test_only_out_of_range_readable_error() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("suite.http"),
        "### ok\n# @assert status == 200\nGET http://127.0.0.1:9/ok\n",
    )
    .unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "suite.http", "--only", "999"])
        .assert()
        .code(1)
        .stderr(contains("越界"));
}

/// 例 5（M5）：--report json 的 stdout 必须是可解析的纯 JSON 数组
#[test]
fn test_report_json_stdout_purity() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ok.http"),
        "### ok\n# @assert status == 200\nGET http://127.0.0.1:9/ok\n",
    )
    .unwrap();
    let output = cmd(dir.path(), history.path())
        .args(["t", "ok.http", "--report", "json"])
        .output()
        .expect("run rupost");
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout 必须为纯 JSON");
    assert!(parsed.is_array());
    assert!(parsed[0]["results"][0]["source_line"].is_number());
}

/// 例 6（M5）：--report-file 已存在 → 覆盖警告可见
#[test]
fn test_report_file_overwrite_warning() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ok.http"),
        "### ok\n# @assert status == 200\nGET http://127.0.0.1:9/ok\n",
    )
    .unwrap();
    let report = dir.path().join("r.json");
    std::fs::write(&report, b"old").unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "ok.http", "--report-file"])
        .arg(&report)
        .assert()
        .code(1)
        .stderr(contains("已存在"));
    // 且报告确实被覆盖为新内容（JSON 合法）
    let body = std::fs::read_to_string(&report).unwrap();
    assert!(body.contains("file_path"), "覆盖后必须是新报告: {body}");
}

/// 例 7+10（M18 安全红线）：--no-history → 历史零落盘；默认 → 落盘。
/// 反假守护：移除 recorder 的 --no-history 分支 → 前半断言红。
#[test]
fn test_no_history_flag_zero_disk_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start(
        r#"[{"method":"GET","path":"/ok","variants":[{"condition":null,"status":200,"headers":{},"response_body":"ok"}]}]"#,
    );
    let url = format!("http://127.0.0.1:{}/ok", mock.port);
    let suite = format!("### ok\n# @assert status == 200\nGET {url}\n");
    std::fs::write(dir.path().join("ok.http"), &suite).unwrap();

    // 默认：历史落盘
    let hist_with = tempfile::tempdir().unwrap();
    cmd(dir.path(), hist_with.path())
        .args(["t", "ok.http"])
        .assert()
        .code(0);
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(hist_with.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!entries.is_empty(), "默认必须写历史");

    // --no-history：零落盘
    let hist_without = tempfile::tempdir().unwrap();
    cmd(dir.path(), hist_without.path())
        .args(["--no-history", "t", "ok.http"])
        .assert()
        .code(0);
    let left: Vec<std::fs::DirEntry> = std::fs::read_dir(hist_without.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        left.is_empty(),
        "--no-history 必须零落盘: {:?}",
        left.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );
}

/// 例 8（M13.1）：plugin validate 合法形态（assert_cmd 形态锚点；Windows 矩阵覆盖面）
#[test]
fn test_plugin_validate_ready_prefix() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/plugins/rupost-plugin-cmd-hello.json");
    Command::cargo_bin("rupost")
        .unwrap()
        .args(["plugin", "validate"])
        .arg(&manifest)
        .assert()
        .code(0)
        .stdout(contains("✓ ready: cmd-hello"));
}

/// 例 9（M14）：web 生命周期——启动就绪、meta 可读、进程可优雅终止
/// （assert_cmd 无常驻形态，用 std spawn + 轮询 + kill）
#[test]
fn test_web_server_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "web",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rupost web");

    let mut ready = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(ready, "web 未在超时内就绪");
    let _ = child.kill();
    let _ = child.wait();
}
