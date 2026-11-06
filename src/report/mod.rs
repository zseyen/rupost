pub mod generator;
pub mod html;
pub mod types;

pub use generator::ReportGenerator;
pub use html::HtmlReportGenerator;
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
    reports: &[crate::reporter::JsonFileReport],
    title: Option<&str>,
    environment: Option<&str>,
) -> crate::Result<String> {
    let context = ReportContext::from_json_file_reports(reports, title, environment);
    let generator = HtmlReportGenerator;
    generator.generate(&context)
}
