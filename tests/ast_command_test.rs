//! WP-AST 守护：`rupost ast` 输出稳定 JSON 契约（AstDocument v1.0）；
//! 错误路径诚实失败。插件经 `RUPOST_BIN` 自调消费（见评估报告 §7/§8）。
mod common;

use std::process::Command;
use tempfile::tempdir;

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

/// 反假守护：改 AstDocument 顶层字段名 → schema 断言红；删除投影回填 → 字段断言红。
#[test]
fn test_ast_json_contract() {
    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 ### / 2 @name / 3 assert / 4 GET / 5 空 / 6 ### / 7 assert / 8 POST
    //           9 Authorization / 10 空 / 11 body / 12 空 / 13 @capture
    // （.http 的 ### 标题不是 @name——名字只来自 # @name 指令；md 标题才映射 name）
    let doc = "### ok\n# @name ok\n# @assert status == 200\nGET {{base_url}}/ok\n\n### login\n# @assert status == 200\nPOST {{base_url}}/login\nAuthorization: Bearer {{token}}\n\n{\"u\":1}\n\n@capture auth_token from body.token\n";
    std::fs::write(dir.path().join("suite.http"), doc).unwrap();

    let out = run(dir.path(), history.path(), &["ast", "suite.http"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("ast 输出必须为纯 JSON: {stdout}");

    // 顶层契约：字段集精确（4 键）
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 4, "{obj:?}");
    for k in ["schema_version", "source_path", "dependencies", "requests"] {
        assert!(obj.contains_key(k), "missing: {k}");
    }
    assert_eq!(v["schema_version"], "1.0");
    assert_eq!(v["source_path"], "suite.http");

    // 请求级契约
    let reqs = v["requests"].as_array().unwrap();
    assert_eq!(reqs.len(), 2);
    let ok = &reqs[0];
    assert_eq!(ok["number"], 1);
    assert_eq!(ok["method"], "GET");
    assert_eq!(ok["url"], "{{base_url}}/ok");
    assert_eq!(ok["name"], "ok");
    assert_eq!(ok["skip"], false);
    assert_eq!(ok["assertions"][0], "status == 200");
    assert_eq!(ok["captures"].as_array().unwrap().len(), 0);
    assert_eq!(ok["source_line"], 4);

    assert_eq!(
        reqs[1]["name"],
        serde_json::Value::Null,
        "### 标题不是 @name:.http 的名字只来自指令"
    );
    let login = &reqs[1];
    assert_eq!(login["method"], "POST");
    assert_eq!(
        login["headers"][0],
        serde_json::json!(["Authorization", "Bearer {{token}}"])
    );
    assert_eq!(login["body"], "{\"u\":1}");
    assert_eq!(
        login["captures"][0], "@capture auth_token from body.token",
        "capture 原始声明文本"
    );
    assert_eq!(login["source_line"], 8);
}

/// 错误路径：缺失文件诚实非零
#[test]
fn test_ast_missing_file_fails_honestly() {
    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let out = run(dir.path(), history.path(), &["ast", "nope.http"]);
    assert_ne!(out.status.code(), Some(0), "缺失文件必须非零");
}

/// markdown 路径同契约（围栏块行号 = 真实文档行）
#[test]
fn test_ast_markdown_contract() {
    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 标题 / 2 空 / 3 围栏 / 4 GET
    let doc = "标题\n\n```http\nGET {{base_url}}/ok\n\n# @assert status == 200\n```\n";
    std::fs::write(dir.path().join("doc.md"), doc).unwrap();

    let out = run(dir.path(), history.path(), &["ast", "doc.md"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let req = &v["requests"][0];
    assert_eq!(req["method"], "GET");
    assert_eq!(req["source_line"], 4, "md 围栏块方法行 = 围栏+1: {req}");
}
