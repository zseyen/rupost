use crate::ai::redactor::{
    DEFAULT_SNIPPET_MAX_BYTES, redact_and_truncate_body, redact_assertion_value, redact_free_text,
    redact_url, truncate_utf8_safe,
};
use crate::ai::rules::{failure_signature, infer_possible_causes};
use crate::runner::TestResult;
use colored::Colorize;
use rupost_ai::{AiClient, FailedAssertionInfo, FailureAttributor, FailureContext};
use std::collections::HashSet;

/// 单次批量运行最多调用 AI 解释的**不同错误签名**上限（超出自动仅保留确定性提示）
pub const MAX_AI_EXPLAIN_CALLS: usize = 3;
/// 连续 AI 调用报错/超时熔断阈值
pub const CIRCUIT_BREAKER_THRESHOLD: usize = 2;

/// 跟踪单次 `rupost test --explain` 周期内的 AI 调用计数、签名去重与熔断状态
#[derive(Debug, Default)]
pub struct ExplainSession {
    pub ai_calls_made: usize,
    pub consecutive_ai_errors: usize,
    pub limit_warned: bool,
    pub no_key_warned: bool,
    explained_signatures: HashSet<String>,
}

impl ExplainSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 `TestResult` 构建脱敏后的 `FailureContext`
    pub fn build_context(r: &TestResult) -> FailureContext {
        let failed_assertions = r
            .assertions
            .iter()
            .filter(|a| !a.passed)
            .map(|a| FailedAssertionInfo {
                raw: a.raw.clone(),
                // Gap Review P1-2：expected/actual/message 可能携带响应体敏感值
                expected: redact_assertion_value(
                    &a.raw,
                    (!a.expected.is_empty()).then_some(a.expected.as_str()),
                ),
                actual: redact_assertion_value(&a.raw, a.actual.as_deref()),
                message: redact_assertion_value(&a.raw, a.message.as_deref()),
            })
            .collect();

        let response_body_snippet = r
            .response
            .as_ref()
            .map(|resp| redact_and_truncate_body(&resp.body, DEFAULT_SNIPPET_MAX_BYTES))
            .filter(|s| !s.is_empty());

        FailureContext {
            request_name: r.name.clone(),
            method: r.method.clone(),
            url: redact_url(&r.url),
            status_code: r.status,
            // Gap Review P1-1：reqwest 错误文本自带完整原始 URL，必须掩码
            error: r.error.as_deref().map(redact_free_text),
            failed_assertions,
            response_body_snippet,
            warnings: r.warnings.clone(),
        }
    }

    /// 便捷入口：走 B4 配置链（env > 用户级文件 > 本地探测）构建客户端后调用
    /// [`Self::explain_failure_with_client`]。解析失败输出黄色指引（PRD US1 验收条款）。
    pub async fn explain_failure(&mut self, r: &TestResult) {
        let client = match crate::ai::config::resolve().await {
            Ok(resolved) => Some(resolved.client),
            Err(msg) => {
                eprintln!("    {} {}", "⚠".yellow(), msg);
                None
            }
        };
        self.explain_failure_with_client(r, client.as_deref()).await;
    }

    /// 对单个失败请求输出归因分析（核心逻辑，client 可注入以便测试隔离）：
    /// 1. 始终先打印确定性规则版 `[Possible Causes]`（B1 基线）；
    /// 2. 若提供了 AI 客户端且未触发熔断/配额/签名去重，则追加打印 `[AI Explanation]`。
    pub async fn explain_failure_with_client(
        &mut self,
        r: &TestResult,
        client: Option<&dyn AiClient>,
    ) {
        if r.success || r.skipped {
            return;
        }

        let ctx = Self::build_context(r);
        Self::print_deterministic_causes(&ctx);

        // GAP-2（PRD US1）：按错误签名去重——同签名失败只消耗一次 AI 配额
        let signature = failure_signature(&ctx);
        if !self.explained_signatures.insert(signature) {
            return;
        }

        let Some(client) = client else {
            // PRD US1 And：未配置 AI 时输出黄色提示（每轮一次）
            if !self.no_key_warned {
                eprintln!("    {} AI 归因已跳过（未检测到可用 AI 配置）", "⚠".yellow());
                self.no_key_warned = true;
            }
            return;
        };

        if self.consecutive_ai_errors >= CIRCUIT_BREAKER_THRESHOLD {
            return;
        }

        if self.ai_calls_made >= MAX_AI_EXPLAIN_CALLS {
            if !self.limit_warned {
                eprintln!(
                    "    {} Reached per-run AI explanation limit ({MAX_AI_EXPLAIN_CALLS}); using deterministic rules for remaining failures (tip: use `--triage` for batch clustering).",
                    "ℹ".cyan()
                );
                self.limit_warned = true;
            }
            return;
        }

        self.ai_calls_made += 1;
        let attributor = FailureAttributor::new(client);
        match attributor.attribute(&ctx).await {
            Ok(advice) => {
                self.consecutive_ai_errors = 0;
                println!("    {}", "[AI Explanation]".cyan().bold());
                println!("      {}   {}", "Root Cause:".bold(), advice.root_cause);
                println!(
                    "      {} {}",
                    "Suggested Fix:".bold(),
                    advice.fix_suggestion
                );
                if let Some(assert_line) = advice.suggested_assert
                    && !assert_line.trim().is_empty()
                {
                    println!("      {} {}", "Suggested Assert:".bold(), assert_line);
                }
            }
            Err(e) => {
                self.consecutive_ai_errors += 1;
                let safe_err = truncate_utf8_safe(&e.to_string(), 160);
                eprintln!(
                    "    {} [AI Explanation unavailable: {}]",
                    "⚠".yellow(),
                    safe_err
                );
                // 熔断开启的一次性提示（Gap Review P3：此前静默降级无任何说明）
                if self.consecutive_ai_errors == CIRCUIT_BREAKER_THRESHOLD {
                    eprintln!(
                        "    {} AI 连续失败 {} 次，熔断已开启——本轮剩余失败将仅使用规则版归因（tip: `--triage` 可做批量聚类）",
                        "⚠".yellow(),
                        CIRCUIT_BREAKER_THRESHOLD
                    );
                }
            }
        }
    }

    fn print_deterministic_causes(ctx: &FailureContext) {
        let causes = infer_possible_causes(ctx);
        println!("    {}", "[Possible Causes]".yellow().bold());
        for c in causes {
            println!("      - {}", c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::AssertionResult;
    use crate::http::Response;
    use reqwest::header::HeaderMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn failed_result(request_number: usize, url: &str, status: u16) -> TestResult {
        let resp = Response::new(
            status,
            HeaderMap::new(),
            r#"{"error":"x"}"#.to_string(),
            Duration::from_millis(5),
            Duration::from_millis(2),
            Duration::from_millis(3),
        )
        .unwrap();
        let mut r = TestResult::success(
            request_number,
            Some("T".to_string()),
            "GET".to_string(),
            url.to_string(),
            resp,
        );
        r.success = false;
        r.assertions.push(AssertionResult::failure(
            "status == 200".to_string(),
            status.to_string(),
            "200".to_string(),
            "mismatch".to_string(),
        ));
        r
    }

    #[test]
    fn test_build_context_redacts_url_error_and_assertion_values() {
        let mut r = failed_result(1, "http://api.local/v1/login?api_key=topsecret", 401);
        r.response.as_mut().unwrap().body = r#"{"access_token":"leak-me"}"#.to_string();
        // Gap Review P1-1：reqwest 错误文本自带完整 URL
        r.error = Some(
            "error sending request for url (http://api.local/v1/login?api_key=topsecret)"
                .to_string(),
        );
        // Gap Review P1-2：敏感键断言的实际值
        r.assertions.push(AssertionResult::failure(
            "body.access_token exists".to_string(),
            "leak-me-too".to_string(),
            "non-empty".to_string(),
            "field missing".to_string(),
        ));

        let ctx = ExplainSession::build_context(&r);
        assert!(!ctx.url.contains("topsecret"));
        assert!(
            !ctx.error.as_deref().unwrap().contains("topsecret"),
            "error 中的 URL query 密钥必须被掩码: {:?}",
            ctx.error
        );
        let snippet = ctx.response_body_snippet.unwrap();
        assert!(!snippet.contains("leak-me"));
        // 敏感键断言的 expected/actual/message 整体掩码
        let sensitive = &ctx.failed_assertions[1];
        assert_eq!(sensitive.actual.as_deref(), Some("[REDACTED]"));
        assert_eq!(sensitive.expected.as_deref(), Some("[REDACTED]"));
        assert_eq!(sensitive.message.as_deref(), Some("[REDACTED]"));
        // 非敏感断言值保留
        assert_eq!(ctx.failed_assertions[0].actual.as_deref(), Some("401"));
    }

    /// 测试桩：计数调用并可控成败
    struct StubClient {
        fail: bool,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AiClient for StubClient {
        async fn generate_text(&self, _: &str, _: &str) -> rupost_ai::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(rupost_ai::AiError::TranslationFailed("boom".to_string()))
            } else {
                Ok(r#"{"root_cause":"rc","fix_suggestion":"fix"}"#.to_string())
            }
        }
    }

    fn stub(fail: bool) -> (StubClient, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            StubClient {
                fail,
                calls: calls.clone(),
            },
            calls,
        )
    }

    #[tokio::test]
    async fn test_quota_limits_distinct_signatures_to_max_calls() {
        let (client, calls) = stub(false);
        let mut session = ExplainSession::new();
        for i in 0..5 {
            let r = failed_result(i + 1, &format!("http://api.local/v1/unique-{i}"), 500);
            session.explain_failure_with_client(&r, Some(&client)).await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), MAX_AI_EXPLAIN_CALLS);
        assert!(session.limit_warned);
        assert_eq!(session.ai_calls_made, MAX_AI_EXPLAIN_CALLS);
    }

    #[tokio::test]
    async fn test_signature_dedup_skips_repeated_signatures() {
        let (client, calls) = stub(false);
        let mut session = ExplainSession::new();
        // 前两个同签名（同路径模式），后两个不同签名
        for (i, url) in [
            "http://api.local/v1/users/1",
            "http://api.local/v1/users/2",
            "http://api.local/v1/orders/1",
            "http://api.local/v1/orders/2",
        ]
        .into_iter()
        .enumerate()
        {
            let r = failed_result(i + 1, url, 401);
            session.explain_failure_with_client(&r, Some(&client)).await;
        }
        // 3 个不同签名（users 模式 1 个 + orders 模式 1 个…注意 orders/1 与 orders/2 同模式）
        // users/:id 1 签名 + orders/:id 1 签名 = 2 次调用
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(session.ai_calls_made, 2);
    }

    #[tokio::test]
    async fn test_circuit_breaker_stops_after_consecutive_errors() {
        let (client, calls) = stub(true);
        let mut session = ExplainSession::new();
        for i in 0..4 {
            let r = failed_result(i + 1, &format!("http://api.local/v1/err-{i}"), 500);
            session.explain_failure_with_client(&r, Some(&client)).await;
        }
        // 连续 2 次错误后熔断，第 3、4 个失败不再调用
        assert_eq!(calls.load(Ordering::SeqCst), CIRCUIT_BREAKER_THRESHOLD);
        assert_eq!(session.consecutive_ai_errors, CIRCUIT_BREAKER_THRESHOLD);
    }

    #[tokio::test]
    async fn test_no_key_warns_once_per_run() {
        let mut session = ExplainSession::new();
        for i in 0..3 {
            let r = failed_result(i + 1, &format!("http://api.local/v1/nk-{i}"), 401);
            session.explain_failure_with_client(&r, None).await;
        }
        assert!(session.no_key_warned, "无 Key 应置位一次性警告标记");
    }
}
