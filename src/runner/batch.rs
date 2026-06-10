use crate::Result;
use crate::parser::ParsedFile;
use crate::runner::executor::TestExecutor;
use crate::runner::parallel::ParallelScheduler;
use crate::runner::types::TestResult;
use crate::variable::VariableContext;
use std::collections::HashMap;
use std::path::PathBuf;

/// 批量执行运行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMode {
    /// 顺序串行执行
    Serial,
    /// 拓扑并行执行
    Parallel,
}

/// 批量测试运行请求参数防腐层
pub struct BatchRunRequest<'a> {
    /// 解析排好序的文件路径列表
    pub execution_order: Vec<PathBuf>,
    /// 节点的依赖映射图
    pub dependencies: HashMap<PathBuf, Vec<PathBuf>>,
    /// 所有的解析文件数据
    pub files_map: HashMap<PathBuf, ParsedFile>,
    /// 变量上下文引用
    pub context: &'a mut VariableContext,
    /// 执行模式
    pub mode: BatchMode,
    /// 并行时的并发度
    pub concurrency: usize,
    /// 失败时是否立刻中断
    pub fail_fast: bool,
}

/// 批量测试执行器
pub struct BatchExecutor {
    executor: TestExecutor,
}

impl BatchExecutor {
    /// 创建批量执行器
    pub fn new(executor: TestExecutor) -> Self {
        Self { executor }
    }

    /// 执行批处理测试
    pub async fn execute_batch(
        &self,
        mut request: BatchRunRequest<'_>,
    ) -> Result<Vec<(PathBuf, Vec<TestResult>)>> {
        let mut results = Vec::new();

        match request.mode {
            BatchMode::Parallel => {
                let scheduler = ParallelScheduler::new(
                    &self.executor,
                    request.execution_order.clone(),
                    request.dependencies,
                    request.files_map,
                    request.context,
                    request.concurrency,
                    request.fail_fast,
                );
                results = scheduler.run().await?;
            }
            BatchMode::Serial => {
                for file_path in &request.execution_order {
                    if let Some(parsed_file) = request.files_map.remove(file_path) {
                        let res = self
                            .executor
                            .execute_all(parsed_file, request.context)
                            .await?;
                        let has_failure = res.iter().any(|r| !r.success);
                        results.push((file_path.clone(), res));

                        if has_failure && request.fail_fast {
                            break;
                        }
                    }
                }
            }
        }

        // 最终按照原定的拓扑顺序排序，以保持稳定输出
        let order_map: HashMap<PathBuf, usize> = request
            .execution_order
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();

        results.sort_by_key(|(path, _)| order_map.get(path).cloned().unwrap_or(usize::MAX));

        Ok(results)
    }
}
