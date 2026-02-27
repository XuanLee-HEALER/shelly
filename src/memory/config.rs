// Memory configuration

use std::path::PathBuf;

/// Memory configuration
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Storage directory
    pub storage_dir: PathBuf,
    /// Number of entries to retrieve
    pub top_k: usize,
    /// Maximum cognition rounds
    pub max_cognition_rounds: usize,
    /// Embedding model identifier
    pub embedding_model: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            storage_dir: dirs::home_dir()
                .map(|p| p.join(".shelly").join("memory"))
                .unwrap_or_else(|| PathBuf::from(".shelly/memory")),
            top_k: 5,
            max_cognition_rounds: 3,
            embedding_model: "default".to_string(),
        }
    }
}
