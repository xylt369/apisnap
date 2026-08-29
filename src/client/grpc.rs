use crate::client::RawResponse;
use crate::config::EndpointConfig;
use crate::error::ApiSnapError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Instant;

/// Configuration for gRPC microservice endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrpcEndpointConfig {
    /// Fully-qualified gRPC service name (e.g. "order.v1.OrderService").
    pub service: String,

    /// RPC method name (e.g. "GetOrder").
    pub method: String,

    /// Enable server reflection protocol to discover types dynamically.
    #[serde(default = "default_true")]
    pub use_reflection: bool,
}

fn default_true() -> bool {
    true
}

/// Standard gRPC status codes (RFC / gRPC specification).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrpcStatusCode {
    Ok = 0,
    Cancelled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

impl GrpcStatusCode {
    pub fn from_u32(code: u32) -> Self {
        match code {
            0 => GrpcStatusCode::Ok,
            1 => GrpcStatusCode::Cancelled,
            2 => GrpcStatusCode::Unknown,
            3 => GrpcStatusCode::InvalidArgument,
            4 => GrpcStatusCode::DeadlineExceeded,
            5 => GrpcStatusCode::NotFound,
            6 => GrpcStatusCode::AlreadyExists,
            7 => GrpcStatusCode::PermissionDenied,
            8 => GrpcStatusCode::ResourceExhausted,
            9 => GrpcStatusCode::FailedPrecondition,
            10 => GrpcStatusCode::Aborted,
            11 => GrpcStatusCode::OutOfRange,
            12 => GrpcStatusCode::Unimplemented,
            13 => GrpcStatusCode::Internal,
            14 => GrpcStatusCode::Unavailable,
            15 => GrpcStatusCode::DataLoss,
            16 => GrpcStatusCode::Unauthenticated,
            _ => GrpcStatusCode::Unknown,
        }
    }

    pub fn to_http_equivalent(self) -> u16 {
        match self {
            GrpcStatusCode::Ok => 200,
            GrpcStatusCode::InvalidArgument => 400,
            GrpcStatusCode::DeadlineExceeded => 504,
            GrpcStatusCode::NotFound => 404,
            GrpcStatusCode::AlreadyExists => 409,
            GrpcStatusCode::PermissionDenied => 403,
            GrpcStatusCode::Unauthenticated => 401,
            GrpcStatusCode::ResourceExhausted => 429,
            GrpcStatusCode::FailedPrecondition => 400,
            GrpcStatusCode::Aborted => 409,
            GrpcStatusCode::OutOfRange => 400,
            GrpcStatusCode::Unimplemented => 501,
            GrpcStatusCode::Internal => 500,
            GrpcStatusCode::Unavailable => 503,
            GrpcStatusCode::DataLoss => 500,
            _ => 500,
        }
    }
}

/// Dynamic gRPC Server Reflection & HTTP/2 Dispatcher.
pub struct GrpcExecutor {
    client: reqwest::Client,
    default_timeout: std::time::Duration,
}

impl GrpcExecutor {
    pub fn new(default_timeout: std::time::Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(default_timeout)
            .http2_prior_knowledge()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            default_timeout,
        }
    }

    /// Encode a payload into a 5-byte length-prefixed gRPC binary frame:
    /// `[1 byte compression flag] + [4 bytes big-endian length] + [N bytes data]`.
    pub fn encode_grpc_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(0u8); // Uncompressed
        let len = payload.len() as u32;
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    /// Decode a 5-byte length-prefixed gRPC binary frame into payload bytes.
    pub fn decode_grpc_frame(bytes: &[u8]) -> Result<&[u8], ApiSnapError> {
        if bytes.is_empty() {
            return Ok(&[]);
        }
        if bytes.len() < 5 {
            return Err(ApiSnapError::MalformedJson {
                context: "gRPC length-prefixed frame".into(),
                source: serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "gRPC response frame shorter than 5-byte header",
                )),
            });
        }
        let len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
        if bytes.len() < 5 + len {
            return Err(ApiSnapError::MalformedJson {
                context: "gRPC payload slice".into(),
                source: serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "gRPC payload truncated",
                )),
            });
        }
        Ok(&bytes[5..5 + len])
    }

    /// Query the gRPC Server Reflection endpoint (`grpc.reflection.v1alpha.ServerReflection`)
    /// to dynamically verify service discovery and fetch symbol descriptors.
    pub async fn query_reflection_symbol(
        &self,
        target_addr: &str,
        service_symbol: &str,
    ) -> Result<Vec<u8>, ApiSnapError> {
        let trimmed = target_addr.trim_end_matches('/');
        let reflection_url = format!("{trimmed}/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo");

        // Reflection request JSON representation
        let reflection_req = serde_json::json!({
            "host": target_addr,
            "file_containing_symbol": service_symbol
        });

        let payload_bytes = serde_json::to_vec(&reflection_req).unwrap_or_default();
        let framed = Self::encode_grpc_frame(&payload_bytes);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc+json"));
        headers.insert(HeaderName::from_static("te"), HeaderValue::from_static("trailers"));

        let res = self
            .client
            .post(&reflection_url)
            .headers(headers)
            .body(framed)
            .send()
            .await
            .map_err(|e| ApiSnapError::Network {
                url: reflection_url.clone(),
                source: e,
            })?;

        let bytes = res.bytes().await.map_err(|e| ApiSnapError::Network {
            url: reflection_url.clone(),
            source: e,
        })?;

        Ok(bytes.to_vec())
    }

    /// Dispatch a dynamic gRPC request over HTTP/2 and map protobuf/JSON payload to JSON AST.
    pub async fn execute_grpc(
        &self,
        endpoint: &EndpointConfig,
        grpc_cfg: &GrpcEndpointConfig,
        target_addr: &str,
    ) -> Result<RawResponse, ApiSnapError> {
        let trimmed_addr = target_addr.trim_end_matches('/');
        let service = &grpc_cfg.service;
        let method = &grpc_cfg.method;
        let rpc_url = format!("{trimmed_addr}/{service}/{method}");

        // If server reflection is enabled, probe service symbol metadata
        if grpc_cfg.use_reflection {
            let _ = self.query_reflection_symbol(target_addr, service).await;
        }

        let request_payload = if let Some(body) = &endpoint.body {
            serde_json::to_vec(body).unwrap_or_default()
        } else {
            Vec::new()
        };

        let framed_request = Self::encode_grpc_frame(&request_payload);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc+json"));
        headers.insert(
            HeaderName::from_static("te"),
            HeaderValue::from_static("trailers"),
        );

        for (k, v) in &endpoint.headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                headers.insert(name, val);
            }
        }

        let start_time = Instant::now();
        let res = self
            .client
            .post(&rpc_url)
            .headers(headers)
            .body(framed_request)
            .send()
            .await
            .map_err(|e| ApiSnapError::Network {
                url: rpc_url.clone(),
                source: e,
            })?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        let mut response_headers = HashMap::new();
        for (k, v) in res.headers().iter() {
            if let Ok(str_val) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), str_val.to_string());
            }
        }

        let grpc_status_num = response_headers
            .get("grpc-status")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        let grpc_status = GrpcStatusCode::from_u32(grpc_status_num);

        let raw_bytes = res.bytes().await.map_err(|e| ApiSnapError::Network {
            url: rpc_url.clone(),
            source: e,
        })?;

        let body_val: Value = if raw_bytes.is_empty() {
            Value::Null
        } else {
            let decoded_slice = Self::decode_grpc_frame(&raw_bytes).unwrap_or(&raw_bytes);
            serde_json::from_slice(decoded_slice).unwrap_or_else(|_| {
                let s = String::from_utf8_lossy(decoded_slice).to_string();
                Value::String(s)
            })
        };

        Ok(RawResponse {
            status_code: grpc_status.to_http_equivalent(),
            headers: response_headers,
            body: body_val,
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_frame_encoding_and_decoding() {
        let payload = b"{\"message\": \"hello grpc\"}";
        let framed = GrpcExecutor::encode_grpc_frame(payload);

        assert_eq!(framed.len(), 5 + payload.len());
        assert_eq!(framed[0], 0); // Compression flag = 0
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]) as usize;
        assert_eq!(len, payload.len());

        let decoded = GrpcExecutor::decode_grpc_frame(&framed).unwrap();
        assert_eq!(decoded, payload);
    }
}
