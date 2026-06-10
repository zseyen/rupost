//! 并行 DAG 调度引擎
//!
//! 承载整个并行拓扑图的调度逻辑，负责任务间的依赖同步和并发限制。

use crate::Result;
use crate::parser::ParsedFile;
use crate::runner::executor::TestExecutor;
use crate::runner::path::display_path;
use crate::runner::types::TestResult;
use crate::variable::VariableContext;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::sync::Semaphore;
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

    /// 执行并行调度
    pub async fn run(self) -> Result<Vec<(PathBuf, Vec<TestResult>)>> {
        let mut results = Vec::new();
        let mut join_set = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(self.concurrency));
        let has_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // 1. 为每个节点建立一个 watch 通道以传递完成状态。初始化为 None (未完成)
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();
        for file_path in &self.execution_order {
            let (tx, rx) = watch::channel::<Option<TaskOutput>>(None);
            senders.insert(file_path.clone(), tx);
            receivers.insert(file_path.clone(), rx);
        }

        let mut files_map = self.files_map;

        for file_path in &self.execution_order {
            if let Some(parsed_file) = files_map.remove(file_path) {
                let file_path = file_path.clone();
                let sem = Arc::clone(&semaphore);
                let has_failed_clone = Arc::clone(&has_failed);
                
                // 获取主执行器的配置
                let has_cookies = self.executor.has_cookies();
                let debug = self.executor.debug;
                let debug_on_failure = self.executor.debug_on_failure;
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
                let my_sender = senders.remove(&file_path).unwrap();

                join_set.spawn(async move {
                    // 2. 异步等待所有前置依赖项成功运行完成，并获取其传来的变量和 Cookie 状态
                    let Some((dep_vars, dep_cookie_state)) = Self::wait_for_deps(dep_receivers, my_sender.clone()).await else {
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
                    let _permit = sem.acquire().await.unwrap();

                    // 4. 构建合并了前置变量的局部 Context
                    let mut worker_context = base_context.clone();
                    worker_context.extend(dep_vars);

                    // 5. 实例化当前的 Executor，并反序列化载入前置 Cookie 状态
                    let worker_executor = Self::build_executor_with_state(
                        has_cookies,
                        debug,
                        debug_on_failure,
                        dep_cookie_state,
                    );

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
                    let my_cookie_state = worker_executor.export_cookie_state();

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

        let mut all_completed_vars = HashMap::new();

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
                    return Err(crate::error::RupostError::Other(format!(
                        "任务执行 Join 失败: {}",
                        e
                    )));
                }
            }
        }

        // 我们在通道中也提取接收最终的各节点变量合并到 self.context 
        // 这一步在 run 结束时可以把并行的捕获写回主环境以保持环境变量干净
        for rx in receivers.values() {
            if let Some(output) = &*rx.borrow() {
                all_completed_vars.extend(output.captured_vars.clone());
            }
        }
        self.context.extend(all_completed_vars);

        Ok(results)
    }

    /// 辅助函数 1：异步等待所有前置依赖项运行完毕
    async fn wait_for_deps(
        dep_receivers: Vec<watch::Receiver<Option<TaskOutput>>>,
        my_sender: watch::Sender<Option<TaskOutput>>,
    ) -> Option<(HashMap<String, String>, Option<serde_json::Value>)> {
        let mut merged_vars = HashMap::new();
        let mut last_cookie_state = None;

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
                        if output.cookie_state.is_some() {
                            last_cookie_state = output.cookie_state;
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
        Some((merged_vars, last_cookie_state))
    }

    /// 辅助函数 2：根据前置 Cookie 状态实例化 TestExecutor
    fn build_executor_with_state(
        has_cookies: bool,
        debug: bool,
        debug_on_failure: bool,
        cookie_state: Option<serde_json::Value>,
    ) -> TestExecutor {
        let mut executor = if has_cookies {
            TestExecutor::with_ephemeral_cookies()
        } else {
            TestExecutor::new()
        };
        executor.debug = debug;
        executor.debug_on_failure = debug_on_failure;

        if let Some(state) = cookie_state {
            let _ = executor.import_cookie_state(state);
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
mod tests {
    use super::*;
    use crate::variable::VariableContext;

    #[test]
    fn test_extract_changed_vars() {
        let mut before = VariableContext::new();
        before.insert("a", "1");
        before.insert("b", "2");

        let mut after = before.clone();
        after.insert("b", "modified"); // 修改的
        after.insert("c", "3");        // 新加的

        let diff = ParallelScheduler::extract_changed_vars(&before, &after);
        assert_eq!(diff.len(), 2);
        assert_eq!(diff.get("b").map(|s| s.as_str()), Some("modified"));
        assert_eq!(diff.get("c").map(|s| s.as_str()), Some("3"));
    }

    #[test]
    fn test_build_executor_with_cookie_state() {
        let base_executor = TestExecutor::with_ephemeral_cookies();
        
        // 模拟一个带 Cookie 的状态 Value
        let middleware = crate::middleware::CookieMiddleware::new_ephemeral();
        {
            let store_arc = middleware.cookie_store();
            let mut store = store_arc.lock().unwrap();
            let cookie = cookie::Cookie::build(("session", "abcdef"))
                .domain("api.example.com")
                .path("/")
                .build();
            let url = url::Url::parse("https://api.example.com/").unwrap();
            store.insert_raw(&cookie, &url).unwrap();
        }
        let cookie_state = middleware.export_cookie_state().unwrap();

        // 载入
        let executor = ParallelScheduler::build_executor_with_state(
            true,
            base_executor.debug,
            base_executor.debug_on_failure,
            Some(cookie_state),
        );
        
        assert!(executor.has_cookies());
        let current_state = executor.export_cookie_state().unwrap();
        // 应该能重新导出来
        assert!(current_state.to_string().contains("session"));
        assert!(current_state.to_string().contains("abcdef"));
    }
}
