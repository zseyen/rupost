use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// env 文件加载决策（走查缺口 #6a/6b：显式意图落空必须告警，默认探测缺失静默）。
/// 决策知识单一来源——`.env` / `--env-file` / `.env.<env>` 三个调用点共用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvFileOutcome {
    /// 存在且解析成功
    Loaded(HashMap<String, String>),
    /// 默认 `.env` 不存在（常态，静默跳过）
    Silent,
    /// 显式意图落空/损坏；携带出站 warning 文本
    Warn(String),
}

/// env 文件分类加载（纯函数，决策表测试锁定）。
///
/// `explicit` 区分加载意图：
/// - `true`（`--env-file` 显式指定 / `.env.<env>` 环境专属文件）：不存在、
///   目录形态、解析失败均产出 [`EnvFileOutcome::Warn`]；
/// - `false`（默认探测 `.env`）：缺失是常态，静默；存在但损坏仍告警。
pub fn load_env_file(path: &Path, explicit: bool) -> EnvFileOutcome {
    if !path.exists() {
        return if explicit {
            EnvFileOutcome::Warn(format!(
                "env 文件 '{}' 不存在，未加载（仅使用 toml/系统环境值）",
                path.display()
            ))
        } else {
            EnvFileOutcome::Silent
        };
    }
    if !path.is_file() {
        return EnvFileOutcome::Warn(format!(
            "env 文件 '{}' 是目录而非文件，未加载",
            path.display()
        ));
    }
    match EnvFileParser::parse_file(path) {
        Ok(vars) => EnvFileOutcome::Loaded(vars),
        Err(e) => EnvFileOutcome::Warn(format!(
            "env 文件 '{}' 解析失败: {e}，已跳过",
            path.display()
        )),
    }
}

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
                if ((val.starts_with('"') && val.ends_with('"'))
                    || (val.starts_with('\'') && val.ends_with('\'')))
                    && val.len() >= 2
                {
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
mod outcome_tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// 决策表：路径形态 × explicit 旗标 → 期望决策（走查缺口 #6 契约）
    #[test]
    fn test_load_env_file_decision_table() {
        let temp = TempDir::new().unwrap();
        let ok_path = temp.path().join("ok.env");
        std::fs::write(&ok_path, "token=abc\n").unwrap();
        let dir_path = temp.path().join("dir.env");
        std::fs::create_dir(&dir_path).unwrap();
        // env 解析器对无 '=' 行宽容跳过——不可读形态的解析失败分支由
        // test_load_env_file_unreadable_warns（unix 0o000）单独覆盖，
        // 此处锁定 missing/silent/loaded 三态
        let missing_path = PathBuf::from("/no/such/env_file_for_walkthrough_gap6.env");

        // 1. 成功加载（explicit 与否不影响 Loaded）
        for explicit in [true, false] {
            assert_eq!(
                load_env_file(&ok_path, explicit),
                {
                    let mut vars = HashMap::new();
                    vars.insert("token".to_string(), "abc".to_string());
                    EnvFileOutcome::Loaded(vars)
                },
                "explicit={explicit} 时成功文件应加载"
            );
        }

        // 2. 默认探测缺失 → Silent（常态静默）
        assert_eq!(load_env_file(&missing_path, false), EnvFileOutcome::Silent);

        // 3. 显式指定缺失 → Warn
        match load_env_file(&missing_path, true) {
            EnvFileOutcome::Warn(msg) => {
                assert!(msg.contains("不存在"), "告警应指明缺失: {msg}");
                assert!(msg.contains("未加载"), "告警应指明未加载: {msg}");
            }
            other => panic!("显式缺失应产生 Warn, got {other:?}"),
        }

        // 4. 目录形态（无论 explicit）→ Warn
        for explicit in [true, false] {
            match load_env_file(&dir_path, explicit) {
                EnvFileOutcome::Warn(msg) => {
                    assert!(msg.contains("目录"), "告警应指明目录形态: {msg}");
                }
                other => panic!("目录形态应产生 Warn (explicit={explicit}), got {other:?}"),
            }
        }
    }

    /// 存在但不可读 → Warn（解析失败分支）
    #[cfg(unix)]
    #[test]
    fn test_load_env_file_unreadable_warns() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("unreadable.env");
        std::fs::write(&path, "k=v\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        // root 用户无视 0o000，跳过（CI runner 非 root）
        if std::fs::read_to_string(&path).is_ok() {
            return;
        }

        match load_env_file(&path, true) {
            EnvFileOutcome::Warn(msg) => {
                assert!(msg.contains("解析失败") || msg.contains("读取"), "{msg}");
            }
            other => panic!("不可读文件应产生 Warn, got {other:?}"),
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
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
