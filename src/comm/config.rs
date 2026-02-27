use std::net::SocketAddr;

/// Comm module configuration
#[derive(Debug, Clone)]
pub struct CommConfig {
    /// Listen address (default: 0.0.0.0)
    pub listen_addr: String,
    /// Listen port (default: 9700)
    pub listen_port: u16,
    /// Maximum payload size in bytes (default: 65536)
    pub max_payload_bytes: usize,
    /// UDP receive buffer size (default: 65536)
    #[allow(dead_code)]
    pub recv_buffer_size: usize,
    /// Deduplication table capacity per client (default: 256)
    pub dedup_capacity: usize,
    /// Deduplication entry TTL in seconds (default: 300)
    pub dedup_ttl_secs: u64,
}

impl Default for CommConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 9700,
            max_payload_bytes: 65536,
            recv_buffer_size: 65536,
            dedup_capacity: 256,
            dedup_ttl_secs: 300,
        }
    }
}

impl CommConfig {
    /// Returns the socket address to bind to
    pub fn bind_addr(&self) -> SocketAddr {
        format!("{}:{}", self.listen_addr, self.listen_port)
            .parse()
            .expect("Invalid bind address")
    }
}
