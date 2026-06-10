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
            let (tx, rx) = watch::channel::<Option<bool>>(None);
            senders.insert(file_path.clone(), tx);
            receivers.insert(file_path.clone(), rx);
        }

        let mut files_map = self.files_map;

        for file_path in &self.execution_order {
            if let Some(parsed_file) = files_map.remove(file_path) {
                // 根据主执行器是否有 cookies 配置，决定 worker 的实例化方式
                let mut worker_executor = if self.executor.has_cookies() {
                    TestExecutor::with_ephemeral_cookies()
                } else {
                    TestExecutor::new()
                };
                worker_executor.debug = self.executor.debug;
                worker_executor.debug_on_failure = self.executor.debug_on_failure;

                let worker_executor = Arc::new(worker_executor);
                let file_path = file_path.clone();
                let sem = Arc::clone(&semaphore);
                let has_failed_clone = Arc::clone(&has_failed);
                let mut worker_context = self.context.clone();

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
                let fail_fast = self.fail_fast;

                join_set.spawn(async move {
                    // 2. 异步等待所有前置依赖项成功运行完成
                    for mut rx in dep_receivers {
                        loop {
                            let current_val = *rx.borrow();
                            match current_val {
                                Some(true) => {
                                    break; // 依赖成功，检查下一个
                                }
                                Some(false) => {
                                    // 前置依赖失败，级联跳过，当前节点也宣告失败并退出
                                    let _ = my_sender.send(Some(false));
                                    return (file_path, Vec::new(), true);
                                }
                                None => {
                                    // 还没有数据，等待变更
                                    if rx.changed().await.is_err() {
                                        // 管道关闭，代表崩溃/取消，视作失败
                                        let _ = my_sender.send(Some(false));
                                        return (file_path, Vec::new(), true);
                                    }
                                }
                            }
                        }
                    }

                    // 3. 检查全局的 fail_fast 中断
                    if fail_fast && has_failed_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = my_sender.send(Some(false));
                        return (file_path, Vec::new(), true);
                    }

                    // 获取并发信号量许可
                    let _permit = sem.acquire().await.unwrap();

                    // 4. 执行真正的测试
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

                    // 5. 更新并广播自己的结果状态
                    let _ = my_sender.send(Some(success));

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
                    return Err(crate::error::RupostError::Other(format!(
                        "任务执行 Join 失败: {}",
                        e
                    )));
                }
            }
        }

        Ok(results)
    }
}
