use crate::Result;
use crate::exporter::types::{ExportOptions, ExportableRequest};
use std::sync::Arc;

/// 命令导出器插件接口 (开闭原则 OCP)
pub trait CommandExporter: Send + Sync {
    /// 插件标准名称，如 "curl", "httpie", "wget"
    fn name(&self) -> &'static str;

    /// 别名列表，如 httpie 支持 ["http"]
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// 简短业务描述（用于 --list 输出）
    fn description(&self) -> &'static str;

    /// 将单个 IR 请求转换为目标 CLI 命令文本
    fn export_one(&self, req: &ExportableRequest, opts: &ExportOptions) -> Result<String>;
}

/// 导出器插件注册表
pub struct ExporterRegistry {
    plugins: Vec<Arc<dyn CommandExporter>>,
}

impl ExporterRegistry {
    /// 构建包含所有内置插件的默认注册表
    pub fn new_builtin() -> Self {
        let mut registry = Self {
            plugins: Vec::new(),
        };
        registry.register(Arc::new(super::plugins::curl::CurlExporter));
        registry.register(Arc::new(super::plugins::httpie::HttpieExporter));
        registry.register(Arc::new(super::plugins::wget::WgetExporter));
        registry
    }

    /// 注册新的导出插件
    pub fn register(&mut self, plugin: Arc<dyn CommandExporter>) {
        self.plugins.push(plugin);
    }

    /// 根据名称或别名查找匹配的插件（大小写不敏感）
    pub fn find(&self, name_or_alias: &str) -> Option<Arc<dyn CommandExporter>> {
        let needle = name_or_alias.to_lowercase();
        self.plugins
            .iter()
            .find(|p| {
                p.name().eq_ignore_ascii_case(&needle)
                    || p.aliases().iter().any(|a| a.eq_ignore_ascii_case(&needle))
            })
            .cloned()
    }

    /// 列出所有插件的名称、别名与描述信息
    pub fn list(&self) -> Vec<(&'static str, &'static [&'static str], &'static str)> {
        self.plugins
            .iter()
            .map(|p| (p.name(), p.aliases(), p.description()))
            .collect()
    }
}
