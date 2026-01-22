use crate::Result;
use crate::assertion::{AssertionResult, evaluate_assertion, parse_assertion};
use crate::http::Client;
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::types::TestResult;
use std::time::Instant;

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
    pub async fn execute_all(&self, parsed_file: ParsedFile) -> Result<Vec<TestResult>> {
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

            let result = self.execute_one(parsed_request, request_number).await;
            results.push(result);
        }

        Ok(results)
    }

    /// 执行单个请求
    async fn execute_one(&self, parsed: ParsedRequest, request_number: usize) -> TestResult {
        let method = parsed.method_or_default().to_string();
        let url = parsed.url.clone();
        let name = parsed.name().map(|s| s.to_string());

        // 开始计时
        let start = Instant::now();

        // 提前保存断言列表（在 parsed 被移动前）
        let assertions_to_eval = parsed.metadata.assertions.clone();

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
                // 执行断言求值
                let mut assertion_results = Vec::new();

                for assertion_str in &assertions_to_eval {
                    match parse_assertion(assertion_str) {
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
