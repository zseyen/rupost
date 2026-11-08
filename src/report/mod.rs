pub mod generator;
pub mod html;
pub mod json;
pub mod types;

pub use generator::ReportGenerator;
pub use html::HtmlReportGenerator;
pub use json::{JsonAssertionReport, JsonFileReport, JsonTestReport};
pub use types::{
    ReportAssertionItem, ReportContext, ReportFileEntry, ReportRequestItem, ReportSummary,
    TimingWaterfall, escape_html,
};

/// 门面函数：从文件运行结果列表直接生成静态自包含 HTML 测试报告
pub fn generate_html_report(
    file_results: &[(std::path::PathBuf, Vec<crate::runner::types::TestResult>)],
    title: Option<&str>,
    environment: Option<&str>,
) -> crate::Result<String> {
    let context = ReportContext::from_file_results(file_results, title, environment);
    let generator = HtmlReportGenerator;
    generator.generate(&context)
}

/// 门面函数：从已反序列化的 JsonFileReport 列表离线生成静态自包含 HTML 报告
pub fn generate_html_report_from_json(
    reports: &[crate::report::json::JsonFileReport],
    title: Option<&str>,
    environment: Option<&str>,
) -> crate::Result<String> {
    let context = ReportContext::from_json_file_reports(reports, title, environment);
    let generator = HtmlReportGenerator;
    generator.generate(&context)
}

/// 报告文件格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFileFormat {
    Html,
    Json,
}

/// 报告文件写出产出（消息结构体：打印决策留给派发层，保证 `--report json` 的
/// stdout 纯净可在进程内直接断言——审查 P0-2/P0-3）
#[derive(Debug, Clone, PartialEq)]
pub struct ReportWriteOutcome {
    pub path: std::path::PathBuf,
    pub format: ReportFileFormat,
    /// 目标文件在写出前已存在（被覆盖）
    pub overwritten: bool,
}

/// 依据显式 `--report` 值与文件扩展名决定报告格式（纯函数，表驱动测试锁定）。
/// 显式值永远优先（html/json 忽略大小写）；其余值（含默认 terminal）按扩展名推断。
pub fn determine_report_format(report: &str, report_file: &str) -> ReportFileFormat {
    if report.eq_ignore_ascii_case("html") {
        ReportFileFormat::Html
    } else if report.eq_ignore_ascii_case("json") {
        ReportFileFormat::Json
    } else if report_file.to_ascii_lowercase().ends_with(".html")
        || report_file.to_ascii_lowercase().ends_with(".htm")
    {
        ReportFileFormat::Html
    } else {
        ReportFileFormat::Json
    }
}

/// 渲染并写出报告文件（UseCase 产出的唯一表现层落盘出口）。
///
/// 规则：
/// - `report_file` 存在 → 写该路径，格式由 `determine_report_format` 决定；
/// - `report_file` 缺省且 `--report html` → 写默认 `rupost-report.html`；
/// - 其余情形不产生文件输出（返回 `None`）；
/// - 覆盖提示以数据形态返回（`overwritten`），由派发层渲染到 stderr。
pub fn write_report_files(
    batch_results: &[(std::path::PathBuf, Vec<crate::runner::types::TestResult>)],
    report: &str,
    report_file: Option<&str>,
    environment: Option<&str>,
) -> crate::Result<Option<ReportWriteOutcome>> {
    let (path, format) = match report_file {
        Some(p) => (
            std::path::PathBuf::from(p),
            determine_report_format(report, p),
        ),
        None => {
            if report.eq_ignore_ascii_case("html") {
                (
                    std::path::PathBuf::from("rupost-report.html"),
                    ReportFileFormat::Html,
                )
            } else {
                return Ok(None);
            }
        }
    };

    let overwritten = path.exists();
    match format {
        ReportFileFormat::Html => {
            let html = generate_html_report(batch_results, None, environment)?;
            std::fs::write(&path, html)?;
        }
        ReportFileFormat::Json => {
            let mut file = std::fs::File::create(&path)?;
            json::report_json(batch_results, &mut file)?;
        }
    }

    Ok(Some(ReportWriteOutcome {
        path,
        format,
        overwritten,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 格式判定决策表（表驱动，对齐 cli.rs 契约测试先例）。
    /// row1（显式 json + .html → Json）是行为变更锁定：旧实现扩展名压过显式格式；
    /// row 大小写行锁定既有口径 bug 的统一（stdout 判定精确 vs 文件判定忽略大小写）。
    #[test]
    fn test_determine_report_format_decision_table() {
        let cases = [
            ("json", "out.html", ReportFileFormat::Json), // 显式赢过扩展名（旧行为：Html）
            ("HTML", "out.json", ReportFileFormat::Html),
            ("Json", "out.anything", ReportFileFormat::Json), // 大小写统一
            ("JSON", "out", ReportFileFormat::Json),
            ("html", "out", ReportFileFormat::Html),
            ("terminal", "a.html", ReportFileFormat::Html), // 默认按扩展名推断
            ("terminal", "a.htm", ReportFileFormat::Html),
            ("terminal", "a.json", ReportFileFormat::Json),
            ("terminal", "a", ReportFileFormat::Json), // 无扩展名兜底 JSON
            ("foo", "a.txt", ReportFileFormat::Json),  // 未知 report 值 + 未知扩展名
            ("", "a.html", ReportFileFormat::Html),
        ];
        for (report, file, expected) in cases {
            assert_eq!(
                determine_report_format(report, file),
                expected,
                "report={report:?} file={file:?}"
            );
        }
    }

    #[test]
    fn test_write_report_files_overwrite_semantics_and_outcome() {
        // 覆盖语义回归 + 提示数据断言（拆分自审查 P0-3 指出的假守护单测）
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("report.json");
        std::fs::write(&path, "USER-IMPORTANT-DATA").unwrap();

        let batch: Vec<(std::path::PathBuf, Vec<crate::runner::types::TestResult>)> = Vec::new();
        let outcome = write_report_files(&batch, "terminal", Some(path.to_str().unwrap()), None)
            .unwrap()
            .unwrap();

        assert_eq!(outcome.format, ReportFileFormat::Json);
        assert!(
            outcome.overwritten,
            "existing file must be flagged as overwritten"
        );
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("USER-IMPORTANT-DATA"),
            "file replaced by report"
        );
        assert!(content.starts_with('['), "empty batch yields JSON array");

        // 首次写入（文件不存在）→ overwritten = false
        let fresh = tmp.path().join("fresh.json");
        let outcome2 = write_report_files(&batch, "terminal", Some(fresh.to_str().unwrap()), None)
            .unwrap()
            .unwrap();
        assert!(!outcome2.overwritten);
    }

    #[test]
    fn test_write_report_files_none_when_no_file_output() {
        let batch: Vec<(std::path::PathBuf, Vec<crate::runner::types::TestResult>)> = Vec::new();
        let outcome = write_report_files(&batch, "terminal", None, None).unwrap();
        assert!(
            outcome.is_none(),
            "terminal without report_file writes nothing"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_write_report_files_follows_symlink_by_design() {
        // 行为文档测试（评估 P1-1 symlink 场景）：写入按 OS 语义穿透符号链接到达目标，
        // 由 overwritten 提示暴露落点；实现若改为拒绝/原子 rename 该测试变红
        let tmp = tempfile::tempdir().unwrap();
        let victim = tmp.path().join("victim.txt");
        std::fs::write(&victim, "SECRET-TARGET").unwrap();
        let link = tmp.path().join("link.json");
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        let batch: Vec<(std::path::PathBuf, Vec<crate::runner::types::TestResult>)> = Vec::new();
        let outcome = write_report_files(&batch, "terminal", Some(link.to_str().unwrap()), None)
            .unwrap()
            .unwrap();

        assert!(outcome.overwritten);
        let content = std::fs::read_to_string(&victim).unwrap();
        assert!(
            !content.contains("SECRET-TARGET"),
            "symlink target replaced by design"
        );
    }
}
