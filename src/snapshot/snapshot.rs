use crate::error::ApiSnapError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Metadata captured alongside a snapshot's masked body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotMetadata {
    /// RFC3339 timestamp of when this snapshot was recorded.
    pub recorded_at: String,

    /// Wall-clock duration of the request that produced this snapshot.
    pub duration_ms: u64,

    pub status_code: u16,

    /// Response headers, with values masked via the same masking engine.
    #[serde(default)]
    pub response_headers: HashMap<String, String>,

    /// Semver of the apisnap binary that recorded this snapshot, used to
    /// detect format drift on load.
    pub apisnap_version: String,
}

/// The exact on-disk schema of a `.snap.json` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotFile {
    /// Matches `EndpointConfig::name`.
    pub endpoint_name: String,

    pub metadata: SnapshotMetadata,

    /// The masked response body AST, stored verbatim as `serde_json::Value`.
    pub masked_body: serde_json::Value,
}

/// Storage manager for snapshot files with atomic write guarantees.
pub struct SnapshotStore {
    base_dir: PathBuf,
}

impl SnapshotStore {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    /// Compute the path for a given endpoint's snapshot file.
    pub fn snapshot_path(&self, endpoint_name: &str) -> PathBuf {
        let safe_name = sanitize_filename(endpoint_name);
        self.base_dir.join(format!("{safe_name}.snap.json"))
    }

    /// Check if a snapshot exists for an endpoint.
    pub fn exists(&self, endpoint_name: &str) -> bool {
        self.snapshot_path(endpoint_name).exists()
    }

    /// Read and deserialize a snapshot file from disk.
    pub fn read_snapshot(&self, endpoint_name: &str) -> Result<SnapshotFile, ApiSnapError> {
        let path = self.snapshot_path(endpoint_name);
        if !path.exists() {
            return Err(ApiSnapError::SnapshotNotFound {
                endpoint_name: endpoint_name.to_string(),
                expected_path: path.display().to_string(),
            });
        }

        let content = fs::read_to_string(&path).map_err(|e| ApiSnapError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

        serde_json::from_str(&content).map_err(|e| ApiSnapError::MalformedJson {
            context: format!("snapshot file for '{}'", endpoint_name),
            source: e,
        })
    }

    /// Atomically write a snapshot file using a temporary sibling file and rename.
    pub fn write_snapshot_atomic(
        &self,
        snapshot: &SnapshotFile,
    ) -> Result<PathBuf, ApiSnapError> {
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir).map_err(|e| ApiSnapError::Io {
                path: self.base_dir.display().to_string(),
                source: e,
            })?;
        }

        let final_path = self.snapshot_path(&snapshot.endpoint_name);
        let tmp_path = final_path.with_extension("snap.json.tmp");

        let serialized = serde_json::to_string_pretty(snapshot).map_err(|e| {
            ApiSnapError::MalformedJson {
                context: format!("serializing snapshot '{}'", snapshot.endpoint_name),
                source: e,
            }
        })?;

        // Write to temporary file and fsync
        {
            let mut file = File::create(&tmp_path).map_err(|e| ApiSnapError::Io {
                path: tmp_path.display().to_string(),
                source: e,
            })?;

            file.write_all(serialized.as_bytes())
                .map_err(|e| ApiSnapError::Io {
                    path: tmp_path.display().to_string(),
                    source: e,
                })?;

            file.sync_all().map_err(|e| ApiSnapError::Io {
                path: tmp_path.display().to_string(),
                source: e,
            })?;
        }

        // Atomic rename replacing existing file
        fs::rename(&tmp_path, &final_path).map_err(|e| ApiSnapError::Io {
            path: final_path.display().to_string(),
            source: e,
        })?;

        Ok(final_path)
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
