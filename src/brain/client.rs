// Brain client - HTTP communication with inference backend

use super::{BrainConfig, BrainError, MessageRequest, MessageResponse};
use reqwest::Client;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Brain client for LLM inference
#[derive(Clone)]
pub struct Brain {
    config: BrainConfig,
    client: Client,
}

impl Brain {
    /// Create a new Brain instance
    pub async fn new(config: BrainConfig) -> Result<Self, super::BrainInitError> {
        info!(
            endpoint = %config.endpoint,
            model = %config.default_model,
            timeout_secs = config.request_timeout_secs,
            max_retries = config.max_retries,
            "initializing brain"
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .map_err(super::BrainInitError::ClientError)?;

        info!("brain initialized successfully");
        Ok(Self { config, client })
    }

    /// Get default model
    pub fn default_model(&self) -> &str {
        &self.config.default_model
    }

    /// Get max output tokens
    pub fn max_output_tokens(&self) -> u32 {
        self.config.max_output_tokens
    }

    /// Perform inference
    pub async fn infer(&self, request: MessageRequest) -> Result<MessageResponse, BrainError> {
        info!(
            model = %request.model,
            messages_count = request.messages.len(),
            has_system = request.system.is_some(),
            has_tools = request.tools.is_some(),
            max_tokens = request.max_tokens,
            "starting inference"
        );

        let start = Instant::now();
        let mut retries = 0;
        let max_retries = self.config.max_retries;
        let base_delay = Duration::from_millis(self.config.base_retry_delay_ms);

        loop {
            debug!(retry = retries, "sending request to inference backend");
            match self.send_request(&request).await {
                Ok(response) => {
                    let latency = start.elapsed().as_millis() as u64;
                    let (input_tokens, output_tokens) = response
                        .usage
                        .as_ref()
                        .map(|u| (u.input_tokens, u.output_tokens))
                        .unwrap_or((0, 0));

                    info!(
                        model = %response.model,
                        input_tokens = input_tokens,
                        output_tokens = output_tokens,
                        latency_ms = latency,
                        retries = retries,
                        content_blocks = response.content.len(),
                        stop_reason = ?response.stop_reason,
                        status = "success",
                        "inference completed successfully"
                    );
                    return Ok(response);
                }
                Err(e) => {
                    retries += 1;
                    if retries > max_retries {
                        error!(
                            retries = retries,
                            total_latency_ms = start.elapsed().as_millis(),
                            error = %e,
                            "inference failed: exhausted retries"
                        );
                        return Err(BrainError::Exhausted {
                            retries,
                            last_error: e.to_string(),
                        });
                    }

                    // Determine delay based on error type (exponential backoff)
                    let multiplier = 2u64.saturating_pow(retries - 1);
                    let delay_ms = base_delay.as_millis() as u64 * multiplier;
                    let delay = Duration::from_millis(delay_ms.min(30000));

                    warn!(
                        retry = retries,
                        max_retries = max_retries,
                        delay_ms = delay.as_millis(),
                        error = %e,
                        "inference failed, retrying"
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn send_request(&self, request: &MessageRequest) -> Result<MessageResponse, BrainError> {
        let url = format!("{}/v1/messages", self.config.endpoint.trim_end_matches('/'));

        debug!(url = %url, "sending HTTP request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", &self.config.api_key))
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        let status = response.status();
        debug!(status = status.as_u16(), "received HTTP response");

        if status.is_success() {
            let body = response.text().await?;
            let body_preview = if body.len() > 200 {
                format!("{}...", &body[..200])
            } else {
                body.clone()
            };
            debug!(response_preview = %body_preview, "response body received");

            let response: MessageResponse = serde_json::from_str(&body)?;
            Ok(response)
        } else if status.as_u16() == 401 {
            Err(BrainError::AuthenticationFailed(
                response.text().await.unwrap_or_default(),
            ))
        } else if status.as_u16() == 400 {
            let body = response.text().await.unwrap_or_default();
            Err(BrainError::InvalidRequest(body))
        } else if status.as_u16() == 402 {
            Err(BrainError::InsufficientBalance(
                response.text().await.unwrap_or_default(),
            ))
        } else if status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            Err(BrainError::ModelError(body))
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(BrainError::InvalidRequest(format!(
                "HTTP {}: {}",
                status, body
            )))
        }
    }
}

unsafe impl Send for Brain {}
unsafe impl Sync for Brain {}
