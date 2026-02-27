// Brain module - LLM inference client
// See docs/brain-design.md for design details

pub mod builder;
pub mod client;
pub mod error;
pub mod types;

pub use builder::RequestBuilder;
pub use client::Brain;
pub use error::{BrainError, BrainInitError};
pub use types::{ContentBlock, Message, MessageRequest, MessageResponse, Role, ToolDefinition};

/// Brain configuration
#[derive(Debug, Clone)]
pub struct BrainConfig {
    /// Inference backend URL
    pub endpoint: String,
    /// API key for authentication
    pub api_key: String,
    /// Default model identifier
    pub default_model: String,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Base retry delay in milliseconds
    pub base_retry_delay_ms: u64,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Maximum output tokens
    pub max_output_tokens: u32,
    /// Temperature (0.0-2.0, None = use model default)
    pub temperature: Option<f32>,
    /// Top-P nucleus sampling (0.0-1.0, None = use model default)
    pub top_p: Option<f32>,
    /// Top-K sampling (None = use model default)
    pub top_k: Option<u32>,
}

impl BrainConfig {
    pub fn from_env() -> Result<Self, BrainInitError> {
        dotenvy::dotenv().ok();

        let endpoint = std::env::var("INFERENCE_ENDPOINT")
            .map_err(|_| BrainInitError::ConfigMissing("INFERENCE_ENDPOINT".into()))?;
        let api_key = std::env::var("INFERENCE_API_KEY")
            .map_err(|_| BrainInitError::ConfigMissing("INFERENCE_API_KEY".into()))?;
        let default_model = std::env::var("INFERENCE_MODEL")
            .map_err(|_| BrainInitError::ConfigMissing("INFERENCE_MODEL".into()))?;

        let max_retries = std::env::var("INFERENCE_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        let base_retry_delay_ms = std::env::var("INFERENCE_RETRY_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);

        let request_timeout_secs = std::env::var("INFERENCE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);

        let max_output_tokens = std::env::var("INFERENCE_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096);

        // Inference parameters (optional, use model defaults if not set)
        let temperature = std::env::var("INFERENCE_TEMPERATURE")
            .ok()
            .and_then(|v| v.parse().ok());

        let top_p = std::env::var("INFERENCE_TOP_P")
            .ok()
            .and_then(|v| v.parse().ok());

        let top_k = std::env::var("INFERENCE_TOP_K")
            .ok()
            .and_then(|v| v.parse().ok());

        Ok(Self {
            endpoint,
            api_key,
            default_model,
            max_retries,
            base_retry_delay_ms,
            request_timeout_secs,
            max_output_tokens,
            temperature,
            top_p,
            top_k,
        })
    }
}
