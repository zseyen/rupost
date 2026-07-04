use crate::history::model::RequestSnapshot;
use crate::http::{Response, SseParser};
use crate::middleware::Middleware;
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver, capture::VariableCapture};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::{Duration, Instant};
use tracing::{error, info};

pub struct SseRunnerOptions {
    pub debug: bool,
    pub debug_on_failure: bool,
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub assertions_to_eval: Vec<String>,
    pub captures_to_eval: Vec<VariableCapture>,
    pub sse_timeout: Option<Duration>,
    pub sse_max_events: Option<usize>,
    pub stream_to: Option<String>,
    pub stream_to_append: bool,
    pub stream_sender:
        Option<tokio::sync::mpsc::UnboundedSender<crate::runner::types::StreamEvent>>,
}

pub struct SseRunner;

impl SseRunner {
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        response: reqwest::Response,
        ttfb: Duration,
        options: SseRunnerOptions,
        context: &mut VariableContext,
        start_time: Instant,
        source: Option<String>,
        request_snapshot: RequestSnapshot,
        probe_result: Option<(Duration, Duration)>,
        middlewares: Vec<crate::runner::executor::ExecutorMiddleware>,
    ) -> TestResult {
        let status_code = response.status().as_u16();
        let headers = response.headers().clone();

        // [Middleware] after_response hook (best effort)
        let handshake_response_tmp = Response::new(
            status_code,
            headers.clone(),
            String::new(),
            ttfb,
            ttfb,
            Duration::from_millis(0),
        )
        .unwrap();
        for mw in &middlewares {
            let _ = mw
                .after_response(&handshake_response_tmp)
                .await
                .map_err(|e| {
                    error!("Middleware error: {}", e);
                });
        }

        let mut assertion_results = Vec::new();

        // 评估非 stream 握手断言
        let handshake_response = Response::new(
            status_code,
            headers.clone(),
            String::new(),
            ttfb,
            ttfb,
            Duration::from_millis(0),
        )
        .unwrap();

        let resolved_assertions: Vec<String> = options
            .assertions_to_eval
            .iter()
            .map(|a| VariableResolver::resolve(a, context))
            .collect();

        let handshake_assertions = crate::assertion::evaluate_sse_handshake_assertions(
            &resolved_assertions,
            &handshake_response,
        );
        assertion_results.extend(handshake_assertions);

        let timeout_duration = options.sse_timeout.unwrap_or(Duration::from_secs(30));
        let sleep_timer = tokio::time::sleep(timeout_duration);
        tokio::pin!(sleep_timer);

        let mut byte_stream = response.bytes_stream();
        let mut sse_parser = SseParser::new();
        let mut accumulated_body = String::new();
        let mut accumulated_llm_content = String::new();
        let llm_adapter = crate::http::llm_adapter::LlmStreamAdapter;
        let llm_provider =
            crate::http::llm_adapter::LlmStreamAdapter::detect_provider(&options.url, &headers);
        let mut event_count = 0;

        let mut file_writer = options.stream_to.as_ref().map(|path_str| {
            crate::runner::file_sync::FileSyncWriter::new(
                std::path::PathBuf::from(path_str),
                options.stream_to_append,
            )
        });

        loop {
            if options.sse_max_events.is_some_and(|max| event_count >= max) {
                info!(
                    "SSE max events limit reached: {}",
                    options.sse_max_events.unwrap()
                );
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

                                if let Some(ref sender) = options.stream_sender {
                                    let _ = sender.send(crate::runner::types::StreamEvent::SseChunk(event.data.clone()));
                                }

                                if options.debug {
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

                                let virtual_response = Response::new(
                                    status_code,
                                    sse_headers,
                                    event.data.clone(),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                ).unwrap();

                                // 1. 实时变量捕获
                                VariableCapture::capture_sse_frame(
                                    &options.captures_to_eval,
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

                                if options.sse_max_events.is_some_and(|max| event_count >= max) {
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

                                if let Some(ref sender) = options.stream_sender {
                                    let _ = sender.send(crate::runner::types::StreamEvent::SseChunk(event.data.clone()));
                                }

                                if options.debug {
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

                                let virtual_response = Response::new(
                                    status_code,
                                    sse_headers,
                                    event.data.clone(),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                    Duration::from_millis(0),
                                ).unwrap();

                                // 1. 实时变量捕获
                                VariableCapture::capture_sse_frame(
                                    &options.captures_to_eval,
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

        let final_response = Response::new(
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
            &options.captures_to_eval,
            &final_response.body,
            &final_response.headers,
            context,
        );

        // [History] 保存历史记录 (Best Effort)
        use crate::history::recorder::record_history;
        record_history(request_snapshot, &final_response, source);

        // 构造最终测试结果
        let mut test_result = TestResult::success(
            options.request_number,
            options.name,
            options.method,
            options.url,
            final_response.clone(),
        );
        test_result.assertions = assertion_results;

        if !test_result.assertions.is_empty() {
            test_result.success = test_result.assertions.iter().all(|a| a.passed);
        }

        let need_timing = options.debug || (options.debug_on_failure && !test_result.success);
        test_result.timing = crate::http::timing::DiagnosticsProber::resolve_timing(
            probe_result,
            final_response.ttfb,
            final_response.transfer,
            need_timing,
        );

        test_result
    }
}
