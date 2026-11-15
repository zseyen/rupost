#![allow(clippy::collapsible_if)]
use crate::http::Client;
use crate::http::url::resolve_final_url;
use crate::middleware::{CookieMiddleware, MiddlewarePipeline};
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::WsRunner;
use crate::runner::observer::ExecutionObserver;
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver};
use crate::{Result, RupostError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub enum Runner {
    Http(crate::runner::HttpRunner),
    Ws(WsRunner),
}

impl Runner {
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
        middlewares: &MiddlewarePipeline,
        observer: &Arc<dyn ExecutionObserver>,
        stream_sender: Option<
            tokio::sync::mpsc::UnboundedSender<crate::runner::types::StreamEvent>,
        >,
        debug: bool,
        debug_on_failure: bool,
        client: &Client,
        source: Option<String>,
    ) -> Result<TestResult> {
        match self {
            Self::Http(r) => {
                r.run(
                    parsed,
                    request_number,
                    context,
                    middlewares,
                    observer,
                    stream_sender,
                    debug,
                    debug_on_failure,
                    client,
                    source,
                )
                .await
            }
            Self::Ws(r) => {
                r.run(
                    parsed,
                    request_number,
                    context,
                    middlewares,
                    observer,
                    stream_sender,
                    debug,
                    debug_on_failure,
                    client,
                    source,
                )
                .await
            }
        }
    }
}

pub struct TestExecutor {
    client: Client,
    cookie_middleware: Option<Arc<CookieMiddleware>>,
    routing_middleware: Option<Arc<crate::middleware::routing::RoutingMiddleware>>,
    middlewares: MiddlewarePipeline,
    observer: Arc<dyn ExecutionObserver>,
    pub debug: bool,
    pub debug_on_failure: bool,
    /// E-4（免重填）：`--strict-vars`——未解析 `{{var}}` 拒绝外发（默认 false 维持
    /// 字面量发送 + 事后警告的既有语义），防漏配变量打注定失败的请求
    pub strict_vars: bool,
    pub stream_sender: Option<tokio::sync::mpsc::UnboundedSender<super::types::StreamEvent>>,
    /// 插件注册表（M3：`--save-snapshot`/插件注解存在时由 app 层装配；None 时由
    /// `executor_plugin` 回退空注册表进入管线，防止绕过严格门禁）。
    pub plugin_registry: Option<Arc<crate::plugin::PluginRegistry>>,
    pub plugin_options: crate::plugin::pipeline::PluginRuntimeOptions,
    pub suite_hook_cache: Arc<crate::plugin::pipeline::SuiteHookCache>,
}

impl TestExecutor {
    /// Check if the executor has cookie support enabled.
    pub fn has_cookies(&self) -> bool {
        self.cookie_middleware.is_some()
    }

    /// 导出当前执行器的 Cookie 状态为 JSON Value。
    /// 未启用 cookie → Ok(None)；启用但序列化失败 → Err（审查 P1：此前 `.ok()`
    /// 把序列化失败与"未启用"混同，调用方静默跳过落盘/传递）
    pub fn export_cookie_state(&self) -> Result<Option<serde_json::Value>> {
        match self.cookie_middleware.as_ref() {
            Some(mw) => Ok(Some(mw.export_cookie_state()?)),
            None => Ok(None),
        }
    }

    /// 将 Cookie 状态导入到当前的执行器中
    pub fn import_cookie_state(&self, state: serde_json::Value) -> Result<()> {
        if let Some(ref mw) = self.cookie_middleware {
            mw.import_cookie_state(state)?;
        }
        Ok(())
    }

    /// 将持久化模式的 Cookie 落盘。由 BatchExecutor 在每个测试文件执行完后调用，
    /// 使崩溃时的丢失窗口为一个文件而不是整个批次；并行模式（worker 均为临时
    /// 仓库）不调用。Drop 时仍有兜底保存。
    pub fn persist_cookies(&self) {
        if let Some(mw) = &self.cookie_middleware
            && let Err(e) = mw.save()
        {
            tracing::warn!("{}: {}", crate::middleware::ERR_COOKIE_PERSIST, e);
        }
    }

    /// Create a new executor without cookie support.
    pub fn new() -> Self {
        Self {
            client: Client::new(None),
            cookie_middleware: None,
            routing_middleware: None,
            middlewares: MiddlewarePipeline::new(),
            observer: Arc::new(crate::history::recorder::HistoryObserver),
            debug: false,
            debug_on_failure: false,
            strict_vars: false,
            stream_sender: None,
            plugin_registry: None,
            plugin_options: crate::plugin::pipeline::PluginRuntimeOptions::default(),
            suite_hook_cache: Arc::new(crate::plugin::pipeline::SuiteHookCache::new()),
        }
    }

    /// 装配插件注册表与运行时选项（app 层在 discover 后调用）
    pub fn with_plugins(
        mut self,
        registry: Arc<crate::plugin::PluginRegistry>,
        options: crate::plugin::pipeline::PluginRuntimeOptions,
    ) -> Self {
        self.plugin_registry = Some(registry);
        self.plugin_options = options;
        self
    }

    /// 并行调度器共享插件注册表、运行配置与同一个 Suite 级并发缓存。
    pub(crate) fn with_shared_plugins(
        mut self,
        registry: Option<Arc<crate::plugin::PluginRegistry>>,
        options: crate::plugin::pipeline::PluginRuntimeOptions,
        suite_hook_cache: Arc<crate::plugin::pipeline::SuiteHookCache>,
    ) -> Self {
        self.plugin_registry = registry;
        self.plugin_options = options;
        self.suite_hook_cache = suite_hook_cache;
        self
    }

    /// Inject a custom [`ExecutionObserver`] (e.g., `NoopExecutionObserver` in tests).
    pub fn with_observer(mut self, observer: Arc<dyn ExecutionObserver>) -> Self {
        self.observer = observer;
        self
    }

    pub fn with_middleware(
        mut self,
        middleware: Arc<crate::middleware::routing::RoutingMiddleware>,
    ) -> Self {
        self.routing_middleware = Some(middleware.clone());
        self.middlewares.push_arc(middleware);
        self
    }

    /// Set debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set debug-on-failure mode
    pub fn with_debug_on_failure(mut self, debug_on_failure: bool) -> Self {
        self.debug_on_failure = debug_on_failure;
        self
    }

    /// E-4：`--strict-vars`——未解析 `{{var}}` 拒绝外发
    pub fn with_strict_vars(mut self, strict_vars: bool) -> Self {
        self.strict_vars = strict_vars;
        self
    }

    /// Set stream_sender for long-connection notifications
    pub fn with_stream_sender(
        mut self,
        sender: tokio::sync::mpsc::UnboundedSender<super::types::StreamEvent>,
    ) -> Self {
        self.stream_sender = Some(sender);
        self
    }

    /// Create a new executor with cookie persistence.
    ///
    /// # Arguments
    /// * `cookie_file` - Path to the cookie storage file.
    pub fn with_cookies(cookie_file: PathBuf) -> Result<Self> {
        let middleware = CookieMiddleware::new_with_persistence(cookie_file)?;
        let cookie_store = middleware.cookie_store();
        let cookie_middleware = Arc::new(middleware);
        Ok(Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(cookie_middleware.clone()),
            routing_middleware: None,
            middlewares: {
                let mut pipeline = MiddlewarePipeline::new();
                pipeline.push_arc(cookie_middleware);
                pipeline
            },
            observer: Arc::new(crate::history::recorder::HistoryObserver),
            debug: false,
            debug_on_failure: false,
            strict_vars: false,
            stream_sender: None,
            plugin_registry: None,
            plugin_options: crate::plugin::pipeline::PluginRuntimeOptions::default(),
            suite_hook_cache: Arc::new(crate::plugin::pipeline::SuiteHookCache::new()),
        })
    }

    /// Create a new executor with ephemeral (in-memory only) cookies.
    pub fn with_ephemeral_cookies() -> Self {
        let middleware = CookieMiddleware::new_ephemeral();
        let cookie_store = middleware.cookie_store();
        let cookie_middleware = Arc::new(middleware);
        Self {
            client: Client::with_cookie_store(cookie_store, None),
            cookie_middleware: Some(cookie_middleware.clone()),
            routing_middleware: None,
            middlewares: {
                let mut pipeline = MiddlewarePipeline::new();
                pipeline.push_arc(cookie_middleware);
                pipeline
            },
            observer: Arc::new(crate::history::recorder::HistoryObserver),
            debug: false,
            debug_on_failure: false,
            strict_vars: false,
            stream_sender: None,
            plugin_registry: None,
            plugin_options: crate::plugin::pipeline::PluginRuntimeOptions::default(),
            suite_hook_cache: Arc::new(crate::plugin::pipeline::SuiteHookCache::new()),
        }
    }

    /// 批量执行所有请求（TUI 流式版）：逐请求完成即回调 `on_result`
    /// （回调附带走完级联后的变量快照），返回全量结果。
    pub async fn execute_all_streaming(
        &self,
        parsed_file: ParsedFile,
        context: &mut VariableContext,
        mut on_result: impl FnMut(TestResult, std::collections::HashMap<String, String>),
    ) -> Result<Vec<TestResult>> {
        let mut results = Vec::new();

        // Determine source once for the file
        let source = parsed_file
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        // 判断该文件是否包含任何显式标记了 @test 的用例块
        let has_explicit_tests = parsed_file.requests.iter().any(|r| r.metadata.is_test);

        for (index, parsed_request) in parsed_file.requests.into_iter().enumerate() {
            let request_number = index + 1;

            // 检查是否跳过
            // M3：@teardown 请求不在常规阶段执行（占位 skipped 保持原序号；
            // 真实执行在 batch 层 Finally 阶段按逆拓扑序进行）
            let should_skip = parsed_request.should_skip()
                || parsed_request.metadata.is_teardown
                || (has_explicit_tests && !parsed_request.metadata.is_test);

            let is_teardown_req = parsed_request.metadata.is_teardown;
            let result = if should_skip {
                let mut r = TestResult::skipped(
                    request_number,
                    parsed_request.name().map(|s| s.to_string()),
                    parsed_request.method_or_default().to_string(),
                    parsed_request.url.clone(),
                );
                // M3：teardown 占位结果也标记（终端/报告可区分；P1-11）
                if is_teardown_req {
                    r.is_teardown = true;
                }
                r
            } else {
                self.execute_one(
                    parsed_request,
                    request_number,
                    context,
                    Some(source.clone()),
                )
                .await
            };

            let captured = context.variables().clone();
            on_result(result.clone(), captured);
            results.push(result);
        }

        Ok(results)
    }

    /// 批量执行所有请求
    pub async fn execute_all(
        &self,
        parsed_file: ParsedFile,
        context: &mut VariableContext,
    ) -> Result<Vec<TestResult>> {
        self.execute_all_streaming(parsed_file, context, |_, _| {})
            .await
    }

    /// 执行单个请求
    pub async fn execute_one(
        &self,
        mut parsed: ParsedRequest,
        request_number: usize,
        context: &mut VariableContext,
        source: Option<String>,
    ) -> TestResult {
        // 开始计时
        let start = Instant::now();

        // 1. 变量替换
        VariableResolver::resolve_parsed_request(&mut parsed, context);

        // 检测渲染后仍残留的占位符（未定义变量），收集为可见警告避免静默假通过
        let unresolved = VariableResolver::unresolved_placeholders(&parsed);
        // E-4：--strict-vars 下未解析变量 → 拒绝外发（可读错误指名变量）。
        // P2-5（review）：base_url/baseUrl 除外——交由下方 BaseUrlNotConfigured
        // 专用分支（错误形态/处方一致：--env/--var base_url）
        let strict_unresolved: Vec<&String> = unresolved
            .iter()
            .filter(|v| v.as_str() != "base_url" && v.as_str() != "baseUrl")
            .collect();
        if self.strict_vars && !strict_unresolved.is_empty() {
            return TestResult::error(
                request_number,
                parsed.name().map(|s| s.to_string()),
                parsed.method_or_default().to_string(),
                parsed.url.clone(),
                format!(
                    "变量未解析（--strict-vars 拒绝外发；合法 {{{{}}}} 字面量亦会被拦截，请酌情关闭）: {}——请补齐 --var/.env/rupost.toml 或 --var-state",
                    strict_unresolved
                        .iter()
                        .map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                start.elapsed(),
            );
        }

        // 检查未配置的 base_url/baseUrl 占位符
        if parsed.url.contains("{{base_url}}") || parsed.url.contains("{{baseUrl}}") {
            let method = parsed.method_or_default().to_string();
            let url = parsed.url.clone();
            let name = parsed.name().map(|s| s.to_string());
            return TestResult::error(
                request_number,
                name,
                method,
                url,
                RupostError::BaseUrlNotConfigured.to_user_friendly_string(),
                start.elapsed(),
            );
        }

        // 2. 解析最终 URL 并进行补全和拼接
        let default_scheme = context
            .get("__default_scheme")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "http".to_string());

        match resolve_final_url(
            &parsed.url,
            context,
            &default_scheme,
            source.as_deref(),
            parsed.base_path.as_deref(),
        ) {
            Ok(resolved) => {
                parsed.url = resolved;
            }
            Err(e) => {
                let method = parsed.method_or_default().to_string();
                let url = parsed.url.clone();
                let name = parsed.name().map(|s| s.to_string());
                return TestResult::error(
                    request_number,
                    name,
                    method,
                    url,
                    e.to_user_friendly_string(),
                    start.elapsed(),
                );
            }
        }

        let runner = if parsed.metadata.websocket {
            Runner::Ws(WsRunner)
        } else {
            Runner::Http(crate::runner::HttpRunner)
        };

        let method = parsed.method_or_default().to_string();
        let url = parsed.url.clone();
        let name = parsed.name().map(|s| s.to_string());

        // 2.5 插件管线（03 §5.1 阶段不可逆流；03 §2 统一降级语义）：
        // 变量解析后、发包前执行；即使注册表未装配也不旁路，早退路径共享统一诊断收口。
        let (parsed, diag) = match crate::runner::executor_plugin::run_plugin_stage(
            parsed,
            request_number,
            context,
            crate::runner::executor_plugin::PluginContext {
                registry: self.plugin_registry.as_ref(),
                options: &self.plugin_options,
                suite_hook_cache: &self.suite_hook_cache,
            },
            &unresolved,
            start,
        )
        .await
        {
            crate::runner::executor_plugin::PluginDispatchOutcome::Proceed(req, diag) => {
                (*req, diag)
            }
            crate::runner::executor_plugin::PluginDispatchOutcome::EarlyReturn(early) => {
                return *early;
            }
        };

        let result = match runner
            .run(
                parsed,
                request_number,
                context,
                &self.middlewares,
                &self.observer,
                self.stream_sender.clone(),
                self.debug,
                self.debug_on_failure,
                &self.client,
                source,
            )
            .await
        {
            Ok(res) => res,
            Err(e) => TestResult::error(
                request_number,
                name,
                method,
                url,
                e.to_user_friendly_string(),
                start.elapsed(),
            ),
        };
        crate::runner::executor_plugin::finalize_plugin_result(result, unresolved, diag)
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}
