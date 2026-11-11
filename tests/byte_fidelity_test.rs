//! 04 套件 4：请求体字节级保真验证（M1/M5 owner）。
//! 实现裁决（v2.1.0）：WireMock 工作在 HTTP 语义层拿不到原始字节——改用
//! tokio TcpListener 直收原始字节流，按 `\r\n\r\n` 切分后对 body 逐字节比对
//! （断言锁定 body 字节而非整包：Content-Length/分块行为不属守护目标）。

use tokio::io::AsyncReadExt;

/// 接收一条 HTTP 请求的原始字节（读至连接关闭或完整 body）
async fn read_raw_request(listener: &tokio::net::TcpListener) -> Vec<u8> {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        // 完整性启发：收到头部后按 Content-Length 判定 body 是否收齐
        if let Ok(text) = std::str::from_utf8(&buf)
            && let Some(header_end) = text.find("\r\n\r\n")
        {
            let headers = &text[..header_end];
            let body_so_far = &buf[header_end + 4..];
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        v.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if body_so_far.len() >= content_length {
                break;
            }
        }
    }
    buf
}

#[tokio::test]
async fn test_body_byte_level_fidelity_verbatim() {
    let listener = std::sync::Arc::new(tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap());
    let addr = listener.local_addr().unwrap();

    // 多级缩进 + 尾随零 + 空格 + 任意键序——serde_json 重排/压缩即红
    let body = "{\n    \"amount\": 100.00,\n\t\"memo\":\"尾随零  \",\n\"zz_last\": 1,\n    \"nested\": {\"deep\": [1, 2, 3.50]}\n}";
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("fidelity.http");
    std::fs::write(
        &case,
        format!("### t\nPOST http://{addr}/pay\nContent-Type: application/json\n\n{body}\n"),
    )
    .unwrap();

    let listener_for_task = listener.clone();
    let reader = tokio::spawn(async move { read_raw_request(listener_for_task.as_ref()).await });
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["t", case.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = out;

    let raw = reader.await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let header_end = text.find("\r\n\r\n").expect("HTTP 头边界");
    let wire_body = &text[header_end + 4..];

    // 逐字节保真：换行/缩进/tail 零/键序 100% 一致（raw_body 契约，01 §1.6）
    assert!(
        wire_body.trim_end_matches('\n').ends_with(body),
        "线上的 body 必须与文件书写逐字节一致:\n--- wire ---\n{wire_body}\n--- file ---\n{body}"
    );
    assert!(
        wire_body.contains("100.00"),
        "尾随零 100.00 不得被规约为 100"
    );
    assert!(wire_body.contains("\n    \""), "缩进不得被抹除");
    assert!(
        wire_body.find("\"zz_last\"").unwrap() < wire_body.find("\"nested\"").unwrap(),
        "键序不得被重排"
    );
}
