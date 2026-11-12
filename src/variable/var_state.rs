//! R-2（免重填主线）：捕获变量跨运行持久化（`--var-state`）。
//!
//! 契约：套件结束时把运行期捕获的变量（`@capture`/插件 `set_variables`）写入
//! 指定文件（0600 原子写，同快照安全防线）；下次运行经 `load_suite_context`
//! 回灌为**最低优先级层**（只补缺，绝不覆盖 `--var`/`.env`/toml 显式值）——
//! token 型 API 第二次运行免重跑登录。

use crate::Result;
use std::collections::HashMap;
use std::path::Path;

/// 持久化捕获变量（0600 + 原子替换，防 symlink/权限漂移）
pub fn save_var_state(path: &Path, vars: &HashMap<String, String>) -> Result<()> {
    let json = serde_json::to_vec_pretty(&vars)?;
    crate::security::fs::write_private_file_atomic(path, &json)
}

/// 读取持久化变量；文件缺失/损坏返回 None（损坏不静默——由调用方告警区分缺失与损坏）
pub fn load_var_state(path: &Path) -> Option<Result<HashMap<String, String>>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(Err(crate::RupostError::IoError(e))),
    };
    match serde_json::from_str::<HashMap<String, String>>(&raw) {
        Ok(vars) => Some(Ok(vars)),
        Err(e) => Some(Err(crate::RupostError::JsonError(e))),
    }
}

/// 回灌为最低优先级层：仅补 `context` 中未设置的键，显式值（--var/.env/toml）恒优先
pub fn backfill_missing(
    context: &mut crate::variable::VariableContext,
    vars: &HashMap<String, String>,
) -> usize {
    let mut filled = 0;
    for (k, v) in vars {
        if context.get(k).is_none() {
            context.insert(k.clone(), v.clone());
            filled += 1;
        }
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_state_roundtrip_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vars.json");
        assert!(load_var_state(&path).is_none(), "缺失返回 None");

        let mut vars = HashMap::new();
        vars.insert("auth_token".to_string(), "tok-42".to_string());
        save_var_state(&path, &vars).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "var-state 落盘必须 0600");

        let loaded = load_var_state(&path).unwrap().unwrap();
        assert_eq!(loaded.get("auth_token").map(String::as_str), Some("tok-42"));
    }

    #[test]
    fn test_var_state_corrupt_file_is_reported_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{not json").unwrap();
        let outcome = load_var_state(&path);
        assert!(
            outcome.is_some() && outcome.unwrap().is_err(),
            "损坏必须可报告"
        );
    }

    #[test]
    fn test_backfill_missing_never_overrides_explicit_values() {
        let mut ctx = crate::variable::VariableContext::new();
        ctx.insert("auth_token", "explicit-value");
        ctx.insert("fresh_key", "from-capture");
        let mut persisted = HashMap::new();
        persisted.insert("auth_token".to_string(), "stale-token".to_string());
        persisted.insert("only_in_state".to_string(), "filled".to_string());

        let filled = backfill_missing(&mut ctx, &persisted);
        assert_eq!(filled, 1, "仅补缺");
        assert_eq!(ctx.get("auth_token").as_deref(), Some("explicit-value"));
        assert_eq!(ctx.get("only_in_state").as_deref(), Some("filled"));
        assert_eq!(ctx.get("fresh_key").as_deref(), Some("from-capture"));
    }
}
