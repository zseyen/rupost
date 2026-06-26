use crate::assertion::AssertionResult;
use crate::history::model::RequestSnapshot;
use crate::http::{Client, Request, SseParser};
use crate::middleware::{CookieMiddleware, Middleware};
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::types::TestResult;
use crate::variable::{
    VariableContext, VariableResolver, capture::VariableCapture,
};
use crate::{Result, RupostError};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info};

pub struct TestExecutor {
    client: Client,
    cookie_middleware: Option<Arc<CookieMiddleware>>,
    routing_middleware: Option<Arc<crate::middleware::routing::RoutingMiddleware>>,
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
            debug: false,
            debug_on_failure: false,
        }
    }

    pub fn with_middleware(
        mut self,
        middleware: Arc<crate::middleware::routing::RoutingMiddleware>,
    ) -> Self {
        self.routing_middleware = Some(middleware);
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
        Ok(Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(Arc::new(middleware)),
            routing_middleware: None,
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
            routing_middleware: None,
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

        // 调用路由与 Key 注入中间件
        if let Some(ref mw) = self.routing_middleware {
            use crate::middleware::Middleware;
            if let Err(e) = mw.before_request(&mut request).await {
                return TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    format!("Routing middleware error: {}", e),
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
                    return self
                        .execute_stream(
                            response,
                            ttfb,
                            request_number,
                            name,
                            method,
                            url,
                            &assertions_to_eval,
                            &captures_to_eval,
                            sse_timeout,
                            sse_max_events,
                            context,
                            start,
                            source,
                            request_snapshot,
                            probe_result,
                            stream_to,
                            stream_to_append,
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

                // [Cookie] Middleware after_response hook (best effort)
                if let Some(mw) = &self.cookie_middleware {
                    let _ = mw.after_response(&response_obj).await.map_err(|e| {
                        error!("Cookie middleware error: {}", e);
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

    /// 执行 SSE 流式请求
    #[allow(clippy::too_many_arguments)]
    async fn execute_stream(
        &self,
        response: reqwest::Response,
        ttfb: Duration,
        request_number: usize,
        name: Option<String>,
        method: String,
        url: String,
        assertions_to_eval: &[String],
        captures_to_eval: &[VariableCapture],
        sse_timeout: Option<Duration>,
        sse_max_events: Option<usize>,
        context: &mut VariableContext,
        start_time: Instant,
        source: Option<String>,
        request_snapshot: RequestSnapshot,
        probe_result: Option<(Duration, Duration)>,
        stream_to: Option<String>,
        stream_to_append: bool,
    ) -> TestResult {
        let status_code = response.status().as_u16();
        let headers = response.headers().clone();

        // [Cookie] Middleware after_response hook (best effort)
        if let Some(mw) = &self.cookie_middleware {
            let handshake_response_tmp = crate::http::Response::new(
                status_code,
                headers.clone(),
                String::new(),
                ttfb,
                ttfb,
                Duration::from_millis(0),
            )
            .unwrap();
            let _ = mw
                .after_response(&handshake_response_tmp)
                .await
                .map_err(|e| {
                    error!("Cookie middleware error: {}", e);
                });
        }

        let mut assertion_results = Vec::new();

        // 评估非 stream 握手断言
        let handshake_response = crate::http::Response::new(
            status_code,
            headers.clone(),
            String::new(),
            ttfb,
            ttfb,
            Duration::from_millis(0),
        )
        .unwrap();

        let resolved_assertions: Vec<String> = assertions_to_eval
            .iter()
            .map(|a| VariableResolver::resolve(a, context))
            .collect();

        let handshake_assertions = crate::assertion::evaluate_sse_handshake_assertions(
            &resolved_assertions,
            &handshake_response,
        );
        assertion_results.extend(handshake_assertions);

        let timeout_duration = sse_timeout.unwrap_or(Duration::from_secs(30));
        let sleep_timer = tokio::time::sleep(timeout_duration);
        tokio::pin!(sleep_timer);

        let mut byte_stream = response.bytes_stream();
        let mut sse_parser = SseParser::new();
        let mut accumulated_body = String::new();
        let mut accumulated_llm_content = String::new();
        let llm_adapter = crate::http::llm_adapter::LlmStreamAdapter;
        let llm_provider =
            crate::http::llm_adapter::LlmStreamAdapter::detect_provider(&url, &headers);
        let mut event_count = 0;

        let mut file_writer = stream_to.as_ref().map(|path_str| {
            crate::runner::file_sync::FileSyncWriter::new(
                std::path::PathBuf::from(path_str),
                stream_to_append,
            )
        });

        loop {
            if sse_max_events.is_some_and(|max| event_count >= max) {
                info!("SSE max events limit reached: {}", sse_max_events.unwrap());
                break;
            }

            tokio::select! {
                _ = &mut sleep_timer => {
                    info!("SSE stream execution timed out after {:?}", timeout_duration);
                    break;
                }
                maybe_chunk = byte_stream.next() => {
                    match maybe_chunk {
                        Some(Ok(chunk)) => {
                            let chunk_str = String::from_utf8_lossy(&chunk);
                            let events = sse_parser.feed(&chunk_str);
                            for event in events {
                                event_count += 1;
                                accumulated_body.push_str(&event.data);
                                accumulated_body.push('\n');

                                if self.debug {
                                    println!("[SSE Event #{}] event: {:?}, data: {}", event_count, event.event, event.data);
                                }

                                if let Some(ref mut w) = file_writer {
                                    if let Ok(Some(delta)) = w.write_sse_event(&event.data, llm_provider).await {
                                        accumulated_llm_content.push_str(&delta);
                                    }
                                } else if let Some(delta) = llm_adapter.extract_delta(llm_provider, &event.data) {
                                    accumulated_llm_content.push_str(&delta);
                                }

                                // 构造虚拟响应帧以供 capture & assertions
                                let mut sse_headers = HeaderMap::new();
                                if let Some(ref ev) = event.event {
                                    sse_headers.insert("x-sse-event", HeaderValue::from_str(ev).unwrap_or(HeaderValue::from_static("")));
                                }
                                if let Some(ref id) = event.id {
                                    sse_headers.insert("x-sse-id", HeaderValue::from_str(id).unwrap_or(HeaderValue::from_static("")));
                                }
                                for (k, v) in headers.iter() {
                                    sse_headers.insert(k.clone(), v.clone());
                                }

                                let virtual_response = crate::http::Response::new(
                                    status_code,
                                    sse_headers,
                                    event.data.clone(),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                ).unwrap();

                                // 1. 实时变量捕获
                                VariableCapture::capture_sse_frame(
                                    captures_to_eval,
                                    &virtual_response.body,
                                    &virtual_response.headers,
                                    context,
                                );

                                // 2. 实时流断言 (排除包含 stream.llm.content 的断言)
                                let frame_assertions = crate::assertion::evaluate_sse_event_assertions(
                                    &resolved_assertions,
                                    &virtual_response,
                                    event_count,
                                );
                                assertion_results.extend(frame_assertions);

                                if sse_max_events.is_some_and(|max| event_count >= max) {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error reading SSE chunk: {}", e);
                            break;
                        }
                        None => {
                            // 自然结束流，刷新可能未完成的事件
                            if let Some(event) = sse_parser.flush() {
                                event_count += 1;
                                accumulated_body.push_str(&event.data);
                                accumulated_body.push('\n');

                                if self.debug {
                                    println!("[SSE Event #{}] event: {:?}, data: {}", event_count, event.event, event.data);
                                }

                                if let Some(ref mut w) = file_writer {
                                    if let Ok(Some(delta)) = w.write_sse_event(&event.data, llm_provider).await {
                                        accumulated_llm_content.push_str(&delta);
                                    }
                                } else if let Some(delta) = llm_adapter.extract_delta(llm_provider, &event.data) {
                                    accumulated_llm_content.push_str(&delta);
                                }

                                let mut sse_headers = HeaderMap::new();
                                if let Some(ref ev) = event.event {
                                    sse_headers.insert("x-sse-event", HeaderValue::from_str(ev).unwrap_or(HeaderValue::from_static("")));
                                }
                                if let Some(ref id) = event.id {
                                    sse_headers.insert("x-sse-id", HeaderValue::from_str(id).unwrap_or(HeaderValue::from_static("")));
                                }
                                for (k, v) in headers.iter() {
                                    sse_headers.insert(k.clone(), v.clone());
                                }

                                let virtual_response = crate::http::Response::new(
                                    status_code,
                                    sse_headers,
                                    event.data.clone(),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                ).unwrap();

                                // 1. 实时变量捕获
                                VariableCapture::capture_sse_frame(
                                    captures_to_eval,
                                    &virtual_response.body,
                                    &virtual_response.headers,
                                    context,
                                );

                                // 2. 实时流断言 (排除包含 stream.llm.content 的断言)
                                let frame_assertions = crate::assertion::evaluate_sse_event_assertions(
                                    &resolved_assertions,
                                    &virtual_response,
                                    event_count,
                                );
                                assertion_results.extend(frame_assertions);
                            }
                            break;
                        }
                    }
                }
            }
        }

        let total_duration = start_time.elapsed();
        let transfer = total_duration.saturating_sub(ttfb);

        let encoded_llm_content =
            url::form_urlencoded::byte_serialize(accumulated_llm_content.as_bytes())
                .collect::<String>();
        let mut final_headers = headers.clone();
        final_headers.insert(
            "x-sse-llm-content",
            HeaderValue::from_str(&encoded_llm_content)
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );

        let final_response = crate::http::Response::new(
            status_code,
            final_headers,
            accumulated_body,
            total_duration,
            ttfb,
            transfer,
        )
        .unwrap();

        // 3. 评估包含 stream.llm.content 的断言
        let final_assertions = crate::assertion::evaluate_sse_llm_content_assertions(
            &resolved_assertions,
            &final_response,
        );
        assertion_results.extend(final_assertions);

        // 4. 评估包含 stream.llm.content 的捕获
        VariableCapture::capture_sse_llm_content(
            captures_to_eval,
            &final_response.body,
            &final_response.headers,
            context,
        );

        // [History] 保存历史记录 (Best Effort)
        use crate::history::recorder::record_history;
        record_history(request_snapshot, &final_response, source);

        // 构造最终测试结果
        let mut test_result =
            TestResult::success(request_number, name, method, url, final_response.clone());
        test_result.assertions = assertion_results;

        if !test_result.assertions.is_empty() {
            test_result.success = test_result.assertions.iter().all(|a| a.passed);
        }

        let need_timing = self.debug || (self.debug_on_failure && !test_result.success);
        test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
            probe_result,
            final_response.ttfb,
            final_response.transfer,
            need_timing,
        );

        test_result
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}
