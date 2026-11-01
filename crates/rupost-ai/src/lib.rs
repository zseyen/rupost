pub mod attribution;
pub mod client;
pub mod error;
pub mod json_util;
pub mod synthesizer;
pub mod translator;
pub mod triage;

pub use attribution::{AttributionAdvice, FailedAssertionInfo, FailureAttributor, FailureContext};
pub use client::{AiClient, OpenAiClient};
pub use error::{AiError, Result};
pub use synthesizer::{SynthesisAdvice, SynthesisItem, SynthesisOutcome, TestSynthesizer};
pub use translator::AssertionTranslator;
pub use triage::{
    FailureCategory, FailureClassifier, FailureCluster, SignatureBucket, TriageReport,
    bucket_to_deterministic_cluster,
};
