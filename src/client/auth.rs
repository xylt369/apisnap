use crate::error::ApiSnapError;
use async_trait::async_trait;
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Configuration variants for authentication providers in `apisnap.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    Bearer {
        token: String,
    },
    ApiKey {
        header_name: String,
        key: String,
    },
    Basic {
        username: String,
        password: Option<String>,
    },
    Oauth2ClientCredentials {
        token_url: String,
        client_id: String,
        client_secret: String,
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// Abstract authentication provider for enterprise gateway integration.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ApiSnapError>;
}

/// Factory function to construct an `AuthProvider` from configuration.
pub fn create_auth_provider(
    config: &AuthConfig,
    client: reqwest::Client,
) -> Arc<dyn AuthProvider> {
    match config {
        AuthConfig::Bearer { token } => Arc::new(StaticBearerAuth {
            token: token.clone(),
        }),
        AuthConfig::ApiKey { header_name, key } => Arc::new(ApiKeyAuth {
            header_name: header_name.clone(),
            key: key.clone(),
        }),
        AuthConfig::Basic { username, password } => Arc::new(BasicAuth {
            username: username.clone(),
            password: password.clone(),
        }),
        AuthConfig::Oauth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scopes,
        } => Arc::new(OAuth2ClientCredentialsAuth::new(
            client,
            token_url.clone(),
            client_id.clone(),
            client_secret.clone(),
            scopes.clone(),
        )),
    }
}

/// Static Bearer Token Authentication (`Authorization: Bearer <token>`).
pub struct StaticBearerAuth {
    token: String,
}

#[async_trait]
impl AuthProvider for StaticBearerAuth {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ApiSnapError> {
        let auth_val = format!("Bearer {}", self.token);
        if let Ok(val) = HeaderValue::from_str(&auth_val) {
            Ok(builder.header(AUTHORIZATION, val))
        } else {
            Err(ApiSnapError::InvalidConfig {
                location: "auth.bearer".into(),
                reason: "invalid characters in bearer token".into(),
            })
        }
    }
}

/// Custom API Key Header Authentication (e.g. `X-API-Key: <key>`).
pub struct ApiKeyAuth {
    header_name: String,
    key: String,
}

#[async_trait]
impl AuthProvider for ApiKeyAuth {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ApiSnapError> {
        let name = HeaderName::from_str(&self.header_name).map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: "auth.api_key.header_name".into(),
                reason: e.to_string(),
            }
        })?;
        let val = HeaderValue::from_str(&self.key).map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: "auth.api_key.key".into(),
                reason: e.to_string(),
            }
        })?;
        Ok(builder.header(name, val))
    }
}

/// HTTP Basic Authentication (`Authorization: Basic <base64>`).
pub struct BasicAuth {
    username: String,
    password: Option<String>,
}

#[async_trait]
impl AuthProvider for BasicAuth {
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ApiSnapError> {
        Ok(builder.basic_auth(&self.username, self.password.as_deref()))
    }
}

/// Token cache for OAuth2 Client Credentials flow.
#[derive(Debug, Clone)]
struct TokenCache {
    access_token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
struct OAuth2TokenResponse {
    access_token: String,
    #[serde(default = "default_token_expiry")]
    expires_in: u64,
}

fn default_token_expiry() -> u64 {
    3600
}

/// Self-refreshing OAuth2 Client Credentials Authentication Provider.
pub struct OAuth2ClientCredentialsAuth {
    client: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: String,
    scopes: Vec<String>,
    cache: Arc<RwLock<Option<TokenCache>>>,
}

impl OAuth2ClientCredentialsAuth {
    pub fn new(
        client: reqwest::Client,
        token_url: String,
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            client,
            token_url,
            client_id,
            client_secret,
            scopes,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    async fn get_valid_token(&self) -> Result<String, ApiSnapError> {
        // Fast path: read lock check
        {
            let read_guard = self.cache.read().await;
            if let Some(cache) = &*read_guard {
                // Buffer 30 seconds before expiry to avoid edge-race failures
                if Instant::now() + Duration::from_secs(30) < cache.expires_at {
                    return Ok(cache.access_token.clone());
                }
            }
        }

        // Slow path: write lock & fetch fresh token
        let mut write_guard = self.cache.write().await;
        // Double check after acquiring write lock
        if let Some(cache) = &*write_guard {
            if Instant::now() + Duration::from_secs(30) < cache.expires_at {
                return Ok(cache.access_token.clone());
            }
        }

        let mut form_params = vec![
            ("grant_type", "client_credentials"),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        let scope_str = self.scopes.join(" ");
        if !self.scopes.is_empty() {
            form_params.push(("scope", &scope_str));
        }

        let res = self
            .client
            .post(&self.token_url)
            .form(&form_params)
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
            ApiSnapError::MalformedJson {
                context: "OAuth2 token response".into(),
                source: e,
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
    async fn apply(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ApiSnapError> {
        let token = self.get_valid_token().await?;
        let auth_val = format!("Bearer {token}");
        if let Ok(val) = HeaderValue::from_str(&auth_val) {
            Ok(builder.header(AUTHORIZATION, val))
        } else {
            Err(ApiSnapError::InvalidConfig {
                location: "oauth2.token".into(),
                reason: "invalid header characters in retrieved OAuth2 token".into(),
            })
        }
    }
}
