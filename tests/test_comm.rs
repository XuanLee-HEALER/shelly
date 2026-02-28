// Integration tests for Comm module
// This file should be run with cargo test --test test_comm

#[path = "../src/comm/mod.rs"]
mod comm;

fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    });
}

// Message types (must match protocol)
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum MsgType {
    Request = 0x01,
    RequestAck = 0x02,
    Response = 0x03,
}

// Test helper: encode a request packet
fn encode_request(seq: u32, content: &str) -> Vec<u8> {
    use rmp_serde::encode::Serializer;
    use serde::Serialize;

    #[derive(Serialize)]
    struct RequestPayload<'a> {
        content: &'a str,
    }

    let payload = RequestPayload { content };
    let mut payload_bytes = Vec::new();
    let mut ser = Serializer::new(&mut payload_bytes);
    payload.serialize(&mut ser).unwrap();

    let mut packet = vec![MsgType::Request as u8];
    packet.extend_from_slice(&seq.to_be_bytes());
    packet.extend_from_slice(&payload_bytes);
    packet
}

// Test helper: decode response payload
fn decode_response(data: &[u8]) -> (u32, String, bool) {
    use rmp_serde::decode::Deserializer;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct ResponsePayload {
        content: String,
        is_error: bool,
    }

    let seq = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
    let mut de = Deserializer::new(&data[5..]);
    let resp: ResponsePayload = Deserialize::deserialize(&mut de).unwrap();
    (seq, resp.content, resp.is_error)
}

use std::time::Duration;
use tokio::net::UdpSocket;

#[cfg(test)]
mod integration_tests {
    use super::*;

    // T-FLOW-01: Normal request-response
    #[tokio::test]
    async fn test_normal_request_response() {
        init_tracing();

        let config = comm::CommConfig {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 0,
            max_payload_bytes: 65536,
            dedup_capacity: 256,
            dedup_ttl_secs: 300,
            recv_buffer_size: 65536,
        };

        let (comm, mut loop_rx) = comm::Comm::new(config).await.unwrap();
        let comm_addr = comm.local_addr().unwrap();

        // Spawn comm server
        tokio::spawn(async move {
            let _ = comm.run().await;
        });

        // Spawn mock main loop with receiver
        let mock_handle = tokio::spawn(async move {
            if let Some(req) = loop_rx.recv().await {
                req.reply
                    .send(comm::UserResponse::new("hello".to_string()))
                    .ok();
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(comm_addr).await.unwrap();

        let packet = encode_request(1, "test");
        client.send(&packet).await.unwrap();

        // Should receive ACK
        let mut buf = [0u8; 1024];
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(buf[0], MsgType::RequestAck as u8);

        // Wait for mock to complete
        let _ = tokio::time::timeout(Duration::from_secs(1), mock_handle).await;

        // Should receive RESPONSE
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(buf[0], MsgType::Response as u8);
        let (seq, content, is_error) = decode_response(&buf[..len]);
        assert_eq!(seq, 1);
        assert_eq!(content, "hello");
        assert!(!is_error);
    }

    // T-FLOW-04: Duplicate request deduplication
    // Test that duplicate requests are detected and only one request is forwarded to main loop
    #[tokio::test]
    async fn test_duplicate_request_dedup() {
        init_tracing();

        let config = comm::CommConfig {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 0,
            max_payload_bytes: 65536,
            dedup_capacity: 256,
            dedup_ttl_secs: 300,
            recv_buffer_size: 65536,
        };

        let (comm, mut loop_rx) = comm::Comm::new(config).await.unwrap();
        let comm_addr = comm.local_addr().unwrap();

        // Spawn comm server
        tokio::spawn(async move {
            let _ = comm.run().await;
        });

        // Track requests received by main loop
        let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<String>(10);

        // Spawn mock main loop - replies with a simple response (like the passing test)
        tokio::spawn(async move {
            while let Some(req) = loop_rx.recv().await {
                let content = req.content.clone();
                let _ = req_tx.send(content).await;
                // Reply to the request
                let _ = req.reply.send(comm::UserResponse::new("ok".to_string()));
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(comm_addr).await.unwrap();

        let packet = encode_request(1, "test");
        client.send(&packet).await.unwrap();

        // Get ACK for first
        let mut buf = [0u8; 1024];
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(buf[0], MsgType::RequestAck as u8);

        // Wait for response (from first request)
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(buf[0], MsgType::Response as u8);

        // Now send duplicate - should get cached response
        tokio::time::sleep(Duration::from_millis(50)).await;
        client.send(&packet).await.unwrap();

        // Should get cached response
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        // Should be Response (cached), not RequestAck
        assert_eq!(buf[0], MsgType::Response as u8);

        // Count received messages in main loop
        let mut received = Vec::new();
        while let Ok(Some(content)) =
            tokio::time::timeout(Duration::from_millis(100), req_rx.recv()).await
        {
            received.push(content);
        }

        // Main loop should have received only 1 request (not 2)
        assert_eq!(received.len(), 1, "Expected 1 request, got {:?}", received);
    }

    // T-EDGE-01: Empty packet - should be rejected
    #[tokio::test]
    async fn test_empty_packet() {
        init_tracing();

        let config = comm::CommConfig {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 0,
            max_payload_bytes: 65536,
            dedup_capacity: 256,
            dedup_ttl_secs: 300,
            recv_buffer_size: 65536,
        };
        let (comm, _rx) = comm::Comm::new(config).await.unwrap();
        let comm_addr = comm.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = comm.run().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send empty packet
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let _ = client.send_to(&[], comm_addr).await;

        // Should not crash - server continues
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // T-EDGE-04: Invalid REQUEST_ACK from client - should be ignored
    #[tokio::test]
    async fn test_invalid_request_ack_from_client() {
        init_tracing();

        let config = comm::CommConfig {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 0,
            max_payload_bytes: 65536,
            dedup_capacity: 256,
            dedup_ttl_secs: 300,
            recv_buffer_size: 65536,
        };
        let (comm, _rx) = comm::Comm::new(config).await.unwrap();
        let comm_addr = comm.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = comm.run().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send REQUEST_ACK (should be ignored - server->client only)
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut packet = vec![MsgType::RequestAck as u8];
        packet.extend_from_slice(&1u32.to_be_bytes());
        let _ = client.send_to(&packet, comm_addr).await;

        // Should not crash - server continues
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // T-EDGE-10: Daemon not running - client should timeout
    #[tokio::test]
    async fn test_client_timeout_no_daemon() {
        init_tracing();

        // Try to connect to port where nothing is running
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect("127.0.0.1:19999").await.unwrap();

        let packet = encode_request(1, "test");
        let _ = client.send(&packet).await;

        // Receiving should timeout
        let mut buf = [0u8; 1024];
        let result =
            tokio::time::timeout(Duration::from_millis(100), client.recv_from(&mut buf)).await;
        assert!(result.is_err()); // Timeout
    }
}
