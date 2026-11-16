//! WP-T3 确定性表面黄金测试（assert_cmd 精确输出断言形态）。
//! 决策记录：原 trycmd 方案因 1.2 沙箱不自动包含 fixture 文件（目录/子目录
//! 均报 ENOENT，深度 API 考古性价比低）而改为本形态——同等覆盖、零沙箱斗争。
//! 被测表面全部满足"同输入 → 同输出"：ast 纯投影（无时间戳/网络/机器态）、
//! validate 静态体检（固定 fixture 清单）。
use assert_cmd::Command;
use std::path::Path;

fn cmd(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("rupost").unwrap();
    c.env("NO_COLOR", "1").env("CLICOLOR", "0").current_dir(dir);
    c
}

/// 黄金期望（AstDocument v1.0 契约）：fixture 的投影必须逐字段相等。
/// fixture 行号台账：1 ### / 2 @name / 3 @assert / 4 GET
#[test]
fn test_ast_output_matches_golden_contract() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = cmd(manifest_dir)
        .arg("ast")
        .arg("tests/cmd/fixture.http")
        .output()
        .expect("run rupost ast");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let actual: serde_json::Value = serde_json::from_slice(&out.stdout).expect("纯 JSON");
    let expected = serde_json::json!({
        "schema_version": "1.0",
        "source_path": "tests/cmd/fixture.http",
        "dependencies": [],
        "requests": [{
            "number": 1,
            "method": "GET",
            "url": "{{base_url}}/ok",
            "headers": [],
            "body": null,
            "name": "ok",
            "skip": false,
            "assertions": ["status == 200"],
            "captures": [],
            "source_line": 4
        }]
    });
    assert_eq!(actual, expected, "AstDocument 契约黄金比对（源路径为相对 cwd 的展示路径）");
}

/// validate 黄金：合法 fixture → ready 行精确匹配
#[test]
fn test_validate_output_matches_golden() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("tests/cmd/fixtures/rupost-plugin-golden.json");
    let out = cmd(manifest_dir)
        .args(["plugin", "validate"])
        .arg("tests/cmd/fixtures/rupost-plugin-golden.json")
        .output()
        .expect("run rupost plugin validate");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim(),
        "✓ ready: golden v0.1.0（commands: 1, hooks: 0）"
    );
}
