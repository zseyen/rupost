use crate::Result;
use crate::RupostError;
use crate::runner::on_failure;
use crate::runner::selector;
use crate::runner::suite_setup::{build_suite_executor, load_suite_context};
use crate::runner::{
    BatchExecutor, BatchMode, BatchRunRequest, DependencyResolver, DirectoryScanner, TestResult,
    WorkflowGraph,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct TestSuiteOptions {
    pub paths: Vec<String>,
    pub mode: String,
    pub concurrency: usize,
    pub report: String,
    /// 将 JSON 格式报告额外写入指定文件（不干扰终端输出）
    pub report_file: Option<String>,
    /// 对每个失败请求输出原位原因分析（E2，创作/诊断期 AI，B1 基线为规则版）
    pub explain: bool,
    /// 批量汇总后输出失败根因聚类分诊报告（E5，B1 基线为确定性签名分组）
    pub triage: bool,
    pub fail_fast: bool,
    pub env: Option<String>,
    pub var: Vec<String>,
    pub env_file: Option<String>,
    pub verbose: bool,
    pub no_cookies: bool,
    pub cookie_file: Option<String>,
    pub debug: bool,
    pub debug_on_failure: bool,
    pub default_scheme: String,
    pub save_snapshot: Option<String>,
    /// Secrets Masker（opt-in）：快照 headers/body 掩码后再落盘
    pub mask_snapshot: bool,
    /// 单响应体录入上限字节（03 §10.6 防 OOM；`--max-snapshot-body-bytes` 覆盖，缺省 256KB）
    pub max_snapshot_body_bytes: Option<usize>,
    /// 关闭时序采集（`--timing off`，02 §2:135）：快照 timing_waterfall=null、timing_source=none
    pub timing_off: bool,
    /// 单请求选择器参数（`--only <n|@name>`；`path#n` 片段在 paths 内携带）
    pub only: Option<String>,
    /// 失败钩子命令（DX-Debug R7）：每个失败请求触发一次，
    /// 经 RUPOST_FAILED_URL/STATUS/TRACE_ID 环境变量暴露锚点
    pub on_failure: Option<String>,
    /// 插件缺失/失败时降级为警告跳过（03 §2；现场排障即拷即走）
    pub allow_missing_plugins: bool,
    /// CI 严格门禁：发生任何插件降级 → 套件整体判失败（03 §10.5）
    pub strict_plugins: bool,
    /// 单次插件调用超时秒数（默认 5，`--plugin-timeout` 覆盖）
    pub plugin_timeout_secs: u64,
    /// `--plugin-env KEY=VALUE`：requires_env 声明外的显式放行
    pub plugin_env: Vec<String>,
    /// 工作区本地插件放行（03 §3.2 沙箱警戒闸门）
    pub allow_project_plugins: bool,
    /// `--with-deps`（R-1 免重填）：`--only` 时按拓扑序先执行目标文件的
    /// `@depends-on` 依赖闭包（上游登录/取号自动先跑）
    pub with_deps: bool,
    /// `--var-state`（R-2 免重填）：捕获变量持久化文件；结束落盘、启动回灌（最低优先级层）
    pub var_state: Option<String>,
}

impl Default for TestSuiteOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            mode: "sequential".to_string(),
            concurrency: 4,
            report: "terminal".to_string(),
            report_file: None,
            explain: false,
            triage: false,
            fail_fast: false,
            env: None,
            var: Vec::new(),
            env_file: None,
            verbose: false,
            // 与 CLI 默认语义一致：默认启用 cookie 会话
            no_cookies: false,
            cookie_file: None,
            only: None,
            on_failure: None,
            debug: false,
            debug_on_failure: false,
            default_scheme: "http".to_string(),
            save_snapshot: None,
            mask_snapshot: false,
            max_snapshot_body_bytes: None,
            timing_off: false,
            allow_missing_plugins: false,
            strict_plugins: false,
            plugin_timeout_secs: crate::plugin::pipeline::DEFAULT_PLUGIN_TIMEOUT_SECS,
            plugin_env: Vec::new(),
            allow_project_plugins: false,
            with_deps: false,
            var_state: None,
        }
    }
}

/// 测试套件执行产出：失败判定 + 完整批量结果 + 耗时与降级计数（纯领域数据，无渲染副作用）。
#[derive(Debug)]
pub struct TestSuiteOutcome {
    pub has_failure: bool,
    pub batch_results: Vec<(PathBuf, Vec<TestResult>)>,
    pub duration: Duration,
    pub degraded_count: usize,
    /// 执行期生成的非致命提示（由外层呈现器按输出模式打印至 stdout/stderr）。
    pub notices: Vec<SuiteNotice>,
    /// 经选择器规整后的输入路径列表（供快照记录 source_paths）。
    pub resolved_paths: Vec<String>,
}

/// 套件执行期产生的结构化提示（解耦 runner 与终端 I/O）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuiteNotice {
    Stderr(String),
    TerminalStdout(String),
}

/// 纯领域批处理测试执行用例（零 stdout/stderr 渲染，零快照/报告落盘）。
pub async fn execute_test_suite(options: &TestSuiteOptions) -> Result<TestSuiteOutcome> {
    // 0. 单请求选择器（DX-Debug R1）：--only 与 path#n 片段，二者互斥且仅支持单一文件路径
    let plan = selector::build_selector_plan(&options.paths, options.only.as_deref())?;
    let selector = plan.selector;
    let paths = plan.paths;
    if selector.is_some() && paths.len() == 1 && Path::new(&paths[0]).is_dir() {
        return Err(RupostError::Other(
            "单请求选择器（--only / path#n）不支持目录，请指定单个 .http/.md 文件".to_string(),
        ));
    }

    // 1. 递归扫描文件
    let scanned_files = DirectoryScanner::scan(&paths)?;
    if scanned_files.is_empty() {
        return Ok(TestSuiteOutcome {
            has_failure: false,
            batch_results: Vec::new(),
            duration: Duration::ZERO,
            degraded_count: 0,
            notices: vec![SuiteNotice::Stderr("No test files found.".to_string())],
            resolved_paths: paths,
        });
    }

    // 2. 递归自动补全加载所有依赖。显式传入的路径完全信任，
    //    @depends-on 只能落在这些输入路径的目录之下（依赖路径合理性校验）
    let sandbox_roots = DependencyResolver::sandbox_roots_from_inputs(&paths);
    let mut files_map = DependencyResolver::resolve_and_parse(&scanned_files, &sandbox_roots)?;
    let mut notices = Vec::new();

    // 3. 执行序：单请求模式收窄到目标文件（依赖文件不执行、不走 DAG）；
    //    否则拓扑排序解析依赖
    let (execution_order, dependencies) = if let Some(sel) = &selector {
        if scanned_files.len() != 1 {
            return Err(RupostError::Other(
                "单请求选择器（--only / path#n）仅支持单一文件路径".to_string(),
            ));
        }
        let target = scanned_files[0].clone();
        let file = files_map.get_mut(&target).ok_or_else(|| {
            RupostError::Other(format!("选择器未能定位目标文件: {}", target.display()))
        })?;
        let applied = selector::apply_selector(file, sel)?;
        if applied.duplicate_names > 0 {
            notices.push(SuiteNotice::Stderr(format!(
                "[!] @name 在文件内出现 {} 次，已执行首个（第 {} 条）",
                applied.duplicate_names + 1,
                applied.selected_number
            )));
        }
        let name_part = applied
            .selected_name
            .as_ref()
            .map(|n| format!("（@{n}）"))
            .unwrap_or_default();
        notices.push(SuiteNotice::TerminalStdout(format!(
            "[i] 单请求模式：执行第 {} 条{}，其余 {} 条已跳过",
            applied.selected_number, name_part, applied.skipped_count
        )));
        if options.with_deps {
            // R-1 免重填：按拓扑序先执行目标文件的依赖闭包（上游登录/取号自动先跑），
            // @capture 捕获的登录态经变量解析链流入选中请求
            let parse_pairs: Vec<_> = files_map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let graph = WorkflowGraph::new(&parse_pairs);
            let (order, deps_map) = graph.closure_execution_order(std::slice::from_ref(&target))?;
            let dep_count = order.len().saturating_sub(1);
            notices.push(SuiteNotice::TerminalStdout(format!(
                "[i] 单请求模式（--with-deps）：先执行 {dep_count} 个依赖文件，再执行第 {} 条{name_part}",
                applied.selected_number
            )));
            (order, deps_map)
        } else {
            let mut deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
            deps.insert(target.clone(), Vec::new());
            (vec![target], deps)
        }
    } else {
        let parse_pairs: Vec<_> = files_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let graph = WorkflowGraph::new(&parse_pairs);
        let order = graph.resolve_execution_order()?;
        // 提取依赖映射关系，传入 execute_batch 供并行调度使用
        let dependencies: HashMap<PathBuf, Vec<PathBuf>> = graph
            .nodes
            .iter()
            .map(|(k, v)| (k.clone(), v.depends_on.clone()))
            .collect();
        (order, dependencies)
    };

    // 4 & 5. 加载配置上下文与构建 TestExecutor（含环境变量、Cookie 策略与插件装配）
    let mut var_context = load_suite_context(options);
    let executor = build_suite_executor(options)?;
    let batch_executor = BatchExecutor::new(executor);

    // 6. 执行批处理
    let start_time = Instant::now();

    let batch_mode = if options.mode == "parallel" {
        BatchMode::Parallel
    } else {
        BatchMode::Serial
    };

    let batch_request = BatchRunRequest {
        execution_order,
        dependencies,
        files_map,
        context: &mut var_context,
        mode: batch_mode,
        concurrency: options.concurrency,
        fail_fast: options.fail_fast,
    };

    let mut batch_results = batch_executor.execute_batch(batch_request).await?;
    let duration = start_time.elapsed();

    // M7-A 语义边界收敛（02 §2）：并行池竞争 → connector 观测 dns/tcp 标 approximate；
    // `--timing off` → 时序字段整体归零（timing_source=none）。单点后置修正，
    // 快照（snapshot_io 直透字段）与报告/终端消费保持一致口径。
    reconcile_timing_metadata(&mut batch_results, &options.mode, options.timing_off);

    // 6.5 失败钩子（DX-Debug R7）：触发后使警告进入结果上下文
    if let Some(hook_cmd) = &options.on_failure {
        on_failure::fire_failure_hooks(&mut batch_results, hook_cmd).await;
    }

    // 7. 失败与降级汇总判定（纯计算）
    let degraded_count = count_degraded_non_teardown(&batch_results);
    let any_degraded = degraded_count > 0;
    let has_failure = batch_results
        .iter()
        .any(|(_, res_list)| res_list.iter().any(|r| !r.success))
        || (options.strict_plugins && any_degraded);

    // R-2 免重填：捕获变量落盘（0600；@capture/插件 set_variables 的运行期产物）
    if let Some(state_path) = &options.var_state {
        let captured: std::collections::HashMap<String, String> = var_context
            .variables()
            .iter()
            .filter(|(k, _)| k.as_str() != "__default_scheme")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        crate::variable::var_state::save_var_state(Path::new(state_path), &captured)?;
        eprintln!(
            "[i] 捕获变量已持久化至 {state_path}（{} 项，0600）",
            captured.len()
        );
    }

    Ok(TestSuiteOutcome {
        has_failure,
        batch_results,
        duration,
        degraded_count,
        notices,
        resolved_paths: paths,
    })
}

/// 向后兼容包装：直接委托 [`execute_test_suite`]。
pub async fn run_test_suite(options: TestSuiteOptions) -> Result<TestSuiteOutcome> {
    execute_test_suite(&options).await
}

/// 批跑结束后的 timing 元数据收敛（02 §2 契约诚实性，M7-A 语义边界裁决）：
/// 1. `timing_off`：时序字段整体归零——`timing_waterfall=null`、`timing_source=none`
///    （显式记录"未采集"，与字段缺省 null 区分）、归属标记抹除、采集原语清空；
/// 2. 并行模式：connector 观测的 dns/tcp 可能因连接池竞争跨请求错位 →
///    `timing_attribution=approximate`（`ttfb`/`transfer` 永远精确，attribution 仅描述
///    dns/tcp 两段）；probe 独立探测连接与已标 approximate 的代理形态保持不变。
pub fn reconcile_timing_metadata(
    batch_results: &mut [(PathBuf, Vec<crate::runner::types::TestResult>)],
    mode: &str,
    timing_off: bool,
) {
    let parallel = mode == "parallel";
    for (_, res_list) in batch_results {
        for r in res_list {
            if timing_off {
                r.timing = None;
                r.connect_span = None;
                r.timing_source = Some(crate::http::timing::TimingSource::None);
                r.timing_attribution = None;
                continue;
            }
            if parallel
                && r.timing_source == Some(crate::http::timing::TimingSource::Connector)
                && r.timing_attribution == Some(crate::http::timing::TimingAttribution::Exact)
            {
                r.timing_attribution = Some(crate::http::timing::TimingAttribution::Approximate);
            }
        }
    }
}

/// 统计非 `@teardown` 请求的插件降级数（与 `snapshot_io.rs` 一致，teardown 不计入降级告警与 `--strict-plugins` 门禁）
fn count_degraded_non_teardown(batch_results: &[(PathBuf, Vec<TestResult>)]) -> usize {
    batch_results
        .iter()
        .map(|(_, res_list)| {
            res_list
                .iter()
                .filter(|r| r.degraded && !r.is_teardown)
                .count()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::timing::{TimingAttribution, TimingSource};
    use crate::runner::types::TestResult;

    /// P1-1/P1-2（审查收口）：timing 元数据后置收敛决策表。
    /// `--timing off` → 时序字段整体归零（02 §2:135 `none` = 未采集）；
    /// 并行模式 + connector 观测 + exact → approximate（dns/tcp 池竞争可能跨请求错位，
    /// ttfb/transfer 永远精确；probe 独立连接与代理已标 approximate 的形态不变）。
    #[test]
    fn test_reconcile_timing_metadata_decision_table() {
        let mk_result =
            |source: Option<TimingSource>, attribution: Option<TimingAttribution>| -> TestResult {
                let mut r = TestResult::error(
                    1,
                    None,
                    "GET".to_string(),
                    "http://x".to_string(),
                    "e".to_string(),
                    Duration::ZERO,
                );
                r.timing_source = source;
                r.timing_attribution = attribution;
                r
            };

        // 决策 1：--timing off → 无论原形态，时序整体归零且 source 显式记录 none
        for (source, attribution) in [
            (
                Some(TimingSource::Connector),
                Some(TimingAttribution::Exact),
            ),
            (
                Some(TimingSource::ProbeDegraded),
                Some(TimingAttribution::Approximate),
            ),
            (None, None),
        ] {
            let mut batch = vec![(
                PathBuf::from("t.http"),
                vec![mk_result(source, attribution)],
            )];
            reconcile_timing_metadata(&mut batch, "sequential", true);
            let r = &batch[0].1[0];
            assert!(r.timing.is_none(), "off 必须抹除 timing 瀑布");
            assert!(r.connect_span.is_none(), "off 必须抹除 connect_span");
            assert_eq!(
                r.timing_source,
                Some(TimingSource::None),
                "off 必须显式记录 timing_source=none"
            );
            assert!(r.timing_attribution.is_none(), "off 必须抹除归属标记");
        }

        // 决策 2：parallel + connector + exact → approximate
        let mut batch = vec![(
            PathBuf::from("t.http"),
            vec![mk_result(
                Some(TimingSource::Connector),
                Some(TimingAttribution::Exact),
            )],
        )];
        reconcile_timing_metadata(&mut batch, "parallel", false);
        assert_eq!(
            batch[0].1[0].timing_attribution,
            Some(TimingAttribution::Approximate),
            "并行池竞争下 connector dns/tcp 观测必须标 approximate"
        );

        // 决策 3：串行 + connector + exact → 保持 exact（R1 既有守护不回退）
        let mut batch = vec![(
            PathBuf::from("t.http"),
            vec![mk_result(
                Some(TimingSource::Connector),
                Some(TimingAttribution::Exact),
            )],
        )];
        reconcile_timing_metadata(&mut batch, "sequential", false);
        assert_eq!(
            batch[0].1[0].timing_attribution,
            Some(TimingAttribution::Exact)
        );

        // 决策 4：parallel + probe 独立探测连接 → 保持 exact（不受池竞争影响）
        let mut batch = vec![(
            PathBuf::from("t.http"),
            vec![mk_result(
                Some(TimingSource::ProbeDegraded),
                Some(TimingAttribution::Exact),
            )],
        )];
        reconcile_timing_metadata(&mut batch, "parallel", false);
        assert_eq!(
            batch[0].1[0].timing_attribution,
            Some(TimingAttribution::Exact)
        );

        // 决策 5：parallel + 已标 approximate（代理形态） → 保持 approximate
        let mut batch = vec![(
            PathBuf::from("t.http"),
            vec![mk_result(
                Some(TimingSource::Connector),
                Some(TimingAttribution::Approximate),
            )],
        )];
        reconcile_timing_metadata(&mut batch, "parallel", false);
        assert_eq!(
            batch[0].1[0].timing_attribution,
            Some(TimingAttribution::Approximate)
        );
    }

    #[tokio::test]
    async fn test_run_test_suite_empty_paths() {
        let options = TestSuiteOptions {
            paths: vec![],
            no_cookies: true,
            ..Default::default()
        };
        let res = run_test_suite(options).await;
        assert!(res.is_ok());
        // 空路径应直接返回无失败、无结果的产出
        let outcome = res.unwrap();
        assert!(!outcome.has_failure);
        assert!(outcome.batch_results.is_empty());
        assert_eq!(outcome.degraded_count, 0);
        assert_eq!(outcome.duration, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_execute_test_suite_empty_paths_returns_structured_notice() {
        let options = TestSuiteOptions {
            paths: vec![],
            no_cookies: true,
            ..Default::default()
        };
        let outcome = execute_test_suite(&options).await.unwrap();
        assert!(!outcome.has_failure);
        assert!(outcome.batch_results.is_empty());
        assert_eq!(
            outcome.notices,
            vec![SuiteNotice::Stderr("No test files found.".to_string())]
        );
    }

    #[test]
    fn test_count_degraded_non_teardown_excludes_teardown() {
        let mut normal_ok = TestResult::skipped(1, None, "GET".into(), "/ok".into());
        normal_ok.degraded = false;
        normal_ok.is_teardown = false;

        let mut normal_degraded = TestResult::skipped(2, None, "GET".into(), "/deg".into());
        normal_degraded.degraded = true;
        normal_degraded.is_teardown = false;

        let mut teardown_degraded = TestResult::skipped(3, None, "DELETE".into(), "/clean".into());
        teardown_degraded.degraded = true;
        teardown_degraded.is_teardown = true;

        let batch = vec![
            (
                PathBuf::from("a.http"),
                vec![normal_ok, normal_degraded.clone()],
            ),
            (PathBuf::from("b.http"), vec![teardown_degraded]),
        ];
        assert_eq!(count_degraded_non_teardown(&batch), 1);

        // 仅有 teardown 降级时，总数必须为 0（不触发 any_degraded / --strict-plugins）
        let teardown_only = vec![(
            PathBuf::from("c.http"),
            vec![{
                let mut r = TestResult::skipped(1, None, "DELETE".into(), "/t".into());
                r.degraded = true;
                r.is_teardown = true;
                r
            }],
        )];
        assert_eq!(count_degraded_non_teardown(&teardown_only), 0);
    }
}
