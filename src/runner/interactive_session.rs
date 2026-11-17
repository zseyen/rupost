//! 交互式会话共享原语（Web 与 TUI 共用 SSOT）。
//!
//! 统一封装：
//! - 变量回灌（显式变量恒优先，已存在键不覆盖）
//! - 增量变量收割（跳过 `__` 内部键，仅提取新增或变更项）
//! - Cookie 桥导入 / 导出（统一告警口径，避免双端漂移）

use crate::runner::TestExecutor;
use crate::variable::VariableContext;
use serde_json::Value;
use std::collections::HashMap;

/// 将上次会话捕获的变量回灌到当前 `VariableContext`（显式值恒优先）。
pub fn backfill_session_vars(
    var_context: &mut VariableContext,
    captured: &HashMap<String, String>,
) {
    for (k, v) in captured {
        if var_context.get(k).is_none() {
            var_context.insert(k.clone(), v.clone());
        }
    }
}

/// 将上游持久化的 Cookie 状态导入交互执行器；失败时仅记录告警，不中断执行。
pub fn import_session_cookies(executor: &TestExecutor, cookie_state: Option<Value>) {
    if let Some(state_json) = cookie_state
        && let Err(e) = executor.import_cookie_state(state_json)
    {
        tracing::warn!("cookie bridge import failed: {}", e);
    }
}

/// 从交互执行器导出最新 Cookie 状态；失败时统一按 `ERR_COOKIE_PERSIST` 口径记录告警。
pub fn export_session_cookies(executor: &TestExecutor) -> Option<Value> {
    match executor.export_cookie_state() {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!(
                "{}: 状态导出失败: {}",
                crate::middleware::ERR_COOKIE_PERSIST,
                e
            );
            None
        }
    }
}

/// 收割本次执行产生的变量增量（跳过 `__` 内部键，仅保留新增或变化的值）。
pub fn harvest_session_deltas(
    var_context: &VariableContext,
    pre_run_keys: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut deltas = Vec::new();
    for (k, v) in var_context.variables() {
        if k.starts_with("__") {
            continue;
        }
        if pre_run_keys.get(k) != Some(v) {
            deltas.push((k.clone(), v.clone()));
        }
    }
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backfill_session_vars_preserves_explicit_keys() {
        let mut ctx = VariableContext::new();
        ctx.insert("TOKEN", "explicit-token");

        let captured = HashMap::from([
            ("TOKEN".to_string(), "stale-token".to_string()),
            ("USER_ID".to_string(), "42".to_string()),
        ]);
        backfill_session_vars(&mut ctx, &captured);

        assert_eq!(ctx.get("TOKEN").as_deref(), Some("explicit-token"));
        assert_eq!(ctx.get("USER_ID").as_deref(), Some("42"));
    }

    #[test]
    fn test_harvest_session_deltas_skips_internal_and_unchanged() {
        let mut ctx = VariableContext::new();
        ctx.insert("UNCHANGED", "v1");
        ctx.insert("UPDATED", "v2-new");
        ctx.insert("CREATED", "v3");
        ctx.set_default_scheme("https");

        let pre_run = HashMap::from([
            ("UNCHANGED".to_string(), "v1".to_string()),
            ("UPDATED".to_string(), "v2-old".to_string()),
        ]);

        let mut deltas = harvest_session_deltas(&ctx, &pre_run);
        deltas.sort();

        assert_eq!(
            deltas,
            vec![
                ("CREATED".to_string(), "v3".to_string()),
                ("UPDATED".to_string(), "v2-new".to_string()),
            ]
        );
    }

    #[test]
    fn test_import_and_export_session_cookies_roundtrip() {
        let executor = TestExecutor::with_ephemeral_cookies();
        import_session_cookies(&executor, None);

        let exported = export_session_cookies(&executor);
        assert!(exported.is_some());
        import_session_cookies(&executor, exported);
    }
}
