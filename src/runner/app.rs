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
            allow_missing_plugins: false,
            strict_plugins: false,
            plugin_timeout_secs: crate::plugin::pipeline::DEFAULT_PLUGIN_TIMEOUT_SECS,
            plugin_env: Vec::new(),
            allow_project_plugins: false,
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
        let mut deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        deps.insert(target.clone(), Vec::new());
        (vec![target], deps)
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
