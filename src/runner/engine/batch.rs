use super::parallel::ParallelScheduler;
use crate::Result;
use crate::parser::ParsedFile;
use crate::runner::executor::TestExecutor;
use crate::runner::path::display_path;
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

        // M3：@teardown 切分必须在派发之前完成（并行分支 files_map 会 move 进
        // scheduler，Finally 阶段拿不到数据——评审 P0 修订）。
        // teardowns: 每文件 (原始序号, 请求) 列表，按执行序排列；
        // 已被 skip 标记（@skip / --only 未选中）的 teardown 豁免抽取。
        let mut teardowns_map: HashMap<PathBuf, Vec<(usize, crate::parser::ParsedRequest)>> =
            HashMap::new();
        // 只记录不抽取：teardown 请求保留在 files_map 中，常规阶段由 executor 产出
        // 原序号占位 skipped（execute_all_streaming 的 is_teardown 跳过），
        // Finally 阶段原位替换——编号契约零破坏（P0-2）
        for (path, parsed_file) in request.files_map.iter() {
            for (pos, req) in parsed_file.requests.iter().enumerate() {
                let original_number = pos + 1;
                if req.metadata.is_teardown && !req.metadata.skip {
                    teardowns_map
                        .entry(path.clone())
                        .or_default()
                        .push((original_number, req.clone()));
                }
            }
        }
        // 若有 teardown 被跳标（--only / @skip），一次性提示（P1-10：提示不可为死代码）
        let skip_marked_teardowns: usize = request
            .files_map
            .values()
            .map(|f| {
                f.requests
                    .iter()
                    .filter(|r| r.metadata.is_teardown && r.metadata.skip)
                    .count()
            })
            .sum();
        if skip_marked_teardowns > 0 {
            eprintln!(
                "[i] {} 个 @teardown 请求被跳过（--only 未选中或 @skip 标记）——清理阶段不执行",
                skip_marked_teardowns
            );
        }

        let mut parallel_cookie_state: Option<serde_json::Value> = None;

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
                let out = scheduler.run().await?;
                results = out.results;
                parallel_cookie_state = out.final_cookie_state;
            }
            BatchMode::Serial => {
                'outer: for file_path in &request.execution_order {
                    if let Some(parsed_file) = request.files_map.remove(file_path) {
                        let res = match self
                            .executor
                            .execute_all(parsed_file, request.context)
                            .await
                        {
                            Ok(res_list) => res_list,
                            Err(e) => vec![TestResult::error(
                                1,
                                None,
                                "BATCH".to_string(),
                                display_path(file_path),
                                e.to_string(),
                                std::time::Duration::from_secs(0),
                            )],
                        };
                        let has_failure = res.iter().any(|r| !r.success);
                        results.push((file_path.clone(), res));

                        // 每个文件落盘一次：崩溃丢失窗口 = 单个文件
                        self.executor.persist_cookies();

                        if has_failure && request.fail_fast {
                            break 'outer;
                        }
                    }
                }
            }
        }

        // ───── M3 Finally：@teardown 确定性清理 ─────
        // 已实际执行（或已启动）的文件，按执行序的**逆序**执行其 teardown；
        // serial 的 fail_fast break 已由 'outer 标签记录中断点。
        let started: Vec<PathBuf> = results.iter().map(|(p, _)| p.clone()).collect();
        let mut teardown_failure = false;
        // 并行分支若带有捕获的会话 Cookie，只装配临时 teardown 执行器消费，
        // 避免污染主执行器的持久化仓库（守护 cookie_persistence_test 契约 2a/2b）。
        let ephemeral_teardown_executor = if !teardowns_map.is_empty()
            && let Some(cookie_state) = parallel_cookie_state
        {
            Some(ParallelScheduler::build_ephemeral_clone_with_cookies(
                &self.executor,
                Some(cookie_state),
            ))
        } else {
            None
        };
        let teardown_executor = ephemeral_teardown_executor
            .as_ref()
            .unwrap_or(&self.executor);

        for file_path in started.iter().rev() {
            if let Some(teardowns) = teardowns_map.remove(file_path) {
                for (orig_number, req) in teardowns {
                    // P1-9 变量防呆（单次渲染判定）：先渲染副本检残留，
                    // 命中则 skipped（杜绝脏请求外发）；宏双求值限制已在 PRD 声明
                    let mut rendered = req.clone();
                    crate::variable::resolver::VariableResolver::resolve_parsed_request(
                        &mut rendered,
                        request.context,
                    );
                    let unresolved =
                        crate::variable::resolver::VariableResolver::unresolved_placeholders(
                            &rendered,
                        );
                    let name = req.name().map(|s| s.to_string());
                    let method = req.method_or_default().to_string();
                    let url = req.url.clone();

                    let mut result = if !unresolved.is_empty() {
                        let mut r = TestResult::skipped(orig_number, name, method, url);
                        r.warnings.push(format!(
                            "@teardown 跳过：{} 个变量未解析（{:?}）——前置步骤可能失败",
                            unresolved.len(),
                            unresolved
                        ));
                        r
                    } else {
                        let mut r = teardown_executor
                            .execute_one(
                                req,
                                orig_number,
                                request.context,
                                Some(file_path.to_string_lossy().to_string()),
                            )
                            .await;
                        r.is_teardown = true;
                        if !r.success {
                            teardown_failure = true;
                        }
                        r
                    };
                    result.is_teardown = true;

                    // 替换常规阶段占位结果（保持原序号定位）
                    if let Some((_, file_results)) =
                        results.iter_mut().find(|(p, _)| p == file_path)
                        && let Some(slot) = file_results
                            .iter_mut()
                            .find(|r| r.request_number == orig_number)
                    {
                        *slot = result;
                    }
                }
            }
        }
        if teardown_failure {
            eprintln!("[!] @teardown 清理请求执行失败——清理结果已计入失败统计");
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
