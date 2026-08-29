use crate::client::AuthConfig;
use crate::error::ApiSnapError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Top-level configuration loaded from `apisnap.toml` or `apisnap.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSnapConfig {
    /// Base URL prepended to every endpoint path (e.g. "https://api.example.com").
    pub base_url: String,

    /// Global request timeout, applied unless overridden per-endpoint.
    #[serde(with = "humantime_serde", default = "default_timeout")]
    pub timeout: Duration,

    /// Maximum number of concurrently in-flight requests.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

    /// Maximum AST traversal depth to prevent stack overflow on adversarial inputs.
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Float comparison tolerance epsilon. Default: 0.0 (exact).
    #[serde(default = "default_float_epsilon")]
    pub float_epsilon: f64,

    /// Normalize all JSON object keys to Unicode NFC form before comparison.
    #[serde(default = "default_true")]
    pub normalize_unicode_keys: bool,

    /// Global authentication provider configuration.
    #[serde(default)]
    pub auth: Option<AuthConfig>,

    /// Global headers applied to every request.
    #[serde(default)]
    pub global_headers: HashMap<String, String>,

    /// Global masking configuration.
    #[serde(default)]
    pub masking: MaskingConfig,

    /// The set of endpoints under test.
    pub endpoints: Vec<EndpointConfig>,

    /// Directory where `.snap.json` files are stored.
    #[serde(default = "default_snapshot_dir")]
    pub snapshot_dir: String,
}

fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_concurrency() -> usize {
    10
}
fn default_max_depth() -> usize {
    512
}
fn default_float_epsilon() -> f64 {
    0.0
}
fn default_snapshot_dir() -> String {
    "__snapshots__".to_string()
}
fn default_true() -> bool {
    true
}

/// HTTP verbs supported by the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
        }
    }
}

/// Configuration for a single API endpoint under test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,

    #[serde(default)]
    pub headers: HashMap<String, String>,

    #[serde(default)]
    pub query_params: HashMap<String, String>,

    #[serde(default)]
    pub body: Option<serde_json::Value>,

    #[serde(default = "default_expected_status")]
    pub expected_status: u16,

    #[serde(with = "humantime_serde::option", default)]
    pub timeout_override: Option<Duration>,

    #[serde(default)]
    pub float_epsilon_override: Option<f64>,

    #[serde(default)]
    pub auth_override: Option<AuthConfig>,

    #[serde(default)]
    pub mask_overrides: Vec<CustomMaskRule>,

    #[serde(default)]
    pub array_modes: HashMap<String, ArrayDiffMode>,
}

fn default_expected_status() -> u16 {
    200
}

/// Array diffing comparison mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArrayDiffMode {
    #[default]
    Ordered,
    Set,
}

/// Global masking behavior toggles and the list of custom rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaskingConfig {
    #[serde(default = "default_true")]
    pub enable_builtin_heuristics: bool,

    #[serde(default)]
    pub strict_pii_mode: bool,

    #[serde(default)]
    pub unmask_allow_list: Vec<String>,

    #[serde(default = "default_true")]
    pub pre_write_secret_scan: bool,

    #[serde(default)]
    pub custom_rules: Vec<CustomMaskRule>,
}

impl Default for MaskingConfig {
    fn default() -> Self {
        Self {
            enable_builtin_heuristics: true,
            strict_pii_mode: false,
            unmask_allow_list: Vec::new(),
            pre_write_secret_scan: true,
            custom_rules: Vec::new(),
        }
    }
}

/// A single explicit masking rule targeting an exact JSONPath.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMaskRule {
    pub json_path: String,
    pub replacement: String,
    #[serde(default)]
    pub pattern: Option<String>,
}

impl ApiSnapConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ApiSnapError> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|e| ApiSnapError::Io {
            path: path_ref.display().to_string(),
            source: e,
        })?;

        let extension = path_ref
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("toml");

        match extension {
            "yaml" | "yml" => serde_yaml::from_str(&content).map_err(|e| ApiSnapError::InvalidConfig {
                location: path_ref.display().to_string(),
                reason: e.to_string(),
            }),
            _ => toml::from_str(&content).map_err(|e| ApiSnapError::InvalidConfig {
                location: path_ref.display().to_string(),
                reason: e.to_string(),
            }),
        }
    }

    pub fn starter_template() -> String {
        r#"# ApiSnap Configuration File
base_url = "http://localhost:8000"
timeout = "30s"
concurrency = 10
max_depth = 512
float_epsilon = 0.0001
normalize_unicode_keys = true
snapshot_dir = "__snapshots__"

[global_headers]
"Accept" = "application/json"
"User-Agent" = "ApiSnap/0.3.0"

# Optional Enterprise Auth
# [auth]
# type = "bearer"
# token = "secret_token_123"

# [auth]
# type = "oauth2_client_credentials"
# token_url = "https://auth.example.com/oauth/token"
# client_id = "apisnap_client"
# client_secret = "secret_xyz"

[masking]
enable_builtin_heuristics = true
strict_pii_mode = false
pre_write_secret_scan = true

[[endpoints]]
name = "get_user_profile"
method = "GET"
path = "/api/v1/users/1"
expected_status = 200

[[endpoints]]
name = "create_order"
method = "POST"
path = "/api/v1/orders"
expected_status = 201

[endpoints.headers]
"Content-Type" = "application/json"

[endpoints.body]
item_id = "SKU-998"
quantity = 2
"#
        .to_string()
    }
}
