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

    /// Global headers applied to every request (e.g. auth tokens), merged
    /// with (and overridden by) per-endpoint headers.
    #[serde(default)]
    pub global_headers: HashMap<String, String>,

    /// Global masking configuration, applied to every endpoint unless a
    /// per-endpoint `MaskingConfig` disables inheritance.
    #[serde(default)]
    pub masking: MaskingConfig,

    /// The set of endpoints under test.
    pub endpoints: Vec<EndpointConfig>,

    /// Directory where `.snap.json` files are stored, relative to config file.
    #[serde(default = "default_snapshot_dir")]
    pub snapshot_dir: String,
}

fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_concurrency() -> usize {
    10
}
fn default_snapshot_dir() -> String {
    "__snapshots__".to_string()
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
    /// Unique identifier used as the snapshot filename stem.
    pub name: String,

    pub method: HttpMethod,

    /// Path appended to `base_url`, may contain `{param}` style placeholders.
    pub path: String,

    #[serde(default)]
    pub headers: HashMap<String, String>,

    #[serde(default)]
    pub query_params: HashMap<String, String>,

    /// Raw JSON body, if applicable (POST/PUT/PATCH).
    #[serde(default)]
    pub body: Option<serde_json::Value>,

    /// Expected HTTP status code. If actual differs, this is reported as a
    /// top-level `DiffKind::TypeMismatch` on JSONPath `$.__status_code`.
    #[serde(default = "default_expected_status")]
    pub expected_status: u16,

    /// Per-endpoint timeout override.
    #[serde(with = "humantime_serde::option", default)]
    pub timeout_override: Option<Duration>,

    /// Endpoint-level masking overrides, merged on top of global masking
    /// (endpoint rules take precedence on JSONPath collision).
    #[serde(default)]
    pub mask_overrides: Vec<CustomMaskRule>,

    /// Array comparison modes for specific JSON paths: "ordered" (default) or "set".
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
    /// Enable built-in heuristic masking (ISO8601, UUIDv4, JWT, epoch, ObjectId).
    #[serde(default = "default_true")]
    pub enable_builtin_heuristics: bool,

    /// Global custom rules, keyed by JSONPath, applied before builtins.
    #[serde(default)]
    pub custom_rules: Vec<CustomMaskRule>,
}

impl Default for MaskingConfig {
    fn default() -> Self {
        Self {
            enable_builtin_heuristics: true,
            custom_rules: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// A single explicit masking rule targeting an exact JSONPath.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMaskRule {
    /// Exact JSONPath, e.g. "$.data.token" or "$.items[*].id".
    pub json_path: String,

    /// Replacement token written in place of the matched value,
    /// e.g. "<MASKED_TOKEN>".
    pub replacement: String,

    /// Optional regex; if present, only substrings matching this pattern
    /// within a string value are replaced (else the whole value is replaced).
    #[serde(default)]
    pub pattern: Option<String>,
}

impl ApiSnapConfig {
    /// Load configuration from a TOML or YAML file path.
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

    /// Generate a sample starter configuration.
    pub fn starter_template() -> String {
        r#"# ApiSnap Configuration File
base_url = "http://localhost:8000"
timeout = "30s"
concurrency = 10
snapshot_dir = "__snapshots__"

[global_headers]
"Accept" = "application/json"
"User-Agent" = "ApiSnap/0.1.0"

[masking]
enable_builtin_heuristics = true

# Example Global Custom Rule
# [[masking.custom_rules]]
# json_path = "$.data.secret_key"
# replacement = "<MASKED_SECRET>"

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
