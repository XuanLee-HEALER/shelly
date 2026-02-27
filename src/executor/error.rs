// Error types for Executor module
#![allow(dead_code)]

use thiserror::Error;

/// Executor error types
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Invalid input for tool '{0}': {1}")]
    InvalidInput(String, String),

    #[error("Failed to spawn process for tool '{0}': {1}")]
    SpawnFailed(String, String),

    #[error("Execution timeout for tool '{0}' after {1} seconds")]
    Timeout(String, u64),

    #[error("Failed to capture output for tool '{0}': {1}")]
    OutputCaptureFailed(String, String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, ExecutorError>;
