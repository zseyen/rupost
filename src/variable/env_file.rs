use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// `.env` 本地局部环境变量解析器
pub struct EnvFileParser;

impl EnvFileParser {
    /// 从字符串解析 `.env` 格式配置
    pub fn parse_str(content: &str) -> HashMap<String, String> {
        let mut variables = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            // 忽略空行以及注释行
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // 按第一个等号分割键和值
            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let mut val = val.trim();

                // 如果值被单引号或双引号包裹，则剥离外层引号
                let temp_val;
                if ((val.starts_with('"') && val.ends_with('"')) || (val.starts_with('\'') && val.ends_with('\''))) && val.len() >= 2 {
                    temp_val = val[1..val.len() - 1].to_string();
                    val = &temp_val;
                }

                // 排除空键
                if !key.is_empty() {
                    variables.insert(key.to_string(), val.to_string());
                }
            }
        }

        variables
    }

    /// 从指定路径的 `.env` 文件加载并解析
    pub fn parse_file<P: AsRef<Path>>(path: P) -> std::io::Result<HashMap<String, String>> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse_str(&content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_str_basic() {
        let content = r#"
# 这是一个注释
base_url = http://localhost:8080
api_key=secret-123

  trimmed_key   =   trimmed_val  
        "#;

        let vars = EnvFileParser::parse_str(content);
        assert_eq!(vars.len(), 3);
        assert_eq!(vars.get("base_url").unwrap(), "http://localhost:8080");
        assert_eq!(vars.get("api_key").unwrap(), "secret-123");
        assert_eq!(vars.get("trimmed_key").unwrap(), "trimmed_val");
    }

    #[test]
    fn test_parse_str_with_quotes() {
        let content = r#"
token1 = "bearer-token-123"
token2 = 'bearer-token-456'
empty_quote = ""
        "#;

        let vars = EnvFileParser::parse_str(content);
        assert_eq!(vars.len(), 3);
        assert_eq!(vars.get("token1").unwrap(), "bearer-token-123");
        assert_eq!(vars.get("token2").unwrap(), "bearer-token-456");
        assert_eq!(vars.get("empty_quote").unwrap(), "");
    }

    #[test]
    fn test_parse_str_multiple_equals() {
        let content = "connection_string = host=localhost;port=5432;db=test";
        let vars = EnvFileParser::parse_str(content);
        assert_eq!(vars.len(), 1);
        assert_eq!(
            vars.get("connection_string").unwrap(),
            "host=localhost;port=5432;db=test"
        );
    }

    #[test]
    fn test_parse_str_invalid_and_empty() {
        let content = r#"
=invalid_key
invalid_line_no_equals
# comment only
        "#;
        let vars = EnvFileParser::parse_str(content);
        assert!(vars.is_empty());
    }
}
