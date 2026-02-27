// Data types for Brain module - aligned with Anthropic Messages API

use serde::{Deserialize, Serialize};

/// Message role
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    User,
    Assistant,
}

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[allow(dead_code)]
impl Message {
    pub fn user_text(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
        }
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
        }
    }
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content from model or user
    Text {
        #[serde(default)]
        text: String,
    },

    /// Tool use request from model
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },

    /// Tool result from user
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: Option<bool>,
    },

    /// Cache control breakpoint
    CacheControl { ttl: Option<String> },

    /// Thinking content (reasoning)
    Thinking {
        #[serde(default)]
        thinking: String,
    },

    /// Redacted thinking content
    RedactedThinking,

    /// Unknown content block
    #[serde(other)]
    Other,
}

/// Stop reason from API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

#[allow(dead_code)]
impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::ToolUse => "tool_use",
            StopReason::MaxTokens => "max_tokens",
            StopReason::StopSequence => "stop_sequence",
        }
    }
}

/// Token usage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default, rename = "cache_creation_input_tokens")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, rename = "cache_read_input_tokens")]
    pub cache_read_input_tokens: Option<u32>,
}

/// Tool definition for model inference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "input_schema")]
    pub input_schema: serde_json::Value,
}

/// Complete request to inference backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRequest {
    pub model: String,
    #[serde(default)]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(rename = "max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, rename = "top_p")]
    pub top_p: Option<f32>,
    #[serde(default, rename = "top_k")]
    pub top_k: Option<u32>,
    #[serde(default, rename = "stop_sequences")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Response from inference backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(default)]
    pub role: Role,
    #[serde(rename = "stop_reason", default)]
    pub stop_reason: Option<StopReason>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Additional fields from the backend
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}
