// Executor configuration
#![allow(dead_code)]

use crate::executor::types::ExecutionConstraints;
use std::path::PathBuf;

/// Executor configuration
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Default execution constraints
    pub constraints: ExecutionConstraints,
    /// Path to tools.toml configuration file
    pub tools_toml_path: PathBuf,
    /// Shell path for command execution
    pub shell: String,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            constraints: ExecutionConstraints::default(),
            tools_toml_path: PathBuf::from("tools.toml"),
            shell: String::from("/bin/sh"),
        }
    }
}
