//! 统一诊断告警总线（WarningSink）：将核心装配与运行时告警从裸 `eprintln!` 解耦，
//! 避免 TUI Raw Mode 终端撕裂与 Web 端告警静默丢失。

use std::sync::Mutex;

/// 告警严重级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Warn,
    Security,
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
        match severity {
            WarningSeverity::Info => eprintln!("[i] {message}"),
            WarningSeverity::Warn => eprintln!("[WARN] {message}"),
            WarningSeverity::Security => eprintln!("[warn] {message}"),
        }
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
        sink.emit(WarningSeverity::Warn, "executor_factory", "missing plugin");
        sink.emit(WarningSeverity::Security, "redact", "hard key blocked");

        let recs = sink.records();
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].severity, WarningSeverity::Info);
        assert_eq!(recs[0].source, "suite_setup");
        assert_eq!(recs[1].severity, WarningSeverity::Warn);
        assert_eq!(recs[2].severity, WarningSeverity::Security);
        assert_eq!(
            sink.messages(),
            vec!["backfilled 2 vars", "missing plugin", "hard key blocked"]
        );

        let taken = sink.take_records();
        assert_eq!(taken.len(), 3);
        assert!(sink.is_empty());
    }
}
