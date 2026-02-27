// Agent configuration

use super::AgentConfig;
use tracing::warn;

/// Parse an environment variable, logging a warning if the value is present but invalid.
fn parse_env_var<T: std::str::FromStr>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Ok(v) => match v.parse() {
            Ok(parsed) => parsed,
            Err(_) => {
                warn!(var = name, value = %v, "Invalid env var value, using default");
                default
            }
        },
        Err(_) => default,
    }
}

impl AgentConfig {
    /// Load from environment variables
    pub fn from_env() -> Result<Self, AgentConfigError> {
        dotenvy::dotenv().ok();

        let mut config = AgentConfig::default();

        config.max_tool_rounds = parse_env_var("AGENT_MAX_TOOL_ROUNDS", config.max_tool_rounds);
        config.init_timeout_secs =
            parse_env_var("AGENT_INIT_TIMEOUT_SECS", config.init_timeout_secs);
        config.shutdown_timeout_secs =
            parse_env_var("AGENT_SHUTDOWN_TIMEOUT_SECS", config.shutdown_timeout_secs);
        config.handle_timeout_secs =
            parse_env_var("AGENT_HANDLE_TIMEOUT_SECS", config.handle_timeout_secs);

        Ok(config)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
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
