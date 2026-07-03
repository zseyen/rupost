use reqwest::header::HeaderMap;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::assertion::evaluate_assertions;
use crate::http::{Request, Response};
use crate::middleware::Middleware;
use crate::parser::types::ParsedRequest;
use crate::runner::executor::ExecutorMiddleware;
use crate::runner::types::TestResult;
use crate::variable::capture::VariableCapture;
use crate::variable::{VariableContext, VariableResolver};
use crate::ws::{
    FrameDirection, MsgPackDecoder, PayloadDecoder, WsAction, WsActionParser, WsClientConfig,
    WsFrame, WsFrameType, WsSession,
};

pub struct WsRunner;

impl WsRunner {
    /// 执行 WebSocket 流式剧本用例
    pub async fn execute(
        parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
        middlewares: Vec<ExecutorMiddleware>,
    ) -> TestResult {
        let start_time = Instant::now();
        let name = parsed.name().map(|s| s.to_string());
        let initial_url = parsed.url.clone();

        // 保存断言、捕获列表与超时/Body信息
        let assertions_to_eval = parsed.metadata.assertions.clone();
        let captures_to_eval = parsed.metadata.captures.clone();
        let decoder_name = parsed.metadata.decoder.clone();
        let handshake_timeout = parsed.metadata.timeout.unwrap_or(Duration::from_secs(5));
        let body_content = parsed.body.clone().unwrap_or_default();

        // 1. 构建临时的 HTTP Request 升级握手，用于流经中间件管道（注入 Cookie 与安全头）
        let mut temp_req = match Request::try_from(parsed) {
            Ok(req) => req,
            Err(e) => {
                return TestResult::error(
                    request_number,
                    name,
                    "GET".to_string(),
                    initial_url,
                    format!("Request build failed for middleware pipeline: {}", e),
                    start_time.elapsed(),
                );
            }
        };

        // 2. 调用所有中间件的 before_request 拦截与追加逻辑
        for mw in &middlewares {
            if let Err(e) = mw.before_request(&mut temp_req).await {
                return TestResult::error(
                    request_number,
                    name,
                    "GET".to_string(),
                    temp_req.url.to_string(),
                    format!("Middleware before_request error: {}", e),
                    start_time.elapsed(),
                );
            }
        }

        // 3. 提取修改后的 URL，自适应协议转写为 ws:// 或 wss://
        let resolved_url = Self::resolve_ws_url(&temp_req.url.to_string());

        // 提取修改后的 Headers
        let mut resolved_headers = Vec::new();
        for (k, v) in temp_req.headers.iter() {
            if let Ok(v_str) = v.to_str() {
                resolved_headers.push((k.as_str().to_string(), v_str.to_string()));
            }
        }

        info!("Starting WebSocket session to {}", resolved_url);

        // 4. 建立 WebSocket 连接
        let config = WsClientConfig {
            url: resolved_url.clone(),
            headers: resolved_headers,
            ping_interval: Duration::from_secs(10), // 默认 10 秒自动心跳保活
            handshake_timeout,
        };

        let client = match WsSession::connect(config).await {
            Ok(c) => c,
            Err(e) => {
                error!("WebSocket connection failed: {}", e);
                return TestResult::error(
                    request_number,
                    name,
                    "GET".to_string(), // WS 通常以 GET 握手启动
                    resolved_url,
                    format!("Connection failed: {}", e),
                    start_time.elapsed(),
                );
            }
        };

        // 5. 解析 Body 文本为 WsAction 流
        let actions = match WsActionParser::parse_body(&body_content) {
            Ok(act) => act,
            Err(e) => {
                error!("Failed to parse WsActions from body: {}", e);
                return TestResult::error(
                    request_number,
                    name,
                    "GET".to_string(),
                    resolved_url,
                    format!("Parser error in body: {}", e),
                    start_time.elapsed(),
                );
            }
        };

        let mut rx = client.subscribe();
        let mut assertion_results = Vec::new();
        let mut final_error = None;

        // 获取解码器适配器
        let decoder: Option<Box<dyn PayloadDecoder>> = match decoder_name.as_deref() {
            Some("messagepack") => Some(Box::new(MsgPackDecoder)),
            Some(other) => {
                warn!("Unsupported decoder: {}, falling back to None", other);
                None
            }
            None => None,
        };

        let mut executed_actions_count = 0;

        if actions.is_empty() {
            // CLI 直接测试模式：如果有 body 则发包，然后持续打印 5 秒的入站流
            if !body_content.trim().is_empty() {
                let resolved_payload = VariableResolver::resolve(&body_content, context);
                info!("WS [SEND] Payload: {}", resolved_payload);
                let outbound_frame = WsFrame::new(
                    FrameDirection::Outbound,
                    WsFrameType::Text,
                    resolved_payload.into_bytes(),
                    0,
                );
                let _ = client.send_frame(outbound_frame).await;
                executed_actions_count += 1;
            }

            use colored::Colorize;
            info!(
                "WS Entering live monitoring mode for 5 seconds. Listening for incoming frames..."
            );
            let monitor_timer = tokio::time::sleep(Duration::from_secs(5));
            tokio::pin!(monitor_timer);

            loop {
                tokio::select! {
                    _ = &mut monitor_timer => {
                        info!("WS 5 seconds monitoring finished.");
                        break;
                    }
                    maybe_frame = rx.recv() => {
                        match maybe_frame {
                            Ok(frame) => {
                                if frame.direction == FrameDirection::Inbound {
                                    let payload_str = if frame.frame_type == WsFrameType::Binary {
                                        if let Some(ref dec) = decoder {
                                            dec.decode(&frame.payload).map(|v| v.to_string()).unwrap_or_else(|_| format!("0x{}", hex::encode(&frame.payload)))
                                        } else {
                                            format!("0x{}", hex::encode(&frame.payload))
                                        }
                                    } else {
                                        frame.payload_as_string()
                                    };
                                    println!("{} [INBOUND] [Type: {:?}] {}", "[WS]".green().bold(), frame.frame_type, payload_str);
                                    executed_actions_count += 1;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                                warn!("WS monitor lagged by {} frames, skipping lagged packets.", missed);
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }

            // 优雅关闭
            let close_frame =
                WsFrame::new(FrameDirection::Outbound, WsFrameType::Close, Vec::new(), 0);
            let _ = client.send_frame(close_frame).await;
            executed_actions_count += 1;
        } else {
            // 6. 驱动 WsAction 执行流
            for (action_idx, action) in actions.into_iter().enumerate() {
                executed_actions_count = action_idx + 1;
                match Self::execute_action(
                    action,
                    action_idx,
                    &client,
                    &mut rx,
                    context,
                    decoder.as_deref(),
                    &assertions_to_eval,
                    &captures_to_eval,
                    &mut assertion_results,
                )
                .await
                {
                    Ok(should_continue) => {
                        if !should_continue {
                            break;
                        }
                    }
                    Err(e) => {
                        final_error = Some(e);
                        break;
                    }
                }
            }
        }

        // 6. 构造执行报告 TestResult
        let duration = start_time.elapsed();
        let success = final_error.is_none() && assertion_results.iter().all(|a| a.passed);

        let mut test_result = if let Some(err) = final_error {
            TestResult::error(
                request_number,
                name,
                "GET".to_string(),
                resolved_url,
                err,
                duration,
            )
        } else {
            // 成功时构建一个代表长连接总结的 Response
            let summary_response = Response::new(
                200,
                HeaderMap::new(),
                format!(
                    "WebSocket Session Completed Successfully. Run {} actions.",
                    executed_actions_count
                ),
                duration,
                Duration::from_millis(0),
                Duration::from_millis(0),
            )
            .unwrap();

            TestResult::success(
                request_number,
                name,
                "GET".to_string(),
                resolved_url,
                summary_response,
            )
        };

        test_result.assertions = assertion_results;
        test_result.success = success;
        test_result
    }
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl WsRunner {
    /// 执行单个 WsAction 动作
    #[allow(clippy::too_many_arguments)]
    async fn execute_action(
        action: WsAction,
        action_idx: usize,
        client: &WsSession,
        rx: &mut tokio::sync::broadcast::Receiver<WsFrame>,
        context: &mut VariableContext,
        decoder: Option<&dyn PayloadDecoder>,
        assertions_to_eval: &[String],
        captures_to_eval: &[VariableCapture],
        assertion_results: &mut Vec<crate::assertion::AssertionResult>,
    ) -> Result<bool, String> {
        match action {
            WsAction::Connect { .. } => Ok(true),
            WsAction::Send(frame) => {
                let payload_str = frame.payload_as_string();
                let resolved_payload = VariableResolver::resolve(&payload_str, context);

                info!("WS [SEND #{}] Payload: {}", action_idx, resolved_payload);
                let outbound_frame = WsFrame::new(
                    FrameDirection::Outbound,
                    frame.frame_type,
                    resolved_payload.into_bytes(),
                    0,
                );

                if let Err(e) = client.send_frame(outbound_frame).await {
                    Err(format!("Send failed: {}", e))
                } else {
                    Ok(true)
                }
            }
            WsAction::Wait(duration) => {
                info!("WS [WAIT #{}] sleeping for {:?}", action_idx, duration);
                tokio::time::sleep(duration).await;
                Ok(true)
            }
            WsAction::Expect {
                condition,
                segments,
                expected_value,
                operator,
                timeout,
                assertions,
                captures,
            } => {
                let resolved_condition = VariableResolver::resolve(&condition, context);
                info!(
                    "WS [EXPECT #{}] Waiting up to {:?} for condition: {}",
                    action_idx, timeout, resolved_condition
                );

                let resolved_expected_value = expected_value
                    .as_ref()
                    .map(|ev| VariableResolver::resolve(ev, context));

                // 使用复用的领域层组件 WsConditionMatcher
                use crate::ws::FrameMatcher;
                let matcher = crate::ws::WsConditionMatcher::compile(
                    &resolved_condition,
                    &segments,
                    &resolved_expected_value,
                    &operator,
                );

                let expect_timer = tokio::time::sleep(timeout);
                tokio::pin!(expect_timer);

                let mut matched = false;
                let mut last_matching_payload = None;

                loop {
                    tokio::select! {
                        _ = &mut expect_timer => {
                            break;
                        }
                        maybe_frame = rx.recv() => {
                            match maybe_frame {
                                Ok(frame) => {
                                    if frame.direction == FrameDirection::Inbound
                                        && matcher.matches(&frame, decoder)
                                    {
                                        let payload_str = if frame.frame_type == WsFrameType::Binary {
                                            if let Some(dec) = decoder {
                                                match dec.decode(&frame.payload) {
                                                    Ok(val) => val.to_string(),
                                                    Err(e) => {
                                                        warn!("Decoder failed during matching: {}. Falling back to hex.", e);
                                                        format!("0x{}", hex::encode(&frame.payload))
                                                    }
                                                }
                                            } else {
                                                format!("0x{}", hex::encode(&frame.payload))
                                            }
                                        } else {
                                            frame.payload_as_string()
                                        };

                                        last_matching_payload = Some(payload_str);
                                        matched = true;
                                        break;
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                                    warn!("WS expect channel lagged by {} frames, skipping lagged packets.", missed);
                                    continue;
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                    break;
                                }
                            }
                        }
                    }
                }

                if !matched {
                    return Err(format!(
                        "Expect match timed out or failed. Condition: {}",
                        resolved_condition
                    ));
                }

                if let Some(payload_str) = last_matching_payload {
                    let virtual_response = Response::new(
                        200,
                        HeaderMap::new(),
                        payload_str,
                        Duration::from_millis(0),
                        Duration::from_millis(0),
                        Duration::from_millis(0),
                    )
                    .unwrap();

                    if !captures.is_empty() {
                        VariableCapture::capture_normal(
                            &captures,
                            &virtual_response.body,
                            &virtual_response.headers,
                            context,
                        );
                    }

                    if !assertions.is_empty() {
                        let resolved_local_assertions: Vec<String> = assertions
                            .iter()
                            .map(|a| VariableResolver::resolve(a, context))
                            .collect();
                        let local_assert_results =
                            evaluate_assertions(&resolved_local_assertions, &virtual_response);
                        for mut r in local_assert_results {
                            r.stream_event_index = Some(action_idx + 1);
                            assertion_results.push(r);
                        }
                    }

                    VariableCapture::capture_normal(
                        captures_to_eval,
                        &virtual_response.body,
                        &virtual_response.headers,
                        context,
                    );

                    let resolved_assertions: Vec<String> = assertions_to_eval
                        .iter()
                        .map(|a| VariableResolver::resolve(a, context))
                        .collect();

                    let assertions = evaluate_assertions(&resolved_assertions, &virtual_response);
                    assertion_results.extend(assertions);
                }

                Ok(true)
            }
            WsAction::Close => {
                info!("WS [CLOSE #{}] Active Close connection.", action_idx);
                let close_frame =
                    WsFrame::new(FrameDirection::Outbound, WsFrameType::Close, Vec::new(), 0);
                let _ = client.send_frame(close_frame).await;
                Ok(false)
            }
        }
    }
}

impl WsRunner {
    fn resolve_ws_url(url: &str) -> String {
        let mut resolved = url.to_string();
        if resolved.starts_with("https://") {
            resolved.replace_range(0..8, "wss://");
        } else if resolved.starts_with("http://") {
            resolved.replace_range(0..7, "ws://");
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::{FrameDirection, WsFrame, WsFrameType};
    use tokio::sync::broadcast;

    #[test]
    fn test_resolve_ws_url() {
        let url = "http://example.com/api?redirect=http://google.com";
        let resolved = WsRunner::resolve_ws_url(url);
        assert_eq!(resolved, "ws://example.com/api?redirect=http://google.com");
    }

    #[tokio::test]
    async fn test_lagged_broadcast_handling() {
        // Create a broadcast channel with capacity 1
        let (tx, mut rx) = broadcast::channel::<WsFrame>(1);

        // Send two frames to cause the receiver to lag
        let frame1 = WsFrame::new(
            FrameDirection::Inbound,
            WsFrameType::Text,
            b"msg1".to_vec(),
            0,
        );
        let frame2 = WsFrame::new(
            FrameDirection::Inbound,
            WsFrameType::Text,
            b"msg2".to_vec(),
            0,
        );

        tx.send(frame1).unwrap();
        tx.send(frame2).unwrap();

        // Test that our loop continues when encountering Lagged, rather than breaking
        let mut loop_count = 0;
        let mut received_msg2 = false;

        for _ in 0..5 {
            tokio::select! {
                maybe_frame = rx.recv() => {
                    match maybe_frame {
                        Ok(frame) => {
                            if frame.payload == b"msg2" {
                                received_msg2 = true;
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            loop_count += 1;
                            continue;
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        assert_eq!(
            loop_count, 1,
            "Should have encountered exactly 1 Lagged error"
        );
        assert!(
            received_msg2,
            "Should have successfully recovered and received msg2"
        );
    }
}
