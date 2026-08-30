use crate::error::ApiSnapError;
use crate::snapshot::{SnapshotFile, SnapshotMetadata};
use crate::storage::{MerkleCasStore, NodeHash};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Ultra-lightweight pointer file (.ptr) for team branch and PR namespacing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MerkleSnapshotPointer {
    pub endpoint_name: String,
    pub root_hash: String,
    pub status_code: u16,
    pub duration_ms: u64,
    pub recorded_at: String,
    pub apisnap_version: String,
}

impl MerkleSnapshotPointer {
    pub fn new(
        endpoint_name: &str,
        root_hash: NodeHash,
        status_code: u16,
        duration_ms: u64,
    ) -> Self {
        Self {
            endpoint_name: endpoint_name.to_string(),
            root_hash: hex::encode(root_hash),
            status_code,
            duration_ms,
            recorded_at: chrono::Utc::now().to_rfc3339(),
            apisnap_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Save pointer to a branch directory (e.g. `__snapshots__/main/{endpoint}.ptr`)
    pub fn save(&self, branch_dir: &Path) -> Result<PathBuf, ApiSnapError> {
        fs::create_dir_all(branch_dir).map_err(|e| ApiSnapError::Io {
            path: branch_dir.display().to_string(),
            source: e,
        })?;

        let file_path = branch_dir.join(format!("{}.ptr", self.endpoint_name));
        let serialized = serde_json::to_string_pretty(self).map_err(|e| {
            ApiSnapError::InvalidConfig {
                location: file_path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        fs::write(&file_path, serialized).map_err(|e| ApiSnapError::Io {
            path: file_path.display().to_string(),
            source: e,
        })?;

        Ok(file_path)
    }

    /// Read pointer file from disk
    pub fn load(ptr_path: &Path) -> Result<Self, ApiSnapError> {
        let content = fs::read_to_string(ptr_path).map_err(|e| ApiSnapError::Io {
            path: ptr_path.display().to_string(),
            source: e,
        })?;

        serde_json::from_str(&content).map_err(|e| ApiSnapError::InvalidConfig {
            location: ptr_path.display().to_string(),
            reason: format!("Failed to parse pointer file: {e}"),
        })
    }

    /// Reconstruct the full `SnapshotFile` AST from the CAS store using this pointer.
    pub fn reconstruct(&self, cas: &mut MerkleCasStore) -> Result<SnapshotFile, ApiSnapError> {
        let hash_bytes = hex::decode(&self.root_hash).map_err(|e| ApiSnapError::InvalidConfig {
            location: self.endpoint_name.clone(),
            reason: format!("Invalid hex root hash: {e}"),
        })?;

        if hash_bytes.len() != 32 {
            return Err(ApiSnapError::InvalidConfig {
                location: self.endpoint_name.clone(),
                reason: "Root hash must be 32 bytes".into(),
            });
        }

        let mut node_hash = [0u8; 32];
        node_hash.copy_from_slice(&hash_bytes);

        let ast = cas.reconstruct(NodeHash(node_hash)).map_err(|e| ApiSnapError::Io {
            path: self.root_hash.clone(),
            source: e,
        })?;

        Ok(SnapshotFile {
            endpoint_name: self.endpoint_name.clone(),
            metadata: SnapshotMetadata {
                recorded_at: self.recorded_at.clone(),
                duration_ms: self.duration_ms,
                status_code: self.status_code,
                grpc_status_code: None,
                response_headers: Default::default(),
                apisnap_version: self.apisnap_version.clone(),
            },
            masked_body: ast,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_pointer_save_load_reconstruct() {
        let temp = tempdir().unwrap();
        let cas_dir = temp.path().join(".cas");
        let branch_dir = temp.path().join("main");

        let mut cas = MerkleCasStore::new(&cas_dir).unwrap();
        let sample_val = serde_json::json!({
            "service": "billing",
            "version": "v1.2"
        });

        let root_hash = cas.ingest(&sample_val).unwrap();
        let ptr = MerkleSnapshotPointer::new("billing_service", root_hash, 200, 15);

        let saved_path = ptr.save(&branch_dir).unwrap();
        assert!(saved_path.exists());

        let loaded = MerkleSnapshotPointer::load(&saved_path).unwrap();
        assert_eq!(loaded, ptr);

        let snapshot = loaded.reconstruct(&mut cas).unwrap();
        assert_eq!(snapshot.masked_body["service"], "billing");
        assert_eq!(snapshot.metadata.status_code, 200);
    }
}
