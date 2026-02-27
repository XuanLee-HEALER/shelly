// Comm module - UDP communication with external clients
// See docs/comm-design.md for design details

pub mod config;
pub mod error;
pub mod protocol;
pub mod server;
pub mod types;

pub use config::CommConfig;
pub use server::Comm;
pub use types::UserResponse;
#[allow(unused_imports)]
pub use types::UserRequest;
