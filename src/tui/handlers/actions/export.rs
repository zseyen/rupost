//! curl 导出动作（R8）：复用 exporter 全管线，与 CLI export 脱敏语义一致。

use super::execution::inject_quick_auth;
use crate::tui::state::AppState;

/// R8：导出当前编辑器请求为 curl 命令。
/// 复用 exporter 全管线（`RequestPreparer::prepare_one`：变量解析 + 注入 + [REDACTED] 脱敏）
/// —— 脱敏语义与 CLI export 完全一致，手拼 ExportableRequest 会绕过 redact 管线。
pub fn export_current_request(state: &mut AppState, redact: bool) {
    let parsed = match crate::parser::parse_content(&state.editor_text) {
        Ok(p) => p,
        Err(e) => {
            state.last_error = Some(format!("HTTP Parser Error: {e}"));
            return;
        }
    };
    let Some(mut req) = parsed.requests.first().cloned() else {
        state.last_error = Some("No request block found in editor.".to_string());
        return;
    };
    inject_quick_auth(&mut req, state.quick_auth());

    let mut ctx = state.variables.clone();
    ctx.insert("__default_scheme", "http");
    let captured: std::collections::HashSet<String> =
        state.variables.variables().keys().cloned().collect();

    let cookie_store = None; // TUI 会话 cookie 在 bridge 中，导出不注入（与 CLI export 默认一致）
    let exportable = match crate::exporter::preparer::RequestPreparer::prepare_one(
        req,
        &mut ctx,
        cookie_store,
        state.editor_file_path.as_deref(),
        redact,
        &captured,
    ) {
        Ok(x) => x,
        Err(e) => {
            state.last_error = Some(format!("export prepare: {e}"));
            return;
        }
    };

    let registry = crate::exporter::plugin::ExporterRegistry::new_builtin();
    let opts = crate::exporter::types::ExportOptions {
        compact: false,
        redact_secrets: redact,
    };
    match registry.find("curl") {
        Some(exporter) => match exporter.export_one(&exportable, &opts) {
            Ok(cmd) => {
                state.show_export = Some(if redact {
                    format!("curl (secrets [REDACTED])\n\n{cmd}")
                } else {
                    format!("curl (raw secrets — do not share)\n\n{cmd}")
                });
                state.last_error = None;
            }
            Err(e) => state.last_error = Some(format!("export failed: {e}")),
        },
        None => state.last_error = Some("curl exporter not registered".to_string()),
    }
}
