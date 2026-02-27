// Error types for Brain module

use thiserror::Error;

/// Runtime errors from Brain
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum BrainError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),

    #[error("Exhausted: max retries ({retries}) exceeded, last error: {last_error}")]
    Exhausted { retries: u32, last_error: String },

    #[error("Model error: {0}")]
    ModelError(String),

    #[error("Timeout after {0} seconds")]
    Timeout(u64),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Initialization errors for Brain
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum BrainInitError {
    #[error("Configuration missing: {0}")]
    ConfigMissing(String),

    #[error("Invalid configuration: {0}")]
    ConfigInvalid(String),

    #[error("Failed to create HTTP client: {0}")]
    ClientError(#[from] reqwest::Error),

    #[error("Connection check failed: {0}")]
    ConnectionFailed(String),
}
