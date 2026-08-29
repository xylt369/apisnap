use crate::crypto::SnapshotEncryptor;
use crate::engine::scan_unmasked_secrets;
use crate::error::ApiSnapError;
use crate::storage::{MerkleCasStore, NodeHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Metadata captured alongside a snapshot's masked body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotMetadata {
    pub recorded_at: String,
    pub duration_ms: u64,
    pub status_code: u16,
    #[serde(default)]
    pub grpc_status_code: Option<i32>,
    #[serde(default)]
    pub response_headers: HashMap<String, String>,
    pub apisnap_version: String,
}

/// The exact on-disk schema of a `.snap.json` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotFile {
    pub endpoint_name: String,
    pub metadata: SnapshotMetadata,
    pub masked_body: serde_json::Value,
}

/// Storage manager for snapshot files with atomic write guarantees, secret defense,
/// AES-256-GCM encryption, and Merkle DAG CAS deduplication.
pub struct SnapshotStore {
    base_dir: PathBuf,
    pre_write_secret_scan: bool,
    encryptor: Option<SnapshotEncryptor>,
    enable_cas: bool,
}

impl SnapshotStore {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            pre_write_secret_scan: true,
            encryptor: None,
            enable_cas: false,
        }
    }

    pub fn with_secret_scan(mut self, enabled: bool) -> Self {
        self.pre_write_secret_scan = enabled;
        self
    }

    pub fn with_encryptor(mut self, encryptor: Option<SnapshotEncryptor>) -> Self {
        self.encryptor = encryptor;
        self
    }

    pub fn with_cas(mut self, enabled: bool) -> Self {
        self.enable_cas = enabled;
        self
    }

    pub fn snapshot_path(&self, endpoint_name: &str) -> PathBuf {
        let safe_name = sanitize_filename(endpoint_name);
        if self.encryptor.is_some() {
            self.base_dir.join(format!("{safe_name}.snap.enc"))
        } else {
            self.base_dir.join(format!("{safe_name}.snap.json"))
        }
    }

    pub fn exists(&self, endpoint_name: &str) -> bool {
        let safe_name = sanitize_filename(endpoint_name);
        self.base_dir.join(format!("{safe_name}.snap.json")).exists()
            || self.base_dir.join(format!("{safe_name}.snap.enc")).exists()
    }

    pub fn read_snapshot(&self, endpoint_name: &str) -> Result<SnapshotFile, ApiSnapError> {
        let safe_name = sanitize_filename(endpoint_name);
        let enc_path = self.base_dir.join(format!("{safe_name}.snap.enc"));
        let plain_path = self.base_dir.join(format!("{safe_name}.snap.json"));

        let (target_path, is_encrypted) = if enc_path.exists() {
            (enc_path, true)
        } else if plain_path.exists() {
            (plain_path, false)
        } else {
            return Err(ApiSnapError::SnapshotNotFound {
                endpoint_name: endpoint_name.to_string(),
                expected_path: plain_path.display().to_string(),
            });
        };

        let raw_bytes = fs::read(&target_path).map_err(|e| ApiSnapError::Io {
            path: target_path.display().to_string(),
            source: e,
        })?;

        let json_bytes = if is_encrypted {
            if let Some(encryptor) = &self.encryptor {
                encryptor.decrypt(&raw_bytes)?
            } else {
                return Err(ApiSnapError::InvalidConfig {
                    location: target_path.display().to_string(),
                    reason: "snapshot is encrypted with AES-256-GCM; please provide APISNAP_MASTER_KEY to read".into(),
                });
            }
        } else {
            raw_bytes
        };

        serde_json::from_slice(&json_bytes).map_err(|e| ApiSnapError::MalformedJson {
            context: format!("snapshot file for '{}'", endpoint_name),
            source: e,
        })
    }

    /// Atomically write a snapshot file with pre-write secret defense, optional encryption,
    /// and transparent Merkle CAS subtree deduplication.
    pub fn write_snapshot_atomic(
        &self,
        snapshot: &SnapshotFile,
    ) -> Result<PathBuf, ApiSnapError> {
        // Pre-write defense: scan for leaked credentials
        if self.pre_write_secret_scan {
            if let Err(secret_err) = scan_unmasked_secrets(&snapshot.masked_body) {
                return Err(ApiSnapError::InvalidConfig {
                    location: format!("snapshot '{}'", snapshot.endpoint_name),
                    reason: format!("Pre-write secret guard blocked write: {secret_err}"),
                });
            }
        }

        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir).map_err(|e| ApiSnapError::Io {
                path: self.base_dir.display().to_string(),
                source: e,
            })?;
        }

        // Transparent CAS Ingestion if CAS is enabled
        if self.enable_cas {
            let cas_dir = self.base_dir.join(".cas");
            let mut cas_store = MerkleCasStore::new(&cas_dir).map_err(|e| ApiSnapError::Io {
                path: cas_dir.display().to_string(),
                source: e,
            })?;
            let _ = cas_store.ingest(&snapshot.masked_body).map_err(|e| ApiSnapError::Io {
                path: cas_dir.display().to_string(),
                source: e,
            })?;
        }

        let final_path = self.snapshot_path(&snapshot.endpoint_name);
        let tmp_path = final_path.with_extension("tmp");

        let serialized = serde_json::to_string_pretty(snapshot).map_err(|e| {
            ApiSnapError::MalformedJson {
                context: format!("serializing snapshot '{}'", snapshot.endpoint_name),
                source: e,
            }
        })?;

        let payload_bytes = if let Some(encryptor) = &self.encryptor {
            encryptor.encrypt(serialized.as_bytes())?
        } else {
            serialized.into_bytes()
        };

        // Write to temporary file and fsync
        {
            let mut file = File::create(&tmp_path).map_err(|e| ApiSnapError::Io {
                path: tmp_path.display().to_string(),
                source: e,
            })?;

            file.write_all(&payload_bytes).map_err(|e| ApiSnapError::Io {
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

    /// Explicit Merkle DAG CAS deduplicated ingestion (RFC-002 Module 1).
    pub fn write_snapshot_cas(
        &self,
        snapshot: &SnapshotFile,
    ) -> Result<(PathBuf, NodeHash), ApiSnapError> {
        let cas_dir = self.base_dir.join(".cas");
        let mut cas_store = MerkleCasStore::new(&cas_dir).map_err(|e| ApiSnapError::Io {
            path: cas_dir.display().to_string(),
            source: e,
        })?;

        let root_hash = cas_store.ingest(&snapshot.masked_body).map_err(|e| ApiSnapError::Io {
            path: cas_dir.display().to_string(),
            source: e,
        })?;

        let file_path = self.write_snapshot_atomic(snapshot)?;
        Ok((file_path, root_hash))
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
