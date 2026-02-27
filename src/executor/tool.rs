// Tool trait and implementations
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

use crate::brain::ToolDefinition;
use crate::executor::{Result, ToolOutput};
use async_trait::async_trait;
use tracing::debug;

/// Internal trait for tool implementations
#[async_trait]
pub trait ToolImpl: Send + Sync {
    /// Get the tool definition (name, description, input_schema)
    fn definition(&self) -> ToolDefinition;

    /// Run the tool with JSON input
    async fn run(&self, input: serde_json::Value) -> Result<ToolOutput>;

    /// Get tool name
    fn name(&self) -> String {
        self.definition().name.clone()
    }
}

/// Load tool descriptions from TOML config file
pub fn load_tool_descriptions(
    path: &std::path::Path,
) -> Result<std::collections::HashMap<String, String>> {
    use std::collections::HashMap;

    if !path.exists() {
        debug!(path = %path.display(), "tools.toml not found, using default descriptions");
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(path)?;
    let config: toml::Value = content.parse()?;

    let mut descriptions = HashMap::new();

    if let Some(table) = config.as_table() {
        for (key, value) in table {
            if let Some(desc) = value.get("description") {
                if let Some(s) = desc.as_str() {
                    descriptions.insert(key.clone(), s.to_string());
                }
            }
        }
    }

    debug!(path = %path.display(), tool_count = descriptions.len(), "loaded tool descriptions from config");
    Ok(descriptions)
}
