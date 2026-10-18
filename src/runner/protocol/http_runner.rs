use crate::Result;
use crate::RupostError;
use crate::history::model::RequestSnapshot;
use crate::http::{Client, Request};
use crate::middleware::Middleware;
use crate::parser::types::ParsedRequest;
use crate::runner::executor::ExecutorMiddleware;
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver, capture::VariableCapture};
use std::time::{Duration, Instant};
use tracing::error;

pub struct HttpRunner;

impl HttpRunner {
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
        middlewares: &[ExecutorMiddleware],
        stream_sender: Option<
            tokio::sync::mpsc::UnboundedSender<crate::runner::types::StreamEvent>,
        >,
        debug: bool,
        debug_on_failure: bool,
        client: &Client,
        source: Option<String>,
    ) -> Result<TestResult> {
        let start = Instant::now();
        let method = parsed.method_or_default().to_string();
        let url = parsed.url.clone();
        let name = parsed.name().map(|s| s.to_string());

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
        let enable_diagnose = parsed.metadata.diagnose;

        // [History] 创建请求快照 (在 parsed 被 move 之前)
        let request_snapshot = RequestSnapshot::from_parsed(&parsed);

        // 转换为 Request
        let mut request = match Request::try_from(parsed) {
            Ok(req) => req,
            // e 已是 RupostError，直接渲染，避免二次包装导致前缀重复
            Err(e) => {
                return Ok(TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    e.to_user_friendly_string(),
                    start.elapsed(),
                ));
            }
        };

        // 遍历调用中间件的 before_request 钩子
        for mw in middlewares {
            if let Err(e) = mw.before_request(&mut request).await {
                return Ok(TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    format!("Middleware before_request error: {}", e),
                    start.elapsed(),
                ));
            }
        }

        // 1. 进行 Pre-flight 探测 (带容错，不干扰主流程)
        let final_url = request.url.to_string();
        let probe_result = if debug || debug_on_failure {
            use crate::http::timing::DiagnosticsProber;
            DiagnosticsProber::probe_connection(&final_url).await.ok()
        } else {
            None
        };

        // 执行请求
        match client.execute_raw(request).await {
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
                        debug,
                        debug_on_failure,
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
                        stream_sender,
                    };
                    return Ok(crate::runner::SseRunner::execute(
                        response,
                        ttfb,
                        sse_options,
                        context,
                        start,
                        source,
                        request_snapshot,
                        probe_result,
                        middlewares.to_vec(),
                    )
                    .await);
                }

                // 常规单请求流程
                let status = response.status().as_u16();
                let headers = response.headers().clone();
                let body = match response.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(TestResult::error(
                            request_number,
                            name,
                            method,
                            url,
                            format!("Failed to read response body: {}", e),
                            start.elapsed(),
                        ));
                    }
                };

                let mut response_obj = crate::http::Response::new(
                    status,
                    headers,
                    body,
                    start.elapsed(),
                    ttfb,
                    start.elapsed().saturating_sub(ttfb),
                )
                .unwrap();

                let mut diagnose_report = None;
                if (enable_diagnose || debug)
                    && let Ok(report) = crate::http::diagnose_url(&url).await
                {
                    diagnose_report = Some(report.clone());
                    response_obj = response_obj.with_diagnose_report(report);
                }

                // 遍历调用中间件的 after_response 钩子
                for mw in middlewares {
                    let _ = mw.after_response(&response_obj).await.map_err(|e| {
                        error!("Middleware after_response error: {}", e);
                    });
                }

                // [History] 异步保存历史记录 (Best Effort)
                use crate::history::recorder::record_history;
                record_history(request_snapshot.clone(), &response_obj, source);

                // 2. 变量捕获（失败项收集为警告，不影响其余捕获）
                let capture_warnings = VariableCapture::capture_normal(
                    &captures_to_eval,
                    &response_obj.body,
                    &response_obj.headers,
                    context,
                );

                // 3. 执行断言求值
                let resolved_assertions: Vec<String> = assertions_to_eval
                    .iter()
                    .map(|a| VariableResolver::resolve(a, context))
                    .collect();
                let assertion_results =
                    crate::assertion::evaluate_assertions(&resolved_assertions, &response_obj);

                // 创建成功的测试结果
                let mut test_result = TestResult::success(
                    request_number,
                    name,
                    method,
                    url.clone(),
                    response_obj.clone(),
                );
                test_result.request = Some(request_snapshot);
                test_result.assertions = assertion_results;
                test_result.warnings = capture_warnings;

                if !test_result.assertions.is_empty() {
                    test_result.success = test_result.assertions.iter().all(|a| a.passed);
                }

                test_result.diagnose_report = diagnose_report;

                if debug_on_failure
                    && !test_result.success
                    && test_result.diagnose_report.is_none()
                    && let Ok(report) = crate::http::diagnose_url(&url).await
                {
                    test_result.diagnose_report = Some(report.clone());
                    if let Some(ref mut resp) = test_result.response {
                        *resp = resp.clone().with_diagnose_report(report);
                    }
                }

                let need_timing = debug || (debug_on_failure && !test_result.success);
                test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
                    probe_result,
                    response_obj.ttfb,
                    response_obj.transfer,
                    need_timing,
                );

                Ok(test_result)
            }
            Err(e) => {
                let mut test_result = TestResult::error(
                    request_number,
                    name,
                    method,
                    url.clone(),
                    RupostError::RequestExecutionFailed(e.to_string()).to_user_friendly_string(),
                    start.elapsed(),
                );
                test_result.request = Some(request_snapshot);
                let run_diagnose = enable_diagnose || debug || debug_on_failure;
                if run_diagnose && let Ok(report) = crate::http::diagnose_url(&url).await {
                    test_result.diagnose_report = Some(report);
                }
                let need_timing = debug || debug_on_failure;
                test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
                    probe_result,
                    Duration::ZERO,
                    Duration::ZERO,
                    need_timing,
                );

                Ok(test_result)
            }
        }
    }
}
