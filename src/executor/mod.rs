// Executor module - System operation executor
// See docs/executor-design.md for design details
#![allow(unused_imports)]

pub mod bash;
pub mod config;
pub mod error;
pub mod runner;
pub mod tool;
pub mod types;

pub use config::ExecutorConfig;
pub use error::{ExecutorError, Result};
pub use runner::Executor;
pub use tool::ToolImpl;
pub use types::{ExecutionConstraints, ToolOutput};
