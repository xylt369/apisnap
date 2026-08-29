use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiSnapError {
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("network error while calling '{url}': {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("request to '{url}' timed out after {timeout_ms}ms")]
    Timeout { url: String, timeout_ms: u64 },

    #[error("invalid configuration at '{location}': {reason}")]
    InvalidConfig { location: String, reason: String },

    #[error("malformed JSON in {context}: {source}")]
    MalformedJson {
        context: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("no snapshot found for endpoint '{endpoint_name}' at '{expected_path}'")]
    SnapshotNotFound {
        endpoint_name: String,
        expected_path: String,
    },

    #[error("snapshot mismatch for endpoint '{endpoint_name}': {diff_count} difference(s)")]
    DiffMismatch {
        endpoint_name: String,
        diff_count: usize,
    },

    #[error("fuzzing anomaly detected: {total_anomalies} server crash/leak anomaly(ies)")]
    FuzzAnomalyDetected {
        total_anomalies: usize,
    },

    #[error("OpenAPI contract drift detected: {drift_count} schema violation(s)")]
    OpenApiDrift {
        drift_count: usize,
    },

    #[error("execution error: {0}")]
    Execution(String),
}

impl ApiSnapError {
    pub fn exit_code(&self) -> i32 {
        match self {
            ApiSnapError::DiffMismatch { .. }
            | ApiSnapError::FuzzAnomalyDetected { .. }
            | ApiSnapError::OpenApiDrift { .. } => 1,
            ApiSnapError::Network { .. } | ApiSnapError::Timeout { .. } => 2,
            ApiSnapError::Io { .. }
            | ApiSnapError::InvalidConfig { .. }
            | ApiSnapError::MalformedJson { .. }
            | ApiSnapError::SnapshotNotFound { .. }
            | ApiSnapError::Execution(_) => 3,
        }
    }
}
