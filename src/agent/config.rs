// Agent configuration

use super::AgentConfig;

impl AgentConfig {
    /// Load from environment variables
    pub fn from_env() -> Result<Self, AgentConfigError> {
        dotenvy::dotenv().ok();

        let mut config = AgentConfig::default();

        // Optional configuration via environment
        if let Ok(v) = std::env::var("AGENT_MAX_TOOL_ROUNDS") {
            config.max_tool_rounds = v.parse().unwrap_or(config.max_tool_rounds);
        }

        if let Ok(v) = std::env::var("AGENT_INIT_TIMEOUT_SECS") {
            config.init_timeout_secs = v.parse().unwrap_or(config.init_timeout_secs);
        }

        if let Ok(v) = std::env::var("AGENT_SHUTDOWN_TIMEOUT_SECS") {
            config.shutdown_timeout_secs = v.parse().unwrap_or(config.shutdown_timeout_secs);
        }

        if let Ok(v) = std::env::var("AGENT_HANDLE_TIMEOUT_SECS") {
            config.handle_timeout_secs = v.parse().unwrap_or(config.handle_timeout_secs);
        }

        Ok(config)
    }
}

#[derive(Debug)]
pub enum AgentConfigError {
    ConfigMissing(String),
}

impl std::fmt::Display for AgentConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConfigError::ConfigMissing(s) => write!(f, "Config missing: {}", s),
        }
    }
}

impl std::error::Error for AgentConfigError {}
