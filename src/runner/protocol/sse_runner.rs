#![allow(clippy::collapsible_if)]
use crate::history::model::RequestSnapshot;
use crate::http::{Response, SseEvent, SseParser};
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

/// 单个 SSE 事件的处理上下文。
///
/// 把 `Some(Ok(chunk))` 分支与流结束 flush 分支共用的可变状态收拢到一处，
/// 事件处理逻辑只存在一份（出错分支的尾帧 flush 同样复用），避免此前
/// ~60 行逐字复制导致的三份漂移。
struct SseEventContext<'a> {
    options: &'a SseRunnerOptions,
    resolved_assertions: &'a [String],
    headers: &'a HeaderMap,
    status_code: u16,
    log_path: &'a std::path::Path,
    max_bytes: usize,
    llm_provider: crate::http::llm_adapter::LlmProvider,
    file_writer: &'a mut Option<crate::runner::FileSyncWriter>,
    capture_warnings: &'a mut Vec<String>,
    assertion_results: &'a mut Vec<crate::assertion::AssertionResult>,
    accumulated_body: &'a mut String,
    accumulated_llm_content: &'a mut String,
    context: &'a mut VariableContext,
}

impl SseEventContext<'_> {
    /// 处理一个已解析完成的 SSE 事件：累计正文、写日志、推送 TUI、提取 LLM 增量、
    /// 实时捕获与断言。返回是否已达到 `sse_max_events` 上限（由调用方决定 break）。
    async fn process_event(&mut self, event: SseEvent, event_count: usize) -> bool {
        self.accumulated_body.push_str(&event.data);
        self.accumulated_body.push('\n');

        let log_line = format!("data: {}\n\n", event.data);
        let _ = write_sse_log_with_limit(self.log_path, &log_line, self.max_bytes).await;

        if let Some(ref sender) = self.options.stream_sender {
            let _ = sender.send(crate::runner::types::StreamEvent::SseChunk(
                event.data.clone(),
            ));
        }

        if self.options.debug {
            println!(
                "[SSE Event #{}] event: {:?}, data: {}",
                event_count, event.event, event.data
            );
        }

        let llm_adapter = crate::http::llm_adapter::LlmStreamAdapter;
        if let Some(w) = self.file_writer.as_mut() {
            if let Ok(Some(delta)) = w.write_sse_event(&event.data, self.llm_provider).await {
                self.accumulated_llm_content.push_str(&delta);
            }
        } else if let Some(delta) = llm_adapter.extract_delta(self.llm_provider, &event.data) {
            self.accumulated_llm_content.push_str(&delta);
        }

        // 构造虚拟响应帧以供 capture & assertions
        let mut sse_headers = HeaderMap::new();
        if let Some(ref ev) = event.event {
            sse_headers.insert(
                "x-sse-event",
                HeaderValue::from_str(ev).unwrap_or(HeaderValue::from_static("")),
            );
        }
        if let Some(ref id) = event.id {
            sse_headers.insert(
                "x-sse-id",
                HeaderValue::from_str(id).unwrap_or(HeaderValue::from_static("")),
            );
        }
        for (k, v) in self.headers.iter() {
            sse_headers.insert(k.clone(), v.clone());
        }

        let virtual_response = Response::new(
            self.status_code,
            sse_headers,
            event.data.clone(),
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
        )
        .unwrap();

        // 1. 实时变量捕获（失败项收集为警告）
        self.capture_warnings
            .extend(VariableCapture::capture_sse_frame(
                &self.options.captures_to_eval,
                &virtual_response.body,
                &virtual_response.headers,
                self.context,
            ));

        // 2. 实时流断言 (排除 stream.llm.content 的断言)
        let frame_assertions = crate::assertion::evaluate_sse_event_assertions(
            self.resolved_assertions,
            &virtual_response,
            event_count,
        );
        self.assertion_results.extend(frame_assertions);

        self.options
            .sse_max_events
            .is_some_and(|max| event_count >= max)
    }
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
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let default_log_name = format!("sse_{}.log", timestamp);
        let final_stream_to = options.stream_to.clone().unwrap_or_else(|| {
            let dir = std::env::var("RUPOST_HISTORY_DIR").unwrap_or_else(|_| ".rupost".to_string());
            format!("{}/logs/{}", dir, default_log_name)
        });
        let log_path = std::path::PathBuf::from(&final_stream_to);

        // 通知物理路径给前台
        if let Some(ref sender) = options.stream_sender {
            let _ = sender.send(crate::runner::types::StreamEvent::InitLogPath(
                final_stream_to.clone(),
            ));
        }

        // 懒触发概率后台 GC
        if let Some(parent) = log_path.parent() {
            crate::history::gc::LogGc::try_trigger_lazy_gc(parent);
        }

        let max_bytes = 5 * 1024 * 1024; // 5MB limit
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
        // 逐事件捕获的失败信息；同一 capture 在每个事件上重复失败时去重后再呈现
        let mut capture_warnings: Vec<String> = Vec::new();

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
        let llm_provider =
            crate::http::llm_adapter::LlmStreamAdapter::detect_provider(&options.url, &headers);
        let mut event_count = 0;

        let mut file_writer = options.stream_to.as_ref().map(|path_str| {
            crate::runner::FileSyncWriter::new(
                std::path::PathBuf::from(path_str),
                options.stream_to_append,
            )
        });

        {
            let mut evt = SseEventContext {
                options: &options,
                resolved_assertions: &resolved_assertions,
                headers: &headers,
                status_code,
                log_path: &log_path,
                max_bytes,
                llm_provider,
                file_writer: &mut file_writer,
                capture_warnings: &mut capture_warnings,
                assertion_results: &mut assertion_results,
                accumulated_body: &mut accumulated_body,
                accumulated_llm_content: &mut accumulated_llm_content,
                context: &mut *context,
            };

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
                                for event in sse_parser.feed(&chunk_str) {
                                    event_count += 1;
                                    if evt.process_event(event, event_count).await {
                                        break;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                error!("Error reading SSE chunk: {}", e);
                                // 传输层出错（如服务端在 `data: tail` 之后直接断开）时，
                                // parser 里往往还留着一帧未完结的事件——必须 flush 出来
                                // 照常处理，此前直接 break 导致尾帧静默丢失
                                if let Some(event) = sse_parser.flush() {
                                    event_count += 1;
                                    evt.process_event(event, event_count).await;
                                }
                                break;
                            }
                            None => {
                                // 自然结束流，刷新可能未完成的事件
                                if let Some(event) = sse_parser.flush() {
                                    event_count += 1;
                                    evt.process_event(event, event_count).await;
                                }
                                break;
                            }
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
        capture_warnings.extend(VariableCapture::capture_sse_llm_content(
            &options.captures_to_eval,
            &final_response.body,
            &final_response.headers,
            context,
        ));

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
        capture_warnings.sort();
        capture_warnings.dedup();
        test_result.warnings = capture_warnings;

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

async fn write_sse_log_with_limit(
    log_path: &std::path::Path,
    data: &str,
    max_bytes: usize,
) -> Result<(), std::io::Error> {
    use tokio::fs::OpenOptions;
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = log_path.parent() {
        if !parent.exists() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
    }

    let file_len = log_path.metadata().map(|m| m.len() as usize).unwrap_or(0);
    if file_len >= max_bytes {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(log_path)
        .await?;

    let new_len = file_len + data.len();
    if new_len > max_bytes {
        let _ = file
            .write_all(b"\n[SYSTEM] Log truncated due to size limit (5MB).\n")
            .await;
    } else {
        let _ = file.write_all(data.as_bytes()).await;
    }
    let _ = file.flush().await;
    Ok(())
}
