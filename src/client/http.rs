use crate::client::auth::AuthProvider;
use crate::client::{RawResponse, RequestExecutor};
use crate::config::{EndpointConfig, HttpMethod};
use crate::engine::FastJsonEngine;
use crate::error::ApiSnapError;
use async_trait::async_trait;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Standard HTTP request executor built on top of `reqwest`.
#[derive(Clone)]
pub struct ReqwestExecutor {
    client: reqwest::Client,
    default_timeout: Duration,
    fast_engine: Arc<FastJsonEngine>,
}

impl ReqwestExecutor {
    pub fn new(default_timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(default_timeout)
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true)
            .build()
            .expect("Failed to build reqwest HTTP client");

        Self {
            client,
            default_timeout,
            fast_engine: Arc::new(FastJsonEngine::default()),
        }
    }

    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// Construct with custom root CA certificate.
    pub fn with_custom_tls(
        root_ca_pem: &[u8],
        _client_identity_pem: Option<&[u8]>,
        default_timeout: Duration,
    ) -> Result<Self, ApiSnapError> {
        let cert = reqwest::Certificate::from_pem(root_ca_pem).map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: "tls.root_ca".into(),
                reason: e.to_string(),
            }
        })?;

        let builder = reqwest::Client::builder()
            .timeout(default_timeout)
            .add_root_certificate(cert)
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true);

        let client = builder.build().map_err(|e| ApiSnapError::InvalidConfig {
            location: "tls.client_builder".into(),
            reason: e.to_string(),
        })?;

        Ok(Self {
            client,
            default_timeout,
            fast_engine: Arc::new(FastJsonEngine::default()),
        })
    }

    fn resolve_url(&self, base_url: &str, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            let base = base_url.trim_end_matches('/');
            let sub = path.trim_start_matches('/');
            format!("{base}/{sub}")
        }
    }
}

impl Default for ReqwestExecutor {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[async_trait]
impl RequestExecutor for ReqwestExecutor {
    async fn execute(
        &self,
        endpoint: &EndpointConfig,
        base_url: &str,
        global_headers: &HashMap<String, String>,
        auth: Option<&dyn AuthProvider>,
    ) -> Result<RawResponse, ApiSnapError> {
        let method = match endpoint.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
        };

        let target_url = self.resolve_url(base_url, &endpoint.path);
        let mut req = self.client.request(method, &target_url);

        // Apply global headers first
        for (k, v) in global_headers {
            let h_name = HeaderName::from_str(k).map_err(|e| ApiSnapError::InvalidConfig {
                location: format!("global_headers.{}", k),
                reason: format!("invalid header name: {e}"),
            })?;
            let h_val = HeaderValue::from_str(v).map_err(|e| ApiSnapError::InvalidConfig {
                location: format!("global_headers.{}", k),
                reason: format!("invalid header value: {e}"),
            })?;
            req = req.header(h_name, h_val);
        }

        // Apply per-endpoint headers (override global)
        for (k, v) in &endpoint.headers {
            let h_name = HeaderName::from_str(k).map_err(|e| ApiSnapError::InvalidConfig {
                location: format!("endpoint '{}'.headers.{}", endpoint.name, k),
                reason: format!("invalid header name: {e}"),
            })?;
            let h_val = HeaderValue::from_str(v).map_err(|e| ApiSnapError::InvalidConfig {
                location: format!("endpoint '{}'.headers.{}", endpoint.name, k),
                reason: format!("invalid header value: {e}"),
            })?;
            req = req.header(h_name, h_val);
        }

        // Apply query params
        if !endpoint.query_params.is_empty() {
            req = req.query(&endpoint.query_params);
        }

        // Apply body
        if let Some(body) = &endpoint.body {
            req = req.json(body);
        }

        // Apply timeout override
        if let Some(timeout) = endpoint.timeout_override {
            req = req.timeout(timeout);
        } else {
            req = req.timeout(self.default_timeout);
        }

        // Apply Auth provider
        if let Some(auth_provider) = auth {
            req = auth_provider.apply(req).await?;
        }

        // Dispatch and benchmark execution time
        let start = Instant::now();
        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                ApiSnapError::Timeout {
                    url: target_url.clone(),
                    timeout_ms: endpoint.timeout_override.unwrap_or(self.default_timeout).as_millis() as u64,
                }
            } else {
                ApiSnapError::Network {
                    url: target_url.clone(),
                    source: e,
                }
            }
        })?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let status_code = response.status().as_u16();

        let mut headers = HashMap::new();
        for (k, v) in response.headers() {
            if let Ok(str_val) = v.to_str() {
                headers.insert(k.as_str().to_string(), str_val.to_string());
            }
        }

        let bytes = response.bytes().await.map_err(|e| ApiSnapError::Network {
            url: target_url.clone(),
            source: e,
        })?;

        // Empty body fallback
        if bytes.is_empty() {
            return Ok(RawResponse {
                body: Value::Null,
                status_code,
                headers,
                duration_ms,
            });
        }

        // Use SIMD-JSON for large payloads (>= 1MB) or fallback to Standard Parser
        let body: Value = if bytes.len() >= 1024 * 1024 {
            let mut mut_bytes = bytes.to_vec();
            self.fast_engine
                .parse_slice(&mut mut_bytes)
                .map_err(|e| ApiSnapError::MalformedJson {
                    context: format!("endpoint '{}' large response body", endpoint.name),
                    source: serde_json::Error::io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e,
                    )),
                })?
        } else {
            serde_json::from_slice(&bytes).map_err(|e| ApiSnapError::MalformedJson {
                context: format!("endpoint '{}' response body", endpoint.name),
                source: e,
            })?
        };

        Ok(RawResponse {
            body,
            status_code,
            headers,
            duration_ms,
        })
    }
}
