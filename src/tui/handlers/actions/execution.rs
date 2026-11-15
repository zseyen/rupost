//! 执行编排动作：快速授权注入、单请求/多请求 streaming 触发（R2/R3/R4/R5/R7）。

use crate::tui::event::{Action, TuiEvent};
use crate::tui::state::{AppState, SidebarTab};
use tokio::sync::mpsc;

/// 快速授权注入（R3 决策表）：
/// - 未设置快速授权 → 原样
/// - 已设置且请求未显式声明 Authorization → 注入
/// - 已设置但请求显式声明了 Authorization → 显式优先，原样
pub(crate) fn inject_quick_auth(req: &mut crate::parser::ParsedRequest, auth: Option<String>) {
    let Some(value) = auth else {
        return;
    };
    let has_explicit = req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("authorization"));
    if !has_explicit {
        req.headers
            .push(("Authorization".to_string(), value.to_string()));
    }
}

/// 触发异步发包测试执行的主控逻辑（R2/R4/R5：多请求 streaming、选择器、cookie 延续）。
/// 共用于 Files 侧边栏 r/R 键与 Editor 区域运行键。
pub async fn trigger_request_execution(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>) {
    if state.is_loading {
        return;
    }
    // R7 互斥：交互会话活跃时禁止剧本执行（连接与变量上下文冲突）
    if state.ws_session.is_some() {
        state.last_error =
            Some("interactive WS session active — press D to close before running".to_string());
        return;
    }
    let parsed_file = if state.active_sidebar_tab == SidebarTab::History {
        // 历史重放：内存中的单请求（不读盘）
        match state.current_request.clone() {
            Some(req) => crate::parser::ParsedFile {
                requests: vec![req],
                ..Default::default()
            },
            None => {
                state.last_response = Some(crate::http::Response::error(
                    "No request in history selection.".to_string(),
                ));
                state.response_visual_lines.clear();
                return;
            }
        }
    } else {
        match crate::parser::parse_content(&state.editor_text) {
            Ok(parsed) => parsed,
            Err(e) => {
                state.last_response = Some(crate::http::Response::error(format!(
                    "HTTP Parser Error: {}",
                    e
                )));
                state.response_visual_lines.clear();
                return;
            }
        }
    };

    // R4：选择器接入（#n / @name，复用 runner 纯函数）
    if let Some(sel) = state.run_selector.clone()
        && !sel.trim().is_empty()
    {
        // 语法归一化：`#3` 与 `3` 等价（# 前缀来自路径片段语法习惯）
        let raw = sel.trim();
        let normalized = raw.strip_prefix('#').unwrap_or(raw);
        let selector = match crate::runner::selector::parse_only_selector(normalized) {
            Ok(s) => s,
            Err(e) => {
                state.last_error = Some(format!("selector '{}': {}", sel, e));
                return;
            }
        };
        let mut file = parsed_file;
        if let Err(e) = crate::runner::selector::apply_selector(&mut file, &selector) {
            state.last_error = Some(format!("selector '{}': {}", sel, e));
            return;
        }
        return trigger_file_execution(state, file, event_tx).await;
    }
    trigger_file_execution(state, parsed_file, event_tx).await;
}

/// 执行已解析文件（单请求与多请求统一走 streaming 路径）
async fn trigger_file_execution(
    state: &mut AppState,
    mut parsed_file: crate::parser::ParsedFile,
    event_tx: &mpsc::Sender<TuiEvent>,
) {
    // R3 快速授权注入：多请求时对每个请求注入（显式 Authorization 优先）
    for req in &mut parsed_file.requests {
        inject_quick_auth(req, state.quick_auth());
    }

    let run_total = parsed_file.requests.len();
    if run_total == 0 {
        state.last_response = Some(crate::http::Response::error(
            "No request block found in editor.".to_string(),
        ));
        state.response_visual_lines.clear();
        return;
    }

    // 状态机兼容：SendRequest 登记当前运行的主请求（is_loading/文件执行状态/缓冲清理）。
    // 选择器模式下登记**选中请求**而非文件首——current_request 供 Ctrl+S 快照等下游语义。
    let primary = parsed_file
        .requests
        .iter()
        .find(|r| !r.should_skip())
        .cloned()
        .or_else(|| parsed_file.requests.first().cloned());
    if let Some(primary) = primary {
        state.update(Action::SendRequest(Box::new(primary)));
    }
    state.run_results.clear();
    state.run_index = 0;
    state.run_total = run_total;
    if run_total > 1 {
        // 多请求：自动切 Results 视图
        state.active_detail_tab = crate::tui::state::DetailTab::Results;
        state.response_visual_lines.clear();
    }

    let mut var_context = state.variables.clone();
    var_context.insert("__default_scheme", "http");

    // R5：cookie 会话延续——桥接上一次运行的导出状态。
    // B-1 组合根收口：交互执行器经共享工厂装配本机插件注册表（免假性"未安装"阻断）
    // B3（WP-2026-11-14-01）：toml [plugins] 段 → 交互执行器（告警走状态栏）
    let (plugin_cfg, plugin_warnings) =
        crate::variable::ConfigLoader::find_and_load_plugin_config();
    for w in &plugin_warnings {
        tracing::warn!("tui plugins config: {w}");
    }
    let mut executor = crate::runner::executor_factory::build_interactive_executor_with_options(
        &crate::plugin::DiscoverOptions {
            allow_project_plugins: plugin_cfg.allow_project_plugins
                || crate::security::fs::env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
            project_root: None,
        },
        &plugin_cfg,
        plugin_cfg.strict_vars,
    );
    executor.debug_on_failure = true; // R7：失败自动诊断（对齐 CLI --debug-on-failure）
    if let Some(state_json) = state
        .cookie_bridge
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        && let Err(e) = executor.import_cookie_state(state_json)
    {
        tracing::warn!("cookie bridge import failed: {}", e);
    }

    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
    let executor = executor.with_stream_sender(stream_tx);
    let tx_clone = event_tx.clone();
    let req_id = uuid::Uuid::new_v4();

    let _ = tx_clone.send(TuiEvent::RequestStarted(req_id)).await;

    let s_tx = event_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = stream_rx.recv().await {
            match event {
                crate::runner::types::StreamEvent::SseChunk(chunk) => {
                    let _ = s_tx
                        .send(TuiEvent::StreamChunk {
                            id: req_id,
                            chunk,
                            total_lines: 0,
                        })
                        .await;
                }
                crate::runner::types::StreamEvent::WsFrame { is_send, content } => {
                    let _ = s_tx
                        .send(TuiEvent::WsFrame {
                            id: req_id,
                            is_send,
                            content,
                            total_lines: 0,
                        })
                        .await;
                }
                crate::runner::types::StreamEvent::InitLogPath(path) => {
                    let _ = s_tx.send(TuiEvent::InitLogPath { id: req_id, path }).await;
                }
            }
        }
    });

    let cookie_bridge = state.cookie_bridge.clone();
    let cookie_file_path = state.cookie_file_path.clone();
    tokio::spawn(async move {
        // 结果回调走无界内部通道（FnMut 同步上下文不能 await），由转发 task 投递
        let (res_tx, mut res_rx) = mpsc::unbounded_channel::<(
            crate::runner::types::TestResult,
            std::collections::HashMap<String, String>,
        )>();
        // B3 strict 门禁：严格模式下降级形态的成功 → 判失败（与 CLI --strict-plugins 一致）
        let strict_plugins = plugin_cfg.strict_plugins;
        let relay = tokio::spawn(async move {
            while let Some((mut res, captured)) = res_rx.recv().await {
                if strict_plugins && res.degraded && res.success {
                    res.success = false;
                    res.warnings.push(
                        "strict_plugins：降级形态判失败（--strict-plugins 语义）".to_string(),
                    );
                }
                let _ = tx_clone
                    .send(TuiEvent::RequestResult {
                        result: Box::new(res),
                        captured_vars: captured,
                        run_total,
                    })
                    .await;
            }
        });
        let results = executor
            .execute_all_streaming(parsed_file, &mut var_context, |res, captured| {
                let _ = res_tx.send((res, captured));
            })
            .await;
        if let Err(e) = results {
            tracing::error!("multi-request execution failed: {}", e);
        }
        drop(res_tx);
        let _ = relay.await;
        // R5：cookie 状态导出到桥（供下次触发 import——executor 为 &self 调用后仍存活）
        // 审查 P1：导出/落盘失败不得静默（备用屏内禁 eprintln，走 tracing，
        // 与 integrations.rs 落盘告警同口径；TUI 上屏管线为后续项）
        match executor.export_cookie_state() {
            Ok(Some(state_json)) => {
                *cookie_bridge.lock().unwrap_or_else(|e| e.into_inner()) = Some(state_json.clone());
                // R13：--cookie-file 提供时同步落盘
                if let Some(path) = &cookie_file_path
                    && let Err(e) = std::fs::write(path, state_json.to_string())
                {
                    tracing::warn!(
                        "{}: {}: {}",
                        crate::middleware::ERR_COOKIE_PERSIST,
                        std::path::Path::new(path).display(),
                        e
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    "{}: 状态导出失败: {}",
                    crate::middleware::ERR_COOKIE_PERSIST,
                    e
                )
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- quick auth 决策表（自 editor.rs quick_auth_tests 迁入）----

    fn req_with_auth(auth: Option<&str>) -> crate::parser::ParsedRequest {
        let mut req = crate::parser::ParsedRequest {
            method: Some("GET".to_string()),
            url: "http://localhost".to_string(),
            headers: vec![],
            body: None,
            metadata: Default::default(),
            line_number: 1,
            base_path: None,
        };
        if let Some(v) = auth {
            req.headers
                .push(("Authorization".to_string(), v.to_string()));
        }
        req
    }

    /// R3 决策表：未设置 / 注入 / 显式优先
    #[test]
    fn test_inject_quick_auth_decision_table() {
        // 未设置快速授权 → 原样
        let mut req = req_with_auth(None);
        inject_quick_auth(&mut req, None);
        assert!(req.headers.is_empty());

        // 已设置 + 无显式声明 → 注入
        let mut req = req_with_auth(None);
        inject_quick_auth(&mut req, Some("Bearer t-1".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].0, "Authorization");
        assert_eq!(req.headers[0].1, "Bearer t-1");

        // 已设置 + 显式声明 → 显式优先，不覆盖
        let mut req = req_with_auth(Some("Bearer explicit"));
        inject_quick_auth(&mut req, Some("Bearer quick".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].1, "Bearer explicit");

        // 大小写不敏感的显式声明识别
        let mut req = req_with_auth(Some("Basic abc"));
        req.headers[0].0 = "authorization".to_string();
        inject_quick_auth(&mut req, Some("Bearer quick".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].1, "Basic abc");
    }

    // ---- trigger 接线守护（自 editor.rs wiring_tests / selector_wiring_tests 迁入）----

    /// 接线守护（R3）：trigger_request_execution 必须调用快速授权注入——
    /// state.quick_auth 已设置时，wiremock 收到的请求携带 Authorization。
    /// 删掉 trigger 内的 inject_quick_auth 调用 → 本测试红。
    #[tokio::test]
    async fn test_trigger_injects_quick_auth_into_real_request() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/secure"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let mut state = AppState::new();
        state.editor_text = format!("### Secured\nGET {}/secure\n", server.uri());
        state.set_quick_auth("Bearer wiring-1");

        let (tx, _rx) = mpsc::channel(8);
        trigger_request_execution(&mut state, &tx).await;

        // 执行 task 异步发包（RequestFinished 事件由 app.rs 主循环消费，测试外无关）
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let received = loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(
                tokio::time::Instant::now() < deadline,
                "request must arrive within 5s window"
            );
            if let Ok(Some(reqs)) = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                server.received_requests(),
            )
            .await
                && !reqs.is_empty()
            {
                break reqs;
            }
        };
        assert!(!received.is_empty());
        let auth = received[0]
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(auth, "Bearer wiring-1", "quick auth must reach the wire");
    }

    /// R4 接线守护：run_selector 生效时仅选中请求收流量——
    /// `#` 前缀归一化（#2 ≡ 2）+ apply_selector 接入。
    #[tokio::test]
    async fn test_trigger_with_selector_hits_only_selected() {
        let server = wiremock::MockServer::start().await;
        for i in 1..=3 {
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path(format!("/r{i}")))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(format!("r{i}")))
                .mount(&server)
                .await;
        }

        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("multi.http");
        std::fs::write(
            &file,
            format!(
                "### one\nGET {}/r1\n\n### two\nGET {}/r2\n\n### three\nGET {}/r3\n",
                server.uri(),
                server.uri(),
                server.uri()
            ),
        )
        .unwrap();

        let mut state = AppState::new();
        state.editor_text = std::fs::read_to_string(&file).unwrap();
        state.editor_file_path = Some(file.to_string_lossy().into_owned());
        state.run_selector = Some("#2".to_string()); // TUI 用户语法

        let (tx, mut rx) = mpsc::channel(64);
        trigger_request_execution(&mut state, &tx).await;

        // 手动消费事件（模拟 app.rs 主循环），驱动 run_results 簿记
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline && state.run_results.is_empty() {
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(TuiEvent::RequestResult {
                    result,
                    captured_vars,
                    run_total,
                })) => {
                    state.handle_request_result(*result, captured_vars, run_total);
                }
                Ok(Some(_)) => continue,
                _ => {}
            }
        }
        // 等全部结果（含 2 个 SKIP——选择器语义：非选中标记 skip）
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline && state.run_results.len() < 3 {
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(TuiEvent::RequestResult {
                    result,
                    captured_vars,
                    run_total,
                })) => {
                    state.handle_request_result(*result, captured_vars, run_total);
                }
                Ok(Some(_)) => continue,
                _ => {}
            }
        }
        assert_eq!(state.run_results.len(), 3, "skipped requests still report");

        // 选中请求（2）真实执行，1/3 SKIP
        assert!(state.run_results[0].skipped, "r1 must be skipped");
        assert!(
            !state.run_results[1].skipped && state.run_results[1].success,
            "r2 must execute: {:?}",
            state.run_results[1].error
        );
        assert!(state.run_results[2].skipped, "r3 must be skipped");

        let received = server.received_requests().await.expect("recorded");
        assert_eq!(received.len(), 1, "exactly one request must hit the wire");
        assert_eq!(received[0].url.path(), "/r2");
    }

    /// R4 守护：非法选择器浮出可读错误、不执行
    #[tokio::test]
    async fn test_trigger_with_out_of_range_selector_surfaces_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut state = AppState::new();
        state.editor_text = format!(
            "### one\nGET {}/r1\n\n### two\nGET {}/r2\n",
            server.uri(),
            server.uri()
        );
        state.run_selector = Some("9".to_string());

        let (tx, _rx) = mpsc::channel(64);
        trigger_request_execution(&mut state, &tx).await;

        assert!(
            state.last_error.as_deref().unwrap().contains("越界"),
            "out-of-range must surface: {:?}",
            state.last_error
        );
        // 无流量
        let received = server.received_requests().await.unwrap_or_default();
        assert!(received.is_empty(), "no traffic on selector error");
    }
}
