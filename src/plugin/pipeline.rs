use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::driver::invoke_plugin;
use super::registry::PluginRegistry;
use super::types::{PluginCacheScope, PluginDirective, PluginStage};

/// 操作系统 Stdio IPC 管道硬限制载荷阈值 (10MB，防止打满管道缓冲区挂死)
pub const MAX_PLUGIN_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

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

/// suite 级 Hook 结果缓存（03 §8.3 `cache_scope=suite`：OAuth2 类插件全套件仅执行一次）
#[derive(Debug, Default)]
pub struct SuiteHookCache {
    inner: HashMap<String, super::PluginPatch>,
}

impl SuiteHookCache {
    pub fn new() -> Self {
        Self::default()
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

/// 插件生命周期管线调度器 (Pipeline Dispatcher)
#[derive(Debug, Clone, Default)]
pub struct PluginPipeline;

impl PluginPipeline {
    /// 对声明的插件注解集合执行确定性排序仲裁（03 §5.2 唯一权威）：
    /// 1. 严格依据生命周期阶段不可逆流升序 (Prepare -> Transform -> Sign)；
    /// 2. 同阶段内 100% 保留 `.http` 文件书写声明行序（稳定排序，所见即所发）——
    ///    `priority` 等底层魔数已彻底废除（05 ADR-D6 防复活清单）。
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

    /// 对单请求执行完整插件管线（03 §5.1 阶段不可逆流 + §2 统一降级语义）。
    ///
    /// - `resolved_url/method/headers/body`：变量解析后的最终形态（快照镜像同源）
    /// - 命中 `cache_scope=suite` 的插件直接复用缓存 Patch，不启动子进程
    /// - 缺失/四类失败：`?optional` 或 `--allow-missing-plugins` → 降级三件套；
    ///   否则 `blocking_error`（含搜索路径清单）
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_for_request(
        directives: &[PluginDirective],
        registry: &PluginRegistry,
        options: &PluginRuntimeOptions,
        suite_cache: &mut SuiteHookCache,
        resolved_url: &str,
        resolved_method: &str,
        resolved_headers: &[(String, String)],
        resolved_body: Option<&str>,
    ) -> PluginStageOutcome {
        let mut outcome = PluginStageOutcome::default();
        let mut ordered = directives.to_vec();
        Self::sort_directives(&mut ordered);

        let mut acc = super::PluginPatch::default();
        for directive in &ordered {
            let Some((exe_path, hook)) = registry.find_hook(&directive.name) else {
                let message = format!(
                    "插件 '{}' 未安装，已降级跳过（现场数据已记录至快照）。搜索路径: $PATH 各目录、~/.rupost/plugins、./.rupost/plugins（需 --allow-project-plugins）",
                    directive.name
                );
                outcome.warnings.push(message.clone());
                Self::gate_degradation(&mut outcome, directive, options, &message);
                continue;
            };

            // suite 级缓存命中：不启动子进程
            if matches!(hook.cache_scope, PluginCacheScope::Suite)
                && let Some(cached) = suite_cache.inner.get(&directive.name)
            {
                acc.merge_with(cached.clone());
                outcome.patch = Some(acc.clone());
                continue;
            }

            let payload = json!({
                "protocol_version": "1.0",
                "stage": match directive.stage {
                    PluginStage::Prepare => "prepare",
                    PluginStage::Transform => "transform",
                    PluginStage::Sign => "sign",
                    _ => "transform",
                },
                "plugin": directive.name,
                "request": {
                    "method": resolved_method,
                    "url": resolved_url,
                    "headers": resolved_headers,
                    "body": resolved_body,
                },
                "variables": {},
                "params": directive.params,
            })
            .to_string();

            let invocation = invoke_plugin(
                exe_path,
                &payload,
                options.plugin_timeout,
                &options.plugin_env,
            )
            .await;
            let invocation = match invocation {
                Ok(inv) => inv,
                Err(e) => {
                    let message = format!("插件 '{}' 启动失败：{e}", directive.name);
                    outcome.warnings.push(message.clone());
                    Self::gate_degradation(&mut outcome, directive, options, &message);
                    continue;
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
            if let Some(failure) = &invocation.failure {
                let message = format!(
                    "插件 '{}' 失败（{}: {}），已降级跳过（现场数据已记录至快照）",
                    directive.name,
                    failure.kind_label(),
                    failure.describe()
                );
                outcome.warnings.push(message.clone());
                Self::gate_degradation(&mut outcome, directive, options, &message);
                continue;
            }
            let Some(output) = invocation.output else {
                continue;
            };
            let patch = super::PluginPatch {
                extra_headers: output
                    .get("extra_headers")
                    .and_then(|v| serde_json::from_value(v.clone()).unwrap_or_default())
                    .unwrap_or_default(),
                remove_headers: output
                    .get("remove_headers")
                    .and_then(|v| serde_json::from_value(v.clone()).unwrap_or_default())
                    .unwrap_or_default(),
                mutated_body: output.get("mutated_body").cloned().and_then(|v| {
                    if v.is_null() {
                        None
                    } else {
                        v.as_str().map(|s| s.to_string())
                    }
                }),
                set_variables: output
                    .get("set_variables")
                    .and_then(|v| serde_json::from_value(v.clone()).unwrap_or_default())
                    .unwrap_or_default(),
                warnings: output
                    .get("warnings")
                    .and_then(|v| serde_json::from_value(v.clone()).unwrap_or_default())
                    .unwrap_or_default(),
            };
            if matches!(hook.cache_scope, PluginCacheScope::Suite) {
                suite_cache
                    .inner
                    .insert(directive.name.clone(), patch.clone());
            }
            acc.merge_with(patch);
        }

        if !outcome.degraded || outcome.blocking_error.is_none() {
            outcome.patch = Some(acc);
        }
        outcome
    }

    /// 统一降级门（03 §2.1）：`?optional` 或全局旗标 → degraded=true；
    /// 否则阻断错误（套件失败路径，exit 1）。strict 由套件层汇总 degraded 判定。
    fn gate_degradation(
        outcome: &mut PluginStageOutcome,
        directive: &PluginDirective,
        options: &PluginRuntimeOptions,
        message: &str,
    ) {
        if directive.optional || options.allow_missing_plugins {
            outcome.degraded = true;
            outcome.patch = None;
        } else {
            outcome.blocking_error = Some(message.to_string());
            outcome.patch = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginPatch;
    use std::collections::HashMap;

    #[test]
    fn test_sort_directives_stage_invariance_and_declaration_fifo() {
        let mut directives = vec![
            // 乱序声明：先声明了 Sign，再声明了 Prepare，再声明了 Transform
            PluginDirective {
                directive: "sign".to_string(),
                name: "aws-sigv4".to_string(),
                stage: PluginStage::Sign,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "oauth2-token".to_string(),
                stage: PluginStage::Prepare,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "transform".to_string(),
                name: "aes-encrypt".to_string(),
                stage: PluginStage::Transform,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "api-key".to_string(),
                stage: PluginStage::Prepare,
                optional: true,
                params: HashMap::new(),
            },
        ];

        PluginPipeline::sort_directives(&mut directives);

        let names: Vec<&str> = directives.iter().map(|d| d.name.as_str()).collect();
        // 阶段不可逆：Prepare -> Transform -> Sign；
        // 同阶段内保留书写行序 FIFO（oauth2-token 先于 api-key 声明）——
        // priority 魔数已废除（05 ADR-D6），声明顺序即执行顺序
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
