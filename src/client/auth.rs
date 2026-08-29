use crate::error::ApiSnapError;
use async_trait::async_trait;
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Configuration for API authentication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    /// Static Bearer token in the `Authorization: Bearer <TOKEN>` header.
    Bearer { token: String },

    /// API Key passed via header (e.g. `X-API-Key: <VALUE>`).
    ApiKey {
        header_name: String,
        api_key: String,
    },

    /// Basic HTTP Authentication (`Authorization: Basic <BASE64>`).
    Basic {
        username: String,
        password: Option<String>,
    },

    /// OAuth2 Client Credentials grant with auto-refreshing token cache.
    Oauth2ClientCredentials {
        token_url: String,
        client_id: String,
        client_secret: String,
        #[serde(default)]
        scopes: Vec<String>,
        #[serde(default)]
        custom_params: HashMap<String, String>,
    },
}

/// Abstract authentication provider for decorating outgoing requests.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Mutate and return the request builder with appropriate auth headers applied.
    async fn apply(&self, builder: RequestBuilder) -> Result<RequestBuilder, ApiSnapError>;
}

/// Static Bearer token provider.
pub struct StaticBearerAuth {
    token: String,
}

impl StaticBearerAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

#[async_trait]
impl AuthProvider for StaticBearerAuth {
    async fn apply(&self, builder: RequestBuilder) -> Result<RequestBuilder, ApiSnapError> {
        let auth_val = format!("Bearer {}", self.token);
        let header_val = HeaderValue::from_str(&auth_val).map_err(|e| ApiSnapError::InvalidConfig {
            location: "auth.bearer.token".into(),
            reason: format!("invalid header value: {e}"),
        })?;
        Ok(builder.header(AUTHORIZATION, header_val))
    }
}

/// Header-based API Key provider.
pub struct ApiKeyAuth {
    header_name: HeaderName,
    api_key: HeaderValue,
}

impl ApiKeyAuth {
    pub fn new(header_name: &str, api_key: &str) -> Result<Self, ApiSnapError> {
        let h_name = HeaderName::from_str(header_name).map_err(|e| ApiSnapError::InvalidConfig {
            location: "auth.api_key.header_name".into(),
            reason: format!("invalid header name '{header_name}': {e}"),
        })?;
        let h_val = HeaderValue::from_str(api_key).map_err(|e| ApiSnapError::InvalidConfig {
            location: "auth.api_key.api_key".into(),
            reason: format!("invalid header value: {e}"),
        })?;
        Ok(Self {
            header_name: h_name,
            api_key: h_val,
        })
    }
}

#[async_trait]
impl AuthProvider for ApiKeyAuth {
    async fn apply(&self, builder: RequestBuilder) -> Result<RequestBuilder, ApiSnapError> {
        Ok(builder.header(self.header_name.clone(), self.api_key.clone()))
    }
}

/// HTTP Basic Authentication provider.
pub struct BasicAuth {
    username: String,
    password: Option<String>,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: Option<String>) -> Self {
        Self {
            username: username.into(),
            password,
        }
    }
}

#[async_trait]
impl AuthProvider for BasicAuth {
    async fn apply(&self, builder: RequestBuilder) -> Result<RequestBuilder, ApiSnapError> {
        Ok(builder.basic_auth(&self.username, self.password.as_deref()))
    }
}

/// Token cached in memory with expiration tracking.
#[derive(Debug, Clone)]
struct TokenCache {
    access_token: String,
    expires_at: Instant,
}

/// Self-refreshing OAuth2 Client Credentials Token Provider.
pub struct OAuth2ClientCredentialsAuth {
    token_url: String,
    client_id: String,
    client_secret: String,
    scopes: Vec<String>,
    custom_params: HashMap<String, String>,
    client: reqwest::Client,
    cache: Arc<RwLock<Option<TokenCache>>>,
}

#[derive(Deserialize)]
struct OAuth2TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_expires_in() -> u64 {
    3600 // 1 hour default
}

impl OAuth2ClientCredentialsAuth {
    pub fn new(
        token_url: String,
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
        custom_params: HashMap<String, String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            token_url,
            client_id,
            client_secret,
            scopes,
            custom_params,
            client,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    async fn get_valid_token(&self) -> Result<String, ApiSnapError> {
        // Fast path: Read lock check
        {
            let read_guard = self.cache.read().await;
            if let Some(cache) = &*read_guard {
                // Refresh if expiring within 30 seconds
                if Instant::now() + Duration::from_secs(30) < cache.expires_at {
                    return Ok(cache.access_token.clone());
                }
            }
        }

        // Slow path: Acquire write lock and fetch new token
        let mut write_guard = self.cache.write().await;
        // Double-check condition after write lock acquisition
        if let Some(cache) = &*write_guard {
            if Instant::now() + Duration::from_secs(30) < cache.expires_at {
                return Ok(cache.access_token.clone());
            }
        }

        let mut form = HashMap::new();
        form.insert("grant_type", "client_credentials".to_string());
        form.insert("client_id", self.client_id.clone());
        form.insert("client_secret", self.client_secret.clone());

        if !self.scopes.is_empty() {
            form.insert("scope", self.scopes.join(" "));
        }
        for (k, v) in &self.custom_params {
            form.insert(k.as_str(), v.clone());
        }

        let res = self
            .client
            .post(&self.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| ApiSnapError::Network {
                url: self.token_url.clone(),
                source: e,
            })?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(ApiSnapError::InvalidConfig {
                location: format!("oauth2.token_endpoint '{}'", self.token_url),
                reason: format!("Token endpoint returned HTTP {status}: {body}"),
            });
        }

        let token_data: OAuth2TokenResponse = res.json().await.map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: format!("oauth2.token_response from '{}'", self.token_url),
                reason: format!("failed to parse token response: {e}"),
            }
        })?;

        let expires_at = Instant::now() + Duration::from_secs(token_data.expires_in);
        let access_token = token_data.access_token.clone();

        *write_guard = Some(TokenCache {
            access_token: token_data.access_token,
            expires_at,
        });

        Ok(access_token)
    }
}

#[async_trait]
impl AuthProvider for OAuth2ClientCredentialsAuth {
    async fn apply(&self, builder: RequestBuilder) -> Result<RequestBuilder, ApiSnapError> {
        let token = self.get_valid_token().await?;
        let auth_val = format!("Bearer {token}");
        let header_val = HeaderValue::from_str(&auth_val).map_err(|e| ApiSnapError::InvalidConfig {
            location: "oauth2.token".into(),
            reason: format!("invalid token header value: {e}"),
        })?;
        Ok(builder.header(AUTHORIZATION, header_val))
    }
}

/// Helper function to construct an `AuthProvider` from configuration.
pub fn create_auth_provider(
    config: &AuthConfig,
    client: reqwest::Client,
) -> Arc<dyn AuthProvider> {
    match config {
        AuthConfig::Bearer { token } => Arc::new(StaticBearerAuth::new(token)),
        AuthConfig::ApiKey {
            header_name,
            api_key,
        } => Arc::new(
            ApiKeyAuth::new(header_name, api_key)
                .expect("Failed to initialize API key auth provider"),
        ),
        AuthConfig::Basic { username, password } => {
            Arc::new(BasicAuth::new(username, password.clone()))
        }
        AuthConfig::Oauth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scopes,
            custom_params,
        } => Arc::new(OAuth2ClientCredentialsAuth::new(
            token_url.clone(),
            client_id.clone(),
            client_secret.clone(),
            scopes.clone(),
            custom_params.clone(),
            client,
        )),
    }
}
