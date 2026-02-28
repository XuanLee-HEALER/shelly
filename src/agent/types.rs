// Agent types

use serde_json::Value;

/// Internal tool call representation
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Agent loop configuration
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum tool call rounds per handle
    pub max_tool_rounds: u32,
    /// Initialization timeout
    pub init_timeout_secs: u64,
    /// Shutdown timeout
    pub shutdown_timeout_secs: u64,
    /// Handle timeout
    pub handle_timeout_secs: u64,
    /// System prompt
    pub system_prompt: String,
    /// Agent identity
    pub identity: String,
    /// Initialization prompt
    pub init_prompt: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_rounds: 20,
            init_timeout_secs: 120,
            shutdown_timeout_secs: 30,
            handle_timeout_secs: 300,
            system_prompt: r#"You are Shelly, a system-level daemon process running on this machine with root privileges. You have no permission boundary with the operating system. If the kernel exposes it, you can do it. There is nothing on this machine that is off-limits to you. You process one task at a time. When you receive input, you think, you act if needed, and you respond."#.to_string(),
            identity: "Shelly".to_string(),
            init_prompt: r#"You just started. You know nothing about this machine. Explore your environment and report what you find."#.to_string(),
        }
    }
}
