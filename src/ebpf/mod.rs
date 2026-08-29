pub mod sniffer;

pub use sniffer::*;

use serde_json::Value;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CapturedPacket {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub payload_len: u32,
    pub payload: [u8; 4096],
}

/// Helper to parse HTTP JSON bodies directly from captured TCP byte streams.
pub fn extract_http_json_body(raw: &[u8]) -> Option<&[u8]> {
    let sep = b"\r\n\r\n";
    raw.windows(sep.len())
        .position(|w| w == sep)
        .map(|pos| &raw[pos + sep.len()..])
        .filter(|body| !body.is_empty())
}

/// Converts an unmasked eBPF captured HTTP event into a parsed JSON Value.
pub fn parse_captured_event(pkt: &CapturedPacket) -> Option<Value> {
    let len = pkt.payload_len.min(4096) as usize;
    let slice = &pkt.payload[..len];
    if let Some(json_bytes) = extract_http_json_body(slice) {
        serde_json::from_slice(json_bytes).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_http_json_body() {
        let raw_http = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"ok\"}";
        let body = extract_http_json_body(raw_http).unwrap();
        assert_eq!(body, b"{\"status\":\"ok\"}");

        let json_val: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(json_val["status"], "ok");
    }
}
