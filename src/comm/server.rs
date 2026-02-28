use crate::comm::config::CommConfig;
use crate::comm::error::{CommError, CommInitError};
use crate::comm::protocol::{
    decode_header, decode_request_payload, encode_request_ack, encode_response,
};
use crate::comm::types::{MsgType, ResponsePayload, UserRequest, UserResponse};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::result::Result as StdResult;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Sequence deduplication entry
#[derive(Debug)]
struct DedupEntry {
    /// When this entry was created
    instant: Instant,
    /// Cached response to resend if duplicate
    cached_response: Option<Vec<u8>>,
}

/// Comm server - handles UDP communication with clients
pub struct Comm {
    socket: UdpSocket,
    config: CommConfig,
    /// Channel sender to forward UserRequests to main loop
    loop_sender: mpsc::Sender<UserRequest>,
    /// Sequence deduplication table per client
    dedup: Arc<tokio::sync::Mutex<HashMap<SocketAddr, HashMap<u32, DedupEntry>>>>,
}

impl Comm {
    /// Get local socket address
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

impl Comm {
    /// Create a new Comm instance and bind UDP socket
    /// Returns the comm instance and receiver for communication with main loop
    pub async fn new(
        config: CommConfig,
    ) -> StdResult<(Comm, mpsc::Receiver<UserRequest>), CommInitError> {
        let socket = UdpSocket::bind(config.bind_addr())
            .await
            .map_err(|e| CommInitError::BindFailed(e.to_string()))?;

        info!("Comm listening on {}", socket.local_addr().unwrap());

        let (tx, rx) = mpsc::channel(1024);

        Ok((
            Self {
                socket,
                config,
                loop_sender: tx,
                dedup: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            },
            rx,
        ))
    }

    /// Run the Comm server
    pub async fn run(self) -> StdResult<(), CommError> {
        let mut buf = vec![0u8; self.config.max_payload_bytes + 1024]; // Extra space for header
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            let packet = &buf[..len];
                            if let Err(e) = self.handle_packet(packet, addr).await {
                                warn!("Failed to handle packet from {}: {}", addr, e);
                            }
                        }
                        Err(e) => {
                            error!("Recv error: {}", e);
                            return Err(CommError::RecvError(e.to_string()));
                        }
                    }
                }
                _ = cleanup_interval.tick() => {
                    // Periodic cleanup of dedup table
                    self.cleanup_dedup().await;
                }
            }
        }
    }

    /// Handle incoming packet
    async fn handle_packet(
        &self,
        packet: &[u8],
        client_addr: SocketAddr,
    ) -> StdResult<(), CommError> {
        // Check for truncated packet (minimum: type + seq = 5 bytes)
        if packet.len() < 5 {
            warn!(
                "Truncated packet from {}: only {} bytes",
                client_addr,
                packet.len()
            );
            return Err(CommError::DecodeError("Packet too short".to_string()));
        }

        // Check payload size
        let payload_len = packet.len() - 5;
        if payload_len > self.config.max_payload_bytes {
            warn!(
                "Payload too large from {}: {} bytes",
                client_addr, payload_len
            );
            return Err(CommError::PayloadTooLarge(payload_len));
        }

        // Decode header
        let (msg_type, seq) = decode_header(packet)?;
        let payload = &packet[5..];

        debug!(
            "Received {} from {} seq={}",
            msg_type as u8, client_addr, seq
        );

        match msg_type {
            MsgType::Request => self.handle_request(payload, seq, client_addr).await,
            _ => {
                warn!(
                    "Unexpected message type: {} from {}",
                    msg_type as u8, client_addr
                );
                Ok(())
            }
        }
    }

    /// Handle incoming REQUEST
    async fn handle_request(
        &self,
        payload_bytes: &[u8],
        seq: u32,
        client_addr: SocketAddr,
    ) -> Result<(), CommError> {
        // Check for duplicate
        let is_dup = {
            let mut dedup = self.dedup.lock().await;
            let client_entries = dedup.entry(client_addr).or_insert_with(HashMap::new);

            // T-EDGE-07: Enforce capacity limit
            if client_entries.len() >= self.config.dedup_capacity {
                // Remove oldest entry to make room
                let oldest_seq = client_entries
                    .iter()
                    .min_by_key(|(_, e)| e.instant)
                    .map(|(seq, _)| *seq);
                if let Some(seq_to_remove) = oldest_seq {
                    client_entries.remove(&seq_to_remove);
                    debug!(
                        "Dedup table at capacity, removed oldest entry seq={}",
                        seq_to_remove
                    );
                }
            }

            match client_entries.entry(seq) {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    // Duplicate - return cached response if available
                    if let Some(ref cached) = entry.get().cached_response {
                        info!(
                            "Duplicate request seq={} from {}, resending cached response",
                            seq, client_addr
                        );
                        let cached_clone = cached.clone();
                        drop(dedup); // Release lock before sending
                        self.socket
                            .send_to(&cached_clone, client_addr)
                            .await
                            .map_err(|e| CommError::SendError(e.to_string()))?;
                    } else {
                        // No cached response yet (original request still being processed)
                        // Send ACK to indicate we're still working on it
                        debug!(
                            "Duplicate request seq={} from {}, no cached response yet, sending ACK",
                            seq, client_addr
                        );
                        let ack = encode_request_ack(seq)?;
                        drop(dedup);
                        self.socket
                            .send_to(&ack, client_addr)
                            .await
                            .map_err(|e| CommError::SendError(e.to_string()))?;
                    }
                    true
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    // New request - create dedup entry immediately (before processing)
                    // This ensures duplicate requests during processing are recognized
                    entry.insert(DedupEntry {
                        instant: Instant::now(),
                        cached_response: None,
                    });

                    // Decode payload
                    let request_payload = decode_request_payload(payload_bytes)?;

                    info!(
                        "New request seq={} from {} content_len={}",
                        seq,
                        client_addr,
                        request_payload.content.len()
                    );

                    // Send ACK immediately
                    let ack = encode_request_ack(seq)?;
                    self.socket
                        .send_to(&ack, client_addr)
                        .await
                        .map_err(|e| CommError::SendError(e.to_string()))?;
                    debug!("Sent REQUEST_ACK seq={} to {}", seq, client_addr);

                    // Create channel for response
                    let (reply_tx, reply_rx) = oneshot::channel::<UserResponse>();

                    // Send request to main loop
                    let user_request = UserRequest {
                        content: request_payload.content,
                        reply: reply_tx,
                        source_addr: client_addr,
                    };

                    // Drop dedup lock before sending to main loop and waiting for response
                    drop(dedup);
                    let send_result = self.loop_sender.send(user_request).await;

                    match send_result {
                        Ok(_) => {
                            // Wait for response from main loop
                            match timeout(Duration::from_secs(300), reply_rx).await {
                                Ok(Ok(response)) => {
                                    // Send response to client
                                    let response_payload = ResponsePayload {
                                        content: response.content,
                                        is_error: response.is_error,
                                    };
                                    let response_bytes = encode_response(seq, &response_payload)?;
                                    self.socket
                                        .send_to(&response_bytes, client_addr)
                                        .await
                                        .map_err(|e| CommError::SendError(e.to_string()))?;

                                    // Cache the response for deduplication
                                    let mut dedup = self.dedup.lock().await;
                                    if let Some(client_entries) = dedup.get_mut(&client_addr) {
                                        client_entries.insert(
                                            seq,
                                            DedupEntry {
                                                instant: Instant::now(),
                                                cached_response: Some(response_bytes),
                                            },
                                        );
                                    }
                                    debug!("Sent RESPONSE seq={} to {}", seq, client_addr);
                                }
                                Ok(Err(_)) => {
                                    // Channel closed without response
                                    warn!("Channel closed without response for seq={}", seq);
                                    let error_payload = ResponsePayload {
                                        content: "No response from handler".to_string(),
                                        is_error: true,
                                    };
                                    let response_bytes = encode_response(seq, &error_payload)?;
                                    self.socket
                                        .send_to(&response_bytes, client_addr)
                                        .await
                                        .map_err(|e| CommError::SendError(e.to_string()))?;
                                }
                                Err(_) => {
                                    // Timeout waiting for response
                                    warn!("Timeout waiting for response for seq={}", seq);
                                    let error_payload = ResponsePayload {
                                        content: "Response timeout".to_string(),
                                        is_error: true,
                                    };
                                    let response_bytes = encode_response(seq, &error_payload)?;
                                    self.socket
                                        .send_to(&response_bytes, client_addr)
                                        .await
                                        .map_err(|e| CommError::SendError(e.to_string()))?;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to send request to main loop: {}", e);
                            // Send error response to client
                            let error_payload = ResponsePayload {
                                content: "Internal server error".to_string(),
                                is_error: true,
                            };
                            let response = encode_response(seq, &error_payload)?;
                            self.socket
                                .send_to(&response, client_addr)
                                .await
                                .map_err(|e| CommError::SendError(e.to_string()))?;
                            return Err(CommError::ChannelClosed);
                        }
                    }

                    return Ok(());
                }
            }
        };

        if is_dup {
            debug!("Duplicate request seq={} from {}", seq, client_addr);
        }

        Ok(())
    }

    /// Cleanup expired entries from deduplication table
    async fn cleanup_dedup(&self) {
        let mut dedup = self.dedup.lock().await;
        let ttl = Duration::from_secs(self.config.dedup_ttl_secs);
        let now = Instant::now();

        for (_addr, entries) in dedup.iter_mut() {
            entries.retain(|_seq, entry| now.duration_since(entry.instant) < ttl);
        }

        // Clean up empty client entries
        dedup.retain(|_addr, entries| !entries.is_empty());

        debug!("Dedup table cleaned, {} clients tracked", dedup.len());
    }
}
