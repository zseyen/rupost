//! M6-5（P1-5 残尾）：退出码三值契约归位测试（01 §2.1）。
//! 2 = 命令行参数错误或 AST 语法解析错误；1 = 断言失败/网络错误等执行失败。

use std::process::Command;

/// AST 语法错误（非法 HTTP 方法 → parser::ParseError）必须 exit 2，
/// 此前经 main 返回 Err 退出 1（冻结清单登记的归位残尾）。
#[test]
fn test_ast_syntax_error_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.http");
    std::fs::write(&bad, "FTP https://example.invalid\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["test", bad.to_str().unwrap(), "--no-history"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "AST 语法错误必须 exit 2，实际 {:?}，stderr:\n{stderr}",
        out.status.code(),
    );
    assert!(
        stderr.contains("解析错误") || stderr.contains("FTP"),
        "错误消息必须指明语法问题: {stderr}"
    );
}

/// 防假守护对照组：非语法类执行失败（连接拒绝）仍必须 exit 1。
#[test]
fn test_execution_failure_still_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let ok_syntax = dir.path().join("refused.http");
    // 语法合法但端口 1 无监听 → 连接拒绝 → 断言/网络失败 exit 1
    std::fs::write(
        &ok_syntax,
        "GET http://127.0.0.1:1/refused\n\n===\nassert status: 200\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["test", ok_syntax.to_str().unwrap(), "--no-history"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "执行失败（连接拒绝）必须维持 exit 1，实际 {:?}，stderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}
