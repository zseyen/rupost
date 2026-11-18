//! 统一诊断告警总线（WarningSink）：将核心装配与运行时告警从裸 `eprintln!` 解耦。
//!
//! 警告通道裁决（批三-7，审查批二/接手成本审查收敛）：
//! - `TestResult.warnings` —— **事实通道**（随结果落 JSON/HTML/快照，必经）；
//! - `WarningSink` —— **终端实时渲染通道**（不承载需进报告的事实）；
//! - `tracing::warn!` —— 选项级/装配级提示（测试结果存在之前）；
//! - `eprintln!` —— 仅限表现层（src/cli/**、suite_presenter、main.rs）。
//!
//! 核心层（runner/ai/exporter/middleware 等）禁止裸 eprintln——CI grep 守护
//! 冻结增长（allowlist 见 ci.yml），存量迁移登记架构审查 #13；该守护同时避免
//! TUI Raw Mode 终端撕裂与 Web 端告警静默丢失。

use std::sync::Mutex;

/// 告警严重级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Alert,
    Warn,
    Security,
}

/// CLI 终端前缀格式化纯函数（锁定 `[i]` / `[!]` / `[WARN]` / `[warn]` 契约）。
pub fn format_stderr_line(severity: WarningSeverity, message: &str) -> String {
    match severity {
        WarningSeverity::Info => format!("[i] {message}"),
        WarningSeverity::Alert => format!("[!] {message}"),
        WarningSeverity::Warn => format!("[WARN] {message}"),
        WarningSeverity::Security => format!("[warn] {message}"),
    }
}

/// 单条结构化告警记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningRecord {
    pub severity: WarningSeverity,
    pub source: &'static str,
    pub message: String,
}

/// 跨适配器（CLI / TUI / Web / 测试）统一诊断告警接收端。
pub trait WarningSink: Send + Sync {
    fn emit(&self, severity: WarningSeverity, source: &'static str, message: &str);
}

/// CLI 默认接收端：格式化输出到 `stderr`，保持既有终端前缀语义。
#[derive(Debug, Default, Clone, Copy)]
pub struct StderrWarningSink;

impl WarningSink for StderrWarningSink {
    fn emit(&self, severity: WarningSeverity, _source: &'static str, message: &str) {
        eprintln!("{}", format_stderr_line(severity, message));
    }
}

/// 内存缓冲接收端：供 Web 响应、TUI 状态栏与单元测试确定性捕获告警（零 `stderr` 泄露）。
#[derive(Debug, Default)]
pub struct BufferedWarningSink {
    records: Mutex<Vec<WarningRecord>>,
}

impl BufferedWarningSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回当前已捕获的全部结构化告警副本。
    pub fn records(&self) -> Vec<WarningRecord> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 返回当前已捕获的纯文本消息列表。
    pub fn messages(&self) -> Vec<String> {
        self.records().into_iter().map(|r| r.message).collect()
    }

    /// 取出并清空全部已捕获告警。
    pub fn take_records(&self) -> Vec<WarningRecord> {
        let mut guard = self.records.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    }

    pub fn is_empty(&self) -> bool {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }
}

impl WarningSink for BufferedWarningSink {
    fn emit(&self, severity: WarningSeverity, source: &'static str, message: &str) {
        let mut guard = self.records.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(WarningRecord {
            severity,
            source,
            message: message.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffered_warning_sink_captures_all_severities() {
        let sink = BufferedWarningSink::new();
        assert!(sink.is_empty());

        sink.emit(WarningSeverity::Info, "suite_setup", "backfilled 2 vars");
        sink.emit(WarningSeverity::Alert, "executor_factory", "alert msg");
        sink.emit(WarningSeverity::Warn, "executor_factory", "missing plugin");
        sink.emit(WarningSeverity::Security, "redact", "hard key blocked");

        let recs = sink.records();
        assert_eq!(recs.len(), 4);
        assert_eq!(recs[0].severity, WarningSeverity::Info);
        assert_eq!(recs[0].source, "suite_setup");
        assert_eq!(recs[1].severity, WarningSeverity::Alert);
        assert_eq!(recs[2].severity, WarningSeverity::Warn);
        assert_eq!(recs[3].severity, WarningSeverity::Security);
        assert_eq!(
            sink.messages(),
            vec![
                "backfilled 2 vars",
                "alert msg",
                "missing plugin",
                "hard key blocked"
            ]
        );

        let taken = sink.take_records();
        assert_eq!(taken.len(), 4);
        assert!(sink.is_empty());
    }

    #[test]
    fn test_format_stderr_line_prefixes() {
        assert_eq!(format_stderr_line(WarningSeverity::Info, "ok"), "[i] ok");
        assert_eq!(format_stderr_line(WarningSeverity::Alert, "bad"), "[!] bad");
        assert_eq!(
            format_stderr_line(WarningSeverity::Warn, "warn"),
            "[WARN] warn"
        );
        assert_eq!(
            format_stderr_line(WarningSeverity::Security, "sec"),
            "[warn] sec"
        );
    }

    #[test]
    fn test_buffered_warning_sink_concurrent_emit_and_take() {
        use std::sync::Arc;
        let sink = Arc::new(BufferedWarningSink::new());
        let mut handles = Vec::new();
        for t in 0..8 {
            let s = Arc::clone(&sink);
            handles.push(std::thread::spawn(move || {
                for i in 0..25 {
                    s.emit(WarningSeverity::Warn, "concurrent", &format!("t{t}-{i}"));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let records = sink.take_records();
        assert_eq!(records.len(), 200);
        assert!(sink.is_empty());
    }
}
