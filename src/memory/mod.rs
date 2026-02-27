// Memory module - stores agent context and history
// See docs/mem-design.md for design details

pub mod config;
pub mod error;
pub mod storage;
pub mod similarity;
pub mod types;

pub use config::MemoryConfig;
pub use error::MemoryError;
pub use storage::Memory;
pub use types::{JournalEntry, MemoryEntry};
