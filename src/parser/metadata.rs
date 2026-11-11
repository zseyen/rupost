use crate::parser::types::RequestMetadata;
use crate::parser::types::{Metadata, ParseError, ParseResult};
use crate::variable::capture::VariableCapture;
use std::time::Duration;

/// 主解析函数（统一入口）
pub fn parse_metadata(line: &str) -> ParseResult<Option<Metadata>> {
    let line = line.trim();
    if !line.starts_with('@') {
        return Ok(None);
    }

    // 分割指令和内容：@directive content
    let (directive, content) = match line.split_once(|c: char| c.is_whitespace()) {
        Some((d, c)) => (d, c.trim()),
        None => (line, ""),
    };

    match directive {
        "@name" => parse_name(content).map(Some),
        "@skip" => parse_skip(content).map(Some),
        "@timeout" => parse_timeout(content).map(Some),
        "@assert" => parse_assert(content).map(Some),
        "@capture" => parse_capture(content).map(Some),
        "@test" => Ok(Some(Metadata::Test)),
        "@sse" => parse_sse(content).map(Some),
        "@sse_timeout" => parse_sse_timeout(content).map(Some),
        "@sse_max_events" => parse_sse_max_events(content).map(Some),
        "@stream_to" => parse_stream_to(content).map(Some),
        "@forward_to" => parse_forward_to(content).map(Some),
        "@base_path" => parse_base_path(content).map(Some),
        "@websocket" => parse_websocket(content).map(Some),
        "@decoder" => parse_decoder(content).map(Some),
        "@diagnose" => parse_diagnose(content).map(Some),
        "@teardown" => Ok(Some(Metadata::Teardown)),
        "@sensitive" => parse_key_list(content).map(|keys| Some(Metadata::Sensitive(keys))),
        "@no-redact" => parse_key_list(content).map(|keys| Some(Metadata::NoRedact(keys))),
        "@auth" | "@transform" | "@sign" | "@plugin" => {
            parse_plugin_directive(directive, content).map(Some)
        }
        _ => Ok(None), // 未识别的元数据
    }
}

/// 应用元数据到 RequestMetadata
#[inline]
pub fn apply_metadata(metadata: &Metadata, target: &mut RequestMetadata) {
    match metadata {
        Metadata::Name(name) => {
            target.name = Some(name.clone());
        }
        Metadata::Skip(skip) => {
            target.skip = *skip;
        }
        Metadata::Timeout(duration) => {
            target.timeout = Some(*duration);
        }
        Metadata::Assert(expr) => {
            target.assertions.push(expr.clone());
        }
        Metadata::Capture { var_name, source } => {
            target
                .captures
                .push(VariableCapture::parse(var_name, source));
        }
        Metadata::Test => {
            target.is_test = true;
        }
        Metadata::Sse(sse) => {
            target.sse = *sse;
        }
        Metadata::SseTimeout(duration) => {
            target.sse_timeout = Some(*duration);
        }
        Metadata::SseMaxEvents(count) => {
            target.sse_max_events = Some(*count);
        }
        Metadata::StreamTo { path, append } => {
            target.stream_to = Some(path.clone());
            target.stream_to_append = *append;
        }
        Metadata::ForwardTo(url) => {
            target.forward_to = Some(url.clone());
        }
        Metadata::BasePath(_) => {}
        Metadata::Websocket(ws) => {
            target.websocket = *ws;
        }
        Metadata::Decoder(decoder) => {
            target.decoder = Some(decoder.clone());
        }
        Metadata::Diagnose(diagnose) => {
            target.diagnose = *diagnose;
        }
        Metadata::Teardown => {
            target.is_teardown = true;
        }
        Metadata::Sensitive(keys) => {
            target.sensitive_keys.extend(keys.clone());
        }
        Metadata::NoRedact(keys) => {
            target.no_redact_keys.extend(keys.clone());
        }
        Metadata::Plugin(directive) => {
            target.plugins.push(directive.clone());
        }
    }
}

// === 各个解析器实现 ===

/// 解析以逗号或空白字符分隔的键名列表，统一小写化并过滤空项
fn parse_key_list(content: &str) -> ParseResult<Vec<String>> {
    let keys: Vec<String> = content
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(keys)
}

/// 解析插件语义注解：@auth, @transform, @sign, @plugin
///
/// 语法契约（03 §4.1 唯一权威）：
/// - 阶段由注解本身决定（`@auth`=Prepare / `@transform`=Transform / `@sign`=Sign），
///   `stage=` / `priority=` 等底层调度魔数已彻底废除，出现即语法错误（05 ADR-D6）；
/// - 可选声明唯一拼写为 `?optional` 后缀（P1-1 裁决，`optional=true` 废除）；
/// - 参数分词采用引号感知状态机（支持 `"..."` / `'...'`），含空格参数不被撕裂。
fn parse_plugin_directive(directive: &str, content: &str) -> ParseResult<Metadata> {
    let content = content.trim();
    if content.is_empty() {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: format!("{} directive requires at least a plugin name", directive),
        });
    }

    let default_stage = match directive {
        "@auth" => crate::plugin::PluginStage::Prepare,
        "@transform" => crate::plugin::PluginStage::Transform,
        "@sign" => crate::plugin::PluginStage::Sign,
        _ => crate::plugin::PluginStage::Transform,
    };

    let mut tokens = tokenize_plugin_args(content).into_iter();
    let name = strip_surrounding_quotes(&tokens.next().unwrap()).to_string();

    let stage = default_stage;
    let mut optional = false;
    let mut params = std::collections::HashMap::new();

    for raw_token in tokens {
        let token = strip_surrounding_quotes(&raw_token).to_string();
        if token == "?optional" {
            optional = true;
        } else if let Some((k, v)) = token.split_once('=') {
            match k {
                // 已废除的底层调度魔数：显式报错引导迁移，绝不静默吞掉
                "stage" | "priority" => {
                    return Err(ParseError::InvalidMetadata {
                        line: 0,
                        message: format!(
                            "'{k}=' 已废除：阶段由注解决定（@auth/@transform/@sign），执行顺序为阶段内书写行序，无需 {k}"
                        ),
                    });
                }
                "optional" => {
                    return Err(ParseError::InvalidMetadata {
                        line: 0,
                        message: "'optional=' 已废除：请使用 ?optional 后缀声明可选降级"
                            .to_string(),
                    });
                }
                _ => {
                    params.insert(k.to_string(), strip_surrounding_quotes(v).to_string());
                }
            }
        }
        // 不含 '=' 的裸 token 与旧解析行为一致：忽略
    }

    let dir_name = directive.strip_prefix('@').unwrap_or(directive).to_string();
    Ok(Metadata::Plugin(crate::plugin::PluginDirective {
        directive: dir_name,
        name,
        stage,
        optional,
        params,
    }))
}

/// 引号感知的插件参数分词器：顶层空白分词，`"..."` / `'...'` 内的空白不撕裂
/// （引号剥除由 `strip_surrounding_quotes` 统一处理）。
fn tokenize_plugin_args(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for ch in content.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quote.is_some() => {
                escaped = true;
                cur.push(ch);
            }
            q @ ('\'' | '"') if in_quote == Some(q) => {
                in_quote = None;
                cur.push(ch);
            }
            q @ ('\'' | '"') if in_quote.is_none() => {
                in_quote = Some(q);
                cur.push(ch);
            }
            c if c.is_whitespace() && in_quote.is_none() => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// 剥除首尾配对引号（成对才剥除，不成对视为字面量）
fn strip_surrounding_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && (b[0] == b'"' && b[b.len() - 1] == b'"' || b[0] == b'\'' && b[b.len() - 1] == b'\'')
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_diagnose(content: &str) -> ParseResult<Metadata> {
    let value = if content.is_empty() {
        true
    } else {
        content.parse::<bool>().unwrap_or(true)
    };
    Ok(Metadata::Diagnose(value))
}

fn parse_name(content: &str) -> ParseResult<Metadata> {
    Ok(Metadata::Name(content.to_string()))
}

fn parse_skip(content: &str) -> ParseResult<Metadata> {
    let value = if content.is_empty() {
        true
    } else {
        content.parse::<bool>().unwrap_or(true)
    };
    Ok(Metadata::Skip(value))
}

fn parse_timeout(content: &str) -> ParseResult<Metadata> {
    let duration = parse_duration(content)?;
    Ok(Metadata::Timeout(duration))
}

fn parse_assert(content: &str) -> ParseResult<Metadata> {
    Ok(Metadata::Assert(content.to_string()))
}

fn parse_capture(content: &str) -> ParseResult<Metadata> {
    let mut parts = content.split_whitespace();
    let var_name = parts.next().ok_or_else(|| ParseError::InvalidMetadata {
        line: 0,
        message: "Invalid @capture syntax. Expected: @capture <var> from <source>".to_string(),
    })?;

    let from_keyword = parts.next().ok_or_else(|| ParseError::InvalidMetadata {
        line: 0,
        message: "Invalid @capture syntax. Expected: @capture <var> from <source>".to_string(),
    })?;

    if from_keyword != "from" {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: "Invalid @capture syntax. Expected: @capture <var> from <source>".to_string(),
        });
    }

    // 提取 from 后面的所有内容（保留空格，如正则表达式）。
    // 必须跳过变量名再定位 `from`：变量名本身含 "from"（如 from_id）时，
    // content.find("from") 会命中变量名内的子串导致源表达式错位。
    let after_var = &content[var_name.len()..];
    let from_idx = var_name.len() + after_var.find("from").unwrap();
    let source = content[from_idx + "from".len()..].trim();

    if source.is_empty() {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: "Invalid @capture syntax. Source cannot be empty".to_string(),
        });
    }

    Ok(Metadata::Capture {
        var_name: var_name.to_string(),
        source: source.to_string(),
    })
}

fn parse_sse(content: &str) -> ParseResult<Metadata> {
    let value = if content.is_empty() {
        true
    } else {
        content.parse::<bool>().unwrap_or(true)
    };
    Ok(Metadata::Sse(value))
}

fn parse_sse_timeout(content: &str) -> ParseResult<Metadata> {
    let duration = parse_duration(content)?;
    Ok(Metadata::SseTimeout(duration))
}

fn parse_sse_max_events(content: &str) -> ParseResult<Metadata> {
    let count: usize = content.parse().map_err(|_| ParseError::InvalidMetadata {
        line: 0,
        message: format!("Invalid max events count: {}", content),
    })?;
    Ok(Metadata::SseMaxEvents(count))
}

fn parse_stream_to(content: &str) -> ParseResult<Metadata> {
    let mut parts = content.split_whitespace();
    let path = parts.next().ok_or_else(|| ParseError::InvalidMetadata {
        line: 0,
        message: "Invalid @stream_to syntax. Expected: @stream_to <path> [append|overwrite]"
            .to_string(),
    })?;

    let mode = parts.next().unwrap_or("overwrite");
    let append = mode == "append";

    Ok(Metadata::StreamTo {
        path: path.to_string(),
        append,
    })
}

fn parse_forward_to(content: &str) -> ParseResult<Metadata> {
    if content.is_empty() {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: "@forward_to URL cannot be empty".to_string(),
        });
    }
    Ok(Metadata::ForwardTo(content.to_string()))
}

fn parse_base_path(content: &str) -> ParseResult<Metadata> {
    if content.is_empty() {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: "@base_path cannot be empty".to_string(),
        });
    }
    Ok(Metadata::BasePath(content.to_string()))
}

/// 解析时间字符串（支持 "5s", "1000ms", "2m" 以及带等号的 "= 3s", "= 100ms"）
pub fn parse_duration(s: &str) -> ParseResult<Duration> {
    let s = s.trim();
    let s = s.strip_prefix('=').unwrap_or(s).trim();

    if let Some(ms) = s.strip_suffix("ms") {
        let millis: u64 = ms.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_millis(millis))
    } else if let Some(sec) = s.strip_suffix('s') {
        let secs: u64 = sec.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_secs(secs))
    } else if let Some(min) = s.strip_suffix('m') {
        let mins: u64 = min.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_secs(mins * 60))
    } else {
        Err(ParseError::InvalidMetadata {
            line: 0,
            message: format!("Duration must end with 'ms', 's', or 'm': {}", s),
        })
    }
}

fn parse_websocket(content: &str) -> ParseResult<Metadata> {
    let value = if content.is_empty() {
        true
    } else {
        content.parse::<bool>().unwrap_or(true)
    };
    Ok(Metadata::Websocket(value))
}

fn parse_decoder(content: &str) -> ParseResult<Metadata> {
    if content.is_empty() {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: "@decoder value cannot be empty".to_string(),
        });
    }
    Ok(Metadata::Decoder(content.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name() {
        let result = parse_metadata("@name Test Request").unwrap().unwrap();
        assert!(matches!(result, Metadata::Name(ref s) if s == "Test Request"));
    }

    #[test]
    fn test_parse_skip() {
        let result = parse_metadata("@skip").unwrap().unwrap();
        assert!(matches!(result, Metadata::Skip(true)));

        let result = parse_metadata("@skip false").unwrap().unwrap();
        assert!(matches!(result, Metadata::Skip(false)));
    }

    #[test]
    fn test_parse_timeout() {
        let result = parse_metadata("@timeout 5s").unwrap().unwrap();
        assert!(matches!(result, Metadata::Timeout(d) if d == Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_duration_with_equals() {
        let d = parse_duration("= 3s").unwrap();
        assert_eq!(d, Duration::from_secs(3));
        let d = parse_duration("=100ms").unwrap();
        assert_eq!(d, Duration::from_millis(100));
    }

    #[test]
    fn test_parse_assert() {
        let result = parse_metadata("@assert status == 200").unwrap().unwrap();
        assert!(matches!(result, Metadata::Assert(ref s) if s == "status == 200"));
    }

    #[test]
    fn test_parse_capture() {
        let result = parse_metadata("@capture token from body.token")
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "token" && source == "body.token"
        ));
    }

    #[test]
    fn test_parse_capture_var_name_containing_from() {
        // 回归：变量名含 "from" 子串时，此前 content.find("from") 命中变量名，
        // 源表达式被错切成 "_id from body.id"，捕获静默失效。
        let result = parse_metadata("@capture from_id from body.id")
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "from_id" && source == "body.id"
        ));

        // 变量名恰好叫 from
        let result = parse_metadata("@capture from from body.x")
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "from" && source == "body.x"
        ));

        // regex 源内部的 "from" 不受影响
        let result = parse_metadata(r#"@capture from_date from regex "from (\d+)""#)
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "from_date" && source == r#"regex "from (\d+)""#
        ));
    }

    #[test]
    fn test_parse_capture_invalid() {
        let result = parse_metadata("@capture invalid syntax");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_capture_with_spaces() {
        let result =
            parse_metadata(r#"@capture token from regex <input name="csrf" value="([^"]+)">"#)
                .unwrap()
                .unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "token" && source == r#"regex <input name="csrf" value="([^"]+)">"#
        ));
    }

    #[test]
    fn test_parse_sse() {
        let result = parse_metadata("@sse").unwrap().unwrap();
        assert!(matches!(result, Metadata::Sse(true)));

        let result = parse_metadata("@sse false").unwrap().unwrap();
        assert!(matches!(result, Metadata::Sse(false)));
    }

    #[test]
    fn test_parse_sse_timeout() {
        let result = parse_metadata("@sse_timeout 10s").unwrap().unwrap();
        assert!(matches!(result, Metadata::SseTimeout(d) if d == Duration::from_secs(10)));
    }

    #[test]
    fn test_parse_sse_max_events() {
        let result = parse_metadata("@sse_max_events 50").unwrap().unwrap();
        assert!(matches!(result, Metadata::SseMaxEvents(50)));
    }

    #[test]
    fn test_parse_stream_to() {
        let result = parse_metadata("@stream_to ./output.md").unwrap().unwrap();
        assert!(
            matches!(result, Metadata::StreamTo { ref path, append } if path == "./output.md" && !append)
        );

        let result = parse_metadata("@stream_to ./log.txt append")
            .unwrap()
            .unwrap();
        assert!(
            matches!(result, Metadata::StreamTo { ref path, append } if path == "./log.txt" && append)
        );
    }

    #[test]
    fn test_parse_forward_to() {
        let result = parse_metadata("@forward_to http://my-proxy.com")
            .unwrap()
            .unwrap();
        assert!(matches!(result, Metadata::ForwardTo(ref url) if url == "http://my-proxy.com"));
    }

    #[test]
    fn test_parse_diagnose() {
        let result = parse_metadata("@diagnose").unwrap().unwrap();
        assert!(matches!(result, Metadata::Diagnose(true)));

        let result = parse_metadata("@diagnose false").unwrap().unwrap();
        assert!(matches!(result, Metadata::Diagnose(false)));

        let mut target = RequestMetadata::default();
        apply_metadata(&Metadata::Diagnose(true), &mut target);
        assert!(target.diagnose);
    }

    #[test]
    fn test_parse_unrecognized() {
        let result = parse_metadata("@unknown directive").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_websocket_and_decoder() {
        let result = parse_metadata("@websocket").unwrap().unwrap();
        assert!(matches!(result, Metadata::Websocket(true)));

        let result = parse_metadata("@websocket false").unwrap().unwrap();
        assert!(matches!(result, Metadata::Websocket(false)));

        let result = parse_metadata("@decoder messagepack").unwrap().unwrap();
        assert!(matches!(result, Metadata::Decoder(ref s) if s == "messagepack"));
    }
    /// M3：@teardown 解析与应用（无参数指令）
    #[test]
    fn test_teardown_metadata_parse_and_apply() {
        let result = parse_metadata("@teardown").unwrap();
        assert!(matches!(result, Some(Metadata::Teardown)));

        let mut target = RequestMetadata::default();
        apply_metadata(&Metadata::Teardown, &mut target);
        assert!(target.is_teardown);

        // 带尾部空白的形态
        let result = parse_metadata("  @teardown  ").unwrap();
        assert!(matches!(result, Some(Metadata::Teardown)));

        // 非 teardown 请求不受影响
        let mut plain = RequestMetadata::default();
        apply_metadata(&Metadata::Test, &mut plain);
        assert!(!plain.is_teardown);
    }

    /// Step 4 Red 测试：@sensitive 与 @no-redact 元数据解析与应用
    #[test]
    fn test_sensitive_and_no_redact_metadata() {
        // 1. @sensitive 支持多值逗号与空白分隔，且小写规范化
        let res = parse_metadata("@sensitive X-Sentry-Key, fastly_key  custom-token")
            .unwrap()
            .expect("should parse @sensitive");
        assert_eq!(
            res,
            Metadata::Sensitive(vec![
                "x-sentry-key".to_string(),
                "fastly_key".to_string(),
                "custom-token".to_string()
            ])
        );

        // 2. @no-redact 解析与小写规范化
        let res = parse_metadata("@no-redact next_token, cursor, Page_Token")
            .unwrap()
            .expect("should parse @no-redact");
        assert_eq!(
            res,
            Metadata::NoRedact(vec![
                "next_token".to_string(),
                "cursor".to_string(),
                "page_token".to_string()
            ])
        );

        // 3. 应用到 RequestMetadata
        let mut meta = RequestMetadata::default();
        apply_metadata(&res, &mut meta);
        assert_eq!(
            meta.no_redact_keys,
            vec!["next_token", "cursor", "page_token"]
        );

        let sensitive_meta = Metadata::Sensitive(vec!["x-secret".to_string()]);
        apply_metadata(&sensitive_meta, &mut meta);
        assert_eq!(meta.sensitive_keys, vec!["x-secret"]);
    }

    #[test]
    fn test_parse_plugin_directives_contract() {
        use crate::plugin::PluginStage;

        // 1. @auth 默认进入 Prepare 阶段，支持 ?optional
        let res = parse_metadata("@auth aws-sigv4 region=us-east-1 service=s3 ?optional")
            .unwrap()
            .expect("should parse @auth");
        if let Metadata::Plugin(p) = res {
            assert_eq!(p.directive, "auth");
            assert_eq!(p.name, "aws-sigv4");
            assert_eq!(p.stage, PluginStage::Prepare);
            assert!(p.optional);
            assert_eq!(p.params.get("region").unwrap(), "us-east-1");
            assert_eq!(p.params.get("service").unwrap(), "s3");
        } else {
            panic!("Expected Metadata::Plugin");
        }

        // 2. 引号感知分词：含空格的参数值不被撕裂（04 套件 5 契约）
        let res = parse_metadata(r#"@auth oauth2 scope="read:orders write:orders""#)
            .unwrap()
            .expect("should parse quoted scope");
        if let Metadata::Plugin(p) = res {
            assert_eq!(p.params.get("scope").unwrap(), "read:orders write:orders");
        } else {
            panic!("Expected Metadata::Plugin");
        }

        // 3. @transform 默认进入 Transform 阶段，支持显式引号
        let res = parse_metadata(r#"@transform aes-gcm key="my-secret-key""#)
            .unwrap()
            .expect("should parse @transform");
        if let Metadata::Plugin(p) = res {
            assert_eq!(p.directive, "transform");
            assert_eq!(p.name, "aes-gcm");
            assert_eq!(p.stage, PluginStage::Transform);
            assert!(!p.optional);
            assert_eq!(p.params.get("key").unwrap(), "my-secret-key");
        } else {
            panic!("Expected Metadata::Plugin");
        }

        // 4. @sign 默认进入 Sign 阶段
        let res = parse_metadata("@sign hmac-sha256")
            .unwrap()
            .expect("should parse @sign");
        if let Metadata::Plugin(p) = res {
            assert_eq!(p.directive, "sign");
            assert_eq!(p.name, "hmac-sha256");
            assert_eq!(p.stage, PluginStage::Sign);
        } else {
            panic!("Expected Metadata::Plugin");
        }

        // 5. @plugin 兼容写法（默认 Transform 阶段）+ ?optional
        let res = parse_metadata("@plugin custom-tool ?optional")
            .unwrap()
            .expect("should parse @plugin");
        if let Metadata::Plugin(ref p) = res {
            assert_eq!(p.directive, "plugin");
            assert_eq!(p.name, "custom-tool");
            assert_eq!(p.stage, PluginStage::Transform);
            assert!(p.optional);
        } else {
            panic!("Expected Metadata::Plugin");
        }

        // 6. 应用到 RequestMetadata
        let mut req_meta = RequestMetadata::default();
        apply_metadata(&res, &mut req_meta);
        assert_eq!(req_meta.plugins.len(), 1);
        assert_eq!(req_meta.plugins[0].name, "custom-tool");
    }

    /// 已废除的底层调度魔数（stage=/priority=/optional=true）必须显式报错，
    /// 绝不静默吞掉或降级（05 ADR-D6 防复活清单 + 反静默哲学）
    #[test]
    fn test_parse_plugin_abolished_magic_numbers_rejected() {
        for input in [
            "@plugin custom-tool stage=sign",
            "@auth aws-sigv4 priority=99",
            "@sign hmac-sha256 optional=true",
        ] {
            match parse_metadata(input) {
                Err(ParseError::InvalidMetadata { message, .. }) => {
                    assert!(
                        message.contains("已废除"),
                        "error should cite abolition, got: {message}"
                    );
                    let hint_matches = input.contains("stage=")
                        && message.contains("阶段由注解")
                        || input.contains("priority=")
                            && message.contains("无需 priority")
                        || input.contains("optional=") && message.contains("?optional");
                    assert!(hint_matches, "guidance mismatch for {input}: {message}");
                }
                other => panic!("expected InvalidMetadata for {input}, got: {other:?}"),
            }
        }
    }
}
