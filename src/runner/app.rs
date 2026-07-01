use crate::Result;
use crate::history::save_batch_snapshot;
use crate::middleware::resolve_cookie_path;
use crate::runner::{
    BatchExecutor, BatchMode, BatchRunRequest, DependencyResolver, DirectoryScanner, TestExecutor,
    TestReporter, WorkflowGraph,
};
use crate::variable::ConfigLoader;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub struct TestSuiteOptions {
    pub paths: Vec<String>,
    pub mode: String,
    pub concurrency: usize,
    pub report: String,
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
}

/// 执行批处理测试流程，如果发现有测试失败，则返回 Ok(true)，否则返回 Ok(false)
pub async fn run_test_suite(options: TestSuiteOptions) -> Result<bool> {
    // 1. 递归扫描文件
    let scanned_files = DirectoryScanner::scan(&options.paths)?;
    if scanned_files.is_empty() {
        println!("No test files found.");
        return Ok(false);
    }

    // 2. 递归自动补全加载所有依赖（带安全沙箱限制）
    let sandbox_root = std::env::current_dir()?;
    let files_map = DependencyResolver::resolve_and_parse(&scanned_files, &sandbox_root)?;

    // 3. 拓扑排序解析依赖
    let parse_pairs: Vec<_> = files_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let graph = WorkflowGraph::new(&parse_pairs);
    let execution_order = graph.resolve_execution_order()?;

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
        let resolved = resolve_cookie_path(PathBuf::from(cookie_path), options.env.as_deref());
        TestExecutor::with_cookies(resolved)?
    } else {
        TestExecutor::with_ephemeral_cookies()
    };

    executor = executor
        .with_debug(options.debug)
        .with_debug_on_failure(options.debug_on_failure);

    let batch_executor = BatchExecutor::new(executor);

    // 6. 执行批处理
    let start_time = Instant::now();
    // 提取依赖映射关系，传入 execute_batch 供并行调度使用
    let dependencies: HashMap<PathBuf, Vec<PathBuf>> = graph
        .nodes
        .iter()
        .map(|(k, v)| (k.clone(), v.depends_on.clone()))
        .collect();

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
        TestReporter::report_json(&batch_results, &mut std::io::stdout())?;
    } else {
        let reporter = TestReporter::new(options.verbose);
        for (path, file_results) in &batch_results {
            let path_str = path.to_string_lossy().to_string();
            reporter.print_header(&path_str, file_results.len());
            for r in file_results {
                reporter.print_result(r);
            }
        }
        TestReporter::print_batch_summary(&batch_results, duration);
    }

    // 保存快照
    if let Some(snapshot_path) = &options.save_snapshot {
        save_batch_snapshot(&batch_results, &options.paths, snapshot_path)?;
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
        };
        let res = run_test_suite(options).await;
        assert!(res.is_ok());
        // 空路径应直接返回 false (没有发生失败)
        assert_eq!(res.unwrap(), false);
    }
}
