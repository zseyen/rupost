//! Step 3 TDD 守护测试：CLI 插件管理子命令、动态双模路由参数剥离、环境放行与内置命令保护。

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn install_cli_plugin(project_root: &Path, name: &str, directives: &str) -> std::path::PathBuf {
    let plugin_dir = project_root.join(".rupost").join("plugins");
    let bin = common::mock_plugin::install_mock_executable(
        &plugin_dir,
        &format!("rupost-plugin-{name}"),
        directives,
    );
    let manifest = serde_json::json!({
        "manifest_version": "1.0",
        "name": name,
        "version": "0.1.0",
        "description": "cli test plugin",
        "commands": [{
            "name": name,
            "usage": format!("rupost {name}"),
            "description": "cli demo command",
            "requires_tty": false
        }],
        "hooks": []
    });
    std::fs::write(
        plugin_dir.join(format!("rupost-plugin-{name}.json")),
        manifest.to_string(),
    )
    .unwrap();
    bin.canonicalize().unwrap_or(bin)
}

fn preseed_tofu(fake_home: &Path, exe_path: &Path) {
    use sha2::{Digest, Sha256};
    let rupost_dir = fake_home.join(".rupost");
    std::fs::create_dir_all(&rupost_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&rupost_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let bytes = std::fs::read(exe_path).unwrap();
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let tofu_path = rupost_dir.join("tofu-commands.txt");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&tofu_path)
        .unwrap();
    writeln!(file, "{} {}", exe_path.display(), digest).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tofu_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn test_cli_plugin_list_accepts_allow_project_plugins_flag() {
    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    install_cli_plugin(dir.path(), "clilistdemo", "STDOUT_LINE=ok\n");

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args(["plugin", "list", "--allow-project-plugins"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "rupost plugin list --allow-project-plugins 必须成功: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("clilistdemo"),
        "输出应列出已发现的工作区插件命令: {stdout}"
    );
}

#[test]
fn test_cli_dynamic_plugin_route_strips_rupost_flags_and_forwards_env() {
    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let capture_file = dir.path().join("captured.txt");
    let bin = install_cli_plugin(
        dir.path(),
        "mycli",
        &format!(
            "CAPTURE_ARGS_ENV={}|CLI_PLUGIN_TEST_VAR\n",
            capture_file.display()
        ),
    );
    preseed_tofu(&fake_home, &bin);

    // 场景 A：全局/路由级参数放在命令前（rupost --allow-project-plugins mycli hello）
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .env("CLI_PLUGIN_TEST_VAR", "val42")
        .args([
            "--allow-project-plugins",
            "mycli",
            "--plugin-env=CLI_PLUGIN_TEST_VAR",
            "hello",
        ])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "前置 --allow-project-plugins 路由必须成功: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let captured = std::fs::read_to_string(&capture_file).unwrap();
    assert!(
        captured.contains("args=hello"),
        "剥离 rupost 自有旗标后仅应透传插件业务参数 hello: {captured}"
    );
    assert!(
        captured.contains("env=val42"),
        "--plugin-env 声明的父环境变量必须透传: {captured}"
    );
}

#[test]
fn test_cli_plugin_route_after_cmd_and_builtin_plugin_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let capture_file = dir.path().join("captured_after.txt");
    let bin = install_cli_plugin(
        dir.path(),
        "postcli",
        &format!(
            "CAPTURE_ARGS_ENV={}|CLI_AFTER_VAR\n",
            capture_file.display()
        ),
    );
    preseed_tofu(&fake_home, &bin);
    // 恶意注册名为 "plugin" 的插件命令：内置 `rupost plugin` 始终优先
    let _ = install_cli_plugin(dir.path(), "plugin", "STDOUT_LINE=hijacked\nEXIT_CODE=99\n");

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args([
            "postcli",
            "--allow-project-plugins",
            "--plugin-env",
            "CLI_AFTER_VAR=explicit_ok",
            "world",
        ])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "后置路由旗标必须成功: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let captured = std::fs::read_to_string(&capture_file).unwrap();
    assert!(captured.contains("args=world"), "{captured}");
    assert!(captured.contains("env=explicit_ok"), "{captured}");

    let builtin_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args(["plugin", "schema", "--allow-project-plugins"])
        .output()
        .unwrap();
    assert!(builtin_out.status.success());
    let stdout = String::from_utf8_lossy(&builtin_out.stdout);
    assert!(
        stdout.contains("rupost plugin directive parameters"),
        "{stdout}"
    );
    assert!(!stdout.contains("hijacked"), "{stdout}");
}

#[test]
fn test_allow_project_plugins_env_falsey_values_do_not_enable_project_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let _ = install_cli_plugin(dir.path(), "projonly", "STDOUT_LINE=ok\n");

    for val in ["0", "false", "no", "off", ""] {
        let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
            .current_dir(dir.path())
            .env("HOME", &fake_home)
            .env("PATH", "/usr/bin:/bin")
            .env("RUPOST_ALLOW_PROJECT_PLUGINS", val)
            .args(["plugin", "list"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains("projonly"),
            "RUPOST_ALLOW_PROJECT_PLUGINS={val:?} 不得放行项目插件: {stdout}"
        );
    }

    let enabled_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .env("PATH", "/usr/bin:/bin")
        .env("RUPOST_ALLOW_PROJECT_PLUGINS", "true")
        .args(["plugin", "list"])
        .output()
        .unwrap();
    assert!(enabled_out.status.success());
    let stdout = String::from_utf8_lossy(&enabled_out.stdout);
    assert!(
        stdout.contains("projonly"),
        "RUPOST_ALLOW_PROJECT_PLUGINS=true 应放行项目插件: {stdout}"
    );
}

#[test]
fn test_cli_plugin_unseen_tofu_fails_closed_in_non_tty() {
    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let marker = dir.path().join("should_not_run.txt");
    let _ = install_cli_plugin(
        dir.path(),
        "unseencli",
        &format!("TOUCH={}\n", marker.display()),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args(["--allow-project-plugins", "unseencli"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // 即使在非 TTY 管道 stdin 中写入 y\n，也必须 fail closed
    child.stdin.as_mut().unwrap().write_all(b"y\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        !out.status.success(),
        "非 TTY 下未确认 TOFU 的插件命令必须被拒绝执行"
    );
    assert!(!marker.exists(), "被 TOFU 拒绝的插件不得被执行");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("TOFU") || stderr.contains("非交互"),
        "报错应明确指出非交互终端 TOFU 拦截: {stderr}"
    );
}

#[test]
fn test_cli_dangling_plugin_env_returns_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["GET", "http://127.0.0.1:1/x", "--plugin-env"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "悬空 --plugin-env（缺少值）必须显式报错退出"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--plugin-env"),
        "错误信息应提示 --plugin-env 参数缺失: {stderr}"
    );
}

#[tokio::test]
async fn test_cli_inline_request_strips_routing_flags_via_passthrough() {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/inline-ok"))
        .and(body_json(serde_json::json!({"key": "val"})))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok-inline"))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("RUPOST_HISTORY_DIR", dir.path().join("hist"))
        .args([
            "POST",
            &format!("{}/inline-ok", server.uri()),
            "--allow-project-plugins",
            "--plugin-env",
            "DUMMY_ENV=1",
            "key=val",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "内联请求剥离路由旗标后必须按 httpie 风格成功发送: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("ok-inline"), "{stdout}");
    server.verify().await;
}
