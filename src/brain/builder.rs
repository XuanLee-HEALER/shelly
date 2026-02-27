// RequestBuilder - type-safe chainable builder for MessageRequest
#![allow(dead_code)]

use super::{ContentBlock, Message, MessageRequest, Role, ToolDefinition};

pub struct RequestBuilder {
    model: String,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<ToolDefinition>>,
    max_tokens: u32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    stop_sequences: Option<Vec<String>>,
    stream: Option<bool>,
    metadata: Option<serde_json::Value>,
}

impl RequestBuilder {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: None,
            messages: Vec::new(),
            tools: None,
            max_tokens: 4096,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: None,
            metadata: None,
        }
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn user_text(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::user_text(content));
        self
    }

    pub fn user_content(mut self, content: Vec<ContentBlock>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content,
        });
        self
    }

    pub fn assistant_text(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::assistant_text(content));
        self
    }

    pub fn assistant_content(mut self, content: Vec<ContentBlock>) -> Self {
        self.messages.push(Message {
            role: Role::Assistant,
            content,
        });
        self
    }

    pub fn user_tool_result(
        mut self,
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: Option<bool>,
    ) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
        });
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn tool(mut self, tool: ToolDefinition) -> Self {
        match &mut self.tools {
            Some(t) => t.push(tool),
            None => self.tools = Some(vec![tool]),
        }
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    pub fn stop_sequences(mut self, sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(sequences);
        self
    }

    pub fn stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn build(self) -> Result<MessageRequest, &'static str> {
        if self.messages.is_empty() {
            return Err("messages cannot be empty");
        }

        // Validate: first message must be user role
        if self.messages.first().map(|m| &m.role) != Some(&Role::User) {
            return Err("first message must have user role");
        }

        Ok(MessageRequest {
            model: self.model,
            system: self.system,
            messages: self.messages,
            tools: self.tools,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stop_sequences: self.stop_sequences,
            stream: self.stream,
            metadata: self.metadata,
        })
    }
}
