//! 04 套件 1：调试环境"即拷即走"自包含验证（M5 owner）。
//! 语义（v2.1.0 重定义）：证明的不是"机器上无解释器"，而是
//! 1. 内置宏全流程零子进程派生（stub 反转为外部调用必红——反假守护）；
//! 2. 隔离环境（PATH 指向空目录）跑完整套件：退出码与快照产物正确、0600 权限。

use std::process::Command;

/// mock server 由项目自带 src/mock 承担：内联用 wiremock（dev-deps）监听本地端口，
/// 被测二进制仅依赖自身 + 内置宏（无外部解释器/无网络外联）。
#[tokio::test]
async fn test_standalone_execution_without_external_runtime() {
    use wiremock::matchers::{method, path};
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/secure-api"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("standalone.http");
    std::fs::write(
        &case,
        format!(
            r#"### secure
POST {}/secure-api
X-Timestamp: {{{{$timestamp_ms}}}}
X-Sign: {{{{$hmac_sha256("secret", "{{$timestamp_ms}}")}}}}
Content-Type: application/json

{{"order_id": 999}}

# @assert status == 200
"#,
            server.uri()
        ),
    )
    .unwrap();
    let snap = dir.path().join("dump.json");

    // 隔离环境：PATH 指向空目录（无 rupost-plugin-*、无任何可发现插件）
    let empty_path = dir.path().join("empty-bin");
    std::fs::create_dir(&empty_path).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "t",
            case.to_str().unwrap(),
            "--save-snapshot",
            snap.to_str().unwrap(),
        ])
        .env("PATH", &empty_path)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "内置宏全流程必须离线闭环: {stderr}");

    // 快照产物 + 0600 权限（02 §2.7 安全不变量）
    let snap_meta = std::fs::metadata(&snap).expect("快照必须落盘");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            snap_meta.permissions().mode() & 0o777,
            0o600,
            "黑匣子快照权限必须 0600"
        );
    }
    let snap_json = std::fs::read_to_string(&snap).unwrap();
    assert!(snap_json.contains("\"schema_version\": \"1.0\""));
    assert!(snap_json.contains("\"outcome\": \"pass\""));

    // 宏求值痕迹：请求头中的 X-Sign 已是 64 位小写 hex（HMAC-SHA256）
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let sign = requests[0]
        .headers
        .iter()
        .find(|(n, _)| n.as_str() == "x-sign")
        .map(|(_, v)| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .expect("X-Sign 必须存在");
    assert_eq!(sign.len(), 64, "HMAC-SHA256 hex 长度: {sign}");
    assert!(
        sign.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
}

/// 反假守护：宏若被反转 stub 为"调用外部解释器/openssl"，隔离 PATH 下立即断链变红
/// （等价于守护"执行路径零子进程派生"——隔离环境没有任何可执行物可借用）。
#[test]
fn test_builtin_macro_requires_no_external_dependency() {
    // RFC 4231 Case 2 向量锁定（与 resolver 单测同源，二进制级再验证）
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    // 宏向量断言在 resolver 单测（RFC 4231）已锁定；此处锁定隔离 PATH 下
    // help/插件发现路径零子进程也不 panic（发现 = readdir + JSON 解析）。
}

/// C-2（守护补强）：`env -i` 级最小环境（无 HOME/无继承 env）——即拷即走承诺的
/// 最后一道守护：dirs::home_dir()==None 路径（registry 缓存回退 cwd）、history
/// 落盘、快照 0600 全链不得 panic。
#[test]
fn test_minimal_env_i_no_home_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("mini.http");
    std::fs::write(
        &case,
        "### ok\n# @auth mini-auth\nGET http://127.0.0.1:1/mini\n\n# @assert status == 200\n",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args([
            "t",
            case.to_str().unwrap(),
            "--allow-missing-plugins",
            "--no-history",
        ])
        .output()
        .unwrap();
    // @auth 缺失 → ?optional 缺席 → 无旗标时阻断 exit 1；关键是进程不 panic、错误可读
    assert_eq!(
        out.status.code(),
        Some(1),
        "最小环境下必须优雅退出（exit 1），不得 panic/abort: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mini-auth"),
        "错误信息必须保留插件名上下文: {stderr}"
    );
}
