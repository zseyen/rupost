use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::assertion::{evaluate_assertions, AssertionResult};
use crate::http::{Request, Response};
use crate::parser::types::ParsedRequest;
use crate::runner::types::TestResult;
use crate::variable::capture::VariableCapture;
use crate::variable::{VariableContext, VariableResolver};
use crate::ws::{WsSession, WsClientConfig, WsAction, WsActionParser, WsFrame, WsFrameType, FrameDirection, MsgPackDecoder, PayloadDecoder};
use crate::runner::executor::ExecutorMiddleware;
use crate::middleware::Middleware;

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
        let mut resolved_url = temp_req.url.to_string();
        if resolved_url.starts_with("https://") {
            resolved_url = resolved_url.replace("https://", "wss://");
        } else if resolved_url.starts_with("http://") {
            resolved_url = resolved_url.replace("http://", "ws://");
        }

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
            info!("WS Entering live monitoring mode for 5 seconds. Listening for incoming frames...");
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
                            Err(_) => break,
                        }
                    }
                }
            }
            
            // 优雅关闭
            let close_frame = WsFrame::new(
                FrameDirection::Outbound,
                WsFrameType::Close,
                Vec::new(),
                0,
            );
            let _ = client.send_frame(close_frame).await;
            executed_actions_count += 1;
        } else {
            // 6. 驱动 WsAction 执行流
            for (action_idx, action) in actions.into_iter().enumerate() {
                executed_actions_count = action_idx + 1;
                match action {
                    WsAction::Connect { .. } => {
                        // Connect 已在初始化时执行，跳过
                    }
                    WsAction::Send(frame) => {
                        // 对发送的 Payload 执行变量替换
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
                            final_error = Some(format!("Send failed: {}", e));
                            break;
                        }
                    }
                    WsAction::Wait(duration) => {
                        info!("WS [WAIT #{}] sleeping for {:?}", action_idx, duration);
                        tokio::time::sleep(duration).await;
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
                        // 动态替换 Expect 中的匹配变量
                        let resolved_condition = VariableResolver::resolve(&condition, context);
                        info!("WS [EXPECT #{}] Waiting up to {:?} for condition: {}", action_idx, timeout, resolved_condition);

                        // 预先对期望值模板执行变量替换，避免在高频收发帧循环中重复计算
                        let resolved_expected_value = expected_value.as_ref().map(|ev| {
                            VariableResolver::resolve(ev, context)
                        });

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
                                            if frame.direction == FrameDirection::Inbound {
                                                // 检查是否能匹配上条件
                                                if Self::matches_condition(
                                                    &frame,
                                                    &resolved_condition,
                                                    &segments,
                                                    &resolved_expected_value,
                                                    &operator,
                                                    decoder.as_deref()
                                                ) {
                                                    // 自适应解码出文本用于后续打印与 capture 提取
                                                    let payload_str = if frame.frame_type == WsFrameType::Binary {
                                                        if let Some(ref dec) = decoder {
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
                                        }
                                        Err(_) => break,
                                    }
                                }
                            }
                        }

                        if !matched {
                            final_error = Some(format!("Expect match timed out or failed. Condition: {}", resolved_condition));
                            break;
                        }

                        // 执行对匹配帧的局部断言与捕获 (Stage 4)
                        if let Some(payload_str) = last_matching_payload {
                            let virtual_response = Response::new(
                                200,
                                HeaderMap::new(),
                                payload_str,
                                Duration::from_millis(0),
                                Duration::from_millis(0),
                                Duration::from_millis(0),
                            ).unwrap();

                            // 1. 运行步骤级局部捕获 (Stage 4)
                            if !captures.is_empty() {
                                VariableCapture::capture_normal(
                                    &captures,
                                    &virtual_response.body,
                                    &virtual_response.headers,
                                    context,
                                );
                            }

                            // 2. 运行步骤级局部断言 (Stage 4)
                            if !assertions.is_empty() {
                                let resolved_local_assertions: Vec<String> = assertions
                                    .iter()
                                    .map(|a| VariableResolver::resolve(a, context))
                                    .collect();
                                let local_assert_results = evaluate_assertions(&resolved_local_assertions, &virtual_response);
                                for mut r in local_assert_results {
                                    r.stream_event_index = Some(action_idx + 1);
                                    assertion_results.push(r);
                                }
                            }

                            // 3. 运行全局变量捕获（对匹配帧适用，保持向后兼容）
                            VariableCapture::capture_normal(
                                &captures_to_eval,
                                &virtual_response.body,
                                &virtual_response.headers,
                                context,
                            );

                            // 4. 评估全局断言（对匹配帧适用，保持向后兼容）
                            let resolved_assertions: Vec<String> = assertions_to_eval
                                .iter()
                                .map(|a| VariableResolver::resolve(a, context))
                                .collect();

                            let assertions = evaluate_assertions(&resolved_assertions, &virtual_response);
                            assertion_results.extend(assertions);
                        }
                    }
                    WsAction::Close => {
                        info!("WS [CLOSE #{}] Active Close connection.", action_idx);
                        let close_frame = WsFrame::new(
                            FrameDirection::Outbound,
                            WsFrameType::Close,
                            Vec::new(),
                            0,
                        );
                        let _ = client.send_frame(close_frame).await;
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
                format!("WebSocket Session Completed Successfully. Run {} actions.", executed_actions_count),
                duration,
                Duration::from_millis(0),
                Duration::from_millis(0),
            ).unwrap();

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

    /// 判定接收帧内容是否匹配 Expect 条件
    fn matches_condition(
        frame: &WsFrame,
        condition: &str,
        segments: &Option<Vec<String>>,
        expected_value: &Option<String>,
        operator: &Option<String>,
        decoder: Option<&dyn crate::ws::PayloadDecoder>,
    ) -> bool {
        use crate::ws::matcher::{FrameMatcher, JsonPathMatcher, TextContainsMatcher};

        let condition = condition.trim();
        if condition.is_empty() {
            return true;
        }

        // 如果有预编译的 segments，使用 JsonPathMatcher (Stage 3)
        if let Some(segs) = segments {
            let matcher = JsonPathMatcher::new(segs.clone(), expected_value.clone(), operator.clone());
            return matcher.matches(frame, decoder);
        }

        // 1. 如果 condition 是一个合法的 JSON，尝试执行 JSON 子集匹配
        let msg_str = if let Some(dec) = decoder {
            match dec.decode(&frame.payload) {
                Ok(val) => match val {
                    Value::String(ref s) => s.clone(),
                    _ => val.to_string(),
                },
                Err(_) => frame.payload_as_string(),
            }
        } else {
            frame.payload_as_string()
        };

        if let (Ok(cond_val), Ok(msg_val)) = (
            serde_json::from_str::<Value>(condition),
            serde_json::from_str::<Value>(&msg_str)
        ) {
            return Self::match_json_subset(&cond_val, &msg_val);
        }

        // 2. 否则，降级使用子字符串包含匹配器
        let matcher = TextContainsMatcher { pattern: condition.to_string() };
        matcher.matches(frame, decoder)
    }

    /// 检查 pattern 是否为 target 的子集
    fn match_json_subset(pattern: &Value, target: &Value) -> bool {
        match (pattern, target) {
            (Value::Object(pat_obj), Value::Object(tgt_obj)) => {
                for (k, v) in pat_obj {
                    match tgt_obj.get(k) {
                        Some(tgt_v) => {
                            if !Self::match_json_subset(v, tgt_v) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (Value::Array(pat_arr), Value::Array(tgt_arr)) => {
                // 如果 pattern 是数组，简化校验：如果 pattern 为空则通过；
                // 否则，要求 pattern 的第一个元素能在 target 数组中匹配上
                if pat_arr.is_empty() {
                    return true;
                }
                pat_arr.iter().all(|p| tgt_arr.iter().any(|t| Self::match_json_subset(p, t)))
            }
            (p, t) => p == t,
        }
    }
}

/// 简单的 Hex 辅助实现 (防 hex 依赖版本冲突，使用内置格式化实现)
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
