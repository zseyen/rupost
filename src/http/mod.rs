pub mod client;
pub mod diagnose;
pub mod llm_adapter;
pub mod mock;
pub mod request;
pub mod request_builder;
pub mod response;
pub mod stream;
pub mod timing;
pub mod timing_connector;
pub mod types;

// Re-export commonly used types for convenient access
pub use client::Client;
pub use diagnose::{
    CertInfo, DiagnosticsReport, diagnose_url, diagnose_url_lenient, print_diagnose_report,
};
pub use llm_adapter::{LlmProvider, LlmStreamAdapter};
pub use mock::MockLlmServer;
pub use request::Request;
pub use request_builder::{to_request, try_from_parsed_with_warnings};
pub use response::Response;
pub use stream::{SseEvent, SseParser};
pub use timing::{
    DiagnosticsProber, NetworkLatency, RequestTiming, TimingAttribution, TimingSource,
};
pub use timing_connector::{ConnectSpan, TimingConnectorLayer, TimingDnsResolver};
pub use types::{Method, Status, Url};
pub mod url;
pub use url::resolve_final_url;

use crate::Result;

/// 执行网络诊断命令的机制层函数
pub async fn run_diagnose(url: &str, report: &str) -> Result<()> {
    let report_data = diagnose_url(url)
        .await
        .map_err(crate::RupostError::NetworkError)?;
    if report.eq_ignore_ascii_case("json") {
        println!("{}", serde_json::to_string_pretty(&report_data)?);
    } else {
        print_diagnose_report(&report_data);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_diagnose_invalid_url_returns_err() {
        let res = run_diagnose("not a valid url ://", "terminal").await;
        assert!(res.is_err(), "无效 URL 应返回 Err 而非退出进程");
    }
}
