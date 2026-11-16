pub mod warning_sink;

pub use warning_sink::{
    BufferedWarningSink, StderrWarningSink, WarningRecord, WarningSeverity, WarningSink,
};
