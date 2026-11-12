//! 黑匣子契约快照与重放快照脱敏（02 §4 凭据掩码 SSOT）。

use crate::exporter::redact::{
    is_sensitive_key, mask_body_secrets, mask_inline_secrets, mask_pii, redact_free_text,
    redact_header_value, redact_url,
};

/// 自由文本脱敏 SSOT：URL 凭据 -> 内联键值/Bearer/Basic -> PII（邮箱/卡号/手机号）
fn mask_free_text_and_secrets(text: &str) -> String {
    mask_pii(&mask_inline_secrets(&redact_free_text(text)))
}

/// 判断断言表达式引用的字段名（引号之外的标识符）是否命中敏感键规则
fn assertion_references_sensitive_field(expr: &str) -> bool {
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    let mut ident = String::new();
    let flush_ident = |buf: &mut String| -> bool {
        if buf.is_empty() {
            return false;
        }
        let hit = is_sensitive_key(buf);
        buf.clear();
        hit
    };

    for ch in expr.chars() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                in_quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            if flush_ident(&mut ident) {
                return true;
            }
            in_quote = Some(ch);
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ident.push(ch);
        } else if flush_ident(&mut ident) {
            return true;
        }
    }
    flush_ident(&mut ident)
}

/// 替换表达式中的字符串字面量为 `"[REDACTED]"`，保留字段名与比较符便于排障
fn redact_quoted_literals(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for ch in expr.chars() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                out.push_str("[REDACTED]");
                out.push(ch);
                in_quote = None;
            }
        } else {
            out.push(ch);
            if ch == '"' || ch == '\'' {
                in_quote = Some(ch);
            }
        }
    }
    out
}

fn mask_snapshot_assertion(a: &mut crate::history::model::SnapshotAssertion) {
    if assertion_references_sensitive_field(&a.expression) {
        a.expression = redact_quoted_literals(&a.expression);
        if a.actual.is_some() {
            a.actual = Some("[REDACTED]".to_string());
        }
        if a.expected.is_some() {
            a.expected = Some("[REDACTED]".to_string());
        }
    } else {
        a.expression = mask_free_text_and_secrets(&a.expression);
        a.actual = a.actual.as_deref().map(mask_body_secrets);
        a.expected = a.expected.as_deref().map(mask_body_secrets);
    }
    a.message = a.message.as_deref().map(mask_free_text_and_secrets);
}

/// Secrets Masker（重放格式 SnapshotEntry）：URL 走 `redact_url`，headers 走 `redact_header_value`，
/// body 走 `mask_body_secrets`，error 走 `redact_free_text` + 内联凭据 + PII 擦除。
pub fn mask_snapshot_entry(entry: &mut crate::history::model::SnapshotEntry) {
    // 多值 Header（Set-Cookie 等同名多值）逐值掩码——drain 后 append 重建，
    // 非 UTF-8 值原样保留（掩码通道不损伤结构）
    let mask_headers = |map: &mut reqwest::header::HeaderMap| {
        let mut last_name: Option<reqwest::header::HeaderName> = None;
        let drained: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)> = map
            .drain()
            .filter_map(|(name, value)| {
                if let Some(n) = name {
                    last_name = Some(n);
                }
                last_name.clone().map(|n| (n, value))
            })
            .collect();
        for (name, value) in drained {
            match value.to_str() {
                Ok(val) => {
                    let masked = redact_header_value(name.as_str(), val);
                    if let Ok(hv) = reqwest::header::HeaderValue::from_str(&masked) {
                        map.append(name, hv);
                    }
                }
                Err(_) => {
                    map.append(name, value);
                }
            }
        }
    };
    entry.request.url = redact_url(&entry.request.url);
    mask_headers(&mut entry.request.headers);
    if let Some(ref mut resp) = entry.response {
        mask_headers(&mut resp.headers);
        resp.body = mask_body_secrets(resp.body.as_str());
    }
    entry.request.body = entry.request.body.as_ref().map(|b| mask_body_secrets(b));
    if let Some(ref mut err) = entry.error {
        err.message = mask_free_text_and_secrets(&err.message);
    }
}

/// 黑匣子契约快照 (ExecutionSnapshotEntry) 的 Secrets Masker：
/// 与 [`mask_snapshot_entry`] 共享同一组 redactor SSOT（`src/exporter/redact.rs`），
/// 覆盖 URL、Headers、Body、Error、Assertions 与 Warnings 全展示面（02 §2/§4）。
pub fn mask_execution_snapshot_entry(entry: &mut crate::history::model::ExecutionSnapshotEntry) {
    let mask_pairs = |headers: &mut Vec<(String, String)>| {
        for (name, value) in headers.iter_mut() {
            *value = redact_header_value(name, value);
        }
    };
    entry.request.url = redact_url(&entry.request.url);
    mask_pairs(&mut entry.request.headers);
    if let Some(ref mut resp) = entry.response {
        mask_pairs(&mut resp.headers);
        resp.body = mask_body_secrets(resp.body.as_str());
    }
    entry.request.body = entry.request.body.as_ref().map(|b| mask_body_secrets(b));
    if let Some(ref mut err) = entry.error {
        err.message = mask_free_text_and_secrets(&err.message);
    }
    for assertion in &mut entry.assertions {
        mask_snapshot_assertion(assertion);
    }
    for warning in &mut entry.warnings {
        *warning = mask_free_text_and_secrets(warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{
        ExecutionErrorDetail, ExecutionOutcome, ExecutionSnapshotEntry, RequestSnapshot,
        SnapshotAssertion, SnapshotEntry, SnapshotRequest,
    };
    use reqwest::header::HeaderMap;

    fn sample_entry() -> ExecutionSnapshotEntry {
        ExecutionSnapshotEntry {
            request_number: 1,
            name: None,
            file_path: "t.http".to_string(),
            success: false,
            outcome: ExecutionOutcome::Fail,
            status: Some(401),
            duration_us: 1_000,
            plugin_duration_us: None,
            request: SnapshotRequest {
                method: "GET".to_string(),
                url: "https://alice:toppass@api.example.com/x?access_token=leaky_tok&page=2"
                    .to_string(),
                headers: vec![
                    ("Authorization".to_string(), "Bearer real-token".to_string()),
                    ("Content-Type".to_string(), "application/json".to_string()),
                ],
                body: Some(r#"{"password": "hunter2", "city": "Shanghai"}"#.to_string()),
            },
            response: None,
            error: Some(ExecutionErrorDetail {
                kind: "HttpTransportError".to_string(),
                message: "auth failed for Bearer err_tok_123".to_string(),
                duration_ms: 1,
                occurred_at: chrono::Utc::now(),
            }),
            timing_waterfall: None,
            socket_diagnostics: None,
            assertions: vec![SnapshotAssertion {
                expression: "body.token == \"assert_expr_secret\"".to_string(),
                passed: false,
                actual: Some("sk_live_actual_secret".to_string()),
                expected: Some("expected_secret_token".to_string()),
                message: Some("https://u:p@host/cb?token=msg_secret".to_string()),
            }],
            warnings: vec![
                "Authorization: Bearer warn_bearer_secret".to_string(),
                "redirect https://u:p@h/x?api_key=warn_query_secret".to_string(),
            ],
        }
    }

    #[test]
    fn test_mask_execution_snapshot_entry_full_surface() {
        let mut entry = sample_entry();
        mask_execution_snapshot_entry(&mut entry);

        // 1. Headers & Body
        assert_ne!(entry.request.headers[0].1, "Bearer real-token");
        assert!(entry.request.headers[0].1.contains("[REDACTED]"));
        assert!(!entry.request.body.as_ref().unwrap().contains("hunter2"));

        // 2. URL userinfo & sensitive query, preserving non-sensitive param
        assert!(!entry.request.url.contains("toppass"));
        assert!(!entry.request.url.contains("leaky_tok"));
        assert!(entry.request.url.contains("page=2"));

        // 3. Assertions (actual/expected/message/expression)
        let a = &entry.assertions[0];
        assert!(!a.expression.contains("assert_expr_secret"));
        assert!(
            !a.actual
                .as_deref()
                .unwrap()
                .contains("sk_live_actual_secret")
        );
        assert!(
            !a.expected
                .as_deref()
                .unwrap()
                .contains("expected_secret_token")
        );
        assert!(!a.message.as_deref().unwrap().contains("msg_secret"));
        assert!(!a.message.as_deref().unwrap().contains("u:p"));

        // 4. Warnings inline secrets + URL query
        let joined = entry.warnings.join("\n");
        assert!(!joined.contains("warn_bearer_secret"), "{joined}");
        assert!(!joined.contains("warn_query_secret"), "{joined}");
    }

    #[test]
    fn test_mask_snapshot_entry_redacts_url() {
        let mut entry = SnapshotEntry {
            id: "1".to_string(),
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: "https://bob:s3cr3t@api.example.com/v1?api_key=query_secret&v=1".to_string(),
                headers: HeaderMap::new(),
                body: None,
                timeout: None,
            },
            response: None,
            error: None,
        };
        mask_snapshot_entry(&mut entry);
        assert!(!entry.request.url.contains("s3cr3t"));
        assert!(!entry.request.url.contains("query_secret"));
        assert!(entry.request.url.contains("v=1"));
    }
}
