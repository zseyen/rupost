pub mod client;
pub mod diagnose;
pub mod llm_adapter;
pub mod mock;
pub mod request;
pub mod request_builder;
pub mod response;
pub mod stream;
pub mod timing;
pub mod types;

// Re-export commonly used types for convenient access
pub use client::Client;
pub use diagnose::{CertInfo, DiagnosticsReport, diagnose_url, print_diagnose_report};
pub use llm_adapter::{LlmProvider, LlmStreamAdapter};
pub use mock::MockLlmServer;
pub use request::Request;
pub use request_builder::{to_request, try_from_parsed_with_warnings};
pub use response::Response;
pub use stream::{SseEvent, SseParser};
pub use timing::{DiagnosticsProber, NetworkLatency, RequestTiming};
pub mod url;
pub use url::resolve_final_url;

use crate::Result;

/// 执行网络诊断命令的机制层函数
pub async fn run_diagnose(url: &str, report: &str) -> Result<()> {
    match diagnose_url(url).await {
        Ok(report_data) => {
            if report == "json" {
                println!("{}", serde_json::to_string_pretty(&report_data).unwrap());
            } else {
                print_diagnose_report(&report_data);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
