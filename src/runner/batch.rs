use crate::Result;
use crate::parser::ParsedFile;
use crate::runner::executor::TestExecutor;
use crate::runner::types::TestResult;
use crate::variable::VariableContext;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub struct BatchExecutor {
    executor: TestExecutor,
}

impl BatchExecutor {
    pub fn new(executor: TestExecutor) -> Self {
        Self { executor }
    }

    pub async fn execute_batch(
        &self,
        execution_order: Vec<PathBuf>,
        mut files_map: HashMap<PathBuf, ParsedFile>,
        context: &mut VariableContext,
        mode: &str,
        concurrency: usize,
        fail_fast: bool,
    ) -> Result<Vec<(PathBuf, Vec<TestResult>)>> {
        let mut results = Vec::new();

        if mode == "parallel" {
            let mut join_set = JoinSet::new();
            let semaphore = Arc::new(Semaphore::new(concurrency));
            let has_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));

            for file_path in &execution_order {
                if let Some(parsed_file) = files_map.remove(file_path) {
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
                    let mut worker_context = context.clone();

                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();

                        if fail_fast && has_failed_clone.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            return (file_path, Vec::new(), true);
                        }

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
                                    file_path.to_string_lossy().to_string(),
                                    e.to_string(),
                                    std::time::Duration::from_secs(0),
                                )]
                            }
                        };

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
                        if fail_fast && run_results.iter().any(|r| !r.success) {
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
        } else {
            for file_path in &execution_order {
                if let Some(parsed_file) = files_map.remove(file_path) {
                    let res = self.executor.execute_all(parsed_file, context).await?;
                    let has_failure = res.iter().any(|r| !r.success);
                    results.push((file_path.clone(), res));

                    if has_failure && fail_fast {
                        break;
                    }
                }
            }
        }

        let order_map: HashMap<PathBuf, usize> = execution_order
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();

        results.sort_by_key(|(path, _)| order_map.get(path).cloned().unwrap_or(usize::MAX));

        Ok(results)
    }
}
