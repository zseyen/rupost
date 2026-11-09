use crate::report::types::ReportContext;

/// 报告生成器接口（开闭原则：支持 HTML、JSON、Markdown 等不同格式渲染器）
pub trait ReportGenerator {
    /// 将统一报告上下文编译为目标格式字符串
    fn generate(&self, context: &ReportContext) -> crate::Result<String>;
}
