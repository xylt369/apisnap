use crate::config::{EndpointConfig, HttpMethod};
use crate::error::ApiSnapError;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, Instant};

/// Raw response captured from a network dispatch before masking.
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub duration_ms: u64,
}

/// Abstraction over the HTTP transport for unit and integration mock testing.
#[async_trait]
pub trait RequestExecutor: Send + Sync {
    async fn execute(
        &self,
        endpoint: &EndpointConfig,
        base_url: &str,
        global_headers: &HashMap<String, String>,
    ) -> Result<RawResponse, ApiSnapError>;
}

/// Production HTTP request dispatcher backed by `reqwest`.
#[derive(Clone)]
pub struct ReqwestExecutor {
    client: reqwest::Client,
    default_timeout: Duration,
}

impl ReqwestExecutor {
    pub fn new(default_timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(default_timeout)
            .pool_max_idle_per_host(20)
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true)
            .build()
            .expect("failed to construct reqwest client");

        Self {
            client,
            default_timeout,
        }
    }

    /// Constructs a client with a custom root CA for self-signed or staging environments.
    pub fn with_custom_tls(
        default_timeout: Duration,
        root_ca_pem: &[u8],
        client_identity_pem: Option<&[u8]>,
    ) -> Result<Self, ApiSnapError> {
        let cert = reqwest::Certificate::from_pem(root_ca_pem).map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: "tls.root_ca".into(),
                reason: e.to_string(),
            }
        })?;

        let mut builder = reqwest::Client::builder()
            .timeout(default_timeout)
            .add_root_certificate(cert)
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true);

        if let Some(identity_pem) = client_identity_pem {
            let identity = reqwest::Identity::from_pem(identity_pem).map_err(|e| {
                ApiSnapError::InvalidConfig {
                    location: "tls.client_identity".into(),
                    reason: e.to_string(),
                }
            })?;
            builder = builder.identity(identity);
        }

        let client = builder.build().map_err(|e| ApiSnapError::InvalidConfig {
            location: "tls.client_builder".into(),
            reason: e.to_string(),
        })?;

        Ok(Self {
            client,
            default_timeout,
        })
    }
}

#[async_trait]
impl RequestExecutor for ReqwestExecutor {
    async fn execute(
        &self,
        endpoint: &EndpointConfig,
        base_url: &str,
        global_headers: &HashMap<String, String>,
    ) -> Result<RawResponse, ApiSnapError> {
        let full_url = build_url(base_url, &endpoint.path, &endpoint.query_params);

        let req_method = match endpoint.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
        };

        let mut req_builder = self.client.request(req_method, &full_url);

        // Set timeout override if present
        if let Some(timeout) = endpoint.timeout_override {
            req_builder = req_builder.timeout(timeout);
        } else {
            req_builder = req_builder.timeout(self.default_timeout);
        }

        // Merge global and endpoint headers
        let mut header_map = HeaderMap::new();
        for (k, v) in global_headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                header_map.insert(name, val);
            }
        }
        for (k, v) in &endpoint.headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                header_map.insert(name, val);
            }
        }
        req_builder = req_builder.headers(header_map);

        // Attach request JSON body if present
        if let Some(body_val) = &endpoint.body {
            req_builder = req_builder.json(body_val);
        }

        let start_time = Instant::now();
        let res = req_builder.send().await.map_err(|e| {
            if e.is_timeout() {
                let timeout_val = endpoint
                    .timeout_override
                    .unwrap_or(self.default_timeout)
                    .as_millis() as u64;
                ApiSnapError::Timeout {
                    url: full_url.clone(),
                    timeout_ms: timeout_val,
                }
            } else {
                ApiSnapError::Network {
                    url: full_url.clone(),
                    source: e,
                }
            }
        })?;

        let duration_ms = start_time.elapsed().as_millis() as u64;
        let status_code = res.status().as_u16();

        // Extract headers
        let mut response_headers = HashMap::new();
        for (k, v) in res.headers().iter() {
            if let Ok(str_val) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), str_val.to_string());
            }
        }

        // Parse JSON body or fallback to string Value
        let body_bytes = res.bytes().await.map_err(|e| ApiSnapError::Network {
            url: full_url.clone(),
            source: e,
        })?;

        let body_val: Value = if body_bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body_bytes).unwrap_or_else(|_| {
                let s = String::from_utf8_lossy(&body_bytes).to_string();
                Value::String(s)
            })
        };

        Ok(RawResponse {
            status_code,
            headers: response_headers,
            body: body_val,
            duration_ms,
        })
    }
}

fn build_url(
    base_url: &str,
    path: &str,
    query_params: &HashMap<String, String>,
) -> String {
    let trimmed_base = base_url.trim_end_matches('/');
    let trimmed_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let mut url_str = format!("{trimmed_base}{trimmed_path}");
    if !query_params.is_empty() {
        let mut pairs = Vec::new();
        for (k, v) in query_params {
            pairs.push(format!("{k}={v}"));
        }
        url_str = format!("{url_str}?{}", pairs.join("&"));
    }

    url_str
}
