// Memory errors

use thiserror::Error;

/// Memory errors
#[derive(Debug, Error)]
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
pub enum MemoryError {
    #[error("Failed to load memory: {0}")]
    LoadFailed(String),

    #[error("Failed to store memory: {0}")]
    StoreFailed(String),

    #[error("Failed to generate embedding: {0}")]
    EmbeddingFailed(String),
}
