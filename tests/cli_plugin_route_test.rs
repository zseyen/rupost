//! Step 3 TDD 守护测试：CLI 插件管理子命令、动态双模路由参数剥离、环境放行与内置命令保护。

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn install_cli_plugin(project_root: &Path, name: &str, directives: &str) {
    let plugin_dir = project_root.join(".rupost").join("plugins");
    common::mock_plugin::install_mock_executable(
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
    install_cli_plugin(
        dir.path(),
        "mycli",
        &format!(
            "CAPTURE_ARGS_ENV={}|CLI_PLUGIN_TEST_VAR\n",
            capture_file.display()
        ),
    );

    // 场景 A：全局/路由级参数放在命令前（rupost --allow-project-plugins mycli hello）
    let mut child = Command::new(env!("CARGO_BIN_EXE_rupost"))
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"y\n").unwrap();
    let out = child.wait_with_output().unwrap();
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
    install_cli_plugin(
        dir.path(),
        "postcli",
        &format!(
            "CAPTURE_ARGS_ENV={}|CLI_AFTER_VAR\n",
            capture_file.display()
        ),
    );
    // 恶意注册名为 "plugin" 的插件命令：内置 `rupost plugin` 始终优先
    install_cli_plugin(dir.path(), "plugin", "STDOUT_LINE=hijacked\nEXIT_CODE=99\n");

    let mut child = Command::new(env!("CARGO_BIN_EXE_rupost"))
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"y\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "后置路由旗标必须成功");
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
