pub mod app;
pub mod executor;
mod executor_plugin;
pub mod observer;
pub mod on_failure;
pub mod selector;
mod suite_setup;
pub mod types;

pub mod engine;
pub mod protocol;
pub mod utils;

pub use app::{TestSuiteOptions, TestSuiteOutcome, execute_test_suite, run_test_suite};
pub use executor::TestExecutor;
pub use observer::{ExecutionObserver, NoopExecutionObserver};
pub use types::{TestResult, TestSummary};

pub use engine::batch::{BatchExecutor, BatchMode, BatchRunRequest};
pub use engine::replayer::{ReplayEntryResult, ReplayExecutor, ReplayReport};
pub use engine::resolver::DependencyResolver;
pub use engine::workflow::WorkflowGraph;

pub use protocol::http_runner::HttpRunner;
pub use protocol::sse_runner::{SseRunner, SseRunnerOptions};
pub use protocol::ws_runner::WsRunner;

pub use utils::file_sync::FileSyncWriter;
pub use utils::path::{self, display_path};

pub use crate::history::gc::{perform_clear, perform_prune};
pub use crate::http::url;
pub use crate::parser::scanner::DirectoryScanner;
pub use crate::reporter::TestReporter;
