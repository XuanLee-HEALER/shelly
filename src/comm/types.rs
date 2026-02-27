use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::sync::oneshot;

/// Message types for the protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType {
    /// Client → Shelly: Client sends a request
    Request = 0x01,
    /// Shelly → Client: Shelly acknowledges the request
    RequestAck = 0x02,
    /// Shelly → Client: Shelly returns the response
    Response = 0x03,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Request),
            0x02 => Some(Self::RequestAck),
            0x03 => Some(Self::Response),
            _ => None,
        }
    }
}

/// Request payload from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPayload {
    /// User input text
    pub content: String,
}

/// Response payload from Shelly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePayload {
    /// Shelly's reply text
    pub content: String,
    /// Whether this is an error response
    pub is_error: bool,
}

/// Request sent from Comm to main loop
#[derive(Debug)]
pub struct UserRequest {
    /// User input content
    pub content: String,
    /// Channel to send response back to Comm
    pub reply: oneshot::Sender<UserResponse>,
    /// Client source address
    pub source_addr: SocketAddr,
}

/// Response sent from main loop to Comm
#[derive(Debug)]
pub struct UserResponse {
    /// Response content
    pub content: String,
    /// Whether this is an error response
    pub is_error: bool,
}

impl UserResponse {
    pub fn new(content: String) -> Self {
        Self {
            content,
            is_error: false,
        }
    }

    pub fn error(content: String) -> Self {
        Self {
            content,
            is_error: true,
        }
    }
}
