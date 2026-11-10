/// T2：链式值溯源（生成语义执行计划 §4，PRD `2026-11-04-p1-batch-*.md` §三 T2）。
/// 历史存的是已解析快照，链式数据流（登录产出 token → 下游注入）被打平；
/// 导出时逆向重建：上游 `# @capture <name> from <source>` + 下游 `{{name}}`。
/// 算法（PRD 定案）：按时间序遍历，下游高熵候选（≥8 字符）反序搜上游响应
/// （反序就近先得 = 就近消歧，tie 由遍历方向锁定）；scheme 前缀剥离后精确匹配；
/// Cookie 链删头（对现行 Cookie 模板化的显式例外）；派生型（Basic/HMAC）不识别。
use crate::history::model::HistoryEntry;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// 下游候选的最小熵（字符数）——防 `1`/`true` 等平凡值误报
const MIN_ENTROPY: usize = 8;

/// 一条溯源链：上游产出 → 下游注入
#[derive(Debug, Clone, PartialEq)]
pub struct TracedLink {
    /// 下游原始值（已剥离 scheme 前缀后的核心值）
    pub value: String,
    /// 上游条目下标
    pub upstream_idx: usize,
    /// capture 变量名（由产出路径叶子派生）
    pub capture_name: String,
    /// 上游 capture 源（`body.<path>` / `header.<name>`，现行 parser 语法）
    pub capture_source: String,
}

/// Cookie 链：上游 Set-Cookie 下发的 cookie 名集合（下游 Cookie 头删头依据）
#[derive(Debug, Default)]
pub struct TracePlan {
    pub links: Vec<TracedLink>,
    pub upstream_cookie_names: Vec<String>,
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
            .expect("valid uuid regex")
    })
}

/// 收集单条上游响应的产出值：body JSON 字符串叶子（`body.<path>`）+ 响应头（`header.<name>`）
fn upstream_values(entry: &HistoryEntry, entry_idx: usize, out: &mut Vec<(String, String, usize)>) {
    if let Some(body) = &entry.response.body
        && let Ok(json) = serde_json::from_str::<Value>(body)
    {
        collect_body_leaves(&json, "", entry_idx, out);
    }
    for (name, value) in entry.response.headers.iter() {
        if let Ok(v) = value.to_str() {
            out.push((
                format!("header.{}", name.as_str()),
                v.to_string(),
                entry_idx,
            ));
        }
    }
}

fn collect_body_leaves(
    value: &Value,
    path: &str,
    entry_idx: usize,
    out: &mut Vec<(String, String, usize)>,
) {
    match value {
        Value::String(s) => {
            let leaf = if path.is_empty() { "root" } else { path };
            out.push((format!("body.{leaf}"), s.clone(), entry_idx));
        }
        Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_body_leaves(v, &child, entry_idx, out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                collect_body_leaves(v, &format!("{path}[{i}]"), entry_idx, out);
            }
        }
        _ => {}
    }
}

/// 下游候选值剥离 scheme 前缀（`Bearer X`/`bearer X` → `X`；无空格原样）
fn strip_scheme(value: &str) -> &str {
    match value.split_once(' ') {
        Some((scheme, rest))
            if scheme.eq_ignore_ascii_case("bearer") || scheme.eq_ignore_ascii_case("token") =>
        {
            rest.trim()
        }
        _ => value,
    }
}

/// 下游候选收集：仅取 JSON 字符串叶子值
fn collect_candidate_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Object(map) => {
            for (_, v) in map {
                collect_candidate_values(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_candidate_values(v, out);
            }
        }
        _ => {}
    }
}

/// capture 变量名：产出路径叶子派生（slug 化；`body.data.access_token` → `access_token`，
/// 与 header 键名同规则；冲突由调用方 generate 管线的 used_names 语义外处理——v1 直接用叶子）
fn capture_name_from_source(source: &str) -> String {
    let leaf = source.rsplit('.').next().unwrap_or("value");
    let slug: String = leaf
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "value".to_string()
    } else {
        trimmed
    }
}

fn is_key_sensitive(key: &str, policy: Option<&crate::exporter::redact::RedactionPolicy>) -> bool {
    if let Some(p) = policy {
        p.is_sensitive(key)
    } else {
        crate::exporter::redact::is_sensitive_key(key)
    }
}

/// 构建溯源计划（纯函数，时间序即拓扑序）：
/// 对每个下游条目的候选值（Header/Body JSON 叶子/query），反序搜索上游产出精确命中。
pub fn build_trace_plan(entries: &[HistoryEntry]) -> TracePlan {
    build_trace_plan_with_policy(entries, None)
}

/// 接受显式脱敏策略的溯源计划构建入口
pub fn build_trace_plan_with_policy(
    entries: &[HistoryEntry],
    policy: Option<&crate::exporter::redact::RedactionPolicy>,
) -> TracePlan {
    let mut plan = TracePlan::default();

    // Cookie 链：上游 Set-Cookie 下发的 cookie 名
    for entry in entries {
        for (name, value) in entry.response.headers.iter() {
            if name.as_str().eq_ignore_ascii_case("set-cookie")
                && let Ok(v) = value.to_str()
                && let Some((cookie_name, _)) = v.split_once('=')
            {
                let trimmed = cookie_name.trim().to_string();
                if !trimmed.is_empty() && !plan.upstream_cookie_names.contains(&trimmed) {
                    plan.upstream_cookie_names.push(trimmed);
                }
            }
        }
    }

    let mut upstream: Vec<(String, String, usize)> = Vec::new(); // (source, value, 上游条目 idx)
    for (idx, entry) in entries.iter().enumerate() {
        // 1) 本条作为下游：候选值反序搜上游（就近先得）
        let mut candidates: Vec<String> = Vec::new();
        for (name, value) in entry.request.headers.iter() {
            // Cookie 链删头靠罐：Cookie 头不参与溯源（由上游 Cookie 罐自动接续）
            if name.as_str().eq_ignore_ascii_case("cookie") {
                continue;
            }
            if let Ok(v) = value.to_str() {
                candidates.push(v.to_string());
            }
        }
        // URL query 值也进候选（评审 F1：token 在 query 的 API 形态）；
        // 敏感键值跳过——凭据模板化管线负责（管线序：敏感键凭据 > T2）
        // 剥离尾随 #fragment，防止最后一个参数值带入锚点导致比对失效
        if let Some((_, raw_query)) = entry.request.url.split_once('?') {
            let query = raw_query.split_once('#').map(|(q, _)| q).unwrap_or(raw_query);
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=')
                    && !is_key_sensitive(k, policy)
                {
                    candidates.push(v.to_string());
                }
            }
        }
        if let Some(body) = &entry.request.body
            && let Ok(json) = serde_json::from_str::<Value>(body)
        {
            collect_candidate_values(&json, &mut candidates);
        }
        for candidate in candidates {
            let core = strip_scheme(&candidate).to_string();
            if core.len() < MIN_ENTROPY || uuid_re().is_match(&core) {
                continue; // 熵不足跳过；UUID 已由 T1 形状模板化承接（优先级：T2 命中 > T1）
            }
            if plan.links.iter().any(|l| l.value == core) {
                continue; // 同值一处识别一次
            }
            if let Some((source, _, up_idx)) = upstream.iter().rev().find(|(_, v, _)| *v == core) {
                plan.links.push(TracedLink {
                    value: core,
                    upstream_idx: *up_idx,
                    capture_name: capture_name_from_source(source),
                    capture_source: source.clone(),
                });
            }
        }
        // 2) 本条转为上游：产出值入池
        upstream_values(entry, idx, &mut upstream);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use reqwest::header::HeaderMap;

    fn entry(
        method: &str,
        url: &str,
        req_headers: &[(&str, &str)],
        body: Option<&str>,
        resp_status: u16,
        resp_headers: &[(&str, &str)],
        resp_body: Option<&str>,
    ) -> HistoryEntry {
        let mut rh = HeaderMap::new();
        for (k, v) in req_headers {
            rh.insert(
                k.parse::<reqwest::header::HeaderName>().expect("hn"),
                v.parse().expect("hv"),
            );
        }
        let mut sh = HeaderMap::new();
        for (k, v) in resp_headers {
            sh.insert(
                k.parse::<reqwest::header::HeaderName>().expect("hn"),
                v.parse().expect("hv"),
            );
        }
        HistoryEntry {
            id: "x".to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 10,
            request: RequestSnapshot {
                method: method.to_string(),
                url: url.to_string(),
                headers: rh,
                body: body.map(|s| s.to_string()),
                timeout: None,
            },
            source: Some("t".to_string()),
            response: ResponseMeta {
                status: resp_status,
                headers: sh,
                body: resp_body.map(|s| s.to_string()),
            },
        }
    }

    fn plan_of(entries: &[HistoryEntry]) -> TracePlan {
        build_trace_plan(entries)
    }

    /// 决策表 1：单命中——body 产出 → Bearer 注入（scheme 剥离 + capture 语法 body.<path>）
    #[test]
    fn test_single_hit_body_to_bearer() {
        let entries = vec![
            entry(
                "POST",
                "http://a/login",
                &[],
                None,
                200,
                &[],
                Some(r#"{"token":"tok-12345678"}"#),
            ),
            entry(
                "GET",
                "http://a/profile",
                &[("Authorization", "Bearer tok-12345678")],
                None,
                200,
                &[],
                None,
            ),
        ];
        let plan = plan_of(&entries);
        assert_eq!(plan.links.len(), 1);
        let l = &plan.links[0];
        assert_eq!(l.value, "tok-12345678");
        assert_eq!(
            l.capture_source, "body.token",
            "现行 parser 语法 body.<path>"
        );
        assert_eq!(l.capture_name, "token");
    }

    /// 决策表 2：多命中就近消歧（反序先得）+ tie 行锁定
    #[test]
    fn test_multiple_hits_nearest_wins() {
        let entries = vec![
            entry(
                "POST",
                "http://a/old",
                &[],
                None,
                200,
                &[],
                Some(r#"{"token":"dup-12345678"}"#),
            ),
            entry(
                "POST",
                "http://a/new",
                &[],
                None,
                200,
                &[],
                Some(r#"{"token":"dup-12345678"}"#),
            ),
            entry(
                "GET",
                "http://a/p",
                &[("Authorization", "Bearer dup-12345678")],
                None,
                200,
                &[],
                None,
            ),
        ];
        let plan = plan_of(&entries);
        assert_eq!(plan.links.len(), 1);
        // 反序就近：命中 index 1（new）——tie 行锁定遍历方向（变异⑤守护）
        assert_eq!(plan.links[0].upstream_idx, 1, "nearest upstream must win");
        assert_eq!(plan.links[0].capture_source, "body.token");
    }

    /// 决策表 3：熵不足跳过（<8 字符平凡值不溯源）
    #[test]
    fn test_low_entropy_skipped() {
        let entries = vec![
            entry(
                "POST",
                "http://a/login",
                &[],
                None,
                200,
                &[],
                Some(r#"{"token":"ok"}"#),
            ),
            entry(
                "GET",
                "http://a/p",
                &[("Authorization", "Bearer ok")],
                None,
                200,
                &[],
                None,
            ),
        ];
        assert!(plan_of(&entries).links.is_empty());
    }

    /// 决策表 4：派生型不识别（Basic 前缀）+ UUID 让位 T1
    #[test]
    fn test_derived_and_uuid_not_traced() {
        let entries = vec![
            entry(
                "POST",
                "http://a/x",
                &[],
                None,
                200,
                &[],
                Some(r#"{"id":"8f9c2d55-1a2b-4c3d-9e4f-5a6b7c8d9e0f"}"#),
            ),
            entry(
                "GET",
                "http://a/y",
                &[
                    ("Authorization", "Basic dXNlcjpwYXNz"),
                    ("X-Id", "8f9c2d55-1a2b-4c3d-9e4f-5a6b7c8d9e0f"),
                ],
                None,
                200,
                &[],
                None,
            ),
        ];
        let plan = plan_of(&entries);
        // Basic 派生值无上游命中 → 无链；UUID 让位 T1 → 无链
        assert!(
            plan.links.is_empty(),
            "derived and uuid excluded: {:?}",
            plan.links
        );
    }

    /// 决策表 5：Cookie 链删头——上游 Set-Cookie 名进 plan，下游 Cookie 头不溯源
    #[test]
    fn test_cookie_chain_header_omission() {
        let entries = vec![
            entry(
                "POST",
                "http://a/login",
                &[],
                None,
                200,
                &[("Set-Cookie", "sid=abc123def456; Path=/")],
                None,
            ),
            entry(
                "GET",
                "http://a/p",
                &[("Cookie", "sid=abc123def456")],
                None,
                200,
                &[],
                None,
            ),
        ];
        let plan = plan_of(&entries);
        assert_eq!(plan.upstream_cookie_names, vec!["sid".to_string()]);
        // Cookie 头不产生溯源链（删头靠罐，非 {{}} 注入）
        assert!(plan.links.is_empty());
    }

    /// header 产出 → 下游 header 注入（header.<name> 语法）
    #[test]
    fn test_header_produced_value() {
        let entries = vec![
            entry(
                "POST",
                "http://a/x",
                &[],
                None,
                200,
                &[("X-Request-Id", "req-99112233")],
                None,
            ),
            entry(
                "GET",
                "http://a/y",
                &[("X-Req", "req-99112233")],
                None,
                200,
                &[],
                None,
            ),
        ];
        let plan = plan_of(&entries);
        assert_eq!(plan.links.len(), 1);
        assert_eq!(plan.links[0].capture_source, "header.x-request-id");
    }

    /// URL query 中的溯源值末尾带 #fragment 时不应被锚点污染；自定义脱敏键不进溯源候选
    #[test]
    fn test_trace_query_with_fragment_and_custom_policy() {
        use crate::exporter::redact::RedactionPolicy;

        let entries = vec![
            entry(
                "POST",
                "http://a/login",
                &[],
                None,
                200,
                &[],
                Some(r#"{"session_code":"sess-12345678"}"#),
            ),
            entry(
                "GET",
                "http://a/dashboard?x_custom_sec=sec888&sid=sess-12345678#view-tab",
                &[],
                None,
                200,
                &[],
                None,
            ),
        ];

        // 未配置策略时：sid 正常命中上游 session_code（不会因为 #view-tab 污染而失效）
        let plan = build_trace_plan(&entries);
        assert_eq!(plan.links.len(), 1);
        assert_eq!(plan.links[0].value, "sess-12345678");
        assert_eq!(plan.links[0].capture_name, "session_code");

        // 配置自定义策略忽略 x_custom_sec：不会被误提取
        let policy = RedactionPolicy::new(["x_custom_sec"], Vec::<&str>::new());
        let plan_with_policy = build_trace_plan_with_policy(&entries, Some(&policy));
        assert_eq!(plan_with_policy.links.len(), 1);
        assert_eq!(plan_with_policy.links[0].value, "sess-12345678");
    }
}
