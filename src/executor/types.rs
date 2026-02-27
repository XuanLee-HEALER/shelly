// Data types for Executor module
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Output from a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The text content from execution (stdout/stderr combined)
    pub content: String,
    /// Whether the execution resulted in an error (non-zero exit code)
    #[serde(default)]
    pub is_error: bool,
}

impl ToolOutput {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Constraints for a single execution
#[derive(Debug, Clone)]
pub struct ExecutionConstraints {
    /// Maximum execution time in seconds
    pub timeout_secs: u64,
    /// Maximum output size in bytes (stdout + stderr)
    pub max_output_bytes: usize,
    /// Working directory for execution
    pub working_dir: Option<std::path::PathBuf>,
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_output_bytes: 1048576, // 1MB
            working_dir: None,
        }
    }
}
