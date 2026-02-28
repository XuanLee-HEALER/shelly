// Agent module - Core orchestration layer
// See docs/mainloop-design.md for design details

pub mod config;
pub mod error;
pub mod loop_;
pub mod types;

pub use loop_::AgentLoop;
pub use types::AgentConfig;
