//! curl → rupost `.http` 导入器（通用模块，非 AI）。
//!
//! 与 `src/exporter/` 互为逆变换：export 把 `.http` 变成 curl，import 把 curl 变回
//! 可执行的 `.http` 请求块。发射复用 `HttpGenerator`（合成 `HistoryEntry` 后交给
//! 既有发射器，凭据模板化/`@name`/JSON 美化全部免费获得）。

use crate::generator::HttpGenerator;
use crate::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use crate::{Result, RupostError};
use chrono::Utc;
use colored::Colorize;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

/// curl 导入请求参数
#[derive(Debug, Clone, Default)]
pub struct CurlImportRequest {
    /// 原始 curl 命令文本（可含或不含首词 `curl`）
    pub raw: String,
    /// 输出文件（已存在则拒绝）；None → stdout
    pub output: Option<String>,
    /// origin 模板化为 `{{base_url}}`（跨环境运行）
    pub templatize_host: bool,
}

/// POSIX 子集分词：空白切分 + 单引号（字面）/ 双引号（`\\"` `\\\\` `\\$` `\\`` 转义）+
/// 反斜杠转义。`$'..'`（ANSI-C 引用）显式不支持。任何情况下不产生 UTF-8 裸切片。
fn tokenize(input: &str) -> std::result::Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c2) => cur.push(c2),
                        None => return Err("未闭合的单引号".to_string()),
                    }
                }
            }
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            // 双引号内仅这四个转义有 POSIX 语义，其余反斜杠按字面保留
                            Some(e @ ('"' | '\\' | '$' | '`')) => cur.push(e),
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return Err("未闭合的双引号（转义符后无字符）".to_string()),
                        },
                        Some(c2) => cur.push(c2),
                        None => return Err("未闭合的双引号".to_string()),
                    }
                }
            }
            '\\' => {
                in_token = true;
                match chars.next() {
                    Some(c2) => cur.push(c2),
                    None => return Err("末尾悬空的反斜杠".to_string()),
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                return Err("暂不支持 ANSI-C 引用（$'..'）：请改用单引号或双引号".to_string());
            }
            c if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                in_token = true;
                cur.push(c);
            }
        }
    }
    if in_token {
        tokens.push(cur);
    }
    Ok(tokens)
}

/// 解析后的 curl 请求
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlCommand {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// curl flag 决策表（浏览器 Copy as cURL 的常规形态）：
/// - 支持：`-X/--request`、`-H/--header`（可重复）、`-d/--data/--data-raw/--data-ascii`、
///   `--url`、`-b/--cookie`、`--compressed` 与传输层杂项（`-s/-k/-L/-i/-v/-#`，忽略）
/// - 显式报错：`-F/--form`、`--data-urlencode`、`-u/--user`、`-G/--get`、未知参数
fn parse_tokens(tokens: &[String]) -> std::result::Result<CurlCommand, String> {
    let mut toks = tokens;
    if let Some(first) = toks.first()
        && first == "curl"
    {
        toks = &toks[1..];
    }
    if toks.is_empty() {
        return Err("空的 curl 命令".to_string());
    }

    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body: Option<String> = None;
    let mut has_body = false;

    let mut i = 0usize;
    while i < toks.len() {
        let t = toks[i].as_str();
        let value = || -> std::result::Result<String, String> {
            toks.get(i + 1)
                .cloned()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("参数 {t} 缺少值"))
        };

        match t {
            "-X" | "--request" => {
                if method.is_none() {
                    method = Some(value()?.to_ascii_uppercase());
                }
                i += 2;
            }
            "-H" | "--header" => {
                let h = value()?;
                let (name, val) = h
                    .split_once(':')
                    .ok_or_else(|| format!("Header 缺少冒号: {h}"))?;
                headers.push((name.trim().to_string(), val.trim().to_string()));
                i += 2;
            }
            "-d" | "--data" | "--data-raw" | "--data-ascii" => {
                // curl 语义：多个 -d 以 & 拼接；`-d ''` 为合法空体（配合 POST 语义），
                // 故此处不走 value() 的空串过滤；@file 为 MVP 限制外，显式拒绝
                let v = toks
                    .get(i + 1)
                    .cloned()
                    .ok_or_else(|| format!("参数 {t} 缺少值"))?;
                if v.starts_with('@') {
                    return Err(format!(
                        "不支持 -d {v}：@file 读取为本导入器 MVP 限制之外——请将文件内容展开为字面量后导入"
                    ));
                }
                body = Some(match body {
                    Some(prev) => format!("{prev}&{v}"),
                    None => v,
                });
                has_body = true;
                i += 2;
            }
            "--url" => {
                if url.is_none() {
                    url = Some(value()?);
                } else {
                    return Err("出现多个 URL".to_string());
                }
                i += 2;
            }
            "-b" | "--cookie" => {
                headers.push(("Cookie".to_string(), value()?));
                i += 2;
            }
            "-F" | "--form" | "--form-string" | "--data-urlencode" | "-u" | "--user" | "-G"
            | "--get" | "--upload-file" | "-T" => {
                return Err(format!(
                    "暂不支持 curl 参数 {t}（MVP 范围：-X/-H/-d/--data-raw/--url/-b）"
                ));
            }
            "--compressed" | "-s" | "--silent" | "-k" | "--insecure" | "-L" | "--location"
            | "-i" | "--include" | "-v" | "--verbose" | "-#" | "--progress-bar" => i += 1,
            t if t.starts_with('-') => {
                return Err(format!("暂不支持的 curl 参数: {t}"));
            }
            url_token => {
                if url.is_some() {
                    return Err("出现多个 URL".to_string());
                }
                url = Some(url_token.to_string());
                i += 1;
            }
        }
    }

    let url = url.ok_or_else(|| "未找到请求 URL".to_string())?;
    let method = method.unwrap_or_else(|| {
        if has_body {
            "POST".to_string()
        } else {
            "GET".to_string()
        }
    });

    Ok(CurlCommand {
        method,
        url,
        headers,
        body,
    })
}

/// 解析 + 合成 `HistoryEntry`（response 仅占位 status 200，供发射器生成 status 断言）
pub fn parse_curl_to_entry(raw: &str) -> Result<HistoryEntry> {
    // bash 反斜杠续行（`\` + 换行）先归一化——最常见的手写/脚本粘贴形态
    let normalized = raw.replace("\\\r\n", "").replace("\\\n", "");
    let tokens = tokenize(&normalized).map_err(RupostError::ParseError)?;
    let cmd = parse_tokens(&tokens).map_err(RupostError::ParseError)?;

    let mut header_map = HeaderMap::new();
    for (name, value) in &cmd.headers {
        header_map.append(
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| RupostError::ParseError(format!("非法 Header 名 {name}: {e}")))?,
            HeaderValue::from_str(value)
                .map_err(|e| RupostError::ParseError(format!("非法 Header 值: {e}")))?,
        );
    }

    Ok(HistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        duration_ms: 0,
        request: RequestSnapshot {
            method: cmd.method.clone(),
            url: cmd.url.clone(),
            headers: header_map,
            body: cmd.body.clone(),
            timeout: None,
        },
        source: Some("curl-import".to_string()),
        response: ResponseMeta {
            status: 200,
            headers: HeaderMap::new(),
            body: None,
        },
    })
}

/// F6 入口：导入 curl 命令 → `.http` 请求块（stdout 或 `-o` 文件，防覆盖）
pub fn run_curl_import(req: &CurlImportRequest) -> Result<()> {
    if req
        .raw
        .lines()
        .filter(|l| l.trim().starts_with("curl "))
        .count()
        > 1
    {
        return Err(RupostError::ParseError(
            "检测到多条 curl 命令：MVP 一次只导入一条，请分行逐条导入".to_string(),
        ));
    }
    let entry = parse_curl_to_entry(&req.raw)?;
    let (text, _) = HttpGenerator::generate_with_variables(&[entry], req.templatize_host)?;

    match &req.output {
        Some(path) => {
            if std::path::Path::new(path).exists() {
                return Err(RupostError::Other(format!(
                    "输出文件 {path} 已存在——导入不覆盖既有文件"
                )));
            }
            std::fs::write(path, &text)?;
            println!("{} 已导入 → {path}", "[✓]".green().bold());
        }
        None => print!("{text}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── tokenizer 决策表 ──

    #[test]
    fn test_tokenize_simple_and_quotes() {
        assert_eq!(
            tokenize("curl -X POST http://a.b -d '{\"a\":1}'").unwrap(),
            vec!["curl", "-X", "POST", "http://a.b", "-d", r#"{"a":1}"#]
        );
        assert_eq!(
            tokenize(r#"curl -d "{\"a\":\"b\"}" http://a.b"#).unwrap(),
            vec!["curl", "-d", r#"{"a":"b"}"#, "http://a.b"]
        );
    }

    #[test]
    fn test_tokenize_backslash_escape_and_multibyte() {
        assert_eq!(tokenize(r#"abc\ def"#).unwrap(), vec!["abc def"]);
        assert_eq!(tokenize("登录 https://api.local/测试").unwrap().len(), 2);
    }

    #[test]
    fn test_tokenize_unterminated_and_ansi_c_rejected() {
        assert!(tokenize("curl 'http://a.b").is_err());
        assert!(tokenize("curl \"http://a.b").is_err());
        assert!(tokenize("curl \\").is_err());
        assert!(tokenize("curl $'\\t' http://a.b").is_err());
    }

    // ── parse 决策表 ──

    fn parse(input: &str) -> std::result::Result<CurlCommand, String> {
        // 复刻生产入口的续行归一化
        let normalized = input.replace("\\\r\n", "").replace("\\\n", "");
        parse_tokens(&tokenize(&normalized).map_err(|e| e.to_string())?)
    }

    #[test]
    fn test_parse_get_simple() {
        let cmd = parse("curl https://api.example.com/users").unwrap();
        assert_eq!(cmd.method, "GET");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert!(cmd.body.is_none());
    }

    #[test]
    fn test_parse_post_json_with_headers() {
        let cmd = parse(
            "curl -X POST 'https://api.example.com/login' -H 'Content-Type: application/json' -H 'Authorization: Bearer eyJabc' -d '{\"user\":\"admin\"}'",
        )
        .unwrap();
        assert_eq!(cmd.method, "POST");
        assert_eq!(cmd.headers.len(), 2);
        assert_eq!(
            cmd.headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(cmd.body.as_deref(), Some(r#"{"user":"admin"}"#));
    }

    #[test]
    fn test_parse_data_implies_post_and_raw_flag() {
        let cmd = parse("curl --data-raw '{\"a\":1}' https://api.example.com").unwrap();
        assert_eq!(cmd.method, "POST");
        assert!(cmd.body.is_some());
    }

    #[test]
    fn test_parse_explicit_method_beats_data_default() {
        let cmd = parse("curl -X PUT -d 'x' https://api.example.com").unwrap();
        assert_eq!(cmd.method, "PUT");
    }

    #[test]
    fn test_parse_cookie_and_ignore_transport_flags() {
        let cmd = parse("curl --compressed -s -k -L -b 'session=abc' https://api.example.com/me")
            .unwrap();
        assert!(
            cmd.headers
                .iter()
                .any(|(k, v)| k == "Cookie" && v == "session=abc")
        );
    }

    #[test]
    fn test_parse_leading_curl_token_stripped_and_multiple_headers() {
        let cmd = parse("curl -H 'A: 1' -H 'B: 2' -H 'C: 3' https://api.example.com").unwrap();
        assert_eq!(cmd.headers.len(), 3);
    }

    #[test]
    fn test_parse_unsupported_flags_rejected() {
        for bad in [
            "curl -F file=@x https://a.b",
            "curl --data-urlencode a=1 https://a.b",
            "curl -u admin:pw https://a.b",
            "curl -G -d a=1 https://a.b",
            "curl --form-string a=b https://a.b",
        ] {
            assert!(parse(bad).is_err(), "应拒绝: {bad}");
        }
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse("curl").is_err(), "空命令");
        assert!(
            parse("curl -H 'no-colon' https://a.b").is_err(),
            "Header 缺冒号"
        );
        assert!(parse("curl https://a.b https://c.d").is_err(), "多 URL");
        assert!(parse("curl -X").is_err(), "-X 缺值");
        assert!(
            parse("curl --unknown-flag https://a.b").is_err(),
            "未知参数"
        );
    }

    #[test]
    fn test_tokenize_multiline_continuation_normalized() {
        // bash 反斜杠续行：parse_curl_to_entry 前已归一化（见 parse_curl_to_entry）
        let raw = "curl -X POST \\\n  http://a.b";
        // tokenizer 本身不处理续行——归一化在入口做；此处锁定 tokenzier 对字面 \\n 的行为
        let toks = tokenize(raw).unwrap();
        assert!(toks.iter().any(|t| t.contains("http://a.b")));
    }

    #[test]
    fn test_parse_duplicate_headers_preserved_via_append() {
        let cmd = parse("curl -H 'X-Tag: aaa' -H 'X-Tag: bbb' https://api.example.com").unwrap();
        assert_eq!(cmd.headers.len(), 2);
        assert_eq!(cmd.headers[0], ("X-Tag".to_string(), "aaa".to_string()));
        assert_eq!(cmd.headers[1], ("X-Tag".to_string(), "bbb".to_string()));
    }

    #[test]
    fn test_parse_multiple_data_concatenates_with_ampersand() {
        // curl 语义：多个 -d 以 & 拼接
        let cmd = parse("curl -d 'a=1' -d 'b=2' https://api.example.com").unwrap();
        assert_eq!(cmd.body.as_deref(), Some("a=1&b=2"));
    }

    #[test]
    fn test_parse_multiline_continuation_end_to_end() {
        // 三行 devtools 形态（反斜杠续行）→ 单条命令
        let cmd = parse(
            "curl -X POST 'https://api.example.com/login' \\\n  -H 'Content-Type: application/json' \\\n  -d 'a=1'",
        )
        .unwrap();
        assert_eq!(cmd.method, "POST");
        assert_eq!(cmd.url, "https://api.example.com/login");
        assert!(cmd.body.is_some());
    }

    // ── 语料（真实 devtools 形态）→ 合成 HistoryEntry → 发射器 ──

    #[test]
    fn test_corpus_devtools_post_end_to_end() {
        let entry = parse_curl_to_entry(
            "curl -X POST 'http://127.0.0.1:9000/api/auth/login?next=/home' \
             -H 'Content-Type: application/json' \
             -H 'Authorization: Bearer eyJhbGciOi' \
             --data-raw '{\"username\":\"admin\",\"password\":\"pw\"}'",
        )
        .unwrap();
        assert_eq!(entry.request.method, "POST");
        assert!(entry.request.url.contains("next=/home"));
        let (text, variables) = HttpGenerator::generate_with_variables(&[entry], true).unwrap();
        // 发射器复用红利：凭据模板化 + {{base_url}} + @name 全部免费获得
        assert!(text.contains("POST {{base_url}}/api/auth/login"));
        assert!(text.contains("authorization: Bearer {{authorization}}"));
        assert!(text.contains("# @name post_api_auth_login"));
        assert!(text.contains("# @assert status == 200"));
        assert!(variables.contains(&"authorization".to_string()));
        assert!(!text.contains("eyJhbGciOi"), "凭据不得明文落盘");
    }
}
