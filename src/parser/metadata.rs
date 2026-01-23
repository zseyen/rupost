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
    let parts: Vec<&str> = content.split_whitespace().collect();

    // 语法: <var_name> from <source> (注意：这里 content 已经去掉了 @capture)
    if parts.len() < 3 || parts[1] != "from" {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid @capture syntax. Expected: @capture <var> from <source>"),
        });
    }

    Ok(Metadata::Capture {
        var_name: parts[0].to_string(),
        source: parts[2].to_string(),
    })
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
    fn test_parse_unrecognized() {
        let result = parse_metadata("@unknown directive").unwrap();
        assert!(result.is_none());
    }
}
