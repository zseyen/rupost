use crate::Result;
use crate::RupostError;
use crate::history::model::RequestSnapshot;
use crate::http::Client;
use crate::middleware::MiddlewarePipeline;
use crate::parser::types::ParsedRequest;
use crate::runner::observer::ExecutionObserver;
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
        middlewares: &MiddlewarePipeline,
        observer: &std::sync::Arc<dyn ExecutionObserver>,
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
        let source_line = parsed.metadata.source_line;
        let is_sse_requested = parsed.is_sse_stream();
        let sse_timeout = parsed.metadata.sse_timeout;
        let sse_max_events = parsed.metadata.sse_max_events;
        let stream_to = parsed.metadata.stream_to.clone();
        let stream_to_append = parsed.metadata.stream_to_append;

        let assertions_to_eval = parsed.metadata.assertions.clone();
        let captures_to_eval: Vec<VariableCapture> = parsed
            .metadata
            .captures
            .iter()
            .map(VariableCapture::from)
            .collect();
        let enable_diagnose = parsed.metadata.diagnose;

        // [History] 创建请求快照 (在 parsed 被 move 之前)
        let request_snapshot = RequestSnapshot::from_parsed(&parsed);

        // 转换为 Request（同时收集构建期警告，如畸形 JSON 降级发送）
        let (mut request, build_warnings) = match crate::http::try_from_parsed_with_warnings(parsed)
        {
            Ok(v) => v,
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
        if let Err(e) = middlewares.run_before_request(&mut request).await {
            return Ok(TestResult::error(
                request_number,
                name,
                method,
                url,
                format!("Middleware before_request error: {}", e),
                start.elapsed(),
            ));
        }

        // 1. 进行 Pre-flight 探测 (带容错，不干扰主流程)
        let final_url = request.url.to_string();
        let probe_result = if debug || debug_on_failure {
            use crate::http::timing::DiagnosticsProber;
            DiagnosticsProber::probe_connection(&final_url).await.ok()
        } else {
            None
        };

        // 执行请求（M7-A: 始终在 task_local! 作用域内采集单连接 DNS/TCP 时序）
        match client.execute_raw_with_timing(request).await {
            Ok((response, ttfb, connect_span)) => {
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
                        warnings: build_warnings,
                        connect_span,
                        source_line,
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
                        middlewares.clone(),
                        observer.clone(),
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
                let _ = middlewares
                    .run_after_response(&response_obj)
                    .await
                    .map_err(|e| {
                        error!("Middleware after_response error: {}", e);
                    });

                // [History] 异步保存历史记录 (Best Effort)
                observer.on_record_history(request_snapshot.clone(), &response_obj, source);

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
                test_result.source_line = source_line;
                test_result.assertions = assertion_results;
                // 构建期警告（畸形 JSON 降级等）在前，捕获警告在后
                let mut all_warnings = build_warnings;
                all_warnings.extend(capture_warnings);
                test_result.warnings = all_warnings;

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

                // M7-A: 默认始终记录单连接时序（无需 --debug）；连接复用时 connect_span 为 None
                //（对应快照 dns_lookup_us/tcp_connect_us = null）。
                apply_connector_timing(
                    &mut test_result,
                    connect_span,
                    response_obj.ttfb,
                    response_obj.transfer,
                );

                Ok(test_result)
            }
            Err(e) => {
                // reqwest 顶层错误只说 "error sending request"，超时原因藏在 source 链里；
                // 单独识别出来，否则用户看不出是自己的 @timeout 触发的。
                let message = match &e {
                    RupostError::HttpError(re) if re.is_timeout() => {
                        "请求超时 (request timed out)".to_string()
                    }
                    _ => {
                        RupostError::RequestExecutionFailed(e.to_string()).to_user_friendly_string()
                    }
                };
                let mut test_result = TestResult::error(
                    request_number,
                    name,
                    method,
                    url.clone(),
                    message,
                    start.elapsed(),
                );
                test_result.request = Some(request_snapshot);
                test_result.source_line = source_line;
                let run_diagnose = enable_diagnose || debug || debug_on_failure;
                // 传输层失败恰是最需要诊断的场景：用宽容版诊断保证 DNS/TCP
                // 阶段性失败也能产出带 connection_error 分类的部分瀑布（D2 拆分）
                if run_diagnose && let Ok(report) = crate::http::diagnose_url_lenient(&url).await {
                    test_result.diagnose_report = Some(report);
                }
                let need_timing = debug || debug_on_failure;
                test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
                    probe_result,
                    Duration::ZERO,
                    Duration::ZERO,
                    need_timing,
                );
                if test_result.timing.is_some() {
                    test_result.timing_source =
                        Some(crate::http::timing::TimingSource::ProbeDegraded);
                    test_result.timing_attribution =
                        Some(if crate::http::timing_connector::has_proxy_env() {
                            crate::http::timing::TimingAttribution::Approximate
                        } else {
                            crate::http::timing::TimingAttribution::Exact
                        });
                }

                Ok(test_result)
            }
        }
    }
}

/// 将 M7-A 连接器采集的 `ConnectSpan` 与 `ttfb`/`transfer` 写入 `TestResult`（供 HTTP 与 SSE 复用）
pub(super) fn apply_connector_timing(
    test_result: &mut TestResult,
    connect_span: Option<crate::http::timing_connector::ConnectSpan>,
    ttfb: Duration,
    transfer: Duration,
) {
    let span_dns = connect_span
        .and_then(|s| s.dns_us)
        .map(Duration::from_micros)
        .unwrap_or(Duration::ZERO);
    let span_tcp = connect_span
        .and_then(|s| s.tcp_us)
        .map(Duration::from_micros)
        .unwrap_or(Duration::ZERO);
    test_result.timing = Some(crate::http::timing::RequestTiming {
        dns_lookup: span_dns,
        tcp_connect: span_tcp,
        ttfb,
        transfer,
    });
    test_result.connect_span = connect_span;
    test_result.timing_source = Some(crate::http::timing::TimingSource::Connector);
    test_result.timing_attribution = Some(if crate::http::timing_connector::has_proxy_env() {
        crate::http::timing::TimingAttribution::Approximate
    } else {
        crate::http::timing::TimingAttribution::Exact
    });
}
