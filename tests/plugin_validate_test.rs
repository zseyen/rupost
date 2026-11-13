//! WP-PL2 守护：`rupost plugin validate` 作者工具——静态校验（不执行插件、
//! 不触发 TOFU）；合法 ✓/违规 ✗ 处方式输出；示例清单全量协同锁定。
use std::process::Command;

fn run_validate(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["plugin", "validate"])
        .arg(path)
        .output()
        .expect("spawn rupost")
}

/// 构造一个合法 manifest + 同目录占位可执行（校验只要求"存在普通文件"，
/// 不执行——Windows CI 同样可跑）
fn write_legal(dir: &std::path::Path, name: &str, desc: &str) -> std::path::PathBuf {
    std::fs::write(dir.join(format!("rupost-plugin-{name}")), b"#!/bin/sh\n").unwrap();
    let manifest = serde_json::json!({
        "manifest_version": "1.0",
        "name": name,
        "version": "0.1.0",
        "description": desc,
        "commands": [{
            "name": name,
            "usage": format!("rupost {name}"),
            "description": "demo",
            "requires_env": []
        }],
        "hooks": []
    });
    let path = dir.join(format!("rupost-plugin-{name}.json"));
    std::fs::write(&path, manifest.to_string()).unwrap();
    path
}

/// 1. 合法清单 → exit 0 + ✓ ready 行
#[test]
fn test_validate_legal_manifest_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_legal(dir.path(), "good", "a valid demo plugin manifest");
    let out = run_validate(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("✓ ready: good v0.1.0"), "{stdout}");
    assert!(stdout.contains("commands: 1, hooks: 0"), "{stdout}");
}

/// 2. description 超长 → exit 1 + 处方指名 description
#[test]
fn test_validate_rejects_overlong_description() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_legal(dir.path(), "longdesc", &"x".repeat(81));
    let out = run_validate(&path);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("description"), "{stderr}");
}

/// 3. 保留字阶段 → exit 1 + 处方含 保留字
#[test]
fn test_validate_rejects_reserved_stage() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rupost-plugin-bad"), b"#!/bin/sh\n").unwrap();
    let manifest = serde_json::json!({
        "manifest_version": "1.0",
        "name": "bad",
        "version": "0.1.0",
        "description": "reserved stage demo",
        "commands": [],
        "hooks": [{
            "directive": "harvest", "stage": "harvest",
            "cache_scope": "none", "requires_env": [], "parameters_schema": {}
        }]
    });
    let path = dir.path().join("rupost-plugin-bad.json");
    std::fs::write(&path, manifest.to_string()).unwrap();
    let out = run_validate(&path);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("保留字"), "{stderr}");
}

/// 4. 同目录可执行缺失 → exit 1 + 处方含 二进制不存在
#[test]
fn test_validate_rejects_missing_executable() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = serde_json::json!({
        "manifest_version": "1.0",
        "name": "orphan",
        "version": "0.1.0",
        "description": "no sibling executable",
        "commands": [{
            "name": "orphan", "usage": "rupost orphan", "description": "demo"
        }],
        "hooks": []
    });
    let path = dir.path().join("rupost-plugin-orphan.json");
    std::fs::write(&path, manifest.to_string()).unwrap();
    let out = run_validate(&path);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("二进制不存在"), "{stderr}");
}

/// 5. 警告级（不阻断）：命名契约 + 内置名冲突 + 空清单
#[test]
fn test_validate_warns_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    // 文件名故意违规（my.json——发现器永远看不到）+ 命令与内置子命令撞名 + 无 hooks。
    // 可执行名派生自 manifest 文件名 stem（my.json → 探测 my / rupost-plugin-my）——
    // 这正是命名契约警告要讲清的发现期约定
    std::fs::write(dir.path().join("my"), b"#!/bin/sh\n").unwrap();
    let manifest = serde_json::json!({
        "manifest_version": "1.0",
        "name": "shadow",
        "version": "0.1.0",
        "description": "warning demo",
        "commands": [{
            "name": "test", "usage": "rupost test", "description": "collides with builtin"
        }]
    });
    let path = dir.path().join("my.json");
    std::fs::write(&path, manifest.to_string()).unwrap();
    let out = run_validate(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "警告不阻断: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("命名契约"), "{stdout}");
    assert!(stdout.contains("永不可达"), "{stdout}");
}

/// 6. 协同锁定（审查 G1/G2）：examples/plugins 下全部插件清单必须 validate
///    通过（跨平台守护示例正确性——静态校验不执行插件，Windows CI 可跑）
#[test]
fn test_validate_all_example_plugin_manifests() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins");
    let mut checked = 0;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&examples).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("rupost-plugin-") || !name.ends_with(".json") {
            continue;
        }
        checked += 1;
        let out = run_validate(&entry.path());
        if out.status.code() != Some(0) {
            failures.push(format!(
                "{}: {}",
                name,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(
        checked >= 2,
        "示例清单应至少有 2 个（echo-sign + cmd-hello）"
    );
    assert!(
        failures.is_empty(),
        "示例清单必须全部通过 validate: {failures:?}"
    );
}

/// 审查修正 B2 回归：裸相对文件名（cwd 内）——parent 为空串时必须退化到 cwd，
/// 而非报 "canonicalize 失败"
#[test]
fn test_validate_bare_relative_filename() {
    let dir = tempfile::tempdir().unwrap();
    write_legal(dir.path(), "bare", "bare relative filename demo");
    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .args(["plugin", "validate"])
        .arg("rupost-plugin-bare.json")
        .output()
        .expect("spawn rupost");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("✓ ready: bare"));
}
