//! 并行 DAG 调度引擎
//!
//! 承载整个并行拓扑图的调度逻辑，负责任务间的依赖同步和并发限制。

use crate::Result;
use crate::parser::ParsedFile;
use crate::runner::executor::TestExecutor;
use crate::runner::path::display_path;
use crate::runner::types::TestResult;
use crate::variable::VariableContext;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinSet;

/// 节点运行完成后的输出数据，包括状态、新变量和 Cookie
#[derive(Clone, Debug)]
pub struct TaskOutput {
    /// 是否执行成功
    pub success: bool,
    /// 该测试文件新捕获的变量增量
    pub captured_vars: HashMap<String, String>,
    /// 该测试文件执行完后 Cookie 罐的序列化状态
    pub cookie_state: Option<serde_json::Value>,
}

/// 工作线程共享的主执行器不可变配置（全 `Arc::clone` 轻量共享，零 Registry/Cache 拷贝）
#[derive(Clone)]
struct WorkerExecutorConfig {
    has_cookies: bool,
    debug: bool,
    debug_on_failure: bool,
    plugin_registry: Option<Arc<crate::plugin::PluginRegistry>>,
    plugin_options: crate::plugin::pipeline::PluginRuntimeOptions,
    suite_hook_cache: Arc<crate::plugin::pipeline::SuiteHookCache>,
    stream_sender: Option<tokio::sync::mpsc::UnboundedSender<crate::runner::types::StreamEvent>>,
}

impl WorkerExecutorConfig {
    fn from_executor(executor: &TestExecutor) -> Self {
        Self {
            has_cookies: executor.has_cookies(),
            debug: executor.debug,
            debug_on_failure: executor.debug_on_failure,
            plugin_registry: executor.plugin_registry.clone(),
            plugin_options: executor.plugin_options.clone(),
            suite_hook_cache: Arc::clone(&executor.suite_hook_cache),
            stream_sender: executor.stream_sender.clone(),
        }
    }
}

fn is_non_empty_cookie_state(state: &serde_json::Value) -> bool {
    !matches!(state, serde_json::Value::Null) && !state.as_array().is_some_and(Vec::is_empty)
}

/// 并行调度产出：结果集 + 已启动文件路径（按拓扑序，含被 fail_fast 中断的在途文件）+ 末位有效会话 Cookie 状态
pub struct ParallelRunOutput {
    pub results: Vec<(PathBuf, Vec<TestResult>)>,
    pub started_files: Vec<PathBuf>,
    pub final_cookie_state: Option<serde_json::Value>,
}

/// 并行调度执行器
pub struct ParallelScheduler<'a> {
    executor: &'a TestExecutor,
    execution_order: Vec<PathBuf>,
    dependencies: HashMap<PathBuf, Vec<PathBuf>>,
    files_map: HashMap<PathBuf, ParsedFile>,
    context: &'a mut VariableContext,
    concurrency: usize,
    fail_fast: bool,
}

impl<'a> ParallelScheduler<'a> {
    /// 创建一个新的并行调度器实例
    pub fn new(
        executor: &'a TestExecutor,
        execution_order: Vec<PathBuf>,
        dependencies: HashMap<PathBuf, Vec<PathBuf>>,
        files_map: HashMap<PathBuf, ParsedFile>,
        context: &'a mut VariableContext,
        concurrency: usize,
        fail_fast: bool,
    ) -> Self {
        Self {
            executor,
            execution_order,
            dependencies,
            files_map,
            context,
            concurrency,
            fail_fast,
        }
    }

    /// 根据主执行器插件配置与给定 Cookie 状态创建临时执行器（供 worker 与并行 `@teardown` 复用）。
    pub(super) fn build_ephemeral_clone_with_cookies(
        base: &TestExecutor,
        cookie_state: Option<serde_json::Value>,
    ) -> TestExecutor {
        let cfg = WorkerExecutorConfig::from_executor(base);
        Self::build_executor_with_state(&cfg, cookie_state)
    }

    /// 执行并行调度
    pub async fn run(self) -> Result<ParallelRunOutput> {
        let mut results = Vec::new();
        let mut join_set = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(self.concurrency));
        let has_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_files = Arc::new(std::sync::Mutex::new(HashSet::<PathBuf>::new()));

        let mut seen_order = HashSet::new();
        let execution_order: Vec<PathBuf> = self
            .execution_order
            .into_iter()
            .filter(|p| seen_order.insert(p.clone()))
            .collect();

        // 1. 为每个节点建立一个 watch 通道以传递完成状态。初始化为 None (未完成)
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();
        for file_path in &execution_order {
            let (tx, rx) = watch::channel::<Option<TaskOutput>>(None);
            senders.insert(file_path.clone(), tx);
            receivers.insert(file_path.clone(), rx);
        }

        let mut files_map = self.files_map;
        let base_worker_config = WorkerExecutorConfig::from_executor(self.executor);

        for file_path in &execution_order {
            if let Some(parsed_file) = files_map.remove(file_path) {
                let file_path = file_path.clone();
                let sem = Arc::clone(&semaphore);
                let has_failed_clone = Arc::clone(&has_failed);
                let started_files_clone = Arc::clone(&started_files);
                let worker_config = base_worker_config.clone();
                let fail_fast = self.fail_fast;

                // 节点在进入线程前的基础 Context
                let base_context = self.context.clone();

                // 找出当前节点所依赖的所有前置节点的 Receiver
                let mut dep_receivers = Vec::new();
                if let Some(deps) = self.dependencies.get(&file_path) {
                    for dep in deps {
                        if let Some(rx) = receivers.get(dep) {
                            dep_receivers.push(rx.clone());
                        }
                    }
                }

                // 获取自己的 Sender，以广播结果状态给后续节点
                let my_sender = senders.remove(&file_path).ok_or_else(|| {
                    crate::error::RupostError::Other(format!(
                        "并行调度内部错误：缺少节点 {} 的状态通道",
                        display_path(&file_path)
                    ))
                })?;

                join_set.spawn(async move {
                    // 2. 异步等待所有前置依赖项成功运行完成，并获取其传来的变量和 Cookie 状态
                    let Some((dep_vars, dep_cookie_state)) =
                        Self::wait_for_deps(dep_receivers, my_sender.clone()).await
                    else {
                        return (file_path, Vec::new(), true);
                    };

                    // 3. 检查全局的 fail_fast 中断
                    if fail_fast && has_failed_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = my_sender.send(Some(TaskOutput {
                            success: false,
                            captured_vars: HashMap::new(),
                            cookie_state: None,
                        }));
                        return (file_path, Vec::new(), true);
                    }

                    // 获取并发信号量许可
                    let Ok(_permit) = sem.acquire().await else {
                        return (file_path, Vec::new(), true);
                    };

                    // 获取许可后再次检查 fail_fast（避免排队后在已中断情况下启动）
                    if fail_fast && has_failed_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = my_sender.send(Some(TaskOutput {
                            success: false,
                            captured_vars: HashMap::new(),
                            cookie_state: None,
                        }));
                        return (file_path, Vec::new(), true);
                    }

                    // 记录已实际启动执行的文件（供 fail_fast abort_all 后仍触发 @teardown）
                    {
                        let mut started = started_files_clone
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        started.insert(file_path.clone());
                    }

                    // 4. 构建合并了前置变量的局部 Context
                    let mut worker_context = base_context.clone();
                    worker_context.extend(dep_vars);

                    // 5. 实例化当前的 Executor，继承主插件体系并载入前置 Cookie 状态
                    let worker_executor =
                        Self::build_executor_with_state(&worker_config, dep_cookie_state);

                    // 6. 执行真正的测试
                    let res = worker_executor
                        .execute_all(parsed_file, &mut worker_context)
                        .await;

                    let mut success = true;
                    let results_val = match res {
                        Ok(res_list) => {
                            if res_list.iter().any(|r| !r.success) {
                                success = false;
                            }
                            res_list
                        }
                        Err(e) => {
                            success = false;
                            vec![TestResult::error(
                                1,
                                None,
                                "BATCH".to_string(),
                                display_path(&file_path),
                                e.to_string(),
                                std::time::Duration::from_secs(0),
                            )]
                        }
                    };

                    // 7. 计算变量差集增量与 Cookie 导出状态
                    let diff_vars = Self::extract_changed_vars(&base_context, &worker_context);
                    // 导出失败不得静默（审查 P1）：下游子节点将无前置会话，
                    // 依赖 cookie 的用例会连锁 401 且难归因
                    let my_cookie_state =
                        worker_executor.export_cookie_state().unwrap_or_else(|e| {
                            tracing::warn!(
                                "{}: worker cookie 状态导出失败: {}",
                                crate::middleware::ERR_COOKIE_PERSIST,
                                e
                            );
                            None
                        });

                    // 8. 广播并更新自己的状态给后置子节点
                    let _ = my_sender.send(Some(TaskOutput {
                        success,
                        captured_vars: diff_vars,
                        cookie_state: my_cookie_state,
                    }));

                    if !success && fail_fast {
                        has_failed_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                    }

                    (file_path, results_val, false)
                });
            }
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((path, run_results, aborted)) => {
                    if aborted {
                        continue;
                    }
                    if self.fail_fast && run_results.iter().any(|r| !r.success) {
                        join_set.abort_all();
                    }

                    results.push((path, run_results));
                }
                Err(e) => {
                    // M3（评审 P0-1）：fail_fast 的 abort_all 会取消未启动 worker，
                    // 产生 cancelled JoinError——这是预期中断路径，容忍并继续排空
                    // join 队列（否则提前 return Err 跳过变量合并与 Finally teardown）。
                    // 真实 panic 仍然上抛。
                    if e.is_cancelled() {
                        continue;
                    }
                    return Err(crate::error::RupostError::Other(format!(
                        "任务执行 Join 失败: {}",
                        e
                    )));
                }
            }
        }

        // 按确定性 execution_order 合并各已完成节点捕获的变量，并将最终有效 Cookie 状态回灌
        // 主执行器（避免 HashMap 无序迭代的随机性，同时支撑后续 @teardown 会话鉴权）。
        let mut all_completed_vars = HashMap::new();
        let mut non_empty_cookie_states: Vec<serde_json::Value> = Vec::new();
        for file_path in &execution_order {
            if let Some(rx) = receivers.get(file_path)
                && let Some(output) = &*rx.borrow()
            {
                all_completed_vars.extend(output.captured_vars.clone());
                if let Some(state) = &output.cookie_state
                    && is_non_empty_cookie_state(state)
                {
                    non_empty_cookie_states.push(state.clone());
                }
            }
        }
        self.context.extend(all_completed_vars);

        if non_empty_cookie_states.len() > 1
            && non_empty_cookie_states.windows(2).any(|w| w[0] != w[1])
        {
            tracing::warn!(
                "{} 个并行分支均捕获了不一致的 Cookie 状态：仅保留拓扑序末位分支的会话状态供后续 @teardown 临时使用",
                non_empty_cookie_states.len()
            );
        }

        let started_set = started_files.lock().unwrap_or_else(|e| e.into_inner());
        let started_files: Vec<PathBuf> = execution_order
            .iter()
            .filter(|p| started_set.contains(*p) || results.iter().any(|(rp, _)| rp == *p))
            .cloned()
            .collect();

        Ok(ParallelRunOutput {
            results,
            started_files,
            final_cookie_state: non_empty_cookie_states.pop(),
        })
    }

    /// 辅助函数 1：异步等待所有前置依赖项运行完毕
    async fn wait_for_deps(
        dep_receivers: Vec<watch::Receiver<Option<TaskOutput>>>,
        my_sender: watch::Sender<Option<TaskOutput>>,
    ) -> Option<(HashMap<String, String>, Option<serde_json::Value>)> {
        let mut merged_vars = HashMap::new();
        let mut cookie_states: Vec<serde_json::Value> = Vec::new();

        for mut rx in dep_receivers {
            loop {
                let current_val = rx.borrow().clone();
                match current_val {
                    Some(output) => {
                        if !output.success {
                            // 级联失败，告知后续依赖
                            let _ = my_sender.send(Some(TaskOutput {
                                success: false,
                                captured_vars: HashMap::new(),
                                cookie_state: None,
                            }));
                            return None;
                        }
                        merged_vars.extend(output.captured_vars);
                        if let Some(state) = output.cookie_state {
                            cookie_states.push(state);
                        }
                        break;
                    }
                    None => {
                        if rx.changed().await.is_err() {
                            // 通道被关闭，视作前置失败
                            let _ = my_sender.send(Some(TaskOutput {
                                success: false,
                                captured_vars: HashMap::new(),
                                cookie_state: None,
                            }));
                            return None;
                        }
                    }
                }
            }
        }

        // 变量可以合并，Cookie 罐不能安全合并（域/路径/过期冲突无通用裁决）：
        // 多父节点时只继承依赖声明顺序中的最后一个。此前这里静默覆盖，钻石依赖
        // （A→B, A→C, B+C→D）会悄悄丢掉一半会话状态；现在差异时显式警告。
        if cookie_states.len() > 1 && cookie_states.windows(2).any(|w| w[0] != w[1]) {
            tracing::warn!(
                "{} 个前置依赖均携带 Cookie 状态且内容不一致：仅继承最后一个（按依赖声明顺序），\
                 其余父节点的会话 Cookie 不会合并",
                cookie_states.len()
            );
        }
        let last_cookie_state = cookie_states.pop();

        Some((merged_vars, last_cookie_state))
    }

    /// 辅助函数 2：根据主配置与前置 Cookie 状态实例化 Worker `TestExecutor`
    fn build_executor_with_state(
        config: &WorkerExecutorConfig,
        cookie_state: Option<serde_json::Value>,
    ) -> TestExecutor {
        let mut executor = if config.has_cookies {
            TestExecutor::with_ephemeral_cookies()
        } else {
            TestExecutor::new()
        }
        .with_debug(config.debug)
        .with_debug_on_failure(config.debug_on_failure)
        .with_shared_plugins(
            config.plugin_registry.clone(),
            config.plugin_options.clone(),
            Arc::clone(&config.suite_hook_cache),
        );
        if let Some(sender) = config.stream_sender.clone() {
            executor = executor.with_stream_sender(sender);
        }

        if let Some(state) = cookie_state
            && let Err(e) = executor.import_cookie_state(state)
        {
            // 与 TUI 侧对齐（此前静默）：导入失败 → 本 worker 无前置会话
            tracing::warn!("{}: 导入失败: {}", crate::middleware::ERR_COOKIE_PERSIST, e);
        }
        executor
    }

    /// 辅助函数 3：提取前置和后置的 Context 差集增量
    fn extract_changed_vars(
        before: &VariableContext,
        after: &VariableContext,
    ) -> HashMap<String, String> {
        let mut diff = HashMap::new();
        for (k, v) in after.variables() {
            if before.get(k) != Some(v.clone()) {
                diff.insert(k.clone(), v.clone());
            }
        }
        diff
    }
}

#[cfg(test)]
#[path = "parallel_tests.rs"]
mod tests;
