// Agent Loop - Core orchestration layer
// See docs/mainloop-design.md for design details

pub mod config;

use crate::brain::{Brain, ContentBlock, Message, MessageRequest, RequestBuilder, Role, ToolDefinition};
use crate::comm::{UserRequest, UserResponse};
use crate::executor::Executor;
use crate::memory::Memory;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{error, info, warn};

/// Internal tool call representation
struct ToolCall {
    id: String,
    name: String,
    input: Value,
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
            system_prompt: r#"You are Shelly, a system daemon running on this machine.
You are helpful, cautious, and thorough. You prefer to observe and understand before acting.
When you need to perform operations, use the tools available to you.
Always explain your reasoning before taking actions that could have side effects.
Log your important decisions and observations."#.to_string(),
            identity: "Shelly - a system daemon agent".to_string(),
            init_prompt: r#"You just started. Explore your environment:
- Check system metadata (hostname, OS version)
- Check disk usage
- Check network status
- Check running services

Use the tools available to you. Report what you find."#.to_string(),
        }
    }
}

/// Agent loop state
pub struct AgentLoop {
    brain: Brain,
    executor: Executor,
    memory: Arc<Mutex<Memory>>,
    config: AgentConfig,
}

impl AgentLoop {
    /// Create new agent loop
    pub fn new(brain: Brain, executor: Executor, config: AgentConfig) -> Self {
        let memory = Memory::new(config.identity.clone());
        Self {
            brain,
            executor,
            memory: Arc::new(Mutex::new(memory)),
            config,
        }
    }

    /// Run initialization phase
    pub async fn run_init(&self) -> Result<(), AgentError> {
        info!("Starting agent initialization...");

        // Get tool definitions
        let tool_defs = self.executor.tool_definitions();

        // Build initialization request
        let mut request = self
            .build_request(self.config.init_prompt.clone(), tool_defs.clone())
            .map_err(AgentError::RequestBuild)?;

        // Run inference with timeout
        let result = timeout(
            Duration::from_secs(self.config.init_timeout_secs),
            self.brain.infer(request),
        )
        .await;

        match result {
            Ok(Ok(response)) => {
                info!(stop_reason = ?response.stop_reason, "Init inference completed");

                // Handle tool calls
                self.handle_inference_response(response, &tool_defs).await?;

                info!("Agent initialization completed");
                Ok(())
            }
            Ok(Err(e)) => {
                error!(error = %e, "Init inference failed");
                Err(AgentError::Inference(e.to_string()))
            }
            Err(_) => {
                error!("Init inference timed out");
                Err(AgentError::Timeout(self.config.init_timeout_secs))
            }
        }
    }

    /// Run main loop - handles user requests
    pub async fn handle_user_request(&self, req: UserRequest) {
        let input = req.content.clone();
        let reply = req.reply;

        info!(addr = %req.source_addr, input = %input, "Handling user request");

        let result = timeout(
            Duration::from_secs(self.config.handle_timeout_secs),
            self.handle(input),
        )
        .await;

        let response = match result {
            Ok(Ok(response)) => {
                // Add to memory
                let mut mem = self.memory.lock().await;
                mem.add_interaction(&req.content, &response);
                UserResponse::new(response)
            }
            Ok(Err(e)) => {
                warn!(error = %e, "Handle failed");
                let mut mem = self.memory.lock().await;
                mem.add_error(format!("{}", e));
                UserResponse::error(e.to_string())
            }
            Err(_) => {
                error!("Handle timed out");
                let mut mem = self.memory.lock().await;
                mem.add_error("Handle timeout".to_string());
                UserResponse::error("Request timeout".to_string())
            }
        };

        if reply.send(response).is_err() {
            warn!("Failed to send response to client");
        }
    }

    /// Core handle function - handles input with tool loop
    async fn handle(&self, user_input: String) -> Result<String, AgentError> {
        // Get memory context and tool definitions
        let (context, tool_defs) = {
            let mem = self.memory.lock().await;
            (mem.context(), self.executor.tool_definitions())
        };

        // Build system prompt with context
        let system = format!(
            "{}\n\n# Current Context\n{}",
            self.config.system_prompt, context
        );

        // Tool call loop
        let mut tool_rounds = 0;
        let mut messages: Vec<Message> = Vec::new();

        // Add user message
        messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: user_input.clone() }],
        });

        loop {
            tool_rounds += 1;
            if tool_rounds > self.config.max_tool_rounds {
                warn!(
                    rounds = tool_rounds,
                    "Max tool rounds reached, stopping"
                );
                break;
            }

            info!(round = tool_rounds, "Inference round");

            // Build request
            let mut builder = RequestBuilder::new(self.brain.default_model().to_string())
                .system(system.clone())
                .max_tokens(self.brain.max_output_tokens());

            // Add messages
            for msg in &messages {
                builder = match msg.role {
                    Role::User => builder.user_content(msg.content.clone()),
                    Role::Assistant => builder.assistant_content(msg.content.clone()),
                };
            }

            // Add tools
            builder = builder.tools(tool_defs.clone());

            // Add inference parameters
            if let Some(temp) = self.brain.temperature() {
                builder = builder.temperature(temp);
            }
            if let Some(tp) = self.brain.top_p() {
                builder = builder.top_p(tp);
            }
            if let Some(tk) = self.brain.top_k() {
                builder = builder.top_k(tk);
            }

            let request = builder.build().map_err(AgentError::RequestBuild)?;

            // Run inference
            let response = self.brain.infer(request).await.map_err(|e| {
                AgentError::Inference(e.to_string())
            })?;

            // Extract text content
            let text_content: String = response
                .content
                .iter()
                .filter_map(|block| {
                    if let ContentBlock::Text { text } = block {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("");

            // Check stop reason
            match response.stop_reason {
                Some(crate::brain::types::StopReason::ToolUse) => {
                    info!("Tool use detected");

                    // Extract tool calls
                    let tool_calls: Vec<ToolCall> = response
                        .content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::ToolUse { id, name, input } = block {
                                Some(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Add assistant message with tool calls
                    messages.push(Message {
                        role: Role::Assistant,
                        content: response.content.clone(),
                    });

                    // Execute each tool
                    for call in tool_calls {
                        info!(tool = %call.name, id = %call.id, "Executing tool");
                        match self.executor.execute(&call.name, call.input.clone()).await {
                            Ok(output) => {
                                let result_text = if output.is_error {
                                    format!("Error: {}", output.content)
                                } else {
                                    output.content
                                };

                                // Add tool result message
                                messages.push(Message {
                                    role: Role::User,
                                    content: vec![ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content: result_text.clone(),
                                        is_error: Some(output.is_error),
                                    }],
                                });

                                // Record in memory
                                let mut mem = self.memory.lock().await;
                                mem.add_tool_result(&call.name, &result_text);
                            }
                            Err(e) => {
                                error!(tool = %call.name, error = %e, "Tool execution failed");
                                let err_msg = format!("Error: {}", e);
                                messages.push(Message {
                                    role: Role::User,
                                    content: vec![ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content: err_msg.clone(),
                                        is_error: Some(true),
                                    }],
                                });

                                let mut mem = self.memory.lock().await;
                                mem.add_error(format!("{}: {}", call.name, e));
                            }
                        }
                    }
                }
                _ => {
                    // EndTurn or other stop reason - return the response
                    info!(stop_reason = ?response.stop_reason, "Inference completed");
                    return Ok(text_content);
                }
            }
        }

        // Max rounds reached
        Ok("Maximum tool call rounds reached. Operation aborted.".to_string())
    }

    /// Handle inference response (used in init phase)
    async fn handle_inference_response(
        &self,
        response: crate::brain::MessageResponse,
        tool_defs: &[ToolDefinition],
    ) -> Result<(), AgentError> {
        let text_content: String = response
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text { text } = block {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        // Record initial response in memory
        let mut mem = self.memory.lock().await;
        mem.add_observation(&text_content);

        // Handle tool calls if any
        match response.stop_reason {
            Some(crate::brain::types::StopReason::ToolUse) => {
                let tool_calls: Vec<ToolCall> = response
                    .content
                    .iter()
                    .filter_map(|block| {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            Some(ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                for call in tool_calls {
                    match self.executor.execute(&call.name, call.input.clone()).await {
                        Ok(output) => {
                            mem.add_tool_result(&call.name, &output.content);
                        }
                        Err(e) => {
                            mem.add_error(format!("{}: {}", call.name, e));
                        }
                    }
                }
            }
            _ => {
                // Just record the response
            }
        }

        Ok(())
    }

    /// Build request with tool definitions
    fn build_request(&self, user_input: String, tools: Vec<ToolDefinition>) -> Result<MessageRequest, &'static str> {
        let system = self.config.system_prompt.clone();

        RequestBuilder::new(self.brain.default_model().to_string())
            .system(system)
            .user_text(user_input)
            .max_tokens(self.brain.max_output_tokens())
            .tools(tools)
            .build()
    }

    /// Run shutdown handling
    pub async fn shutdown(&self) {
        info!("Starting shutdown handling...");

        let shutdown_prompt = "The system is about to shut down. Please save any important state \
            and perform any necessary cleanup. Report what you did.";

        let result = timeout(
            Duration::from_secs(self.config.shutdown_timeout_secs),
            self.handle(shutdown_prompt.to_string()),
        )
        .await;

        match result {
            Ok(Ok(response)) => {
                info!(response = %response, "Shutdown handling completed");
                let mut mem = self.memory.lock().await;
                mem.add_observation(format!("Shutdown: {}", response));
            }
            Ok(Err(e)) => {
                warn!(error = %e, "Shutdown handling failed");
            }
            Err(_) => {
                warn!("Shutdown handling timed out");
            }
        }
    }

    /// Get memory for debugging
    pub async fn memory(&self) -> Arc<Mutex<Memory>> {
        self.memory.clone()
    }
}

/// Agent errors
#[derive(Debug)]
pub enum AgentError {
    Inference(String),
    RequestBuild(&'static str),
    Timeout(u64),
    Executor(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Inference(s) => write!(f, "Inference error: {}", s),
            AgentError::RequestBuild(s) => write!(f, "Request build error: {}", s),
            AgentError::Timeout(secs) => write!(f, "Timeout after {}s", secs),
            AgentError::Executor(s) => write!(f, "Executor error: {}", s),
        }
    }
}

impl std::error::Error for AgentError {}
