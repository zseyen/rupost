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
    }
}

// === 各个解析器实现 ===

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

    // 提取 from 后面的所有内容（保留空格，如正则表达式）
    let from_idx = content.find("from").unwrap();
    let source = content[from_idx + 4..].trim();

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
        message: "Invalid @stream_to syntax. Expected: @stream_to <path> [append|overwrite]".to_string(),
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

/// 解析时间字符串（支持 "5s", "1000ms", "2m"）
pub fn parse_duration(s: &str) -> ParseResult<Duration> {
    let s = s.trim();

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
        assert!(matches!(result, Metadata::StreamTo { ref path, append } if path == "./output.md" && !append));

        let result = parse_metadata("@stream_to ./log.txt append").unwrap().unwrap();
        assert!(matches!(result, Metadata::StreamTo { ref path, append } if path == "./log.txt" && append));
    }

    #[test]
    fn test_parse_forward_to() {
        let result = parse_metadata("@forward_to http://my-proxy.com").unwrap().unwrap();
        assert!(matches!(result, Metadata::ForwardTo(ref url) if url == "http://my-proxy.com"));
    }

    #[test]
    fn test_parse_unrecognized() {
        let result = parse_metadata("@unknown directive").unwrap();
        assert!(result.is_none());
    }
}
