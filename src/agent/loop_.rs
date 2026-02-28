// Agent loop implementation

use crate::brain::{
    Brain, ContentBlock, Message, MessageResponse, RequestBuilder, Role, ToolDefinition,
};
use crate::comm::{UserRequest, UserResponse};
use crate::executor::Executor;
use crate::memory::Memory;

use super::error::AgentError;
use super::types::{AgentConfig, ToolCall};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{error, info, warn};

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

    /// Build an inference request from the current state
    fn build_request(
        &self,
        system: &str,
        messages: &[Message],
        tool_defs: &[ToolDefinition],
    ) -> Result<crate::brain::MessageRequest, AgentError> {
        let mut builder = RequestBuilder::new(self.brain.default_model().to_string())
            .system(system.to_string())
            .max_tokens(self.brain.max_output_tokens());

        for msg in messages {
            builder = match msg.role {
                Role::User => builder.user_content(msg.content.clone()),
                Role::Assistant => builder.assistant_content(msg.content.clone()),
            };
        }

        builder = builder.tools(tool_defs.to_vec());

        if let Some(temp) = self.brain.temperature() {
            builder = builder.temperature(temp);
        }
        if let Some(tp) = self.brain.top_p() {
            builder = builder.top_p(tp);
        }
        if let Some(tk) = self.brain.top_k() {
            builder = builder.top_k(tk);
        }

        builder.build().map_err(AgentError::RequestBuild)
    }

    /// Extract text content from a response
    fn extract_text(response: &MessageResponse) -> String {
        response
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
            .join("")
    }

    /// Extract tool calls from a response
    fn extract_tool_calls(response: &MessageResponse) -> Vec<ToolCall> {
        response
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
            .collect()
    }

    /// Execute tool calls and append results to messages
    async fn execute_tool_calls(&self, tool_calls: Vec<ToolCall>, messages: &mut Vec<Message>) {
        for call in tool_calls {
            info!(tool = %call.name, id = %call.id, "Executing tool");
            match self.executor.execute(&call.name, call.input.clone()).await {
                Ok(output) => {
                    let result_text = if output.is_error {
                        format!("Error: {}", output.content)
                    } else {
                        output.content
                    };

                    messages.push(Message {
                        role: Role::User,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: call.id,
                            content: result_text.clone(),
                            is_error: Some(output.is_error),
                        }],
                    });

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

    /// Run initialization phase
    pub async fn run_init(&self) -> Result<(), AgentError> {
        info!("Starting agent initialization...");

        let tool_defs = self.executor.tool_definitions();
        let system = self.config.system_prompt.clone();

        let mut tool_rounds = 0;
        let mut messages: Vec<Message> = Vec::new();

        messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: self.config.init_prompt.clone(),
            }],
        });

        loop {
            tool_rounds += 1;
            if tool_rounds > self.config.max_tool_rounds {
                warn!(rounds = tool_rounds, "Max tool rounds reached during init");
                break;
            }

            info!(round = tool_rounds, "Init inference round");

            let request = self.build_request(&system, &messages, &tool_defs)?;

            let result = timeout(
                Duration::from_secs(self.config.init_timeout_secs),
                self.brain.infer(request),
            )
            .await;

            match result {
                Ok(Ok(response)) => {
                    info!(stop_reason = ?response.stop_reason, "Init inference completed");

                    let text_content = Self::extract_text(&response);

                    {
                        let mut mem = self.memory.lock().await;
                        mem.add_observation(&text_content);
                    }

                    match response.stop_reason {
                        Some(crate::brain::types::StopReason::ToolUse) => {
                            info!("Tool use detected in init");
                            let tool_calls = Self::extract_tool_calls(&response);

                            messages.push(Message {
                                role: Role::Assistant,
                                content: response.content.clone(),
                            });

                            self.execute_tool_calls(tool_calls, &mut messages).await;
                        }
                        Some(crate::brain::types::StopReason::MaxTokens) => {
                            warn!("Init inference stopped due to max tokens");
                            break;
                        }
                        _ => {
                            info!("Init inference finished");
                            break;
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!(error = %e, "Init inference failed");
                    return Err(AgentError::Inference(e.to_string()));
                }
                Err(_) => {
                    error!("Init inference timed out");
                    return Err(AgentError::Timeout(self.config.init_timeout_secs));
                }
            }
        }

        info!("Agent initialization completed");
        Ok(())
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
        let (context, tool_defs) = {
            let mem = self.memory.lock().await;
            (mem.context(), self.executor.tool_definitions())
        };

        let system = format!(
            "{}\n\n# Current Context\n{}",
            self.config.system_prompt, context
        );

        let mut tool_rounds = 0;
        let mut messages: Vec<Message> = Vec::new();

        messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: user_input.clone(),
            }],
        });

        loop {
            tool_rounds += 1;
            if tool_rounds > self.config.max_tool_rounds {
                warn!(rounds = tool_rounds, "Max tool rounds reached, stopping");
                break;
            }

            info!(round = tool_rounds, "Inference round");

            let request = self.build_request(&system, &messages, &tool_defs)?;

            let response = self
                .brain
                .infer(request)
                .await
                .map_err(|e| AgentError::Inference(e.to_string()))?;

            let text_content = Self::extract_text(&response);

            match response.stop_reason {
                Some(crate::brain::types::StopReason::ToolUse) => {
                    info!("Tool use detected");
                    let tool_calls = Self::extract_tool_calls(&response);

                    messages.push(Message {
                        role: Role::Assistant,
                        content: response.content.clone(),
                    });

                    self.execute_tool_calls(tool_calls, &mut messages).await;
                }
                Some(crate::brain::types::StopReason::MaxTokens) => {
                    warn!("Inference stopped due to max tokens limit");
                    return Ok(text_content);
                }
                Some(crate::brain::types::StopReason::EndTurn) | None => {
                    info!(stop_reason = ?response.stop_reason, "Inference completed");
                    return Ok(text_content);
                }
                Some(crate::brain::types::StopReason::StopSequence) => {
                    info!(stop_reason = ?response.stop_reason, "Inference stopped by sequence");
                    return Ok(text_content);
                }
            }
        }

        Ok("Maximum tool call rounds reached. Operation aborted.".to_string())
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
}
