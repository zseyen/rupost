//! JSON 报告契约模型与序列化输出（从 reporter.rs 迁入，统一 report 词根）
//!
//! 本文件是 JSON 测试报告的唯一契约来源：类型定义、内存结果 → 报告模型
//! 的脱敏转换（`build_json_reports`）、序列化输出（`report_json`）。
//! 消费方：`ai triage`（反序列化磁盘报告）、`--report json`（stdout/文件）、
//! HTML 报告离线二次渲染（`ReportContext::from_json_file_reports`）。

use crate::runner::types::TestResult;
use crate::utils::path_guard::normalize_posix_path;
use serde::{Deserialize, Serialize};

/// 单个文件的 JSON 测试报告（强类型序列化与反序列化契约，供 `ai triage` 消费）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFileReport {
    pub file_path: String,
    pub results: Vec<JsonTestReport>,
}

/// E-5：网络时序分段（毫秒粒度，供离线 HTML 复渲染瀑布）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTiming {
    pub dns_ms: u64,
    pub tcp_ms: u64,
    pub ttfb_ms: u64,
    pub transfer_ms: u64,
}

/// 单个请求的 JSON 测试报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTestReport {
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub skipped: bool,
    /// B-3：插件降级形态的成功（DegradedPass）——CI 看板可见性对齐 --strict-plugins
    #[serde(default)]
    pub degraded: bool,
    /// E-5：网络时序分段（毫秒；DNS/TCP/TTFB/Transfer）——JSON 报告离线渲染不退化。
    /// 恒序列化（None→null）保持键集合稳定（与 diagnose_report 同形态；失败条目
    /// 无时序数据时为 null，键集合不随形态漂移）
    #[serde(default)]
    pub timing: Option<JsonTiming>,
    pub assertions: Vec<JsonAssertionReport>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub diagnose_report: Option<crate::http::DiagnosticsReport>,
    /// 仅在失败请求 (!success) 且存在响应体时记录脱敏 + 512B 截断后的响应片段
    #[serde(default)]
    pub response_body_snippet: Option<String>,
    /// WP-P2a：请求方法行的 1-based 行号（稳定定位 `<path>#L<行号>`；缺席为 null）
    #[serde(default)]
    pub source_line: Option<usize>,
}

/// 单条断言的 JSON 报告（保留 expected/actual/stream_event_index 供 E2/E5 分诊）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonAssertionReport {
    pub raw: String,
    pub passed: bool,
    pub message: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default)]
    pub stream_event_index: Option<usize>,
}

/// 将内存测试结果转换为强类型报告（对失败请求的响应片段与 URL 脱敏并 512B 截断）
pub fn build_json_reports(
    results: &[(std::path::PathBuf, Vec<TestResult>)],
) -> Vec<JsonFileReport> {
    build_json_reports_with_secrets(results, &[])
}

/// 将内存测试结果转换为强类型报告，并在全部展示字段上应用显式密钥值清单（`--mask-secret-value`）。
pub fn build_json_reports_with_secrets(
    results: &[(std::path::PathBuf, Vec<TestResult>)],
    secret_values: &[String],
) -> Vec<JsonFileReport> {
    let ordered = crate::ai::redactor::order_secret_values(secret_values);
    let mask_explicit = |s: String| -> String {
        crate::ai::redactor::redact_explicit_secrets_ordered(&s, &ordered)
    };

    results
        .iter()
        .map(|(path, file_results)| JsonFileReport {
            file_path: normalize_posix_path(path),
            results: file_results
                .iter()
                .map(|r| {
                    let assertions = r
                        .assertions
                        .iter()
                        .map(|a| {
                            // Gap Review P1-2：expected/actual/message 可能携带响应体敏感值
                            // （如 body.access_token 的实际取值），与 snippet 同级脱敏
                            JsonAssertionReport {
                                raw: mask_explicit(a.raw.clone()),
                                passed: a.passed,
                                message: crate::ai::redactor::redact_assertion_value(
                                    &a.raw,
                                    a.message.as_deref(),
                                )
                                .map(mask_explicit),
                                expected: crate::ai::redactor::redact_assertion_value(
                                    &a.raw,
                                    (!a.expected.is_empty()).then_some(a.expected.as_str()),
                                )
                                .map(mask_explicit),
                                actual: crate::ai::redactor::redact_assertion_value(
                                    &a.raw,
                                    a.actual.as_deref(),
                                )
                                .map(mask_explicit),
                                stream_event_index: a.stream_event_index,
                            }
                        })
                        .collect();

                    let response_body_snippet = if !r.success {
                        r.response
                            .as_ref()
                            .map(|resp| {
                                // 先按显式值脱敏再做结构化键名脱敏与 512B 截断，避免截断边界露出半截密钥
                                let pre_masked = mask_explicit(resp.body.clone());
                                crate::ai::redactor::redact_and_truncate_body(
                                    &pre_masked,
                                    crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
                                )
                            })
                            .filter(|s| !s.is_empty())
                    } else {
                        None
                    };

                    JsonTestReport {
                        request_number: r.request_number,
                        name: r.name.clone(),
                        method: r.method.clone(),
                        url: mask_explicit(crate::ai::redactor::redact_url(&r.url)),
                        // WP-P2a：稳定定位行号（构造器缺省 None 的字段显式透传）
                        source_line: r.source_line,
                        status: r.status,
                        duration_ms: r.duration.as_millis() as u64,
                        success: r.success,
                        degraded: r.degraded,
                        timing: r.timing.as_ref().map(|t| JsonTiming {
                            dns_ms: t.dns_lookup.as_millis() as u64,
                            tcp_ms: t.tcp_connect.as_millis() as u64,
                            ttfb_ms: t.ttfb.as_millis() as u64,
                            transfer_ms: t.transfer.as_millis() as u64,
                        }),
                        // Gap Review P1-1：reqwest 错误文本自带完整原始 URL，必须掩码
                        error: r
                            .error
                            .as_deref()
                            .map(crate::ai::redactor::redact_free_text)
                            .map(mask_explicit),
                        skipped: r.skipped,
                        assertions,
                        // 报告级红线：warnings 值源（未定义变量名/位置错误上下文）统一脱敏
                        warnings: r
                            .warnings
                            .iter()
                            .map(|w| mask_explicit(crate::ai::redactor::redact_free_text(w)))
                            .collect(),
                        // diagnose 的 URL 来自 --debug 实测原始地址，出站前与顶层 url 同口径脱敏
                        diagnose_report: r.diagnose_report.clone().map(|mut diag| {
                            diag.url = mask_explicit(crate::ai::redactor::redact_url(&diag.url));
                            diag
                        }),
                        response_body_snippet,
                    }
                })
                .collect(),
        })
        .collect()
}

/// 导出为 JSON 报告（writer 注入形态：stdout、文件与测试缓冲区共用同一实现）
pub fn report_json(
    results: &[(std::path::PathBuf, Vec<TestResult>)],
    writer: &mut impl std::io::Write,
) -> crate::Result<()> {
    report_json_with_secrets(results, &[], writer)
}

/// 导出为 JSON 报告（同时应用显式 `--mask-secret-value` 脱敏清单）
pub fn report_json_with_secrets(
    results: &[(std::path::PathBuf, Vec<TestResult>)],
    secret_values: &[String],
    writer: &mut impl std::io::Write,
) -> crate::Result<()> {
    let json_list = build_json_reports_with_secrets(results, secret_values);
    let json_str = serde_json::to_string_pretty(&json_list)?;
    writer.write_all(json_str.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::TestResult;
    use std::time::Duration;

    #[test]
    fn test_json_report_includes_warnings() {
        // 回归：JSON 报告此前手写字段清单漏掉 warnings——CI 用户（jq 解析）
        // 拿不到任何捕获失败/未定义变量警告
        let mut result = TestResult::error(
            1,
            Some("t".to_string()),
            "GET".to_string(),
            "http://example.com".to_string(),
            "boom".to_string(),
            Duration::from_millis(1),
        );
        result
            .warnings
            .push("变量 {{x}} 未定义，已按字面量发送".to_string());

        let results = vec![(std::path::PathBuf::from("a.http"), vec![result])];
        let mut out = Vec::new();
        report_json(&results, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("\"warnings\""),
            "JSON 报告应包含 warnings 字段"
        );
        assert!(text.contains("未定义"), "警告内容应完整输出: {text}");
    }

    #[test]
    fn test_json_report_round_trip_with_assertions_and_redacted_snippet() {
        use crate::assertion::AssertionResult;
        use crate::http::Response;
        use reqwest::header::HeaderMap;

        let resp = Response::new(
            401,
            HeaderMap::new(),
            r#"{"error":"invalid_token","access_token":"secret-xyz"}"#.to_string(),
            Duration::from_millis(12),
            Duration::from_millis(5),
            Duration::from_millis(7),
        )
        .unwrap();

        let mut result = TestResult::success(
            1,
            Some("AuthCheck".to_string()),
            "POST".to_string(),
            "https://api.example.com/login?api_key=12345".to_string(),
            resp,
        );
        result.success = false;
        // 注意 AssertionResult::failure 参数顺序：(raw, actual, expected, message)
        result.assertions.push(AssertionResult::failure(
            "status == 200".to_string(),
            "401".to_string(),
            "200".to_string(),
            "Status code mismatch".to_string(),
        ));

        let batch = vec![(std::path::PathBuf::from("auth.http"), vec![result])];
        let mut buf = Vec::new();
        report_json(&batch, &mut buf).unwrap();

        let parsed: Vec<JsonFileReport> = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.len(), 1);
        let req = &parsed[0].results[0];
        assert!(
            req.url.contains("api_key=%5BREDACTED%5D") || req.url.contains("api_key=[REDACTED]")
        );
        assert_eq!(req.assertions[0].expected.as_deref(), Some("200"));
        assert_eq!(req.assertions[0].actual.as_deref(), Some("401"));
        assert_eq!(
            req.assertions[0].message.as_deref(),
            Some("Status code mismatch")
        );
        let snippet = req.response_body_snippet.as_deref().unwrap();
        assert!(snippet.contains(r#""access_token":"[REDACTED]""#));
        assert!(!snippet.contains("secret-xyz"));
    }

    #[test]
    fn test_deserialize_legacy_report_with_required_fields_only() {
        // serde(default) 反向兼容：旧版本落盘的最小报告（缺 warnings/diagnose_report/
        // response_body_snippet 与断言的 expected/actual/stream_event_index）必须可反序列化
        let legacy = r#"[
            {
                "file_path": "legacy.http",
                "results": [
                    {
                        "request_number": 1,
                        "name": null,
                        "method": "GET",
                        "url": "http://example.com",
                        "status": 200,
                        "duration_ms": 12,
                        "success": true,
                        "error": null,
                        "skipped": false,
                        "assertions": [
                            { "raw": "status == 200", "passed": true, "message": null }
                        ]
                    }
                ]
            }
        ]"#;

        let parsed: Vec<JsonFileReport> = serde_json::from_str(legacy).unwrap();
        let req = &parsed[0].results[0];
        assert!(req.warnings.is_empty());
        assert!(req.diagnose_report.is_none());
        assert!(req.response_body_snippet.is_none());
        assert!(req.assertions[0].expected.is_none());
        assert!(req.assertions[0].stream_event_index.is_none());
    }

    #[test]
    fn test_build_json_reports_normalizes_windows_path_to_posix() {
        let windows_path = std::path::PathBuf::from("tests\\sub\\test.http");
        let result =
            TestResult::skipped(1, None, "GET".to_string(), "http://example.com".to_string());
        let results = vec![(windows_path, vec![result])];
        let reports = build_json_reports(&results);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].file_path, "tests/sub/test.http");
    }
}
