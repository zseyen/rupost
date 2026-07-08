use crate::assertion::AssertionResult;
use crate::http::Response;
use std::time::Duration;

/// 长连接（SSE/WebSocket）流式事件
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// SSE 实时数据块
    SseChunk(String),
    /// WebSocket 实时收发帧
    WsFrame {
        /// 是否为发送帧
        is_send: bool,
        /// 帧文本内容
        content: String,
    },
    /// 初始化日志物理路径（用于 TUI 视口滑动窗口 seek 加载）
    InitLogPath(String),
}

/// 单个请求的执行结果
#[derive(Debug, Clone)]
pub struct TestResult {
    /// 请求序号（从 1 开始）
    pub request_number: usize,

    /// 请求名称（来自 @name 或自动生成）
    pub name: Option<String>,

    /// HTTP 方法
    pub method: String,

    /// 请求 URL
    pub url: String,

    /// 响应状态码（如果成功）
    pub status: Option<u16>,

    /// 执行耗时
    pub duration: Duration,

    /// 是否成功
    pub success: bool,

    /// 错误消息（如果失败）
    pub error: Option<String>,

    /// 完整的 HTTP 响应（用于详细输出）
    pub response: Option<Response>,

    /// 是否被跳过
    pub skipped: bool,

    /// 断言结果列表
    pub assertions: Vec<AssertionResult>,

    /// 细粒度网络时序诊断
    pub timing: Option<crate::http::timing::RequestTiming>,

    /// 深度网络诊断报告
    pub diagnose_report: Option<crate::http::DiagnosticsReport>,

    /// 完整的 HTTP 请求快照（用于录制重放）
    pub request: Option<crate::history::RequestSnapshot>,
}

impl TestResult {
    pub fn success(
        request_number: usize,
        name: Option<String>,
        method: String,
        url: String,
        response: Response,
    ) -> Self {
        let status = response.status.code();
        let duration = response.duration;
        let success = response.is_success();

        Self {
            request_number,
            name,
            method,
            url,
            status: Some(status),
            duration,
            success,
            error: None,
            response: Some(response),
            skipped: false,
            assertions: Vec::new(),
            timing: None,
            diagnose_report: None,
            request: None,
        }
    }

    pub fn error(
        request_number: usize,
        name: Option<String>,
        method: String,
        url: String,
        error: String,
        duration: Duration,
    ) -> Self {
        Self {
            request_number,
            name,
            method,
            url,
            status: None,
            duration,
            success: false,
            error: Some(error),
            response: None,
            skipped: false,
            assertions: Vec::new(),
            timing: None,
            diagnose_report: None,
            request: None,
        }
    }

    pub fn skipped(
        request_number: usize,
        name: Option<String>,
        method: String,
        url: String,
    ) -> Self {
        Self {
            request_number,
            name,
            method,
            url,
            status: None,
            duration: Duration::from_secs(0),
            success: true, // 跳过的测试算作成功
            error: None,
            response: None,
            skipped: true,
            assertions: Vec::new(),
            timing: None,
            diagnose_report: None,
            request: None,
        }
    }
}

/// 测试摘要
#[derive(Debug, Clone)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration: Duration,
    pub total_assertions: usize,
    pub passed_assertions: usize,
    pub failed_assertions: usize,
}

impl TestSummary {
    pub fn from_results(results: &[TestResult]) -> Self {
        let passed = results.iter().filter(|r| r.success && !r.skipped).count();
        let skipped = results.iter().filter(|r| r.skipped).count();
        let total_duration = results.iter().map(|r| r.duration).sum();

        // 统计断言
        let total_assertions = results.iter().map(|r| r.assertions.len()).sum();
        let passed_assertions = results
            .iter()
            .flat_map(|r| &r.assertions)
            .filter(|a| a.passed)
            .count();
        let failed_assertions = results
            .iter()
            .flat_map(|r| &r.assertions)
            .filter(|a| !a.passed)
            .count();

        Self {
            total: results.len(),
            passed,
            failed: results.len() - passed - skipped,
            skipped,
            total_duration,
            total_assertions,
            passed_assertions,
            failed_assertions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_all_passed() {
        let results = vec![
            TestResult::error(
                1,
                None,
                "GET".to_string(),
                "http://example.com".to_string(),
                "error".to_string(),
                Duration::from_millis(100),
            ),
            TestResult::error(
                2,
                None,
                "POST".to_string(),
                "http://example.com".to_string(),
                "error".to_string(),
                Duration::from_millis(200),
            ),
        ];

        let summary = TestSummary::from_results(&results);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.passed, 0);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.total_duration, Duration::from_millis(300));
    }
}
