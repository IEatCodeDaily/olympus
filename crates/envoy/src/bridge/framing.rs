//! Incremental ACP wire framing independent of child-process I/O.

use anyhow::Result;

use super::acp::AcpMessage;

/// Incremental wire codec. `decode` consumes at most one complete frame and
/// leaves partial or subsequent frames in `buffer`.
pub trait Framing: Send + Sync {
    fn encode(&self, message: &AcpMessage) -> Result<Vec<u8>>;
    fn decode(&self, buffer: &mut Vec<u8>) -> Result<Option<AcpMessage>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NewlineJson;

impl Framing for NewlineJson {
    fn encode(&self, message: &AcpMessage) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(message)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn decode(&self, buffer: &mut Vec<u8>) -> Result<Option<AcpMessage>> {
        let Some(end) = buffer.iter().position(|byte| *byte == b'\n') else {
            return Ok(None);
        };
        let mut frame: Vec<u8> = buffer.drain(..=end).collect();
        frame.pop();
        if frame.last() == Some(&b'\r') {
            frame.pop();
        }
        if frame.iter().all(u8::is_ascii_whitespace) {
            return self.decode(buffer);
        }
        Ok(Some(serde_json::from_slice(&frame)?))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ContentLength;

impl Framing for ContentLength {
    fn encode(&self, message: &AcpMessage) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(message)?;
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend(body);
        Ok(frame)
    }

    fn decode(&self, buffer: &mut Vec<u8>) -> Result<Option<AcpMessage>> {
        let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
            return Ok(None);
        };
        let header = std::str::from_utf8(&buffer[..header_end])?;
        let content_length = header
            .split("\r\n")
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .ok_or_else(|| anyhow::anyhow!("ACP frame missing Content-Length header"))?
            .1
            .trim()
            .parse::<usize>()?;
        let body_start = header_end + 4;
        let frame_end = body_start + content_length;
        if buffer.len() < frame_end {
            return Ok(None);
        }
        let message = serde_json::from_slice(&buffer[body_start..frame_end])?;
        buffer.drain(..frame_end);
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bridge::acp::{AcpId, AcpRequest};

    fn fixture() -> AcpMessage {
        AcpMessage::Request(AcpRequest {
            jsonrpc: "2.0".into(),
            id: AcpId::from(7),
            method: "initialize".into(),
            params: json!({}),
        })
    }

    fn split_round_trip(codec: &dyn Framing) {
        let message = fixture();
        let encoded = codec.encode(&message).unwrap();
        let split = encoded.len() / 2;
        let mut buffer = encoded[..split].to_vec();
        assert_eq!(codec.decode(&mut buffer).unwrap(), None);
        buffer.extend_from_slice(&encoded[split..]);
        assert_eq!(codec.decode(&mut buffer).unwrap(), Some(message));
        assert!(buffer.is_empty());
    }

    #[test]
    fn newline_fixture_round_trips_across_split_buffer() {
        split_round_trip(&NewlineJson);
    }

    #[test]
    fn content_length_fixture_round_trips_across_split_buffer() {
        split_round_trip(&ContentLength);
    }

    #[test]
    fn newline_decoder_preserves_second_frame() {
        let codec = NewlineJson;
        let message = fixture();
        let mut buffer = codec.encode(&message).unwrap();
        buffer.extend(codec.encode(&message).unwrap());
        assert_eq!(codec.decode(&mut buffer).unwrap(), Some(message.clone()));
        assert_eq!(codec.decode(&mut buffer).unwrap(), Some(message));
        assert!(buffer.is_empty());
    }

    #[test]
    fn content_length_accepts_case_insensitive_header_with_extra_headers() {
        let body = serde_json::to_vec(&fixture()).unwrap();
        let mut buffer =
            format!("content-length: {}\r\nX-Test: yes\r\n\r\n", body.len()).into_bytes();
        buffer.extend(body);
        assert_eq!(ContentLength.decode(&mut buffer).unwrap(), Some(fixture()));
    }
}
