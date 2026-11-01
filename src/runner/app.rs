use crate::Result;
use crate::RupostError;
use crate::history::save_batch_snapshot;
use crate::middleware::resolve_cookie_path;
use crate::runner::selector;
use crate::runner::{
    BatchExecutor, BatchMode, BatchRunRequest, DependencyResolver, DirectoryScanner, TestExecutor,
    TestReporter, WorkflowGraph,
};
use crate::variable::ConfigLoader;
use colored::Colorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

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
    /// 单请求选择器参数（`--only <n|@name>`；`path#n` 片段在 paths 内携带）
    pub only: Option<String>,
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
            debug: false,
            debug_on_failure: false,
            default_scheme: "http".to_string(),
            save_snapshot: None,
        }
    }
}

/// 执行批处理测试流程，如果发现有测试失败，则返回 Ok(true)，否则返回 Ok(false)
pub async fn run_test_suite(options: TestSuiteOptions) -> Result<bool> {
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
        println!("No test files found.");
        return Ok(false);
    }

    // 2. 递归自动补全加载所有依赖。显式传入的路径完全信任，
    //    @depends-on 只能落在这些输入路径的目录之下（依赖路径合理性校验）
    let sandbox_roots = DependencyResolver::sandbox_roots_from_inputs(&paths);
    let mut files_map = DependencyResolver::resolve_and_parse(&scanned_files, &sandbox_roots)?;

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
            eprintln!(
                "[!] @name 在文件内出现 {} 次，已执行首个（第 {} 条）",
                applied.duplicate_names + 1,
                applied.selected_number
            );
        }
        let name_part = applied
            .selected_name
            .as_ref()
            .map(|n| format!("（@{n}）"))
            .unwrap_or_default();
        // [i] 提示仅在 terminal 模式输出，保持 --report json 的 stdout 纯净可解析
        if options.report != "json" {
            println!(
                "[i] 单请求模式：执行第 {} 条{}，其余 {} 条已跳过",
                applied.selected_number, name_part, applied.skipped_count
            );
        }
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

    // 4. 加载配置并构建变量上下文
    let config = ConfigLoader::find_and_load().unwrap_or_default();

    // 解析 CLI 变量覆盖
    let cli_vars: Vec<(String, String)> = options
        .var
        .iter()
        .filter_map(|s| ConfigLoader::parse_cli_var(s))
        .collect();

    // 如果指定了 env_file 则使用它，否则默认自动寻找加载并合并本地 .env
    let env_file_to_load = options.env_file.as_deref().or(Some(".env"));

    let mut var_context =
        ConfigLoader::build_context(&config, options.env.as_deref(), &cli_vars, env_file_to_load);
    var_context.insert("__default_scheme", &options.default_scheme);

    // 5. 构建 TestExecutor
    let mut executor = if options.no_cookies {
        TestExecutor::new()
    } else if let Some(cookie_path) = &options.cookie_file {
        let resolved = resolve_cookie_path(PathBuf::from(cookie_path), options.env.as_deref())?;
        TestExecutor::with_cookies(resolved)?
    } else {
        // 默认为临时会话（运行内共享、进程退出即失），与 Postman 等的"默认持久化"
        // 心智不同——不提示会让用户误以为会话已保存
        if options.verbose {
            println!(
                "[i] 会话 Cookie 为临时模式（仅本次运行内共享）；加 --cookie-file <FILE> 可跨运行持久化"
            );
        }
        tracing::debug!("Cookies are ephemeral for this run; pass --cookie-file to persist");
        TestExecutor::with_ephemeral_cookies()
    };

    executor = executor
        .with_debug(options.debug)
        .with_debug_on_failure(options.debug_on_failure);

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

    let batch_results = batch_executor.execute_batch(batch_request).await?;
    let duration = start_time.elapsed();

    // 7. 渲染与汇报结果
    let has_failure = batch_results
        .iter()
        .any(|(_, res_list)| res_list.iter().any(|r| !r.success));

    if options.report == "json" {
        // Gap Review P3-4：JSON 模式下 explain/triage 无法渲染，显式提示而非静默忽略
        if options.explain || options.triage {
            eprintln!(
                "{} --explain/--triage 仅在终端报告模式下渲染（--report json 输出保持纯净，已忽略）",
                "⚠".yellow()
            );
        }
        TestReporter::report_json(&batch_results, &mut std::io::stdout())?;
    } else {
        let reporter = TestReporter::new(options.verbose);
        let mut explain_session = crate::ai::explain::ExplainSession::new();
        // AI 客户端每轮构建一次（避免逐请求重建连接池）；无 Key 时由会话打印一次性提示
        let ai_client = if options.explain {
            // 与 --triage 同源走 B4 配置链（env > ai.toml > 本地探测），避免只有
            // ai.toml/Ollama 的用户在 --explain 下永远拿不到归因；未配置降 None
            crate::ai::config::resolve()
                .await
                .ok()
                .map(|resolved| resolved.client)
        } else {
            None
        };
        for (path, file_results) in &batch_results {
            let path_str = path.to_string_lossy().to_string();
            reporter.print_header(&path_str, file_results.len());
            for r in file_results {
                reporter.print_result(r);
                if options.explain {
                    explain_session
                        .explain_failure_with_client(r, ai_client.as_deref())
                        .await;
                }
            }
        }
        TestReporter::print_batch_summary(&batch_results, duration);
        if options.triage {
            crate::ai::triage::run_inline_triage(&batch_results).await;
        }
    }

    // 保存 JSON 报告到指定文件（不干扰终端输出）
    if let Some(report_path) = &options.report_file {
        let mut file = std::fs::File::create(report_path)?;
        TestReporter::report_json(&batch_results, &mut file)?;
    }

    // 保存快照
    if let Some(snapshot_path) = &options.save_snapshot {
        save_batch_snapshot(&batch_results, &paths, snapshot_path)?;
    }

    Ok(has_failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_test_suite_empty_paths() {
        let options = TestSuiteOptions {
            paths: vec![],
            mode: "sequential".to_string(),
            concurrency: 4,
            report: "terminal".to_string(),
            report_file: None,
            explain: false,
            triage: false,
            fail_fast: false,
            env: None,
            var: vec![],
            env_file: None,
            verbose: false,
            no_cookies: true,
            cookie_file: None,
            debug: false,
            debug_on_failure: false,
            default_scheme: "http".to_string(),
            save_snapshot: None,
            only: None,
        };
        let res = run_test_suite(options).await;
        assert!(res.is_ok());
        // 空路径应直接返回 false (没有发生失败)
        assert!(!res.unwrap());
    }
}
