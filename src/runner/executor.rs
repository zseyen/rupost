use crate::{Result, RupostError};
use crate::assertion::{AssertionResult, evaluate_assertion, parse_assertion};
use crate::history::model::RequestSnapshot;
use crate::http::Client;
use crate::middleware::{CookieMiddleware, Middleware};
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver, capture_from_response};
use reqwest::header::{HeaderName, HeaderValue};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, Duration};
use tracing::{error, info};

pub struct TestExecutor {
    client: Client,
    cookie_middleware: Option<Arc<CookieMiddleware>>,
    pub debug: bool,
    pub debug_on_failure: bool,
}

impl TestExecutor {
    /// Create a new executor without cookie support.
    pub fn new() -> Self {
        Self {
            client: Client::new(None),
            cookie_middleware: None,
            debug: false,
            debug_on_failure: false,
        }
    }

    /// Set debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set debug-on-failure mode
    pub fn with_debug_on_failure(mut self, debug_on_failure: bool) -> Self {
        self.debug_on_failure = debug_on_failure;
        self
    }

    /// Create a new executor with cookie persistence.
    ///
    /// # Arguments
    /// * `cookie_file` - Path to the cookie storage file.
    pub fn with_cookies(cookie_file: PathBuf) -> Result<Self> {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file)?;
        let cookie_store = middleware.cookie_store();
        Ok(Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(Arc::new(middleware)),
            debug: false,
            debug_on_failure: false,
        })
    }

    /// Create a new executor with ephemeral (in-memory only) cookies.
    pub fn with_ephemeral_cookies() -> Self {
        let middleware = CookieMiddleware::new_ephemeral();
        let cookie_store = middleware.cookie_store();
        Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(Arc::new(middleware)),
            debug: false,
            debug_on_failure: false,
        }
    }

    /// 批量执行所有请求
    pub async fn execute_all(
        &self,
        parsed_file: ParsedFile,
        context: &mut VariableContext,
    ) -> Result<Vec<TestResult>> {
        let mut results = Vec::new();

        // Determine source once for the file
        let source = parsed_file
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        for (index, parsed_request) in parsed_file.requests.into_iter().enumerate() {
            let request_number = index + 1;

            // 检查是否跳过
            if parsed_request.should_skip() {
                results.push(TestResult::skipped(
                    request_number,
                    parsed_request.name().map(|s| s.to_string()),
                    parsed_request.method_or_default().to_string(),
                    parsed_request.url.clone(),
                ));
                continue;
            }

            let result = self
                .execute_one(
                    parsed_request,
                    request_number,
                    context,
                    Some(source.clone()),
                )
                .await;
            results.push(result);
        }

        Ok(results)
    }

    /// 执行单个请求
    pub async fn execute_one(
        &self,
        mut parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
        source: Option<String>,
    ) -> TestResult {
        // 开始计时
        let start = Instant::now();

        // 1. 变量替换
        // 替换 URL
        parsed.url = VariableResolver::resolve(&parsed.url, context);

        let method = parsed.method_or_default().to_string();
        let url = parsed.url.clone();
        let name = parsed.name().map(|s| s.to_string());

        // 检查未配置的 base_url/baseUrl 变量
        if parsed.url.contains("{{base_url}}") || parsed.url.contains("{{baseUrl}}") {
            return TestResult::error(
                request_number,
                name,
                method,
                url,
                RupostError::BaseUrlNotConfigured.to_user_friendly_string(),
                start.elapsed(),
            );
        }

        // 替换 Headers
        for (_key, value) in &mut parsed.headers {
            *value = VariableResolver::resolve(value, context);
            // header key 通常不需要替换，也可以根据需求支持
        }

        // 检查全局变量 context 中是否提供了 user_agent，如果是且请求中没有显式设置，则追加
        if let Some(ua) = context.get("user_agent") {
            let has_ua = parsed
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
            if !has_ua {
                parsed.headers.push(("User-Agent".to_string(), ua.to_string()));
            }
        }

        // 替换 Body
        if let Some(body) = &mut parsed.body {
            *body = VariableResolver::resolve(body, context);
        }


        // 提前保存断言列表和捕获配置（在 parsed 被移动前）
        let assertions_to_eval = parsed.metadata.assertions.clone();
        let captures_to_eval = parsed.metadata.captures.clone();

        // [History] 创建请求快照 (在 parsed 被 move 之前)
        let request_snapshot = {
            let mut headers = reqwest::header::HeaderMap::new();
            for (k, v) in &parsed.headers {
                if let (Ok(n), Ok(v)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::from_str(v),
                ) {
                    headers.insert(n, v);
                }
            }

            RequestSnapshot {
                method: method.clone(),
                url: url.clone(),
                headers,
                body: parsed.body.clone(),
            }
        };

        // 转换为 Request
        let request = match crate::http::Request::try_from(parsed) {
            Ok(req) => req,
            Err(e) => {
                return TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    RupostError::RequestBuildFailed(e.to_string()).to_user_friendly_string(),
                    start.elapsed(),
                );
            }
        };

        // 1. 进行 Pre-flight 探测 (带容错，不干扰主流程)
        let probe_result = if self.debug || self.debug_on_failure {
            use crate::http::timing::DiagnosticsProber;
            DiagnosticsProber::probe_connection(&url).await.ok()
        } else {
            None
        };

        // 执行请求
        match self.client.execute(request).await {
            Ok(response) => {
                // [Cookie] Middleware after_response hook (best effort)
                if let Some(ref mw) = self.cookie_middleware {
                    if let Err(e) = mw.after_response(&response).await {
                        error!("Cookie middleware error: {}", e);
                    }
                }

                // [History] 异步保存历史记录 (Best Effort)
                use crate::history::recorder::record_history;
                record_history(request_snapshot, &response, source);

                // 2. 变量捕获
                if !captures_to_eval.is_empty() {
                    match capture_from_response(
                        &response.body,
                        &response.headers,
                        &captures_to_eval,
                    ) {
                        Ok(captured_vars) => {
                            for (key, value) in &captured_vars {
                                info!("Captured variable: {} = '{}'", key, value);
                            }
                            context.extend(captured_vars);
                        }
                        Err(e) => {
                            error!("Failed to capture variables: {}", e);
                            // 捕获失败不应导致测试失败，但需要记录
                        }
                    }
                }

                // 3. 执行断言求值
                let mut assertion_results = Vec::new();

                for assertion_str in &assertions_to_eval {
                    // 先对断言字符串进行变量替换
                    let resolved_assertion = VariableResolver::resolve(assertion_str, context);

                    match parse_assertion(&resolved_assertion) {
                        Ok(assertion_expr) => {
                            let result = evaluate_assertion(&assertion_expr, &response);
                            assertion_results.push(result);
                        }
                        Err(e) => {
                            // 解析失败，生成错误断言结果
                            assertion_results
                                .push(AssertionResult::error(assertion_str.clone(), e));
                        }
                    }
                }

                // 创建成功的测试结果
                let mut test_result =
                    TestResult::success(request_number, name, method, url, response.clone());
                test_result.assertions = assertion_results;

                // 如果有断言失败，标记测试为失败
                if test_result.assertions.iter().any(|a| !a.passed) {
                    test_result.success = false;
                }

                // 判断是否需要挂载 timing
                let need_timing = self.debug || (self.debug_on_failure && !test_result.success);
                if need_timing {
                    if let Some((dns, tcp)) = probe_result {
                        test_result.timing = Some(crate::http::timing::RequestTiming {
                            dns_lookup: dns,
                            tcp_connect: tcp,
                            ttfb: response.ttfb,
                            transfer: response.transfer,
                        });
                    }
                }

                test_result
            }
            Err(e) => {
                let mut test_result = TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    RupostError::RequestExecutionFailed(e.to_string()).to_user_friendly_string(),
                    start.elapsed(),
                );

                let need_timing = self.debug || self.debug_on_failure;
                if need_timing {
                    if let Some((dns, tcp)) = probe_result {
                        test_result.timing = Some(crate::http::timing::RequestTiming {
                            dns_lookup: dns,
                            tcp_connect: tcp,
                            ttfb: Duration::from_millis(0),
                            transfer: Duration::from_millis(0),
                        });
                    }
                }

                test_result
            }
        }
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}
