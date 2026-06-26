use crate::assertion::AssertionResult;
use crate::history::model::RequestSnapshot;
use crate::http::{Client, Request, Response, SseParser};
use crate::middleware::{CookieMiddleware, Middleware};
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::types::TestResult;
use crate::variable::{
    VariableContext, VariableResolver, capture::VariableCapture,
};
use crate::{Result, RupostError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::error;

#[derive(Clone)]
pub enum ExecutorMiddleware {
    Cookie(Arc<CookieMiddleware>),
    Routing(Arc<crate::middleware::routing::RoutingMiddleware>),
}

impl Middleware for ExecutorMiddleware {
    async fn before_request(&self, req: &mut Request) -> Result<()> {
        match self {
            Self::Cookie(mw) => mw.before_request(req).await,
            Self::Routing(mw) => mw.before_request(req).await,
        }
    }

    async fn after_response(&self, resp: &Response) -> Result<()> {
        match self {
            Self::Cookie(mw) => mw.after_response(resp).await,
            Self::Routing(mw) => mw.after_response(resp).await,
        }
    }
}

pub struct TestExecutor {
    client: Client,
    cookie_middleware: Option<Arc<CookieMiddleware>>,
    routing_middleware: Option<Arc<crate::middleware::routing::RoutingMiddleware>>,
    middlewares: Vec<ExecutorMiddleware>,
    pub debug: bool,
    pub debug_on_failure: bool,
}

impl TestExecutor {
    /// Check if the executor has cookie support enabled.
    pub fn has_cookies(&self) -> bool {
        self.cookie_middleware.is_some()
    }

    /// 导出当前执行器的 Cookie 状态为 JSON Value (如果启用了 Cookie)
    pub fn export_cookie_state(&self) -> Option<serde_json::Value> {
        self.cookie_middleware
            .as_ref()
            .and_then(|mw| mw.export_cookie_state().ok())
    }

    /// 将 Cookie 状态导入到当前的执行器中
    pub fn import_cookie_state(&self, state: serde_json::Value) -> Result<()> {
        if let Some(ref mw) = self.cookie_middleware {
            mw.import_cookie_state(state)?;
        }
        Ok(())
    }

    /// Create a new executor without cookie support.
    pub fn new() -> Self {
        Self {
            client: Client::new(None),
            cookie_middleware: None,
            routing_middleware: None,
            middlewares: Vec::new(),
            debug: false,
            debug_on_failure: false,
        }
    }

    pub fn with_middleware(
        mut self,
        middleware: Arc<crate::middleware::routing::RoutingMiddleware>,
    ) -> Self {
        self.routing_middleware = Some(middleware.clone());
        self.middlewares.push(ExecutorMiddleware::Routing(middleware));
        self
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
        let cookie_middleware = Arc::new(middleware);
        Ok(Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(cookie_middleware.clone()),
            routing_middleware: None,
            middlewares: vec![ExecutorMiddleware::Cookie(cookie_middleware)],
            debug: false,
            debug_on_failure: false,
        })
    }

    /// Create a new executor with ephemeral (in-memory only) cookies.
    pub fn with_ephemeral_cookies() -> Self {
        let middleware = CookieMiddleware::new_ephemeral();
        let cookie_store = middleware.cookie_store();
        let cookie_middleware = Arc::new(middleware);
        Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(cookie_middleware.clone()),
            routing_middleware: None,
            middlewares: vec![ExecutorMiddleware::Cookie(cookie_middleware)],
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

        // 判断该文件是否包含任何显式标记了 @test 的用例块
        let has_explicit_tests = parsed_file.requests.iter().any(|r| r.metadata.is_test);

        for (index, parsed_request) in parsed_file.requests.into_iter().enumerate() {
            let request_number = index + 1;

            // 检查是否跳过
            let should_skip = parsed_request.should_skip()
                || (has_explicit_tests && !parsed_request.metadata.is_test);

            if should_skip {
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
        VariableResolver::resolve_parsed_request(&mut parsed, context);

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

        // 提前保存断言列表和捕获配置（在 parsed 被移动前）
        let is_sse_requested = parsed.metadata.sse
            || parsed
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("accept") && v.contains("text/event-stream"));
        let sse_timeout = parsed.metadata.sse_timeout;
        let sse_max_events = parsed.metadata.sse_max_events;
        let stream_to = parsed.metadata.stream_to.clone();
        let stream_to_append = parsed.metadata.stream_to_append;

        let assertions_to_eval = parsed.metadata.assertions.clone();
        let captures_to_eval = parsed.metadata.captures.clone();

        // [History] 创建请求快照 (在 parsed 被 move 之前)
        let request_snapshot = RequestSnapshot::from_parsed(&parsed);

        // 转换为 Request
        let mut request = match Request::try_from(parsed) {
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

        // 遍历调用中间件的 before_request 钩子
        for mw in &self.middlewares {
            if let Err(e) = mw.before_request(&mut request).await {
                return TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    format!("Middleware before_request error: {}", e),
                    start.elapsed(),
                );
            }
        }

        // 1. 进行 Pre-flight 探测 (带容错，不干扰主流程)
        let final_url = request.url.to_string();
        let probe_result = if self.debug || self.debug_on_failure {
            use crate::http::timing::DiagnosticsProber;
            DiagnosticsProber::probe_connection(&final_url).await.ok()
        } else {
            None
        };

        // 执行请求
        match self.client.execute_raw(request).await {
            Ok((response, ttfb)) => {
                // 判断是否是 SSE 响应
                let is_sse_by_content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|val| val.to_str().ok())
                    .map(|val| val.contains("text/event-stream"))
                    .unwrap_or(false);
                let is_sse = is_sse_requested || is_sse_by_content_type;

                if is_sse {
                    let sse_options = crate::runner::SseRunnerOptions {
                        debug: self.debug,
                        debug_on_failure: self.debug_on_failure,
                        request_number,
                        name,
                        method,
                        url,
                        assertions_to_eval,
                        captures_to_eval,
                        sse_timeout,
                        sse_max_events,
                        stream_to,
                        stream_to_append,
                    };
                    return crate::runner::SseRunner::execute(
                        response,
                        ttfb,
                        sse_options,
                        context,
                        start,
                        source,
                        request_snapshot,
                        probe_result,
                        self.middlewares.clone(),
                    )
                    .await;
                }

                // 常规单请求流程
                let status = response.status().as_u16();
                let headers = response.headers().clone();
                let body = match response.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        return TestResult::error(
                            request_number,
                            name,
                            method,
                            url,
                            format!("Failed to read response body: {}", e),
                            start.elapsed(),
                        );
                    }
                };

                let response_obj = crate::http::Response::new(
                    status,
                    headers,
                    body,
                    start.elapsed(),
                    ttfb,
                    start.elapsed().saturating_sub(ttfb),
                )
                .unwrap();

                // 遍历调用中间件的 after_response 钩子
                for mw in &self.middlewares {
                    let _ = mw.after_response(&response_obj).await.map_err(|e| {
                        error!("Middleware after_response error: {}", e);
                    });
                }

                // [History] 异步保存历史记录 (Best Effort)
                use crate::history::recorder::record_history;
                record_history(request_snapshot, &response_obj, source);

                // 2. 变量捕获
                VariableCapture::capture_normal(&captures_to_eval, &response_obj.body, &response_obj.headers, context);

                // 3. 执行断言求值
                let resolved_assertions: Vec<String> = assertions_to_eval
                    .iter()
                    .map(|a| VariableResolver::resolve(a, context))
                    .collect();
                let assertion_results = crate::assertion::evaluate_assertions(&resolved_assertions, &response_obj);

                // 创建成功的测试结果
                let mut test_result =
                    TestResult::success(request_number, name, method, url, response_obj.clone());
                test_result.assertions = assertion_results;

                if !test_result.assertions.is_empty() {
                    test_result.success = test_result.assertions.iter().all(|a| a.passed);
                }

                let need_timing = self.debug || (self.debug_on_failure && !test_result.success);
                test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
                    probe_result,
                    response_obj.ttfb,
                    response_obj.transfer,
                    need_timing,
                );

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
                test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
                    probe_result,
                    Duration::ZERO,
                    Duration::ZERO,
                    need_timing,
                );

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
