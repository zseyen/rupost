use crate::reporter::{JsonFileReport, JsonTestReport};
use rupost_ai::{FailedAssertionInfo, FailureCategory, FailureContext, SignatureBucket};
use std::collections::BTreeMap;

/// 最大分桶上限（防止超过 LLM 上下文窗口）
pub const MAX_SIGNATURE_BUCKETS: usize = 20;

/// 从 `JsonTestReport` 构建 `FailureContext`。
/// 防御性脱敏（Gap Review P3）：`--report-file` 产物已由生产方脱敏，但外部/手写报告
/// 不可信任——URL 与错误文本在此再做一层掩码。
pub fn context_from_json_test_report(r: &JsonTestReport) -> FailureContext {
    let failed_assertions = r
        .assertions
        .iter()
        .filter(|a| !a.passed)
        .map(|a| FailedAssertionInfo {
            raw: a.raw.clone(),
            expected: a.expected.clone(),
            actual: a.actual.clone(),
            message: a.message.clone(),
        })
        .collect();

    FailureContext {
        request_name: r.name.clone(),
        method: r.method.clone(),
        url: crate::ai::redactor::redact_url(&r.url),
        status_code: r.status,
        error: r
            .error
            .as_deref()
            .map(crate::ai::redactor::redact_free_text),
        failed_assertions,
        response_body_snippet: r.response_body_snippet.clone(),
        warnings: r.warnings.clone(),
    }
}

/// E2 确定性原因推导表（B1 基线 & B2 降级兜底）
pub fn infer_possible_causes(ctx: &FailureContext) -> Vec<String> {
    let mut causes = Vec::new();

    // 1. 未定义变量 / 前置捕获失败警告
    if ctx
        .warnings
        .iter()
        .any(|w| w.contains("未定义") || w.contains("capture") || w.contains("捕获"))
    {
        causes.push(
            "检测到未定义变量或前置 @capture 失败警告，请检查 --env 环境配置或前置依赖请求是否成功导出变量。"
                .to_string(),
        );
    }

    // 2. 网络/TLS/连接层错误 (status_code == None)
    if ctx.status_code.is_none()
        && let Some(err) = &ctx.error
    {
        let lower = err.to_ascii_lowercase();
        if lower.contains("connection refused") || lower.contains("connect") {
            causes.push(
                "目标服务拒绝连接 (Connection Refused)，请确认本地或远端测试服务已启动且端口正确。"
                    .to_string(),
            );
        } else if lower.contains("dns") || lower.contains("resolve") {
            causes.push(
                "域名解析失败 (DNS Error)，请检查请求 URL 主机名或本地网络设置。".to_string(),
            );
        } else if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
            causes.push(
                "TLS/证书握手失败，可执行 `rupost diagnose <URL>` 排查证书有效期与 SANs。"
                    .to_string(),
            );
        } else if lower.contains("timed out") || lower.contains("timeout") {
            causes.push("请求超时 (Timeout)，请检查下游服务响应延迟或防火墙策略。".to_string());
        } else {
            causes.push(format!(
                "网络传输层异常 ({err})，请求未能获得 HTTP 状态码。"
            ));
        }
    }

    // 3. HTTP 状态码特征
    if let Some(status) = ctx.status_code {
        match status {
            401 | 403 => causes.push(format!(
                "收到 HTTP {status} 鉴权失败，请检查 Authorization Header、Token 是否过期或 --cookie-file 会话状态。"
            )),
            404 => causes.push(
                "收到 HTTP 404 Not Found，请检查请求路由路径、API 版本前缀或被查询的资源 ID 是否存在。"
                    .to_string(),
            ),
            400 | 422 => causes.push(format!(
                "收到 HTTP {status} 参数校验失败，请核对请求 Body JSON 字段类型、必填项及 Content-Type 请求头。"
            )),
            429 => causes.push(
                "收到 HTTP 429 Too Many Requests，触发服务端限流，请降低 `--concurrency` 并发度或改用 `--mode sequential`。"
                    .to_string(),
            ),
            500..=599 => causes.push(format!(
                "收到 HTTP {status} 服务端异常，表明下游服务或网关发生内部错误，请结合响应片段排查后端日志。"
            )),
            _ => {}
        }
    }

    // 4. 断言不匹配特征
    for fa in &ctx.failed_assertions {
        if fa.raw.contains("body.") || fa.raw.contains("jsonpath") {
            if fa.actual.as_deref() == Some("null") || fa.actual.as_deref() == Some("<missing>") {
                causes.push(format!(
                    "断言 `{}` 提取值为空，可能是响应 JSON 结构发生漂移 (Schema Drift) 或错误响应未返回业务字段。",
                    fa.raw
                ));
            } else {
                causes.push(format!(
                    "响应体断言 `{}` 不匹配（期望 {:?}，实际 {:?}），请核对测试数据或更新断言预期。",
                    fa.raw, fa.expected, fa.actual
                ));
            }
        }
    }

    if causes.is_empty() {
        causes.push(
            "断言条件未满足，请对比上方 Expected 与 Actual 值，核对接口返回状态或测试断言配置。"
                .to_string(),
        );
    }

    causes
}

/// E2 熔断去重键（与 E5 分桶共享 `normalize_url_pattern` 与 status 语义；
/// 注意两者粒度有意不同：E5 对 401/5xx 忽略断言差异合桶，E2 签名含首条错误/断言）。
/// URL 整体替换为 `<url>`：同端点不同实例（error 文本内嵌 URL）必须同签名。
pub fn failure_signature(ctx: &FailureContext) -> String {
    let kind = ctx
        .error
        .as_ref()
        .and_then(|e| e.lines().next())
        .map(crate::ai::redactor::mask_urls)
        .or_else(|| {
            ctx.failed_assertions
                .first()
                .map(|a| a.raw.clone())
                .filter(|raw| !raw.is_empty())
        })
        .unwrap_or_else(|| "no-error".to_string());
    format!(
        "{}|{:?}|{}",
        normalize_url_pattern(&ctx.url),
        ctx.status_code,
        kind
    )
}

/// 将 URL 路径归约为模式签名（数字 ID 与 UUID 替换为 `:id`）
pub fn normalize_url_pattern(raw_url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(raw_url) {
        let segments: Vec<&str> = parsed
            .path_segments()
            .map(|c| c.collect())
            .unwrap_or_default();
        if segments.is_empty() {
            return "/".to_string();
        }
        let normalized: Vec<String> = segments
            .into_iter()
            .take(4)
            .map(|seg| {
                if !seg.is_empty()
                    && (seg.chars().all(|c| c.is_ascii_digit())
                        || (seg.len() >= 16
                            && seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-')))
                {
                    ":id".to_string()
                } else {
                    seg.to_string()
                }
            })
            .collect();
        format!("/{}", normalized.join("/"))
    } else {
        raw_url.to_string()
    }
}

/// 根据单个失败请求计算确定性分桶签名、默认分类与默认原因/建议
fn compute_bucket_key_and_category(
    r: &JsonTestReport,
) -> (String, FailureCategory, String, String) {
    let path_pattern = normalize_url_pattern(&r.url);

    if r.warnings.iter().any(|w| w.contains("未定义")) {
        return (
            format!("UnresolvedVariable @ {path_pattern}"),
            FailureCategory::VariableOrConfig,
            "请求包含未定义的模板变量，导致发送了字面量占位符".to_string(),
            "检查 `--env` 环境名称、`rupost.toml` 或前置请求的 `@capture` 变量名拼写".to_string(),
        );
    }

    match r.status {
        None => (
            format!("NetworkError @ {path_pattern}"),
            FailureCategory::NetworkAndTls,
            r.error
                .clone()
                .unwrap_or_else(|| "网络连接或超时异常".to_string()),
            "确认目标服务已启动且网络可达，或运行 `rupost diagnose <URL>` 检查连接".to_string(),
        ),
        Some(status @ (401 | 403)) => (
            format!("HTTP {status} @ {path_pattern}"),
            FailureCategory::Auth,
            format!("接口返回 HTTP {status} 鉴权或权限拒绝"),
            "检查登录前置步骤、Bearer Token 环境变量或 Cookie 会话文件".to_string(),
        ),
        Some(status @ 500..=599) => (
            format!("HTTP {status} @ {path_pattern}"),
            FailureCategory::Upstream5xx,
            format!("下游服务端返回 HTTP {status} 内部或网关错误"),
            "检查后端服务健康状态与服务端错误日志".to_string(),
        ),
        Some(status) => {
            let first_assertion = r
                .assertions
                .iter()
                .find(|a| !a.passed)
                .map(|a| a.raw.as_str())
                .unwrap_or("status check");
            let category = if first_assertion.contains("body.")
                && r.assertions
                    .iter()
                    .any(|a| !a.passed && a.actual.as_deref() == Some("null"))
            {
                FailureCategory::SchemaDrift
            } else {
                FailureCategory::AssertionMismatch
            };
            (
                format!("HTTP {status} [{first_assertion}] @ {path_pattern}"),
                category,
                format!("HTTP {status} 响应未通过断言 `{first_assertion}`"),
                "对比断言期望值与实际返回值，修正业务参数或更新过期的 `@assert` 断言".to_string(),
            )
        }
    }
}

/// 将 `JsonFileReport` 列表中的失败请求聚合为确定性签名桶（最多 20 个桶，超出部分并入尾桶）
pub fn build_signature_buckets(reports: &[JsonFileReport]) -> Vec<SignatureBucket> {
    struct TempGroup {
        signature: String,
        category: FailureCategory,
        all_requests: Vec<String>,
        sample_error: String,
        sample_snippet: Option<String>,
        default_root_cause: String,
        default_fix_suggestion: String,
    }

    let mut groups: BTreeMap<String, TempGroup> = BTreeMap::new();

    for file_rep in reports {
        for r in &file_rep.results {
            if r.success || r.skipped {
                continue;
            }
            let (sig, cat, root_cause, fix) = compute_bucket_key_and_category(r);
            let req_label = match &r.name {
                Some(name) => format!("{}#{} ({})", file_rep.file_path, r.request_number, name),
                None => format!(
                    "{}#{} ({} {})",
                    file_rep.file_path,
                    r.request_number,
                    r.method,
                    // 外部报告不可信任：URL 可能携带 query token，进 LLM/报告前必须脱敏
                    crate::ai::redactor::redact_url(&r.url)
                ),
            };
            // sample_error 会作为 LLM 上下文与报告字段，必须先脱敏
            // （error 文本自带完整原始 URL；assertion message 可能携带响应体敏感值）
            let err_summary = r
                .error
                .as_deref()
                .map(crate::ai::redactor::redact_free_text)
                .or_else(|| {
                    r.assertions.iter().find(|a| !a.passed).map(|a| {
                        let msg = a.message.as_deref().unwrap_or("assertion failed");
                        crate::ai::redactor::redact_free_text(&format!("{}: {}", a.raw, msg))
                    })
                })
                .unwrap_or_else(|| format!("HTTP {:?}", r.status));

            let entry = groups.entry(sig.clone()).or_insert_with(|| TempGroup {
                signature: sig,
                category: cat,
                all_requests: Vec::new(),
                sample_error: err_summary,
                // 幂等再脱敏：外部报告的 snippet 不经自产管线，防御性重处理
                sample_snippet: r.response_body_snippet.as_ref().map(|s| {
                    crate::ai::redactor::redact_and_truncate_body(
                        s,
                        crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
                    )
                }),
                default_root_cause: root_cause,
                default_fix_suggestion: fix,
            });
            entry.all_requests.push(req_label);
        }
    }

    let mut buckets: Vec<SignatureBucket> = groups
        .into_values()
        .map(|g| SignatureBucket {
            count: g.all_requests.len(),
            sample_requests: g.all_requests.iter().take(5).cloned().collect(),
            all_requests: g.all_requests,
            signature: g.signature,
            default_category: g.category,
            sample_error: g.sample_error,
            sample_snippet: g.sample_snippet,
            default_root_cause: g.default_root_cause,
            default_fix_suggestion: g.default_fix_suggestion,
            // bucket_id 在降序排序后统一重编
            bucket_id: 0,
        })
        .collect();

    // 按失败数量降序排列并重编 bucket_id；超过上限则将尾部并入第 20 个桶
    buckets.sort_by_key(|b| std::cmp::Reverse(b.count));
    for (idx, b) in buckets.iter_mut().enumerate() {
        b.bucket_id = idx;
    }

    if buckets.len() > MAX_SIGNATURE_BUCKETS {
        let tail = buckets.split_off(MAX_SIGNATURE_BUCKETS);
        if let Some(last) = buckets.last_mut() {
            // Gap Review P3：合并后元数据不得只代表最后一个签名
            let merged_kinds = tail.len();
            for t in tail {
                last.count += t.count;
                last.all_requests.extend(t.all_requests);
                for s in t.sample_requests {
                    if last.sample_requests.len() < 5 {
                        last.sample_requests.push(s);
                    }
                }
            }
            last.signature = format!("{} (and other minor signatures)", last.signature);
            last.default_root_cause = format!(
                "长尾聚合：{} 种低频签名合计 {} 条失败（各类原因见签名前缀与受影响请求）",
                merged_kinds + 1,
                last.count
            );
        }
    }

    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reporter::JsonAssertionReport;

    fn make_fail(num: usize, url: &str) -> JsonTestReport {
        JsonTestReport {
            request_number: num,
            name: None,
            method: "GET".to_string(),
            url: url.to_string(),
            status: Some(401),
            duration_ms: 10,
            success: false,
            error: None,
            skipped: false,
            assertions: vec![JsonAssertionReport {
                raw: "status == 200".to_string(),
                passed: false,
                message: Some("mismatch".to_string()),
                expected: Some("200".to_string()),
                actual: Some("401".to_string()),
                stream_event_index: None,
            }],
            warnings: vec![],
            diagnose_report: None,
            response_body_snippet: None,
        }
    }

    #[test]
    fn test_normalize_url_pattern_replaces_numeric_and_uuid_segments() {
        assert_eq!(
            normalize_url_pattern("https://api.example.com/v1/users/98765/orders"),
            "/v1/users/:id/orders"
        );
        assert_eq!(
            normalize_url_pattern(
                "https://api.example.com/v1/items/550e8400-e29b-41d4-a716-446655440000"
            ),
            "/v1/items/:id"
        );
    }

    #[test]
    fn test_infer_possible_causes_covers_401_and_warnings() {
        let ctx = FailureContext {
            request_name: None,
            method: "GET".to_string(),
            url: "http://localhost/api".to_string(),
            status_code: Some(401),
            error: None,
            failed_assertions: vec![],
            response_body_snippet: None,
            warnings: vec!["变量 {{token}} 未定义".to_string()],
        };
        let causes = infer_possible_causes(&ctx);
        assert!(causes.iter().any(|c| c.contains("未定义变量")));
        assert!(causes.iter().any(|c| c.contains("401")));
    }

    #[test]
    fn test_infer_possible_causes_network_errors() {
        let base = |error: &str| FailureContext {
            request_name: None,
            method: "GET".to_string(),
            url: "http://localhost/api".to_string(),
            status_code: None,
            error: Some(error.to_string()),
            failed_assertions: vec![],
            response_body_snippet: None,
            warnings: vec![],
        };
        assert!(
            infer_possible_causes(&base("connection refused"))
                .iter()
                .any(|c| c.contains("拒绝连接"))
        );
        assert!(
            infer_possible_causes(&base("dns resolution failed"))
                .iter()
                .any(|c| c.contains("域名解析"))
        );
        assert!(
            infer_possible_causes(&base("certificate expired"))
                .iter()
                .any(|c| c.contains("TLS"))
        );
        assert!(
            infer_possible_causes(&base("request timed out"))
                .iter()
                .any(|c| c.contains("超时"))
        );
        // 空规则命中时给兜底文案而非空列表
        let unknown = infer_possible_causes(&base("weird failure"));
        assert_eq!(unknown.len(), 1);
        assert!(unknown[0].contains("网络传输层异常"));
    }

    #[test]
    fn test_failure_signature_is_stable_for_same_pattern() {
        let mut a = make_fail(1, "http://api.local/v1/users/101");
        a.name = Some("Login".to_string());
        let b = make_fail(2, "http://api.local/v1/users/999");
        let sig_a = failure_signature(&context_from_json_test_report(&a));
        let sig_b = failure_signature(&context_from_json_test_report(&b));
        assert_eq!(sig_a, sig_b, "同模式不同实例的签名必须一致（E2 去重键）");
    }

    #[test]
    fn test_build_signature_buckets_groups_identical_patterns() {
        let reports = vec![JsonFileReport {
            file_path: "suite.http".to_string(),
            results: vec![
                make_fail(1, "http://api.local/v1/users/101"),
                make_fail(2, "http://api.local/v1/users/202"),
            ],
        }];

        let buckets = build_signature_buckets(&reports);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].count, 2);
        assert_eq!(buckets[0].default_category, FailureCategory::Auth);
        assert_eq!(buckets[0].all_requests.len(), 2);
    }

    #[test]
    fn test_build_signature_buckets_ignores_success_and_skipped() {
        let mut ok = make_fail(1, "http://api.local/v1/ok");
        ok.success = true;
        let mut skipped = make_fail(2, "http://api.local/v1/skip");
        skipped.skipped = true;
        let reports = vec![JsonFileReport {
            file_path: "s.http".to_string(),
            results: vec![ok, skipped, make_fail(3, "http://api.local/v1/fail")],
        }];
        let buckets = build_signature_buckets(&reports);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].count, 1);
    }

    #[test]
    fn test_build_signature_buckets_merges_overflow_into_last_bucket() {
        let mut results = Vec::new();
        for i in 0..(MAX_SIGNATURE_BUCKETS + 5) {
            results.push(make_fail(
                i + 1,
                &format!("http://api.local/v1/unique-path-{i}/detail"),
            ));
        }
        let reports = vec![JsonFileReport {
            file_path: "big.http".to_string(),
            results,
        }];
        let buckets = build_signature_buckets(&reports);
        assert_eq!(buckets.len(), MAX_SIGNATURE_BUCKETS);
        let total: usize = buckets.iter().map(|b| b.count).sum();
        assert_eq!(total, MAX_SIGNATURE_BUCKETS + 5, "溢出合并后计数必须守恒");
        assert!(
            buckets
                .last()
                .unwrap()
                .signature
                .contains("other minor signatures")
        );
        // Gap Review P3：合并后元数据不得只代表最后一个签名
        let last = buckets.last().unwrap();
        assert!(
            last.default_root_cause.contains("长尾聚合"),
            "聚合桶根因应说明为多种签名合并: {}",
            last.default_root_cause
        );
        assert_eq!(last.sample_requests.len(), 5, "样例列表应从被合并桶扩充");
    }
}
