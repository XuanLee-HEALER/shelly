use thiserror::Error;

/// Comm module initialization errors
#[derive(Debug, Error)]
pub enum CommInitError {
    #[error("Failed to bind UDP socket: {0}")]
    BindFailed(String),
}

/// Comm module runtime errors
#[derive(Debug, Error)]
pub enum CommError {
    #[error("Failed to receive packet: {0}")]
    RecvError(String),

    #[error("Failed to send packet: {0}")]
    SendError(String),

    #[error("Failed to decode packet: {0}")]
    DecodeError(String),

    #[error("Failed to encode packet: {0}")]
    EncodeError(String),

    #[error("Payload too large: {0} bytes")]
    PayloadTooLarge(usize),

    #[error("Channel closed")]
    ChannelClosed,
}

/// Result type for comm operations
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, CommError>;
