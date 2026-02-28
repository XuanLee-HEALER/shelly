// Inference loop - Core inference unit for agent
// See docs/mainloop-design.md for design details

use crate::brain::{
    types::StopReason, ContentBlock, Message, MessageRequest, MessageResponse, Role, ToolDefinition,
};
use crate::executor::ToolOutput;

pub use crate::agent::error::InferenceError;

use futures::future::BoxFuture;
use futures::FutureExt;

/// Inference loop result
#[derive(Debug, Clone)]
pub struct InferenceResult {
    /// Final text response
    pub text: String,
    /// Total tool rounds used (only counts actual tool executions)
    pub tool_rounds: u32,
}

/// Run inference loop - the minimal inference unit
///
/// This function drives brain + executor in a loop until:
/// - LLM returns EndTurn (returns result)
/// - LLM returns ToolUse (executes tool, continues loop)
/// - Max tool rounds reached (returns error)
/// - Inference fails (returns error)
///
/// # Arguments
/// * `brain` - LLM inference client (knows model, temperature, max_tokens)
/// * `executor` - Tool executor (provides tool definitions)
/// * `messages` - Conversation messages (in/out)
/// * `system` - System prompt
/// * `max_tool_rounds` - Maximum tool call rounds (recursion depth limit)
/// * `tool_rounds` - Current tool rounds (accumulates through recursion)
pub fn inference_loop<'a, B: BrainRef, E: ExecutorRef>(
    brain: &'a B,
    executor: &'a E,
    messages: &'a mut Vec<Message>,
    system: &'a str,
    max_tool_rounds: u32,
    tool_rounds: u32,
) -> BoxFuture<'a, std::result::Result<InferenceResult, InferenceError>> {
    async move {
        // Get tool definitions from executor
        let tool_defs = executor.tool_definitions();

        // Build request (brain knows its model, temperature, max_tokens)
        let request = build_request(brain, system, messages, &tool_defs)
            .map_err(InferenceError::RequestBuild)?;

        // Call brain
        let response: MessageResponse = brain
            .infer(request)
            .await
            .map_err(|e| InferenceError::InferenceFailed(e.to_string()))?;

        // Extract text content
        let text_content = extract_text(&response);

        // Extract tool calls
        let tool_calls = extract_tool_calls(&response);

        match response.stop_reason {
            Some(StopReason::ToolUse) => {
                // Count actual tool execution
                let new_tool_rounds = tool_rounds + 1;
                if new_tool_rounds > max_tool_rounds {
                    return Err(InferenceError::MaxToolRounds {
                        max_rounds: max_tool_rounds,
                        actual_rounds: new_tool_rounds,
                    });
                }

                // Add assistant message with tool use
                messages.push(Message {
                    role: Role::Assistant,
                    content: response.content.clone(),
                });

                // Execute tool calls
                execute_tool_calls(executor, tool_calls, messages).await;

                // Recursive call
                inference_loop(brain, executor, messages, system, max_tool_rounds, new_tool_rounds).await
            }
            _ => {
                // Non-ToolUse: all are termination conditions
                messages.push(Message {
                    role: Role::Assistant,
                    content: response.content.clone(),
                });

                Ok(InferenceResult {
                    text: text_content,
                    tool_rounds,
                })
            }
        }
    }.boxed()
}

/// Trait for brain reference (for testing)
#[async_trait::async_trait]
pub trait BrainRef: Send + Sync {
    async fn infer(&self, request: MessageRequest) -> Result<MessageResponse, String>;
    fn model(&self) -> &str;
    fn max_output_tokens(&self) -> u32;
    fn temperature(&self) -> Option<f32>;
    fn top_p(&self) -> Option<f32>;
    fn top_k(&self) -> Option<u32>;
}

/// Trait for executor reference (for testing)
#[async_trait::async_trait]
pub trait ExecutorRef: Send + Sync {
    async fn execute(&self, tool_name: &str, input: serde_json::Value) -> Result<ToolOutput, String>;
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
}

/// Build inference request
fn build_request<B: BrainRef>(
    brain: &B,
    system: &str,
    messages: &[Message],
    tool_defs: &[ToolDefinition],
) -> Result<MessageRequest, &'static str> {
    use crate::brain::RequestBuilder;

    let mut builder = RequestBuilder::new(brain.model().to_string())
        .system(system.to_string())
        .max_tokens(brain.max_output_tokens());

    for msg in messages {
        builder = match msg.role {
            Role::User => builder.user_content(msg.content.clone()),
            Role::Assistant => builder.assistant_content(msg.content.clone()),
        };
    }

    builder = builder.tools(tool_defs.to_vec());

    if let Some(temp) = brain.temperature() {
        builder = builder.temperature(temp);
    }
    if let Some(tp) = brain.top_p() {
        builder = builder.top_p(tp);
    }
    if let Some(tk) = brain.top_k() {
        builder = builder.top_k(tk);
    }

    builder.build().map_err(|_| "Failed to build request")
}

/// Extract text content from response
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

/// Extract tool calls from response
fn extract_tool_calls(response: &MessageResponse) -> Vec<super::types::ToolCall> {
    response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::ToolUse { id, name, input } = block {
                Some(super::types::ToolCall {
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
async fn execute_tool_calls<E: ExecutorRef>(
    executor: &E,
    tool_calls: Vec<super::types::ToolCall>,
    messages: &mut Vec<Message>,
) {
    for call in tool_calls {
        let result = executor.execute(&call.name, call.input.clone()).await;

        let (result_text, is_error) = match result {
            Ok(output) => {
                let is_err = output.is_error;
                let text = if output.is_error {
                    format!("Error: {}", output.content)
                } else {
                    output.content
                };
                (text, Some(is_err))
            }
            Err(e) => {
                (format!("Error: {}", e), Some(true))
            }
        };

        messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: call.id,
                content: result_text,
                is_error,
            }],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::{ContentBlock, Message, Role};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::RwLock;

    /// Mock brain for testing
    struct MockBrain {
        responses: RwLock<Vec<MessageResponse>>,
    }

    impl MockBrain {
        fn new(responses: Vec<MessageResponse>) -> Self {
            Self {
                responses: RwLock::new(responses),
            }
        }
    }

    #[async_trait]
    impl BrainRef for MockBrain {
        async fn infer(&self, _request: MessageRequest) -> Result<MessageResponse, String> {
            let mut responses = self.responses.write().unwrap();
            if let Some(response) = responses.pop() {
                Ok(response)
            } else {
                Err("No more responses".to_string())
            }
        }

        fn model(&self) -> &str {
            "test-model"
        }

        fn max_output_tokens(&self) -> u32 {
            4096
        }

        fn temperature(&self) -> Option<f32> {
            None
        }

        fn top_p(&self) -> Option<f32> {
            None
        }

        fn top_k(&self) -> Option<u32> {
            None
        }
    }

    /// Mock executor for testing
    struct MockExecutor {
        results: RwLock<Vec<Result<ToolOutput, String>>>,
    }

    impl MockExecutor {
        fn new(results: Vec<Result<ToolOutput, String>>) -> Self {
            Self {
                results: RwLock::new(results),
            }
        }
    }

    #[async_trait]
    impl ExecutorRef for MockExecutor {
        async fn execute(
            &self,
            _tool_name: &str,
            _input: serde_json::Value,
        ) -> std::result::Result<ToolOutput, String> {
            let mut results = self.results.write().unwrap();
            if let Some(result) = results.pop() {
                result
            } else {
                Err("No more results".to_string())
            }
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![]
        }
    }

    fn create_text_response(text: &str, stop_reason: Option<StopReason>) -> MessageResponse {
        MessageResponse {
            id: "test-id".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model: "test".to_string(),
            role: Role::Assistant,
            stop_reason,
            stop_sequence: None,
            usage: None,
            extra: std::collections::HashMap::new(),
        }
    }

    fn create_tool_use_response(tool_name: &str, tool_input: serde_json::Value) -> MessageResponse {
        MessageResponse {
            id: "test-id".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: tool_name.to_string(),
                input: tool_input,
            }],
            model: "test".to_string(),
            role: Role::Assistant,
            stop_reason: Some(StopReason::ToolUse),
            stop_sequence: None,
            usage: None,
            extra: std::collections::HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_inference_loop_end_turn() {
        let brain = MockBrain::new(vec![create_text_response("Hello!", Some(StopReason::EndTurn))]);
        let executor = MockExecutor::new(vec![]);

        let mut messages = vec![Message::user_text("Hi")];
        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.text, "Hello!");
        assert_eq!(result.tool_rounds, 0);  // No tool call in this test
    }

    #[tokio::test]
    async fn test_inference_loop_tool_use() {
        let brain = MockBrain::new(vec![
            create_text_response("Let me check that.", Some(StopReason::EndTurn)),
            create_tool_use_response("bash", json!({"command": "echo hello"})),
        ]);
        let executor = MockExecutor::new(vec![Ok(ToolOutput::success("hello"))]);

        let mut messages = vec![Message::user_text("Check something")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.text, "Let me check that.");
        assert_eq!(result.tool_rounds, 1);  // Only 1 tool execution
        // Should have user msg, assistant tool use, tool result, assistant final
        assert_eq!(messages.len(), 4);
    }

    #[tokio::test]
    async fn test_inference_loop_max_tool_rounds() {
        // Create responses that all trigger tool use
        let responses: Vec<MessageResponse> = (0..25)
            .map(|i| create_tool_use_response("bash", json!({"command": format!("cmd{}", i)})))
            .collect();

        let brain = MockBrain::new(responses);
        // Executor will return success but we won't use it after max rounds
        let executor = MockExecutor::new(vec![]);

        let mut messages = vec![Message::user_text("Do many things")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, InferenceError::MaxToolRounds { max_rounds: 20, .. }));
    }

    #[tokio::test]
    async fn test_inference_loop_inference_error() {
        struct ErrorBrain;

        #[async_trait]
        impl BrainRef for ErrorBrain {
            async fn infer(&self, _request: MessageRequest) -> Result<MessageResponse, String> {
                Err("API error".to_string())
            }

            fn model(&self) -> &str {
                "test-model"
            }

            fn max_output_tokens(&self) -> u32 {
                4096
            }

            fn temperature(&self) -> Option<f32> {
                None
            }

            fn top_p(&self) -> Option<f32> {
                None
            }

            fn top_k(&self) -> Option<u32> {
                None
            }
        }

        let brain = ErrorBrain;
        let executor = MockExecutor::new(vec![]);

        let mut messages = vec![Message::user_text("Hi")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, InferenceError::InferenceFailed(_)));
    }

    #[tokio::test]
    async fn test_inference_loop_tool_error() {
        let brain = MockBrain::new(vec![
            create_text_response("Got result.", Some(StopReason::EndTurn)),
            create_tool_use_response("bash", json!({"command": "ls"})),
        ]);
        let executor = MockExecutor::new(vec![Err("Command failed".to_string())]);

        let mut messages = vec![Message::user_text("List files")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.text, "Got result.");
        // Check that error was added to messages
        let tool_result_msg = &messages[2];
        if let ContentBlock::ToolResult { content, is_error, .. } = &tool_result_msg.content[0] {
            assert!(content.contains("Error:"));
            assert!(is_error.is_some() && is_error.unwrap());
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[tokio::test]
    async fn test_inference_loop_with_none_stop_reason() {
        // None stop_reason should be treated as EndTurn
        let brain = MockBrain::new(vec![create_text_response("Response", None)]);
        let executor = MockExecutor::new(vec![]);

        let mut messages = vec![Message::user_text("Hi")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.text, "Response");
    }

    #[tokio::test]
    async fn test_inference_loop_max_tokens() {
        let brain = MockBrain::new(vec![create_text_response(
            "Truncated...",
            Some(StopReason::MaxTokens),
        )]);
        let executor = MockExecutor::new(vec![]);

        let mut messages = vec![Message::user_text("Long request")];

        let result: std::result::Result<InferenceResult, InferenceError> = inference_loop(
            &brain,
            &executor,
            &mut messages,
            "You are helpful.",
            20,
            0,
        )
        .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.text, "Truncated...");
    }

    #[tokio::test]
    async fn test_extract_tool_calls() {
        let response = MessageResponse {
            id: "test".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I'll use a tool".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call-123".to_string(),
                    name: "bash".to_string(),
                    input: json!({"command": "echo test"}),
                },
            ],
            model: "test".to_string(),
            role: Role::Assistant,
            stop_reason: Some(StopReason::ToolUse),
            stop_sequence: None,
            usage: None,
            extra: std::collections::HashMap::new(),
        };

        let calls = extract_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-123");
        assert_eq!(calls[0].name, "bash");
    }

    #[tokio::test]
    async fn test_extract_text() {
        let response = MessageResponse {
            id: "test".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Hello ".to_string(),
                },
                ContentBlock::Text {
                    text: "World!".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call-1".to_string(),
                    name: "tool".to_string(),
                    input: json!({}),
                },
            ],
            model: "test".to_string(),
            role: Role::Assistant,
            stop_reason: Some(StopReason::EndTurn),
            stop_sequence: None,
            usage: None,
            extra: std::collections::HashMap::new(),
        };

        let text = extract_text(&response);
        assert_eq!(text, "Hello World!");
    }
}
