use crate::report::json::JsonFileReport;
use crate::runner::types::TestResult;
use crate::utils::path_guard::normalize_posix_path;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::PathBuf;

/// 跨格式统一测试报告上下文（Clean Architecture 领域视图契约）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportContext {
    pub title: String,
    pub generated_at: String,
    pub environment: Option<String>,
    pub summary: ReportSummary,
    pub files: Vec<ReportFileEntry>,
}

/// 全局汇总统计指标
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total_files: usize,
    pub total_requests: usize,
    pub passed_requests: usize,
    pub failed_requests: usize,
    pub skipped_requests: usize,
    pub pass_rate: f64,
    pub total_duration_ms: u64,
}

impl ReportSummary {
    /// 统一计算汇总统计口径（DRY 抽象）
    pub fn calculate(
        total_files: usize,
        total_requests: usize,
        passed_requests: usize,
        failed_requests: usize,
        skipped_requests: usize,
        total_duration_ms: u64,
    ) -> Self {
        let non_skipped = total_requests.saturating_sub(skipped_requests);
        let pass_rate = if non_skipped > 0 {
            (passed_requests as f64 / non_skipped as f64) * 100.0
        } else {
            100.0
        };

        Self {
            total_files,
            total_requests,
            passed_requests,
            failed_requests,
            skipped_requests,
            pass_rate,
            total_duration_ms,
        }
    }
}

/// 单个测试文件的测试记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportFileEntry {
    pub file_path: String,
    pub total_requests: usize,
    pub passed_requests: usize,
    pub failed_requests: usize,
    pub duration_ms: u64,
    pub requests: Vec<ReportRequestItem>,
}

/// 单个请求的结构化展示条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRequestItem {
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub success: bool,
    pub skipped: bool,
    pub error: Option<String>,
    pub assertions: Vec<ReportAssertionItem>,
    pub warnings: Vec<String>,
    pub timing_waterfall: Option<TimingWaterfall>,
    pub body_snippet: Option<String>,
}

/// 单条断言的执行结果展示
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportAssertionItem {
    pub raw: String,
    pub passed: bool,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl From<&crate::assertion::AssertionResult> for ReportAssertionItem {
    fn from(a: &crate::assertion::AssertionResult) -> Self {
        Self {
            raw: a.raw.clone(),
            passed: a.passed,
            message: a.message.clone(),
            expected: Some(a.expected.clone()),
            actual: a.actual.clone(),
        }
    }
}

/// 归一化的时延瀑布模型（用于渲染时延瀑布条）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimingWaterfall {
    pub dns_ms: f64,
    pub tcp_ms: f64,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub transfer_ms: f64,
    pub total_ms: f64,
}

impl TimingWaterfall {
    /// 从 DiagnosticsReport 归一化瀑布图（唯一实现，在线/离线两路径共用）。
    /// transfer 采用残差法：total 减去已计入各段，负值钳为 0。
    pub fn from_diagnostics(diag: &crate::http::DiagnosticsReport) -> Self {
        let dns_ms = diag.dns_lookup_duration.as_secs_f64() * 1000.0;
        let tcp_ms = diag.tcp_connect_duration.as_secs_f64() * 1000.0;
        let tls_ms = diag
            .tls_handshake_duration
            .map(|d| d.as_secs_f64() * 1000.0);
        let ttfb_ms = diag.ttfb.map(|d| d.as_secs_f64() * 1000.0);
        let total_ms = diag.total_duration.as_secs_f64() * 1000.0;
        let accounted = dns_ms + tcp_ms + tls_ms.unwrap_or(0.0) + ttfb_ms.unwrap_or(0.0);
        let transfer_ms = (total_ms - accounted).max(0.0);

        Self {
            dns_ms,
            tcp_ms,
            tls_ms,
            ttfb_ms,
            transfer_ms,
            total_ms,
        }
    }

    /// 纯总耗时 fallback（唯一实现，在线 duration / 离线 duration_ms 两路径共用）。
    /// f64 保亚毫秒精度；total <= 0（skipped/零耗时）返回 None。
    pub fn from_total_ms(total_ms: f64) -> Option<Self> {
        if total_ms <= 0.0 {
            return None;
        }
        Some(Self {
            dns_ms: 0.0,
            tcp_ms: 0.0,
            tls_ms: None,
            ttfb_ms: Some(total_ms),
            transfer_ms: 0.0,
            total_ms,
        })
    }

    /// 从可能存在的 DiagnosticsReport 或 RequestTiming 中提取归一化瀑布图。
    /// 三个分支形态互不可替代：diag（残差 transfer + 可选 TLS）、
    /// timing（total=四段和、transfer=实测）、duration fallback（ttfb=total）。
    pub fn from_test_result(result: &TestResult) -> Option<Self> {
        if let Some(ref diag) = result.diagnose_report {
            Some(Self::from_diagnostics(diag))
        } else if let Some(ref timing) = result.timing {
            let dns_ms = timing.dns_lookup.as_secs_f64() * 1000.0;
            let tcp_ms = timing.tcp_connect.as_secs_f64() * 1000.0;
            let ttfb_val = timing.ttfb.as_secs_f64() * 1000.0;
            let transfer_ms = timing.transfer.as_secs_f64() * 1000.0;
            let total_ms = dns_ms + tcp_ms + ttfb_val + transfer_ms;

            Some(Self {
                dns_ms,
                tcp_ms,
                tls_ms: None,
                ttfb_ms: Some(ttfb_val),
                transfer_ms,
                total_ms,
            })
        } else {
            Self::from_total_ms(result.duration.as_secs_f64() * 1000.0)
        }
    }
}

impl ReportContext {
    /// 将执行器产出的批量运行结果转换为统一报告上下文
    pub fn from_file_results(
        file_results: &[(PathBuf, Vec<TestResult>)],
        title: Option<&str>,
        environment: Option<&str>,
    ) -> Self {
        let mut total_requests: usize = 0;
        let mut passed_requests: usize = 0;
        let mut failed_requests: usize = 0;
        let mut skipped_requests: usize = 0;
        let mut total_duration_ms: u64 = 0;

        let mut file_entries = Vec::new();

        for (path, results) in file_results {
            let mut file_passed = 0;
            let mut file_failed = 0;
            let mut file_duration_ms = 0;
            let mut request_items = Vec::new();

            for res in results {
                total_requests += 1;
                let duration_ms = res.duration.as_millis() as u64;
                file_duration_ms += duration_ms;

                if res.skipped {
                    skipped_requests += 1;
                } else if res.success {
                    passed_requests += 1;
                    file_passed += 1;
                } else {
                    failed_requests += 1;
                    file_failed += 1;
                }

                // 断言 expected/actual/message 与 JSON 出口同源脱敏（可能携带
                // body.access_token 等响应敏感值；raw 命中敏感键 → 整体 [REDACTED]）
                let assertions = res
                    .assertions
                    .iter()
                    .map(|a| ReportAssertionItem {
                        raw: a.raw.clone(),
                        passed: a.passed,
                        message: crate::ai::redactor::redact_assertion_value(
                            &a.raw,
                            a.message.as_deref(),
                        ),
                        expected: crate::ai::redactor::redact_assertion_value(
                            &a.raw,
                            (!a.expected.is_empty()).then_some(a.expected.as_str()),
                        ),
                        actual: crate::ai::redactor::redact_assertion_value(
                            &a.raw,
                            a.actual.as_deref(),
                        ),
                    })
                    .collect();

                // 失败片段与 JSON 报告出口同源脱敏（redact_and_truncate_body：JSON
                // 敏感键掩码 / 非 JSON 自由文本 URL+内联凭据兜底 + UTF-8 安全截断）——
                // HTML 工件常随 CI 归档共享，不得弱于机器可读出口（走查缺口 #2）
                let body_snippet = if !res.success {
                    res.response
                        .as_ref()
                        .and_then(|resp| resp.text().ok())
                        .map(|text| {
                            crate::ai::redactor::redact_and_truncate_body(
                                text,
                                crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
                            )
                        })
                        .filter(|s| !s.is_empty())
                } else {
                    None
                };

                let timing_waterfall = TimingWaterfall::from_test_result(res);

                request_items.push(ReportRequestItem {
                    request_number: res.request_number,
                    name: res.name.clone(),
                    method: res.method.clone(),
                    // 以下四字段与 build_json_reports 同一脱敏原语（审查发现：在线
                    // 直渲路径曾弱于 JSON 出口——url query 凭据/错误文本 URL/断言值）
                    url: crate::ai::redactor::redact_url(&res.url),
                    status: res.status,
                    duration_ms,
                    success: res.success,
                    skipped: res.skipped,
                    error: res
                        .error
                        .as_deref()
                        .map(crate::ai::redactor::redact_free_text),
                    assertions,
                    warnings: res
                        .warnings
                        .iter()
                        .map(|w| crate::ai::redactor::redact_free_text(w))
                        .collect(),
                    timing_waterfall,
                    body_snippet,
                });
            }

            total_duration_ms += file_duration_ms;

            file_entries.push(ReportFileEntry {
                file_path: normalize_posix_path(path),
                total_requests: results.len(),
                passed_requests: file_passed,
                failed_requests: file_failed,
                duration_ms: file_duration_ms,
                requests: request_items,
            });
        }

        let summary = ReportSummary::calculate(
            file_results.len(),
            total_requests,
            passed_requests,
            failed_requests,
            skipped_requests,
            total_duration_ms,
        );

        Self {
            title: title.unwrap_or("Rupost API Test Report").to_string(),
            generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            environment: environment.map(|s| s.to_string()),
            summary,
            files: file_entries,
        }
    }

    /// 从已保存的 JsonFileReport 列表还原报告上下文（用于离线二次渲染）
    pub fn from_json_file_reports(
        reports: &[JsonFileReport],
        title: Option<&str>,
        environment: Option<&str>,
    ) -> Self {
        let mut total_requests: usize = 0;
        let mut passed_requests: usize = 0;
        let mut failed_requests: usize = 0;
        let mut skipped_requests: usize = 0;
        let mut total_duration_ms: u64 = 0;

        let mut file_entries = Vec::new();

        for file_rep in reports {
            let mut file_passed = 0;
            let mut file_failed = 0;
            let mut file_duration_ms = 0;
            let mut request_items = Vec::new();

            for res in &file_rep.results {
                total_requests += 1;
                file_duration_ms += res.duration_ms;

                if res.skipped {
                    skipped_requests += 1;
                } else if res.success {
                    passed_requests += 1;
                    file_passed += 1;
                } else {
                    failed_requests += 1;
                    file_failed += 1;
                }

                let assertions = res
                    .assertions
                    .iter()
                    .map(|a| ReportAssertionItem {
                        raw: a.raw.clone(),
                        passed: a.passed,
                        message: a.message.clone(),
                        expected: a.expected.clone(),
                        actual: a.actual.clone(),
                    })
                    .collect();

                // P1-5（review）：优先 diagnose（TLS/证书）→ 次选 JSON timing 分段
                // （M7 瀑布）→ 回退总时长——离线复渲染不再退化为单段
                let timing_waterfall = res
                    .diagnose_report
                    .as_ref()
                    .map(TimingWaterfall::from_diagnostics)
                    .or_else(|| {
                        res.timing.as_ref().map(|t| TimingWaterfall {
                            dns_ms: t.dns_ms as f64,
                            tcp_ms: t.tcp_ms as f64,
                            tls_ms: None,
                            ttfb_ms: Some(t.ttfb_ms as f64),
                            transfer_ms: t.transfer_ms as f64,
                            total_ms: (t.dns_ms + t.tcp_ms + t.ttfb_ms + t.transfer_ms) as f64,
                        })
                    })
                    .or_else(|| {
                        // 普通请求 fallback：对齐在线模式，提供基于 duration_ms 的简单瀑布
                        TimingWaterfall::from_total_ms(res.duration_ms as f64)
                    });

                request_items.push(ReportRequestItem {
                    request_number: res.request_number,
                    name: res.name.clone(),
                    method: res.method.clone(),
                    url: res.url.clone(),
                    status: res.status,
                    duration_ms: res.duration_ms,
                    success: res.success,
                    skipped: res.skipped,
                    error: res.error.clone(),
                    assertions,
                    warnings: res.warnings.clone(),
                    timing_waterfall,
                    body_snippet: res.response_body_snippet.clone(),
                });
            }

            total_duration_ms += file_duration_ms;

            file_entries.push(ReportFileEntry {
                file_path: file_rep.file_path.clone(),
                total_requests: file_rep.results.len(),
                passed_requests: file_passed,
                failed_requests: file_failed,
                duration_ms: file_duration_ms,
                requests: request_items,
            });
        }

        let summary = ReportSummary::calculate(
            reports.len(),
            total_requests,
            passed_requests,
            failed_requests,
            skipped_requests,
            total_duration_ms,
        );

        Self {
            title: title.unwrap_or("Rupost API Test Report").to_string(),
            generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            environment: environment.map(|s| s.to_string()),
            summary,
            files: file_entries,
        }
    }
}

/// 严格安全的 HTML 字符转义函数，直接委托统一安全清洗组件 HtmlSanitizer
pub fn escape_html(input: &str) -> Cow<'_, str> {
    crate::security::HtmlSanitizer::escape(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_escape_html_security_and_cow() {
        // 无转义字符时，直接借用原字符串，零堆内存分配
        let clean_text = "https://example.com/api/v1/users";
        match escape_html(clean_text) {
            Cow::Borrowed(b) => assert_eq!(b, clean_text),
            Cow::Owned(_) => panic!("Should be borrowed for clean string"),
        }

        // 包含敏感字符时，正确转义为 Owned
        let malicious = "<script>alert('xss & \"attack\"')</script>";
        let clean = escape_html(malicious);
        assert_eq!(
            clean,
            "&lt;script&gt;alert(&#39;xss &amp; &quot;attack&quot;&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_report_summary_pass_rate_calculation() {
        let mut result_ok = TestResult::error(
            1,
            Some("test_ok".to_string()),
            "GET".to_string(),
            "http://example.com".to_string(),
            "fake".to_string(),
            Duration::from_millis(100),
        );
        result_ok.success = true; // 改为成功
        result_ok.error = None;

        let result_fail = TestResult::error(
            2,
            Some("test_fail".to_string()),
            "POST".to_string(),
            "http://example.com".to_string(),
            "404".to_string(),
            Duration::from_millis(150),
        );

        let result_skip = TestResult::skipped(
            3,
            Some("test_skip".to_string()),
            "GET".to_string(),
            "http://example.com".to_string(),
        );

        let file_results = vec![(
            PathBuf::from("tests/suite.http"),
            vec![result_ok, result_fail, result_skip],
        )];

        let ctx =
            ReportContext::from_file_results(&file_results, Some("Custom Title"), Some("staging"));

        assert_eq!(ctx.title, "Custom Title");
        assert_eq!(ctx.environment.as_deref(), Some("staging"));
        assert_eq!(ctx.summary.total_files, 1);
        assert_eq!(ctx.summary.total_requests, 3);
        assert_eq!(ctx.summary.passed_requests, 1);
        assert_eq!(ctx.summary.failed_requests, 1);
        assert_eq!(ctx.summary.skipped_requests, 1);
        assert!((ctx.summary.pass_rate - 50.0).abs() < f64::EPSILON);
        assert_eq!(ctx.summary.total_duration_ms, 250);
    }

    #[test]
    fn test_timing_branch_waterfall_shape() {
        // timing 分支（普通请求，无 diagnose）：total = 四段和、transfer = 实测、tls = None。
        // 与 from_total_ms 的 ttfb=total 形态完全不同，唯一化时不可吞掉（审查 P1-3）。
        use crate::http::timing::RequestTiming;
        let timing = RequestTiming {
            dns_lookup: Duration::from_millis(2),
            tcp_connect: Duration::from_millis(3),
            ttfb: Duration::from_millis(5),
            transfer: Duration::from_millis(7),
        };
        let result = TestResult {
            duration: Duration::from_millis(99),
            timing: Some(timing),
            diagnose_report: None,
            ..TestResult::error(
                1,
                None,
                "GET".to_string(),
                "http://example.com".to_string(),
                "e".to_string(),
                Duration::from_millis(99),
            )
        };
        let wf = TimingWaterfall::from_test_result(&result).unwrap();
        assert_eq!(wf.dns_ms, 2.0);
        assert_eq!(wf.tcp_ms, 3.0);
        assert_eq!(wf.tls_ms, None);
        assert_eq!(wf.ttfb_ms, Some(5.0));
        assert_eq!(wf.transfer_ms, 7.0);
        assert_eq!(wf.total_ms, 17.0);
    }

    #[test]
    fn test_diagnostics_residual_transfer_clamped() {
        // diagnose 分支残差法：total < 已计入各段时 transfer 钳为 0（审查 P1-3）
        use crate::http::DiagnosticsReport;
        let diag = DiagnosticsReport {
            url: "https://example.com".to_string(),
            is_https: true,
            is_websocket: false,
            ws_upgrade_success: Some(false),
            resolved_ips: Vec::new(),
            dns_lookup_duration: Duration::from_millis(4),
            tcp_connect_duration: Duration::from_millis(6),
            tls_handshake_duration: Some(Duration::from_millis(8)),
            cert_info: None,
            http_status: Some(200),
            http_version: None,
            ttfb: Some(Duration::from_millis(10)),
            total_duration: Duration::from_millis(20), // accounted = 28 > 20
            connection_error: None,
        };
        let wf = TimingWaterfall::from_diagnostics(&diag);
        assert_eq!(wf.dns_ms, 4.0);
        assert_eq!(wf.tls_ms, Some(8.0));
        assert_eq!(wf.ttfb_ms, Some(10.0));
        assert_eq!(wf.total_ms, 20.0);
        assert_eq!(wf.transfer_ms, 0.0, "residual must clamp to zero");
    }

    #[test]
    fn test_from_total_ms_zero_is_none() {
        assert!(TimingWaterfall::from_total_ms(0.0).is_none());
        assert!(TimingWaterfall::from_total_ms(-1.0).is_none());
        let wf = TimingWaterfall::from_total_ms(1.5).unwrap();
        assert_eq!(wf.total_ms, 1.5, "f64 preserves sub-millisecond precision");
        assert_eq!(wf.ttfb_ms, Some(1.5));
        assert_eq!(wf.dns_ms, 0.0);
    }
}
