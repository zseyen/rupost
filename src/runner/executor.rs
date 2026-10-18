#![allow(clippy::collapsible_if)]
use crate::http::url::resolve_final_url;
use crate::http::{Client, Request, Response};
use crate::middleware::{CookieMiddleware, Middleware};
use crate::parser::{ParsedFile, ParsedRequest};
use crate::runner::WsRunner;
use crate::runner::types::TestResult;
use crate::variable::{VariableContext, VariableResolver};
use crate::{Result, RupostError};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub enum ExecutorMiddleware {
    Cookie(Arc<CookieMiddleware>),
    Routing(Arc<crate::middleware::routing::RoutingMiddleware>),
}

impl Middleware for ExecutorMiddleware {
    async fn before_request(&self, req: &mut Request) -> Result<()> {
        match self {
            Self::Cookie(mw) => mw.before_request(req).await,
            Self::Routing(mw) => mw.before_request(req).await,
        }
    }

    async fn after_response(&self, resp: &Response) -> Result<()> {
        match self {
            Self::Cookie(mw) => mw.after_response(resp).await,
            Self::Routing(mw) => mw.after_response(resp).await,
        }
    }
}

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
        middlewares: &[ExecutorMiddleware],
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
    middlewares: Vec<ExecutorMiddleware>,
    pub debug: bool,
    pub debug_on_failure: bool,
    pub stream_sender: Option<tokio::sync::mpsc::UnboundedSender<super::types::StreamEvent>>,
}

impl TestExecutor {
    /// Check if the executor has cookie support enabled.
    pub fn has_cookies(&self) -> bool {
        self.cookie_middleware.is_some()
    }

    /// 导出当前执行器的 Cookie 状态为 JSON Value (如果启用了 Cookie)
    pub fn export_cookie_state(&self) -> Option<serde_json::Value> {
        self.cookie_middleware
            .as_ref()
            .and_then(|mw| mw.export_cookie_state().ok())
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
            tracing::warn!("Failed to persist cookies: {}", e);
        }
    }

    /// Create a new executor without cookie support.
    pub fn new() -> Self {
        Self {
            client: Client::new(None),
            cookie_middleware: None,
            routing_middleware: None,
            middlewares: Vec::new(),
            debug: false,
            debug_on_failure: false,
            stream_sender: None,
        }
    }

    pub fn with_middleware(
        mut self,
        middleware: Arc<crate::middleware::routing::RoutingMiddleware>,
    ) -> Self {
        self.routing_middleware = Some(middleware.clone());
        self.middlewares
            .push(ExecutorMiddleware::Routing(middleware));
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
            middlewares: vec![ExecutorMiddleware::Cookie(cookie_middleware)],
            debug: false,
            debug_on_failure: false,
            stream_sender: None,
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
            middlewares: vec![ExecutorMiddleware::Cookie(cookie_middleware)],
            debug: false,
            debug_on_failure: false,
            stream_sender: None,
        }
    }

    /// 批量执行所有请求
    pub async fn execute_all(
        &self,
        parsed_file: ParsedFile,
        context: &mut VariableContext,
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
            let should_skip = parsed_request.should_skip()
                || (has_explicit_tests && !parsed_request.metadata.is_test);

            if should_skip {
                results.push(TestResult::skipped(
                    request_number,
                    parsed_request.name().map(|s| s.to_string()),
                    parsed_request.method_or_default().to_string(),
                    parsed_request.url.clone(),
                ));
                continue;
            }

            let result = self
                .execute_one(
                    parsed_request,
                    request_number,
                    context,
                    Some(source.clone()),
                )
                .await;
            results.push(result);
        }

        Ok(results)
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

        let mut result = match runner
            .run(
                parsed,
                request_number,
                context,
                &self.middlewares,
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
        result.warnings.extend(unresolved);
        result
    }
}

impl Default for TestExecutor {
    fn default() -> Self {
        Self::new()
    }
}
