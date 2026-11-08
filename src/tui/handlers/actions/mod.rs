//! 独立业务动作中心（解耦提案 v2 Phase 2）：编辑器/侧栏按键委托的业务动作。
//! 各动作自包含、可离屏测试；既有先例：`V`/`M` 键委托 `tui::integrations`。

pub mod execution;
pub mod export;
pub mod generator;
pub mod snapshot;
pub mod ws_session;
