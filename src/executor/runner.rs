// Main Executor implementation
#![allow(dead_code)]

use crate::brain::ToolDefinition;
use crate::executor::bash::{BashTool, default_bash_description};
use crate::executor::config::ExecutorConfig;
use crate::executor::error::{ExecutorError, Result};
use crate::executor::tool::ToolImpl;
use crate::executor::types::ToolOutput;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// Main executor for tool execution
pub struct Executor {
    config: ExecutorConfig,
    tools: RwLock<HashMap<String, Arc<dyn ToolImpl>>>,
}

impl Executor {
    /// Create a new Executor instance (backward compatibility)
    pub fn new(config: ExecutorConfig) -> Self {
        Self::init(config)
    }

    /// Initialize with registered tools
    pub fn init(config: ExecutorConfig) -> Self {
        debug!(
            timeout_secs = config.constraints.timeout_secs,
            max_output_bytes = config.constraints.max_output_bytes,
            shell = %config.shell,
            "initializing executor"
        );

        let mut tools = HashMap::new();

        // Load tool descriptions from config file
        let descriptions = crate::executor::tool::load_tool_descriptions(&config.tools_toml_path)
            .unwrap_or_default();

        // Register bash tool
        let bash_desc = descriptions
            .get("bash")
            .cloned()
            .unwrap_or_else(default_bash_description);

        let bash_tool = Arc::new(BashTool::new(bash_desc)) as Arc<dyn ToolImpl>;
        tools.insert("bash".to_string(), bash_tool);

        info!(tool_count = 1, "executor initialized with tools");

        Self {
            config,
            tools: RwLock::new(tools),
        }
    }

    /// Get all tool definitions for Brain
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().unwrap();
        tools.values().map(|t| t.definition()).collect()
    }

    /// Execute a tool by name with JSON input
    pub async fn execute(&self, tool_name: &str, input: serde_json::Value) -> Result<ToolOutput> {
        debug!(tool_name = %tool_name, "looking up tool");

        let tool = {
            let tools = self.tools.read().unwrap();
            tools.get(tool_name).cloned()
        };

        let tool = tool.ok_or_else(|| ExecutorError::UnknownTool(tool_name.to_string()))?;

        info!(tool_name = %tool_name, "executing tool");
        tool.run(input).await
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::init(ExecutorConfig::default())
    }
}
