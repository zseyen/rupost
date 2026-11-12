use crate::parser::ParsedRequest;
use crate::plugin::pipeline::{PluginRuntimeOptions, PluginStageOutcome, SuiteHookCache};
use crate::plugin::{PluginPipeline, PluginRegistry};
use crate::runner::types::TestResult;
use crate::variable::VariableContext;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// 对未显式装配注册表的执行器提供静态空注册表兜底，
/// 确保带 `@auth/@transform/@sign` 指令的请求绝不会绕过缺失拦截与降级门禁。
fn empty_registry() -> &'static Arc<PluginRegistry> {
    static EMPTY: OnceLock<Arc<PluginRegistry>> = OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(PluginRegistry::new()))
}

#[derive(Default)]
pub(crate) struct PreparedPluginOutcome {
    pub degraded: bool,
    pub plugin_duration_us: u64,
    pub plugin_warnings: Vec<String>,
    pub patch_warnings: Vec<String>,
}

pub(crate) struct PluginContext<'a> {
    pub registry: Option<&'a Arc<PluginRegistry>>,
    pub options: &'a PluginRuntimeOptions,
    pub suite_hook_cache: &'a SuiteHookCache,
}

pub(crate) enum PluginDispatchOutcome {
    Proceed(Box<ParsedRequest>, PreparedPluginOutcome),
    EarlyReturn(Box<TestResult>),
}

/// 统一把变量与插件诊断信息收口到 `TestResult`（覆盖正常返回与全部早退路径）。
pub(crate) fn finalize_plugin_result(
    mut result: TestResult,
    unresolved: Vec<String>,
    diag: PreparedPluginOutcome,
) -> TestResult {
    result.warnings.extend(unresolved);
    result.warnings.extend(diag.patch_warnings);
    result.warnings.extend(diag.plugin_warnings);
    result.degraded = diag.degraded;
    result.plugin_duration_us = (diag.plugin_duration_us > 0).then_some(diag.plugin_duration_us);
    result
}

fn outcome_to_diag(
    stage: &PluginStageOutcome,
    patch_warnings: Vec<String>,
) -> PreparedPluginOutcome {
    PreparedPluginOutcome {
        degraded: stage.degraded,
        plugin_duration_us: stage.plugin_duration_us,
        plugin_warnings: stage.warnings.clone(),
        patch_warnings,
    }
}

/// 执行单请求的插件生命周期管线（Prepare → Transform → Sign）并处理阻断/降级兜底早退。
pub(crate) async fn run_plugin_stage(
    mut parsed: ParsedRequest,
    request_number: usize,
    context: &mut VariableContext,
    plugin_ctx: PluginContext<'_>,
    unresolved: &[String],
    start: Instant,
) -> PluginDispatchOutcome {
    if parsed.metadata.plugins.is_empty() {
        return PluginDispatchOutcome::Proceed(Box::new(parsed), PreparedPluginOutcome::default());
    }

    let method = parsed.method_or_default().to_string();
    let url = parsed.url.clone();
    let name = parsed.name().map(|s| s.to_string());
    let registry = plugin_ctx.registry.unwrap_or_else(|| empty_registry());

    let headers: Vec<(String, String)> = parsed
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let directives: Vec<crate::plugin::PluginDirective> = parsed
        .metadata
        .plugins
        .iter()
        .map(crate::plugin::PluginDirective::from)
        .collect();
    let stage = PluginPipeline::execute_for_request(
        &directives,
        registry,
        plugin_ctx.options,
        plugin_ctx.suite_hook_cache,
        &parsed.url,
        &method,
        &headers,
        parsed.body.as_deref(),
    )
    .await;

    if let Some(blocking) = &stage.blocking_error {
        let diag = outcome_to_diag(&stage, Vec::new());
        let err_result = TestResult::error(
            request_number,
            name,
            method,
            url,
            blocking.clone(),
            start.elapsed(),
        );
        return PluginDispatchOutcome::EarlyReturn(Box::new(finalize_plugin_result(
            err_result,
            unresolved.to_vec(),
            diag,
        )));
    }

    let mut patch_warnings = Vec::new();
    if let Some(patch) = &stage.patch {
        let mut header_pairs = std::mem::take(&mut parsed.headers);
        patch.apply_to(&mut header_pairs, &mut parsed.body);
        parsed.headers = header_pairs;
        for (key, value) in &patch.set_variables {
            context.insert(key, value);
        }
        patch_warnings = patch.warnings.clone();
    }

    if stage.degraded {
        for warning in &stage.warnings {
            eprintln!("[WARN] {warning}");
        }
    }

    let diag = outcome_to_diag(&stage, patch_warnings);

    // 03 §2.1 兜底缺失守卫：降级时含未解析 {{var}} 的 Header 不得把
    // 字面量（如 {{DEBUG_FALLBACK_TOKEN}}）当 Bearer 发往生产 → 跳过请求。
    if diag.degraded
        && parsed
            .headers
            .iter()
            .any(|(_, v)| v.contains("{{") && v.contains("}}"))
    {
        let mut skipped = TestResult::skipped(request_number, name, method, url);
        skipped.warnings.push(
            "降级请求的兜底 Header 含未解析 {{var}}（兜底缺失），已跳过外发（03 §2.1）".to_string(),
        );
        return PluginDispatchOutcome::EarlyReturn(Box::new(finalize_plugin_result(
            skipped,
            unresolved.to_vec(),
            diag,
        )));
    }

    PluginDispatchOutcome::Proceed(Box::new(parsed), diag)
}
