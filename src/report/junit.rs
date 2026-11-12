//! M1：JUnit XML 报告生成（Jenkins/GitLab/GitHub Actions 标准规格）。
//!
//! 数据源：已完成统一脱敏的 [`JsonFileReport`](crate::report::json::JsonFileReport)
//! 列表（SSOT 脱敏复用——本模块不做二次脱敏决策）；ANSI 控制符在构造期统一剥离
//! （评审 P1-14：单点出口，覆盖全部文本字段），XML 转义交由 quick-xml 序列化器。
//!
//! 语义（评审 P1-2/P1-3）：skipped > failure > error 优先级；请求级异常走
//! `<error>` 节点（`@errors` 计数），断言失败走 `<failure>`（`@failures`）；
//! time = duration_ms/1000 保留 3 位小数，suite/suites 逐层求和保证可加和性。

use crate::report::json::JsonFileReport;
use serde::Serialize;

/// 终端 ANSI 控制符剥离 + C0 控制字符清理（XML 1.0 同样非法；\t\n\r 保留）。
/// 双层：strip_ansi_escapes 处理 ANSI 序列，正则兜底其余 C0（红队补充：\x00 不在 ANSI 范围）。
fn strip_ansi(text: &str) -> String {
    static C0: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re =
        C0.get_or_init(|| regex::Regex::new("[\x00-\x08\x0b\x0c\x0e-\x1f]").expect("c0 regex"));
    let stripped =
        String::from_utf8_lossy(&strip_ansi_escapes::strip(text.as_bytes())).into_owned();
    re.replace_all(&stripped, "").into_owned()
}

/// 执行主机名取值链：`RUPOST_RUN_HOST` env（CI 显式注入）> `HOSTNAME` env
/// （Linux shell 常见）> 空串兜底（零新依赖；macOS 无 HOSTNAME env 属预期）
fn execution_hostname() -> String {
    std::env::var("RUPOST_RUN_HOST")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
}

/// JUnit 顶层 `<testsuites>`
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "testsuites")]
pub struct JUnitTestSuites {
    #[serde(rename = "@tests")]
    pub tests: usize,
    #[serde(rename = "@failures")]
    pub failures: usize,
    #[serde(rename = "@errors")]
    pub errors: usize,
    #[serde(rename = "@skipped")]
    pub skipped: usize,
    #[serde(rename = "@time")]
    pub time: String,
    /// 聚合时间锚点（RFC3339）——CI 平台按此归档/跨构建比对（走查缺口 #3）
    #[serde(rename = "@timestamp")]
    pub timestamp: String,
    /// 执行主机名（区分 runner 节点；`RUPOST_RUN_HOST` > `HOSTNAME` env > 空串兜底）
    #[serde(rename = "@hostname")]
    pub hostname: String,
    #[serde(rename = "testsuite")]
    pub suites: Vec<JUnitTestSuite>,
}

/// 单文件 `<testsuite>`（name = POSIX 归一化后的测试文件路径）
#[derive(Debug, Clone, Serialize)]
pub struct JUnitTestSuite {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@tests")]
    pub tests: usize,
    #[serde(rename = "@failures")]
    pub failures: usize,
    #[serde(rename = "@errors")]
    pub errors: usize,
    #[serde(rename = "@skipped")]
    pub skipped: usize,
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "testcase", default)]
    pub testcases: Vec<JUnitTestCase>,
}

/// 单请求 `<testcase>`
#[derive(Debug, Clone, Serialize)]
pub struct JUnitTestCase {
    #[serde(rename = "@name")]
    pub name: String, // 形如 "#1 (auth)" 或 "#2"
    #[serde(rename = "@classname")]
    pub classname: String, // 文件路径
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "failure", skip_serializing_if = "Option::is_none")]
    pub failure: Option<JUnitFailure>,
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    pub error: Option<JUnitError>,
    #[serde(rename = "skipped", skip_serializing_if = "Option::is_none")]
    pub skipped: Option<JUnitSkipped>,
    #[serde(rename = "system-out", skip_serializing_if = "Option::is_none")]
    pub system_out: Option<String>,
}

/// 断言失败 `<failure>`
#[derive(Debug, Clone, Serialize)]
pub struct JUnitFailure {
    #[serde(rename = "@message")]
    pub message: String,
    #[serde(rename = "@type")]
    pub failure_type: String, // 恒为 "AssertionFailure"
    #[serde(rename = "$text")]
    pub details: String,
}

/// 请求级异常 `<error>`（网络/DNS/超时）
#[derive(Debug, Clone, Serialize)]
pub struct JUnitError {
    #[serde(rename = "@message")]
    pub message: String,
    #[serde(rename = "@type")]
    pub error_type: String, // 恒为 "RequestError"
    #[serde(rename = "$text")]
    pub details: String,
}

/// `<skipped>`（--only 未选中 / @skip 标记）
#[derive(Debug, Clone, Serialize)]
pub struct JUnitSkipped {
    #[serde(rename = "@message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 毫秒 → 秒字符串（保留 3 位小数）
fn secs(duration_ms: u64) -> String {
    format!("{:.3}", duration_ms as f64 / 1000.0)
}

impl JUnitTestSuites {
    /// 从已完成统一脱敏的 JsonFileReport 列表构建（SSOT 脱敏复用）
    pub fn from_json_file_reports(reports: &[JsonFileReport]) -> Self {
        let mut suites = Vec::new();
        let (mut total, mut failures, mut errors, mut skipped, mut total_ms) =
            (0usize, 0usize, 0usize, 0usize, 0u64);

        for report in reports {
            let mut testcases = Vec::new();
            let (mut s_tests, mut s_fail, mut s_err, mut s_skip, mut s_ms) =
                (0usize, 0usize, 0usize, 0usize, 0u64);

            for r in &report.results {
                let name = match &r.name {
                    Some(n) => format!("#{} ({})", r.request_number, n),
                    None => format!("#{}", r.request_number),
                };
                let time = secs(r.duration_ms);
                s_tests += 1;
                s_ms += r.duration_ms;

                let mut case = JUnitTestCase {
                    name,
                    classname: report.file_path.clone(),
                    time: time.clone(),
                    failure: None,
                    error: None,
                    skipped: None,
                    system_out: None,
                };

                if r.skipped {
                    s_skip += 1;
                    case.skipped = Some(JUnitSkipped {
                        message: Some("skipped by selector or @skip".to_string()),
                    });
                } else if r.degraded && r.success {
                    // B-3：插件降级形态的成功 → <skipped>（非 failure：断言本身通过；
                    // 看板可见性对齐 --strict-plugins 的整体判失败，消灭"绿看板红 job"矛盾）
                    s_skip += 1;
                    case.skipped = Some(JUnitSkipped {
                        message: Some(
                            "plugin degraded: request passed with degraded plugins (--strict-plugins fails the suite)".to_string(),
                        ),
                    });
                } else if let Some(err) = &r.error {
                    // 请求级异常（网络/DNS/超时）→ <error>（评审 P1-3：区分 failure/error）
                    s_err += 1;
                    case.error = Some(JUnitError {
                        message: strip_ansi(err),
                        error_type: "RequestError".to_string(),
                        details: strip_ansi(err),
                    });
                } else if !r.success {
                    // 断言失败 → <failure>（细节携带未通过断言的 expected/actual）
                    s_fail += 1;
                    let failed: Vec<String> = r
                        .assertions
                        .iter()
                        .filter(|a| !a.passed)
                        .map(|a| match (&a.expected, &a.actual) {
                            (Some(e), Some(act)) => {
                                format!("{} (expected {}, got {})", a.raw, e, act)
                            }
                            _ => a.raw.clone(),
                        })
                        .collect();
                    let details = if failed.is_empty() {
                        "assertions failed".to_string()
                    } else {
                        failed.join("\n")
                    };
                    case.failure = Some(JUnitFailure {
                        message: strip_ansi(&details),
                        failure_type: "AssertionFailure".to_string(),
                        details: strip_ansi(&details),
                    });
                }

                // system-out：Method URL / Status / warnings（响应体片段仅失败用例附带）
                let mut so = format!(
                    "{} {}\nStatus: {}",
                    r.method,
                    r.url,
                    r.status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<none>".to_string())
                );
                if !r.warnings.is_empty() {
                    so.push_str("\nWarnings:");
                    for w in &r.warnings {
                        so.push_str(&format!("\n  - {w}"));
                    }
                }
                if !r.success
                    && let Some(snippet) = &r.response_body_snippet
                {
                    so.push_str(&format!("\nBody: {snippet}"));
                }
                case.system_out = Some(strip_ansi(&so));

                testcases.push(case);
            }

            total += s_tests;
            failures += s_fail;
            errors += s_err;
            skipped += s_skip;
            total_ms += s_ms;

            suites.push(JUnitTestSuite {
                name: report.file_path.clone(),
                tests: s_tests,
                failures: s_fail,
                errors: s_err,
                skipped: s_skip,
                time: secs(s_ms),
                testcases,
            });
        }

        JUnitTestSuites {
            tests: total,
            failures,
            errors,
            skipped,
            time: secs(total_ms),
            timestamp: chrono::Utc::now().to_rfc3339(),
            hostname: execution_hostname(),
            suites,
        }
    }

    /// 序列化为符合 XML 规范的字符串（含 XML 声明；转义由 quick-xml 处理）
    pub fn to_xml_string(&self) -> crate::Result<String> {
        let mut buffer = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let xml = quick_xml::se::to_string(self)
            .map_err(|e| crate::RupostError::Other(format!("JUnit XML 序列化失败: {e}")))?;
        buffer.push_str(&xml);
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_execution_hostname_prefers_explicit_env() {
        // 取值链：RUPOST_RUN_HOST > HOSTNAME > 空串（CI 显式注入优先）
        unsafe {
            std::env::set_var("RUPOST_RUN_HOST", "ci-node-7");
        }
        assert_eq!(super::execution_hostname(), "ci-node-7");
        unsafe {
            std::env::remove_var("RUPOST_RUN_HOST");
        }
        // 移除后回落：HOSTNAME 存在则取之，否则空串（两种环境都合法）
        let fallback = super::execution_hostname();
        let expected = std::env::var("HOSTNAME").unwrap_or_default();
        assert_eq!(fallback, expected);
    }
    use super::*;
    use crate::report::json::{JsonAssertionReport, JsonTestReport};

    fn report(results: Vec<JsonTestReport>) -> JsonFileReport {
        JsonFileReport {
            file_path: "tests/demo.http".to_string(),
            results,
        }
    }

    fn passing(n: usize) -> JsonTestReport {
        JsonTestReport {
            request_number: n,
            name: Some(format!("case{n}")),
            method: "GET".to_string(),
            url: "http://x/api".to_string(),
            status: Some(200),
            duration_ms: 120,
            success: true,
            error: None,
            skipped: false,
            degraded: false,
            timing: None,
            assertions: vec![],
            warnings: vec![],
            response_body_snippet: None,
            diagnose_report: None,
        }
    }

    /// 结构与计数：passed/skipped/断言失败/请求异常四形态各归其位，计数守恒
    #[test]
    fn test_junit_counts_and_node_kinds() {
        let mut assertion_fail = passing(2);
        assertion_fail.success = false;
        assertion_fail.assertions = vec![JsonAssertionReport {
            raw: "status == 200".to_string(),
            passed: false,
            message: None,
            expected: Some("200".to_string()),
            actual: Some("500".to_string()),
            stream_event_index: None,
        }];
        let mut skipped = passing(3);
        skipped.skipped = true;
        let mut transport = passing(4);
        transport.success = false;
        transport.error = Some("connection refused".to_string());

        let suites = JUnitTestSuites::from_json_file_reports(&[report(vec![
            passing(1),
            assertion_fail,
            skipped,
            transport,
        ])]);

        assert_eq!(suites.tests, 4);
        assert_eq!(suites.failures, 1, "assertion failure counted in failures");
        assert_eq!(suites.errors, 1, "request error counted in errors");
        assert_eq!(suites.skipped, 1);
        assert_eq!(suites.suites.len(), 1);
        assert_eq!(suites.suites[0].name, "tests/demo.http");

        // failure/error/skipped 节点各归其位（评审 P1-3：<error> 独立于 <failure>）
        let cases = &suites.suites[0].testcases;
        assert!(cases[0].failure.is_none() && cases[0].error.is_none());
        assert!(cases[1].failure.is_some() && cases[1].error.is_none());
        assert!(cases[2].skipped.is_some());
        assert!(cases[3].error.is_some() && cases[3].failure.is_none());
        assert_eq!(cases[3].error.as_ref().unwrap().error_type, "RequestError");
    }

    /// time 聚合：testcase = duration_ms/1000 三位小数；suite/suites 求和可加
    #[test]
    fn test_junit_time_aggregation() {
        let suites =
            JUnitTestSuites::from_json_file_reports(&[report(vec![passing(1), passing(2)])]);
        assert_eq!(suites.suites[0].testcases[0].time, "0.120");
        assert_eq!(suites.suites[0].time, "0.240");
        assert_eq!(suites.time, "0.240");
    }

    /// system-out 口径：Method URL / Status / warnings；响应体片段仅失败用例附带
    #[test]
    fn test_junit_system_out_contract() {
        let mut warn = passing(1);
        warn.warnings = vec!["undefined variable: {{token}}".to_string()];
        let mut fail = passing(2);
        fail.success = false;
        fail.response_body_snippet = Some(r#"{"error":"down"}"#.to_string());
        let suites = JUnitTestSuites::from_json_file_reports(&[report(vec![warn, fail])]);

        let so_ok = suites.suites[0].testcases[0].system_out.as_ref().unwrap();
        assert!(so_ok.contains("GET http://x/api") && so_ok.contains("Status: 200"));
        assert!(so_ok.contains("undefined variable"), "warnings included");
        assert!(!so_ok.contains("Body:"), "passing case has no body snippet");

        let so_fail = suites.suites[0].testcases[1].system_out.as_ref().unwrap();
        assert!(so_fail.contains("Body:"), "failed case carries snippet");
    }

    /// XML 输出三不变量：ANSI 剥离 / 敏感值已脱敏（SSOT 输入假定）/ XML 转义合规
    #[test]
    fn test_junit_xml_ansi_redaction_escaping() {
        let mut ansi_case = passing(1);
        ansi_case.success = false;
        ansi_case.error = Some("\x1b[31mFAIL\x1b[0m & <broken>".to_string());
        let suites = JUnitTestSuites::from_json_file_reports(&[report(vec![ansi_case])]);
        let xml = suites.to_xml_string().unwrap();

        // ANSI 控制符被彻底剥离（评审 P1-14：构造期单点剥离）
        assert!(!xml.contains('\x1b'), "no ANSI escape in xml: {xml}");
        // XML 转义合规：& < > 由 quick-xml 处理
        assert!(
            xml.contains("&amp;") && xml.contains("&lt;broken&gt;"),
            "escaped: {xml}"
        );
        // 声明与根元素
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<testsuites") && xml.contains("</testsuites>"));
    }

    /// 红队变异⑥守护：多文件套件聚合——漏文件变异（take(1)）必须红
    #[test]
    fn test_junit_multi_file_aggregation() {
        let mut second = passing(1);
        second.duration_ms = 80;
        let suites = JUnitTestSuites::from_json_file_reports(&[
            report(vec![passing(1)]),
            report(vec![second]),
        ]);
        assert_eq!(suites.suites.len(), 2, "one testsuite per file");
        assert_eq!(suites.tests, 2, "top-level tests is the sum");
        assert_eq!(suites.suites[0].tests, 1);
        assert_eq!(suites.suites[1].tests, 1);
        assert_eq!(suites.suites[1].name, "tests/demo.http");
    }

    /// 红队缺口 b：空结果文件形态锁定（0 请求 → 自闭合空 suite，Jenkins 合规）
    #[test]
    fn test_junit_empty_results_file_shape() {
        let suites = JUnitTestSuites::from_json_file_reports(&[report(vec![])]);
        assert_eq!(suites.tests, 0);
        assert_eq!(suites.suites[0].tests, 0);
        assert!(suites.suites[0].testcases.is_empty());
    }

    /// 敏感值脱敏：SSOT 输入假定——传入已掩码值（***）在 XML 中保持掩码形态
    #[test]
    fn test_junit_redacted_input_stays_masked() {
        let mut leaked = passing(1);
        leaked.success = false;
        leaked.response_body_snippet = Some(r#"{"token":"***","memo":"ok"}"#.to_string());
        let suites = JUnitTestSuites::from_json_file_reports(&[report(vec![leaked])]);
        let xml = suites.to_xml_string().unwrap();
        assert!(xml.contains("***"), "masked value stays masked: {xml}");
        assert!(!xml.contains("secret"), "no secret appears: {xml}");
    }
}
