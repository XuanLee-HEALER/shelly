// Agent errors

use thiserror::Error;

/// Agent errors
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Inference error: {0}")]
    Inference(String),

    #[error("Request build error: {0}")]
    RequestBuild(&'static str),

    #[error("Timeout after {0}s")]
    Timeout(u64),
}

/// Inference loop errors
#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("Max tool rounds ({max_rounds}) exceeded, reached {actual_rounds} rounds")]
    MaxToolRounds { max_rounds: u32, actual_rounds: u32 },

    #[error("Inference failed: {0}")]
    InferenceFailed(String),

    #[error("Request build error: {0}")]
    RequestBuild(&'static str),
}
