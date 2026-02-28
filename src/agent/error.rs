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
