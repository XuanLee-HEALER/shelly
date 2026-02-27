use crate::comm::error::CommError;
use crate::comm::types::{MsgType, RequestPayload, ResponsePayload};
use rmp_serde::decode::Deserializer;
use rmp_serde::encode::Serializer;
use serde::Deserialize;
use std::io::Cursor;
use std::result::Result as StdResult;

/// Encode a packet with given type, sequence, and payload
pub fn encode_packet(
    msg_type: MsgType,
    seq: u32,
    payload: Option<&impl serde::Serialize>,
) -> StdResult<Vec<u8>, CommError> {
    let mut buf = Vec::new();

    // Write msg type (1 byte)
    buf.push(msg_type as u8);

    // Write seq (4 bytes, big-endian)
    buf.extend_from_slice(&seq.to_be_bytes());

    // Write payload if present
    if let Some(p) = payload {
        let mut ser = Serializer::new(&mut buf);
        p.serialize(&mut ser).map_err(|e| CommError::EncodeError(e.to_string()))?;
    }

    Ok(buf)
}

/// Decode packet type and seq from raw bytes
pub fn decode_header(data: &[u8]) -> StdResult<(MsgType, u32), CommError> {
    if data.len() < 5 {
        return Err(CommError::DecodeError(
            "Packet too short".to_string(),
        ));
    }

    let msg_type = MsgType::from_u8(data[0])
        .ok_or_else(|| CommError::DecodeError(format!("Unknown msg type: {}", data[0])))?;

    let seq = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);

    Ok((msg_type, seq))
}

/// Decode request payload
pub fn decode_request_payload(data: &[u8]) -> StdResult<RequestPayload, CommError> {
    let mut de = Deserializer::new(Cursor::new(data));
    RequestPayload::deserialize(&mut de).map_err(|e| CommError::DecodeError(e.to_string()))
}

/// Decode response payload
#[allow(dead_code)]
pub fn decode_response_payload(data: &[u8]) -> StdResult<ResponsePayload, CommError> {
    let mut de = Deserializer::new(Cursor::new(data));
    ResponsePayload::deserialize(&mut de).map_err(|e| CommError::DecodeError(e.to_string()))
}

/// Encode request ack (no payload)
pub fn encode_request_ack(seq: u32) -> StdResult<Vec<u8>, CommError> {
    encode_packet(MsgType::RequestAck, seq, None::<&()>)
}

/// Encode response
pub fn encode_response(seq: u32, payload: &ResponsePayload) -> StdResult<Vec<u8>, CommError> {
    encode_packet(MsgType::Response, seq, Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-CODEC-01: REQUEST 编码与解码
    #[test]
    fn test_request_encode_decode() {
        let payload = RequestPayload {
            content: "hello".to_string(),
        };
        let seq = 1u32;

        let packet = encode_packet(MsgType::Request, seq, Some(&payload)).unwrap();
        let (decoded_type, decoded_seq) = decode_header(&packet).unwrap();

        assert_eq!(decoded_type, MsgType::Request);
        assert_eq!(decoded_seq, seq);

        let decoded_payload = decode_request_payload(&packet[5..]).unwrap();
        assert_eq!(decoded_payload.content, "hello");
    }

    // T-CODEC-02: REQUEST_ACK 编码与解码
    #[test]
    fn test_request_ack_no_payload() {
        let seq = 42u32;
        let packet = encode_request_ack(seq).unwrap();

        assert_eq!(packet.len(), 5); // type (1) + seq (4)
        let (msg_type, decoded_seq) = decode_header(&packet).unwrap();
        assert_eq!(msg_type, MsgType::RequestAck);
        assert_eq!(decoded_seq, seq);
    }

    // T-CODEC-03: RESPONSE 编码与解码
    #[test]
    fn test_response_encode_decode() {
        let payload = ResponsePayload {
            content: "result".to_string(),
            is_error: false,
        };
        let seq = 1u32;

        let packet = encode_response(seq, &payload).unwrap();
        let (decoded_type, decoded_seq) = decode_header(&packet).unwrap();

        assert_eq!(decoded_type, MsgType::Response);
        assert_eq!(decoded_seq, seq);

        let decoded_payload = decode_response_payload(&packet[5..]).unwrap();
        assert_eq!(decoded_payload.content, "result");
        assert!(!decoded_payload.is_error);
    }

    // T-CODEC-04: RESPONSE is_error=true
    #[test]
    fn test_response_error() {
        let payload = ResponsePayload {
            content: "command not found".to_string(),
            is_error: true,
        };
        let seq = 1u32;

        let packet = encode_response(seq, &payload).unwrap();
        let decoded_payload = decode_response_payload(&packet[5..]).unwrap();

        assert!(decoded_payload.is_error);
        assert_eq!(decoded_payload.content, "command not found");
    }

    // T-CODEC-05: 空 payload REQUEST
    #[test]
    fn test_empty_content_request() {
        let payload = RequestPayload {
            content: "".to_string(),
        };
        let seq = 1u32;

        let packet = encode_packet(MsgType::Request, seq, Some(&payload)).unwrap();
        let decoded_payload = decode_request_payload(&packet[5..]).unwrap();

        assert_eq!(decoded_payload.content, "");
    }

    // T-CODEC-06: 大 payload (60000 字节)
    #[test]
    fn test_large_payload() {
        let large_content = "x".repeat(60000);
        let payload = RequestPayload {
            content: large_content.clone(),
        };
        let seq = 1u32;

        let packet = encode_packet(MsgType::Request, seq, Some(&payload)).unwrap();
        let decoded_payload = decode_request_payload(&packet[5..]).unwrap();

        assert_eq!(decoded_payload.content.len(), 60000);
        assert_eq!(decoded_payload.content, large_content);
    }

    // T-CODEC-08: 非法 type 值
    #[test]
    fn test_invalid_msg_type() {
        let mut packet = vec![0xFFu8];
        packet.extend_from_slice(&1u32.to_be_bytes());

        let result = decode_header(&packet);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CommError::DecodeError(_)));
    }

    // T-CODEC-09: 截断的包
    #[test]
    fn test_truncated_packet() {
        // Only 3 bytes (less than minimum 5 bytes)
        let result = decode_header(&[0x01, 0x00, 0x00]);
        assert!(result.is_err());

        // Exactly 5 bytes (no payload) - should succeed for header
        let result = decode_header(&[0x01, 0x00, 0x00, 0x00, 0x01]);
        assert!(result.is_ok());
    }

    // T-CODEC-10: seq 边界值
    #[test]
    fn test_seq_boundary_values() {
        // seq = 0
        let packet = encode_request_ack(0).unwrap();
        let (_, seq) = decode_header(&packet).unwrap();
        assert_eq!(seq, 0);

        // seq = u32::MAX
        let packet = encode_request_ack(u32::MAX).unwrap();
        let (_, seq) = decode_header(&packet).unwrap();
        assert_eq!(seq, u32::MAX);

        // seq = 256 (big-endian test)
        let packet = encode_request_ack(256).unwrap();
        let (_, seq) = decode_header(&packet).unwrap();
        assert_eq!(seq, 256);
        // Check big-endian encoding: 256 = 0x00000100
        assert_eq!([packet[1], packet[2], packet[3], packet[4]], [0x00, 0x00, 0x01, 0x00]);
    }

    // T-CODEC-11: payload 含特殊字符
    #[test]
    fn test_special_characters() {
        // UTF-8 multi-byte characters (Chinese, emoji)
        let payload = RequestPayload {
            content: "你好🌮🎉".to_string(),
        };
        let seq = 1u32;

        let packet = encode_packet(MsgType::Request, seq, Some(&payload)).unwrap();
        let decoded_payload = decode_request_payload(&packet[5..]).unwrap();

        assert_eq!(decoded_payload.content, "你好🌮🎉");

        // Special characters: \n, \0, \r\n
        let payload = RequestPayload {
            content: "line1\nline2\r\nnull\0end".to_string(),
        };
        let packet = encode_packet(MsgType::Request, seq, Some(&payload)).unwrap();
        let decoded_payload = decode_request_payload(&packet[5..]).unwrap();

        assert_eq!(decoded_payload.content, "line1\nline2\r\nnull\0end");
    }
}
