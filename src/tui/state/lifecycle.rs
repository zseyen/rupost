//! 归约域：单向状态机 `update()` 动作归约与多请求结果归约。
//!
//! 从 state/mod.rs 外科手术式迁出（解耦提案 v2 Phase 1）。
//! 依赖方向：`handle_request_result` 委托 `stream.rs::handle_request_finished`
//! （lifecycle → stream，单向无环）。

use super::{AppState, DetailTab, FileExecState, SidebarTab};
use crate::http::Response;
use crate::tui::event::Action;

impl AppState {
    /// CLI 能力接入（R2/R6/R7）：消费单个请求结果——
    /// 复用既有 finished 语义（last_response/assertions/warnings/cookie 状态机），
    /// 叠加多请求簿记（run_results/run_index）与 timing/diagnose 暂存。
    /// is_loading 在最后一个结果到达时才落 false（多请求运行期保持互斥）。
    pub fn handle_request_result(
        &mut self,
        result: crate::runner::types::TestResult,
        captured_vars: std::collections::HashMap<String, String>,
        run_total: usize,
    ) {
        self.run_total = run_total;
        // skipped 结果短路：不产生响应/不登记文件执行状态/不触发 finished 副作用
        // （success=true 且 response=None 的 skip 结果若走 conv 会被误转为伪造错误）
        if result.skipped {
            self.run_results.push(result);
            self.run_index = self.run_results.len();
            self.is_loading = self.run_index < self.run_total;
            return;
        }
        let conv: Result<Response, String> =
            if result.success {
                result.response.clone().map(Ok).unwrap_or_else(|| {
                    Err("request succeeded but no response returned".to_string())
                })
            } else {
                Err(result
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unknown execution error".to_string()))
            };
        self.handle_request_finished(
            conv,
            captured_vars,
            result.assertions.clone(),
            result.warnings.clone(),
        );
        self.last_timing = result.timing.clone();
        self.last_diagnose = result.diagnose_report.clone();
        let failed = !result.success;
        self.run_results.push(result);
        self.run_index = self.run_results.len();
        self.is_loading = self.run_index < self.run_total;
        // 失败自动切 Diagnose tab（R7：对齐 CLI --debug-on-failure 的失败即诊语义）
        if failed && self.last_diagnose.is_some() {
            self.active_detail_tab = DetailTab::Diagnose;
            self.response_visual_lines.clear();
        }
    }

    /// 单向状态机更新
    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.is_quitting = true;
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::SwitchPanel(panel) => {
                self.active_panel = panel;
            }
            Action::SendRequest(req) => {
                self.current_request = Some(*req);
                self.is_loading = true;
                self.loading_tick = 0;
                self.sse_stream_body.clear();
                self.ws_frames.clear();
                self.warnings.clear();
                self.response_scroll = 0;
                self.response_visual_lines.clear();
                self.sse_visual_lines.clear();
                self.sse_last_processed_pos = 0;
                self.ws_visual_lines.clear();
                // 滑窗域随新请求复位：旧日志路径/行数/视口缓存不得串入本次运行
                // （log_file_path 由本次运行的 InitLogPath 事件重新挂载）
                self.log_file_path = None;
                self.total_log_lines = 0;
                self.viewport_cache.clear();
                self.cache_start_line = 0;
                self.cache_end_line = 0;

                if let Some(ref path) = self.editor_file_path {
                    if self.active_sidebar_tab == SidebarTab::Files {
                        self.file_execution_states
                            .insert(path.clone(), FileExecState::Running);
                    }
                }
            }
        }
    }
}
