// Bash tool implementation
#![allow(dead_code)]

use crate::brain::ToolDefinition;
use crate::executor::{ExecutorError, Result, ToolImpl, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Instant;
use tokio::process::Command;
use tracing::{debug, info};

/// Bash tool input parameters
#[derive(Debug, Deserialize)]
struct BashInput {
    command: String,
}

/// Bash tool implementation
pub struct BashTool {
    description: String,
}

impl BashTool {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

#[async_trait]
impl ToolImpl for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: self.description.clone(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn run(&self, input: serde_json::Value) -> Result<ToolOutput> {
        let start = Instant::now();

        // Parse input
        let BashInput { command } = serde_json::from_value(input)
            .map_err(|e| ExecutorError::InvalidInput("bash".to_string(), e.to_string()))?;

        debug!(command = %command, "executing bash command");

        // Execute command
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(&command)
            .output()
            .await
            .map_err(|e| ExecutorError::SpawnFailed("bash".to_string(), e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Build output string
        let mut content = String::new();

        if !output.stdout.is_empty() {
            content.push_str("[stdout]\n");
            content.push_str(&String::from_utf8_lossy(&output.stdout));
        }

        if !output.stderr.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str("[stderr]\n");
            content.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        content.push_str(&format!(
            "\n[exit_code]\n{}",
            output.status.code().unwrap_or(-1)
        ));

        let is_error = !output.status.success();

        info!(
            command = %command.chars().take(100).collect::<String>(),
            duration_ms = duration_ms,
            exit_code = output.status.code().unwrap_or(-1),
            output_bytes = content.len(),
            is_error = is_error,
            "bash command executed"
        );

        Ok(ToolOutput { content, is_error })
    }
}

/// Default bash tool description
pub fn default_bash_description() -> String {
    r#"Execute a shell command via /bin/sh -c.
The system is Linux.
Commands run with daemon process privileges.
Stdout and stderr are captured. Exit code is returned."#
        .to_string()
}
