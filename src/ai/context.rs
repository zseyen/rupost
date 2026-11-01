//! E2 上下文采集（P0 收尾）：把 `--last N` 历史 / `--response` 文件转化为
//! 脱敏后的 LLM user-prompt 附加段。采集与拼装全部确定性——AI 只看到最终文本。

use crate::ai::redactor::{
    redact_and_truncate_body, redact_free_text, redact_header_value, truncate_utf8_safe,
};
use crate::history::model::HistoryEntry;
use crate::history::storage::HistoryStorage;
use crate::{Result, RupostError};

/// 单条上下文条目的响应体字节上限（PRD US6：4KB/条）
pub const CONTEXT_ENTRY_MAX_BYTES: usize = 4096;

/// `--last N`：取最近 N 条历史（含全量响应体，recorder 已实证无截断）。
/// 空历史 → 显式错误（TC-X03）。
pub fn collect_last(n: usize) -> Result<Vec<HistoryEntry>> {
    let entries = HistoryStorage::new().tail(n)?;
    if entries.is_empty() {
        return Err(RupostError::Other(
            "暂无历史记录：--last 依赖 history，请先执行一次请求或 `rupost test`。".to_string(),
        ));
    }
    Ok(entries)
}

/// 单条历史 → 脱敏文本段（Gap Review P1 通道全覆盖：URL/Header/Body/错误文本）
fn entry_to_context(idx: usize, e: &HistoryEntry, mask: bool) -> String {
    let url = if mask {
        crate::ai::redactor::redact_url(&e.request.url)
    } else {
        e.request.url.clone()
    };
    let mut out = format!(
        "[{}] {} {} -> {:?}\n",
        idx, e.request.method, url, e.response.status
    );

    let headers: Vec<String> = e
        .request
        .headers
        .iter()
        .map(|(k, v)| {
            let k = k.as_str();
            let raw_v = v.to_str().unwrap_or("(binary)");
            if mask {
                format!("{}: {}", k, redact_header_value(k, raw_v))
            } else {
                format!("{}: {}", k, raw_v)
            }
        })
        .collect();
    if !headers.is_empty() {
        out.push_str(&format!("请求头: {}\n", headers.join("; ")));
    }

    let mut handle = |label: &str, body: &str| {
        let processed = if mask {
            redact_and_truncate_body(body, CONTEXT_ENTRY_MAX_BYTES)
        } else {
            truncate_utf8_safe(body, CONTEXT_ENTRY_MAX_BYTES)
        };
        out.push_str(&format!("{label}: {processed}\n"));
    };
    if let Some(body) = &e.request.body {
        handle("请求体", body);
    }
    if let Some(resp) = &e.response.body {
        handle("响应体", resp);
    }
    out
}

/// 历史条目 → 上下文文本（按时间序拼装，每条 4KB 截断）
pub fn history_context(entries: &[HistoryEntry], mask: bool) -> String {
    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&entry_to_context(i + 1, e, mask));
        out.push('\n');
    }
    truncate_utf8_safe(&out, CONTEXT_ENTRY_MAX_BYTES * entries.len().max(1))
}

/// `--response <FILE>`：读取响应体文件并脱敏截断（TC-X04/X05）
pub fn response_file_context(path: &str, mask: bool) -> Result<String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| RupostError::Other(format!("读取响应文件 {path} 失败: {e}")))?;
    let body = if mask {
        redact_and_truncate_body(&raw, CONTEXT_ENTRY_MAX_BYTES)
    } else {
        truncate_utf8_safe(&raw, CONTEXT_ENTRY_MAX_BYTES)
    };
    Ok(format!("[1] 响应文件 {path}\n响应体: {body}"))
}

/// 组装最终 user prompt（TC-X06）：自然语言期望 + 可选的脱敏响应上下文。
/// 空上下文/空脱敏结果时原样返回自然语言。
pub fn assemble_user_prompt(nl: &str, context: Option<String>) -> String {
    match context.filter(|c| !c.trim().is_empty()) {
        Some(c) => format!("{nl}\n\n[Actual response (redacted, truncated)]:\n{c}"),
        None => nl.to_string(),
    }
}

/// 错误文本防御脱敏（外部/历史 error 可能内嵌原始 URL）
pub fn safe_error_text(err: &str) -> String {
    redact_free_text(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use chrono::Utc;
    use reqwest::header::HeaderMap;

    fn entry(
        num: usize,
        url: &str,
        req_body: Option<&str>,
        resp_body: Option<&str>,
    ) -> HistoryEntry {
        HistoryEntry {
            id: format!("id-{num}"),
            timestamp: Utc::now(),
            duration_ms: 42,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: url.to_string(),
                headers: HeaderMap::new(),
                body: req_body.map(|s| s.to_string()),
                timeout: None,
            },
            source: Some("test".to_string()),
            response: ResponseMeta {
                status: 401,
                headers: HeaderMap::new(),
                body: resp_body.map(|s| s.to_string()),
            },
        }
    }

    #[test]
    fn test_entry_context_masks_header_and_bodies() {
        let mut e = entry(
            1,
            "http://api.local/v1/login?api_key=k1",
            Some(r#"{"password":"pw"}"#),
            Some(r#"{"access_token":"leak"}"#),
        );
        e.request
            .headers
            .insert("authorization", "Bearer real-token".parse().unwrap());
        let ctx = entry_to_context(1, &e, true);
        assert!(!ctx.contains("k1"), "URL query 必须掩码: {ctx}");
        assert!(!ctx.contains("real-token"), "鉴权头必须掩码: {ctx}");
        assert!(!ctx.contains("leak"), "响应体敏感键必须掩码: {ctx}");
        assert!(!ctx.contains("\"pw\""), "请求体敏感键必须掩码: {ctx}");
        assert!(
            ctx.contains("GET http://api.local/v1/login"),
            "方法与路径保留"
        );
    }

    #[test]
    fn test_entry_context_truncates_oversized_body() {
        let big = "x".repeat(10_000);
        let e = entry(1, "http://api.local/v1/big", Some(&big), None);
        let ctx = entry_to_context(1, &e, true);
        assert!(ctx.contains("[truncated]"));
        assert!(ctx.len() < 10_000);
    }

    #[test]
    fn test_history_context_orders_and_numbers() {
        let entries = vec![
            entry(1, "http://a.local/one", None, None),
            entry(2, "http://a.local/two", None, None),
        ];
        let ctx = history_context(&entries, true);
        assert!(ctx.contains("[1]"));
        assert!(ctx.contains("[2]"));
        let one = ctx.find("[1]").unwrap();
        let two = ctx.find("[2]").unwrap();
        assert!(one < two, "按时间序编号");
    }

    #[test]
    fn test_no_mask_keeps_raw_values() {
        let mut e = entry(
            1,
            "http://api.local/v1/login?api_key=k1",
            Some(r#"{"password":"pw"}"#),
            Some(r#"{"access_token":"leak"}"#),
        );
        e.request
            .headers
            .insert("authorization", "Bearer real-token".parse().unwrap());
        // mask=false（--no-mask）：原样保留（仅截断）
        let ctx = entry_to_context(1, &e, false);
        assert!(ctx.contains("k1"));
        assert!(ctx.contains("real-token"));
        assert!(ctx.contains("leak"));
        assert!(ctx.contains("\"pw\""));
    }

    #[test]
    fn test_assemble_user_prompt_includes_both_parts() {
        let prompt = assemble_user_prompt("验证登录", Some("响应体: {...}".to_string()));
        assert!(prompt.contains("验证登录"));
        assert!(prompt.contains("Actual response"));
        assert!(prompt.contains("响应体"));
        // 空上下文 → 原样
        assert_eq!(assemble_user_prompt("仅期望", None), "仅期望");
    }

    #[test]
    fn test_response_file_context_missing_file() {
        let res = response_file_context("/nonexistent/path/x.json", true);
        assert!(res.is_err());
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/path/x.json"));
    }

    #[test]
    fn test_response_file_context_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resp.json");
        std::fs::write(&path, "y".repeat(9000)).unwrap();
        let ctx = response_file_context(path.to_str().unwrap(), true).unwrap();
        assert!(ctx.contains("[truncated]"));
    }
}
