pub mod auth;
pub mod grpc;
pub mod http;
pub mod proxy_capture;

pub use auth::*;
pub use grpc::*;
pub use http::*;
pub use proxy_capture::*;

use crate::config::EndpointConfig;
use crate::error::ApiSnapError;
use crate::telemetry::TraceContext;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RawResponse {
    pub body: Value,
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub duration_ms: u64,
    pub trace_context: Option<TraceContext>,
}

#[async_trait]
pub trait RequestExecutor: Send + Sync {
    async fn execute(
        &self,
        endpoint: &EndpointConfig,
        base_url: &str,
        global_headers: &HashMap<String, String>,
        auth: Option<&dyn auth::AuthProvider>,
    ) -> Result<RawResponse, ApiSnapError>;
}
