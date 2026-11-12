use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde_json::json;

use super::driver::invoke_plugin;
pub use super::protocol::MAX_PLUGIN_PAYLOAD_BYTES;
use super::protocol::parse_patch_from_output;
use super::registry::PluginRegistry;
use super::types::{PluginCacheScope, PluginDirective, PluginStage};

/// 插件外部子进程默认执行超时 (5 秒硬熔断，`--plugin-timeout` 可覆盖)
pub const DEFAULT_PLUGIN_TIMEOUT_SECS: u64 = 5;

/// 插件降级 / 门禁运行时选项（03 §2 唯一权威语义的执行载体）
#[derive(Debug, Clone)]
pub struct PluginRuntimeOptions {
    /// CLI `--allow-missing-plugins`：缺失/失败统一降级为警告跳过
    pub allow_missing_plugins: bool,
    /// CLI `--strict-plugins`：发生任何降级 → 套件整体判失败（CI 门禁）
    pub strict_plugins: bool,
    /// 单次插件调用超时（默认 5s，`--plugin-timeout` 覆盖）
    pub plugin_timeout: Duration,
    /// `--plugin-env KEY=VALUE`：requires_env 声明外的显式放行
    pub plugin_env: Vec<(String, String)>,
}

impl Default for PluginRuntimeOptions {
    fn default() -> Self {
        Self {
            allow_missing_plugins: false,
            strict_plugins: false,
            plugin_timeout: Duration::from_secs(DEFAULT_PLUGIN_TIMEOUT_SECS),
            plugin_env: Vec::new(),
        }
    }
}

/// 解析 CLI `--plugin-env` 单条规格：`KEY=VALUE` 直接取值；`KEY` 从父进程环境变量读取（缺失返回 None）。
pub fn parse_plugin_env_spec(spec: &str) -> Option<(String, String)> {
    if let Some((key, value)) = spec.split_once('=') {
        let key = key.trim();
        (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
    } else {
        let key = spec.trim();
        if key.is_empty() {
            return None;
        }
        std::env::var(key).ok().map(|val| (key.to_string(), val))
    }
}

/// 批量解析 `--plugin-env` 参数列表
pub fn parse_plugin_env_specs(specs: &[String]) -> Vec<(String, String)> {
    specs
        .iter()
        .filter_map(|s| parse_plugin_env_spec(s))
        .collect()
}

/// suite 级 Hook 结果缓存（03 §8.3 `cache_scope=suite`：按插件名 + 阶段 + 参数精确隔离）。
/// 外层使用极短同步锁获取独立槽位，各 `cache_key` 异步单飞隔离，避免阻塞无锁的 `PerRequest` 插件。
#[derive(Debug, Default)]
pub struct SuiteHookCache {
    slots: std::sync::Mutex<
        HashMap<String, std::sync::Arc<tokio::sync::Mutex<Option<super::PluginPatch>>>>,
    >,
}

impl SuiteHookCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定缓存键的独立单飞槽位（外层锁仅覆盖索引查找，不跨 `.await` 持有）。
    pub fn slot_for(
        &self,
        key: &str,
    ) -> std::sync::Arc<tokio::sync::Mutex<Option<super::PluginPatch>>> {
        let mut map = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        map.entry(key.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(None)))
            .clone()
    }
}

/// 单请求插件管线执行产物
#[derive(Debug, Default, Clone)]
pub struct PluginStageOutcome {
    /// 折叠后的最终 Patch（失败/降级时为 None——请求按 .http 原样外发）
    pub patch: Option<super::PluginPatch>,
    /// 本次是否发生降级（缺失或失败被放行跳过）
    pub degraded: bool,
    /// 插件管道总耗时（微秒，时延隔离）
    pub plugin_duration_us: u64,
    /// 降级/失败告警（终端 + 报告 + 快照三处同文，03 §2.1）
    pub warnings: Vec<String>,
    /// 未声明降级旗标时的阻断错误（友好报错 + 搜索路径清单，exit 1 路径）
    pub blocking_error: Option<String>,
}

/// Hook 缓存键：suite 与 request 两种作用域共用（插件名 + 阶段 + 排序参数精确隔离）
fn hook_cache_key(directive: &PluginDirective) -> String {
    let sorted_params: BTreeMap<&str, &str> = directive
        .params
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    json!({
        "plugin": directive.name,
        "stage": directive.stage,
        "params": sorted_params,
    })
    .to_string()
}

/// 合并 manifest `requires_env` 声明与 `--plugin-env` 覆盖列表（CLI 覆盖优先级更高）
fn build_plugin_env_overrides(
    requires_env: &[String],
    cli_env: &[(String, String)],
) -> Vec<(String, String)> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    for key in requires_env {
        if let Ok(val) = std::env::var(key) {
            merged.insert(key.clone(), val);
        }
    }
    for (k, v) in cli_env {
        merged.insert(k.clone(), v.clone());
    }
    merged.into_iter().collect()
}

fn apply_patch_to_view(
    patch: &super::PluginPatch,
    acc: &mut super::PluginPatch,
    headers: &mut Vec<(String, String)>,
    body: &mut Option<String>,
    vars: &mut serde_json::Map<String, serde_json::Value>,
) {
    patch.apply_to(headers, body);
    for (k, v) in &patch.set_variables {
        vars.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    acc.merge_with(patch.clone());
}

enum HookInvokeStep {
    Patch(super::PluginPatch),
    Continue,
    Break,
}

/// 插件生命周期管线调度器 (Pipeline Dispatcher)
#[derive(Debug, Clone, Default)]
pub struct PluginPipeline;

impl PluginPipeline {
    /// 对声明的插件注解集合执行确定性排序仲裁（03 §5.2 唯一权威）：
    /// 1. 严格依据生命周期阶段不可逆流升序 (Prepare -> Transform -> Sign)；
    /// 2. 同阶段内 100% 保留 `.http` 文件书写声明行序（稳定排序）。
    pub fn sort_directives(directives: &mut [PluginDirective]) {
        directives.sort_by_key(|d| d.stage);
    }

    /// 提取属于指定生命周期阶段的指令切片
    pub fn filter_by_stage(
        directives: &[PluginDirective],
        stage: PluginStage,
    ) -> Vec<&PluginDirective> {
        directives.iter().filter(|d| d.stage == stage).collect()
    }

    /// 对单请求执行完整插件管线（03 §5.1 阶段串联传递 + §2 统一降级语义）。
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_for_request(
        directives: &[PluginDirective],
        registry: &PluginRegistry,
        options: &PluginRuntimeOptions,
        suite_cache: &SuiteHookCache,
        resolved_url: &str,
        resolved_method: &str,
        resolved_headers: &[(String, String)],
        resolved_body: Option<&str>,
    ) -> PluginStageOutcome {
        let mut outcome = PluginStageOutcome::default();
        let mut ordered = directives.to_vec();
        Self::sort_directives(&mut ordered);

        let mut acc = super::PluginPatch::default();
        let mut current_headers = resolved_headers.to_vec();
        let mut current_body = resolved_body.map(str::to_string);
        let mut current_vars = serde_json::Map::new();
        // 请求局部 Hook 结果备忘录（03 §3.1 `cache_scope=request`：同一请求内同指令复用，
        // 跨请求绝不复用——套件级复用专属 suite 作用域）
        let mut request_cache: HashMap<String, super::PluginPatch> = HashMap::new();

        for directive in &ordered {
            let Some((exe_path, hook)) = registry
                .find_hook_for_stage(&directive.name, directive.stage)
                .or_else(|| {
                    (directive.directive == "plugin")
                        .then(|| registry.find_hook(&directive.name))
                        .flatten()
                })
            else {
                let degraded_msg = format!(
                    "插件 '{}' 未安装，已降级跳过（现场数据已记录至快照）。搜索路径: $PATH 各目录、~/.rupost/plugins、./.rupost/plugins（需 --allow-project-plugins）",
                    directive.name
                );
                let block_msg = format!(
                    "插件 '{}' 未安装。搜索路径: $PATH 各目录、~/.rupost/plugins、./.rupost/plugins（需 --allow-project-plugins）",
                    directive.name
                );
                if Self::gate_degradation(
                    &mut outcome,
                    directive,
                    options,
                    &degraded_msg,
                    &block_msg,
                ) {
                    break;
                }
                continue;
            };

            match hook.cache_scope {
                PluginCacheScope::Suite => {
                    let cache_key = hook_cache_key(directive);
                    let slot = suite_cache.slot_for(&cache_key);
                    let mut guard = slot.lock().await;
                    if let Some(cached) = guard.as_ref() {
                        apply_patch_to_view(
                            cached,
                            &mut acc,
                            &mut current_headers,
                            &mut current_body,
                            &mut current_vars,
                        );
                        continue;
                    }
                    match Self::invoke_hook_once(
                        directive,
                        exe_path,
                        &hook.requires_env,
                        options,
                        &mut outcome,
                        resolved_url,
                        resolved_method,
                        &current_headers,
                        current_body.as_deref(),
                        &current_vars,
                    )
                    .await
                    {
                        HookInvokeStep::Patch(patch) => {
                            *guard = Some(patch.clone());
                            apply_patch_to_view(
                                &patch,
                                &mut acc,
                                &mut current_headers,
                                &mut current_body,
                                &mut current_vars,
                            );
                        }
                        HookInvokeStep::Continue => continue,
                        HookInvokeStep::Break => break,
                    }
                }
                PluginCacheScope::Request => {
                    let cache_key = hook_cache_key(directive);
                    if let Some(cached) = request_cache.get(&cache_key) {
                        apply_patch_to_view(
                            cached,
                            &mut acc,
                            &mut current_headers,
                            &mut current_body,
                            &mut current_vars,
                        );
                        continue;
                    }
                    match Self::invoke_hook_once(
                        directive,
                        exe_path,
                        &hook.requires_env,
                        options,
                        &mut outcome,
                        resolved_url,
                        resolved_method,
                        &current_headers,
                        current_body.as_deref(),
                        &current_vars,
                    )
                    .await
                    {
                        HookInvokeStep::Patch(patch) => {
                            request_cache.insert(cache_key, patch.clone());
                            apply_patch_to_view(
                                &patch,
                                &mut acc,
                                &mut current_headers,
                                &mut current_body,
                                &mut current_vars,
                            );
                        }
                        HookInvokeStep::Continue => continue,
                        HookInvokeStep::Break => break,
                    }
                }
                PluginCacheScope::None => {
                    match Self::invoke_hook_once(
                        directive,
                        exe_path,
                        &hook.requires_env,
                        options,
                        &mut outcome,
                        resolved_url,
                        resolved_method,
                        &current_headers,
                        current_body.as_deref(),
                        &current_vars,
                    )
                    .await
                    {
                        HookInvokeStep::Patch(patch) => {
                            apply_patch_to_view(
                                &patch,
                                &mut acc,
                                &mut current_headers,
                                &mut current_body,
                                &mut current_vars,
                            );
                        }
                        HookInvokeStep::Continue => continue,
                        HookInvokeStep::Break => break,
                    }
                }
            }
        }

        if outcome.blocking_error.is_none() {
            outcome.patch = Some(acc);
        } else {
            outcome.patch = None;
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    async fn invoke_hook_once(
        directive: &PluginDirective,
        exe_path: &std::path::Path,
        requires_env: &[String],
        options: &PluginRuntimeOptions,
        outcome: &mut PluginStageOutcome,
        resolved_url: &str,
        resolved_method: &str,
        current_headers: &[(String, String)],
        current_body: Option<&str>,
        current_vars: &serde_json::Map<String, serde_json::Value>,
    ) -> HookInvokeStep {
        let payload = json!({
            "protocol_version": "1.0",
            "stage": directive.stage,
            "plugin": directive.name,
            "request": {
                "method": resolved_method,
                "url": resolved_url,
                "headers": current_headers,
                "body": current_body,
            },
            "variables": current_vars,
            "params": directive.params,
        })
        .to_string();

        let env_overrides = build_plugin_env_overrides(requires_env, &options.plugin_env);
        let invocation =
            match invoke_plugin(exe_path, &payload, options.plugin_timeout, &env_overrides).await {
                Ok(inv) => inv,
                Err(e) => {
                    let degraded_msg =
                        format!("插件 '{}' 启动失败：{e}，已降级跳过", directive.name);
                    let block_msg = format!("插件 '{}' 启动失败：{e}", directive.name);
                    return if Self::gate_degradation(
                        outcome,
                        directive,
                        options,
                        &degraded_msg,
                        &block_msg,
                    ) {
                        HookInvokeStep::Break
                    } else {
                        HookInvokeStep::Continue
                    };
                }
            };

        outcome.plugin_duration_us += invocation.duration_us;
        if !invocation.stderr_tail.is_empty() {
            let tail: String = invocation
                .stderr_tail
                .chars()
                .rev()
                .take(16 * 1024)
                .collect();
            outcome.warnings.push(format!(
                "插件 '{}' stderr 摘要: {}",
                directive.name,
                tail.chars().rev().collect::<String>()
            ));
        }
        let failure = if let Some(failure) = invocation.failure {
            Some(failure)
        } else if let Some(output) = invocation.output {
            match parse_patch_from_output(&output) {
                Ok(patch) => return HookInvokeStep::Patch(patch),
                Err(err) => Some(err),
            }
        } else {
            return HookInvokeStep::Continue;
        };

        if let Some(failure) = failure {
            let detail = format!("{}: {}", failure.kind_label(), failure.describe());
            let degraded_msg = format!(
                "插件 '{}' 失败（{detail}），已降级跳过（现场数据已记录至快照）",
                directive.name
            );
            let block_msg = format!("插件 '{}' 执行失败（{detail}）", directive.name);
            return if Self::gate_degradation(outcome, directive, options, &degraded_msg, &block_msg)
            {
                HookInvokeStep::Break
            } else {
                HookInvokeStep::Continue
            };
        }
        HookInvokeStep::Continue
    }

    /// 统一降级门（03 §2.1）：返回 `true` 表示必须阻断并中止后续管线。
    fn gate_degradation(
        outcome: &mut PluginStageOutcome,
        directive: &PluginDirective,
        options: &PluginRuntimeOptions,
        degraded_message: &str,
        blocking_message: &str,
    ) -> bool {
        if directive.optional || options.allow_missing_plugins {
            outcome.degraded = true;
            outcome.warnings.push(degraded_message.to_string());
            false
        } else {
            outcome.blocking_error = Some(blocking_message.to_string());
            outcome.patch = None;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginPatch;

    #[test]
    fn test_sort_directives_stage_invariance_and_declaration_fifo() {
        let mut directives = vec![
            PluginDirective::new("sign", "aws-sigv4", PluginStage::Sign),
            PluginDirective::new("auth", "oauth2-token", PluginStage::Prepare),
            PluginDirective::new("transform", "aes-encrypt", PluginStage::Transform),
            PluginDirective::new("auth", "api-key", PluginStage::Prepare),
        ];

        PluginPipeline::sort_directives(&mut directives);

        let names: Vec<&str> = directives.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["oauth2-token", "api-key", "aes-encrypt", "aws-sigv4"]
        );
    }

    #[test]
    fn test_patch_merge_folding() {
        let mut p1 = PluginPatch {
            extra_headers: vec![("Authorization".to_string(), "Bearer token123".to_string())],
            remove_headers: vec![],
            mutated_body: None,
            set_variables: HashMap::from([("TOKEN".to_string(), "token123".to_string())]),
            warnings: vec!["Warn 1".to_string()],
        };
        let p2 = PluginPatch {
            extra_headers: vec![("X-Signature".to_string(), "sig_abc".to_string())],
            remove_headers: vec![],
            mutated_body: Some(r#"{"encrypted": true}"#.to_string()),
            set_variables: HashMap::new(),
            warnings: vec!["Warn 2".to_string()],
        };
        p1.merge_with(p2);
        assert_eq!(p1.extra_headers.len(), 2);
        assert_eq!(p1.mutated_body.as_deref(), Some(r#"{"encrypted": true}"#));
        assert_eq!(p1.set_variables.get("TOKEN").unwrap(), "token123");
        assert_eq!(p1.warnings, vec!["Warn 1", "Warn 2"]);
    }
}
