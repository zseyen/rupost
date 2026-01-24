use crate::Result;
use crate::assertion::{AssertionResult, evaluate_assertion, parse_assertion};
use crate::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use crate::history::storage::get_storage;
use crate::http::Client;
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver, capture_from_response};
use reqwest::header::{HeaderName, HeaderValue};
use std::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct TestExecutor {
    client: Client,
}

impl TestExecutor {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// 批量执行所有请求
    pub async fn execute_all(
        &self,
        parsed_file: ParsedFile,
        context: &mut VariableContext,
    ) -> Result<Vec<TestResult>> {
        let mut results = Vec::new();

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
                .execute_one(parsed_request, request_number, context)
                .await;
            results.push(result);
        }

        Ok(results)
    }

    /// 执行单个请求
    async fn execute_one(
        &self,
        mut parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
    ) -> TestResult {
        // 1. 变量替换
        // 替换 URL
        parsed.url = VariableResolver::resolve(&parsed.url, context);

        // 替换 Headers
        for (_key, value) in &mut parsed.headers {
            *value = VariableResolver::resolve(value, context);
            // header key 通常不需要替换，也可以根据需求支持
        }

        // 替换 Body
        if let Some(body) = &mut parsed.body {
            *body = VariableResolver::resolve(body, context);
        }

        let method = parsed.method_or_default().to_string();
        let url = parsed.url.clone();
        let name = parsed.name().map(|s| s.to_string());

        // 开始计时
        let start = Instant::now();

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
        let request = match parsed.try_into() {
            Ok(req) => req,
            Err(e) => {
                return TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    format!("Failed to build request: {}", e),
                    start.elapsed(),
                );
            }
        };

        // 执行请求
        match self.client.execute(request).await {
            Ok(response) => {
                // 计算耗时
                let duration = start.elapsed();

                // [History] 异步保存历史记录 (Best Effort)
                let history_entry = HistoryEntry {
                    id: Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now(),
                    duration_ms: duration.as_millis() as u64,
                    request: request_snapshot,
                    response: ResponseMeta {
                        status: response.status.code(),
                        headers: response.headers.clone(),
                    },
                };

                // 不等待历史记录写入，避免阻塞测试流程（对于本地文件写很快，同步也无妨）
                // 若要极致性能可放到 spawn blocking，但这里保持简单
                if let Err(e) = get_storage().append(&history_entry) {
                    warn!("Failed to save request history: {}", e);
                }

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
                    TestResult::success(request_number, name, method, url, response);
                test_result.assertions = assertion_results;

                // 如果有断言失败，标记测试为失败
                if test_result.assertions.iter().any(|a| !a.passed) {
                    test_result.success = false;
                }

                test_result
            }
            Err(e) => TestResult::error(
                request_number,
                name,
                method,
                url,
                format!("Request failed: {}", e),
                start.elapsed(),
            ),
        }
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}
