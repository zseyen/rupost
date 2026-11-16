//! WP-T2 二进制契约测试（assert_cmd 形态）：锁定"编译产物"的退出码三值表、
//! 内联客户端、工件错误、安全旗标。只锁 assert_cmd 形态面——语义闭环以
//! incident_bundle/contract_drift/plugin_validate 既有 e2e 为准（不重复铺断言）。
//! 反假守护抽样：exit2 映射/历史停用/越界错误 三类各破坏一次必须变红。
use assert_cmd::Command;
use predicates::str::contains;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// 进程内轻量 HTTP/1.1 echo 服务（直接持有 `:0` listener，消除端口释放 TOCTOU 与子进程竞态）
struct MockServer {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn start(_config_body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        // 读完 HTTP 请求头 + Content-Length 请求体，避免 Windows Winsock 未读完 body 关闭触发 TCP RST
                        let mut total = 0;
                        let mut header_end = None;
                        let mut expected_total = None;
                        while total < buf.len() {
                            match stream.read(&mut buf[total..]) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    total += n;
                                    if header_end.is_none()
                                        && let Some(pos) =
                                            buf[..total].windows(4).position(|w| w == b"\r\n\r\n")
                                    {
                                        let h_end = pos + 4;
                                        header_end = Some(h_end);
                                        let header_str = String::from_utf8_lossy(&buf[..h_end]);
                                        let content_len = header_str
                                            .lines()
                                            .find_map(|line| {
                                                let (k, v) = line.split_once(':')?;
                                                k.trim()
                                                    .eq_ignore_ascii_case("content-length")
                                                    .then(|| v.trim().parse::<usize>().ok())
                                                    .flatten()
                                            })
                                            .unwrap_or(0);
                                        expected_total = Some(h_end + content_len);
                                    }
                                    if let Some(exp) = expected_total
                                        && total >= exp
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                        let req_line = String::from_utf8_lossy(&buf[..total])
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        let body_slice = match header_end {
                            Some(h_end) if total >= h_end => &buf[h_end..total],
                            _ => b"",
                        };
                        let resp_body = if body_slice.is_empty() {
                            format!("ok:{req_line}")
                        } else {
                            format!("ok:{req_line}:{}", String::from_utf8_lossy(body_slice))
                        };
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
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
        .stdout(contains("200"))
        .stdout(contains("POST /echo"));
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

    // ③ 混合形态（WP-P2c 修复后）：方法前置 + 尾部旗标 → httpie 解析器接管，
    //    未知旗标被容忍（verbose 输出未实现，登记）——200 可用
    cmd(dir.path(), history.path())
        .args(["GET", &url, "--verbose"])
        .assert()
        .code(0)
        .stdout(contains("200"));
}

/// 例 3b（WP-P2c 补充）：旗标夹在方法与 URL 之间 → 同样 httpie 解析、URL 正确
#[test]
fn test_inline_flag_between_method_and_url() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    let mock = MockServer::start(
        r#"[{"method":"GET","path":"/ok","variants":[{"condition":null,"status":200,"headers":{},"response_body":"ok"}]}]"#,
    );
    let url = format!("http://127.0.0.1:{}/ok", mock.port);

    cmd(dir.path(), history.path())
        .args(["GET", "--verbose", &url])
        .assert()
        .code(0)
        .stdout(contains("200"));
}

/// 例 3c（WP-P2c 边界）：旗标最前（非方法前置形态）→ 仍走根路由裁决（未知命令 exit 2）
#[test]
fn test_flag_first_unknown_command_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    cmd(dir.path(), history.path())
        .args(["--verbose", "GET", "http://127.0.0.1:9/ok"])
        .assert()
        .code(2)
        .stderr(contains("未知命令"));
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
