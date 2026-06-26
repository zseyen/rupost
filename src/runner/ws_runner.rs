use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::assertion::{evaluate_assertions, AssertionResult};
use crate::http::Response;
use crate::parser::types::ParsedRequest;
use crate::runner::types::TestResult;
use crate::variable::capture::VariableCapture;
use crate::variable::{VariableContext, VariableResolver};
use crate::ws::{WsClient, WsClientConfig, WsAction, WsActionParser, WsFrame, WsFrameType, FrameDirection, MsgPackDecoder, PayloadDecoder};

pub struct WsRunner;

impl WsRunner {
    /// 执行 WebSocket 流式剧本用例
    pub async fn execute(
        request: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
    ) -> TestResult {
        let start_time = Instant::now();
        let name = request.name().map(|s| s.to_string());
        
        // 1. 变量解析 URL 与 Headers
        let resolved_url = VariableResolver::resolve(&request.url, context);
        let mut resolved_headers = Vec::new();
        for (k, v) in &request.headers {
            let resolved_val = VariableResolver::resolve(v, context);
            resolved_headers.push((k.clone(), resolved_val));
        }

        info!("Starting WebSocket session to {}", resolved_url);

        // 2. 建立 WebSocket 连接
        let config = WsClientConfig {
            url: resolved_url.clone(),
            headers: resolved_headers,
            ping_interval: Duration::from_secs(10), // 默认 10 秒自动心跳保活
            handshake_timeout: request.metadata.timeout.unwrap_or(Duration::from_secs(5)),
        };

        let client = match WsClient::connect(config).await {
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

        // 3. 解析 Body 文本为 WsAction 流
        let body_content = request.body.as_deref().unwrap_or("");
        let actions = match WsActionParser::parse_body(body_content) {
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
        let decoder: Option<Box<dyn PayloadDecoder>> = match request.metadata.decoder.as_deref() {
            Some("messagepack") => Some(Box::new(MsgPackDecoder)),
            Some(other) => {
                warn!("Unsupported decoder: {}, falling back to None", other);
                None
            }
            None => None,
        };

        // 4. 驱动 WsAction 执行流
        for (action_idx, action) in actions.into_iter().enumerate() {
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
                WsAction::Expect { condition, timeout } => {
                    // 动态替换 Expect 中的匹配变量
                    let resolved_condition = VariableResolver::resolve(&condition, context);
                    info!("WS [EXPECT #{}] Waiting up to {:?} for condition: {}", action_idx, timeout, resolved_condition);

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
                                            let payload_str = frame.payload_as_string();
                                            // 检查是否能匹配上条件
                                            if Self::matches_condition(&payload_str, &resolved_condition) {
                                                matched = true;
                                                last_matching_payload = Some((frame.payload, frame.frame_type));
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        final_error = Some(format!("Expect failed due to channel error: {}", e));
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if !matched {
                        final_error = Some(format!(
                            "Expect timed out. Expected message matching: '{}'",
                            resolved_condition
                        ));
                        break;
                    } else if let Some((payload, frame_type)) = last_matching_payload {
                        // 5. 匹配成功后，执行变量捕获与断言评估
                        let decoded_body = if frame_type == WsFrameType::Binary {
                            if let Some(ref dec) = decoder {
                                match dec.decode(&payload) {
                                    Ok(val) => val.to_string(),
                                    Err(e) => {
                                        warn!("Decoder failed: {}. Falling back to hex string.", e);
                                        format!("0x{}", hex::encode(&payload))
                                    }
                                }
                            } else {
                                format!("0x{}", hex::encode(&payload))
                            }
                        } else {
                            String::from_utf8_lossy(&payload).into_owned()
                        };

                        info!("WS [MATCHED #{}] Decoded Body: {}", action_idx, decoded_body);

                        // 构造虚拟 Response
                        let virtual_response = Response::new(
                            200,
                            HeaderMap::new(),
                            decoded_body,
                            Duration::from_millis(0),
                            Duration::from_millis(0),
                            Duration::from_millis(0),
                        ).unwrap();

                        // 运行变量捕获
                        VariableCapture::capture_normal(
                            &request.metadata.captures,
                            &virtual_response.body,
                            &virtual_response.headers,
                            context,
                        );

                        // 评估断言
                        let resolved_assertions: Vec<String> = request.metadata.assertions
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
                format!("WebSocket Session Completed Successfully. Run {} actions.", action_idx_count(body_content)),
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
    fn matches_condition(msg_str: &str, condition: &str) -> bool {
        let condition = condition.trim();
        if condition.is_empty() {
            return true;
        }

        // 1. 如果 condition 是一个合法的 JSON，尝试执行 JSON 子集匹配
        if let (Ok(cond_val), Ok(msg_val)) = (
            serde_json::from_str::<Value>(condition),
            serde_json::from_str::<Value>(msg_str)
        ) {
            return Self::match_json_subset(&cond_val, &msg_val);
        }

        // 2. 否则，降级使用子字符串包含校验
        msg_str.contains(condition)
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

/// 辅助统计 action 数量的局部函数
fn action_idx_count(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty() && !t.starts_with('#') && !t.starts_with("//")
        })
        .count()
}

/// 简单的 Hex 辅助实现 (防 hex 依赖版本冲突，使用内置格式化实现)
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
