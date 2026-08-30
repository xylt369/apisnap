use crate::engine::{compare_json_ast, DiffKind, DiffOptions, DiffReport};
use crate::error::ApiSnapError;
use crate::storage::{MerkleCasStore, NodeHash};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Observation source for an API behavioral commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObservationSource {
    ManualRecord,
    CiPipeline { pr_id: Option<String> },
    ShadowProxy,
    EbpfCapture,
}

/// Structural delta summary between two consecutive timeline observations.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeltaSummary {
    pub fields_added: u32,
    pub fields_removed: u32,
    pub fields_type_changed: u32,
    pub latency_delta_ms: f64,
}

/// A historical observation commit in the API Behavioral Timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineCommit {
    pub commit_id: String,
    pub endpoint_name: String,
    pub parent_commit: Option<String>,
    pub observed_at: String,
    pub source: ObservationSource,
    pub response_root_hash: String,
    pub latency_ms: f64,
    pub status_code: u16,
    pub structural_delta_summary: DeltaSummary,
}

/// Timeline storage engine managing chronological append-only audit ledgers.
pub struct TimelineStore {
    base_dir: PathBuf,
}

impl TimelineStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Record a new behavioral observation point in the timeline.
    pub fn record_observation(
        &self,
        endpoint_name: &str,
        root_hash: NodeHash,
        latency_ms: f64,
        status_code: u16,
        source: ObservationSource,
        cas: &mut MerkleCasStore,
    ) -> Result<TimelineCommit, ApiSnapError> {
        let history = self.get_timeline(endpoint_name, 1)?;
        let parent = history.first();

        let mut delta_summary = DeltaSummary::default();
        if let Some(prev) = parent {
            delta_summary.latency_delta_ms = latency_ms - prev.latency_ms;

            let prev_hash_bytes = hex::decode(&prev.response_root_hash).unwrap_or_default();
            if prev_hash_bytes.len() == 32 {
                let mut prev_node_hash = [0u8; 32];
                prev_node_hash.copy_from_slice(&prev_hash_bytes);

                if let (Ok(old_ast), Ok(new_ast)) = (
                    cas.reconstruct(NodeHash(prev_node_hash)),
                    cas.reconstruct(root_hash),
                ) {
                    let diffs = compare_json_ast(&old_ast, &new_ast, &DiffOptions::default());
                    for d in diffs {
                        match d {
                            DiffKind::Added { .. } => delta_summary.fields_added += 1,
                            DiffKind::Removed { .. } => delta_summary.fields_removed += 1,
                            DiffKind::TypeMismatch { .. } => delta_summary.fields_type_changed += 1,
                            _ => {}
                        }
                    }
                }
            }
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        let commit_seed = format!("{endpoint_name}:{}:{timestamp}", hex::encode(root_hash));
        let commit_id = hex::encode(blake3::hash(commit_seed.as_bytes()).as_bytes());

        let commit = TimelineCommit {
            commit_id,
            endpoint_name: endpoint_name.to_string(),
            parent_commit: parent.map(|p| p.commit_id.clone()),
            observed_at: timestamp,
            source,
            response_root_hash: hex::encode(root_hash),
            latency_ms,
            status_code,
            structural_delta_summary: delta_summary,
        };

        self.append_commit(&commit)?;
        Ok(commit)
    }

    /// Retrieve chronological history for an endpoint (newest first).
    pub fn get_timeline(
        &self,
        endpoint_name: &str,
        limit: usize,
    ) -> Result<Vec<TimelineCommit>, ApiSnapError> {
        let file_path = self.timeline_file(endpoint_name);
        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&file_path).map_err(|e| ApiSnapError::Io {
            path: file_path.display().to_string(),
            source: e,
        })?;

        let mut commits = Vec::new();
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(c) = serde_json::from_str::<TimelineCommit>(line) {
                commits.push(c);
                if commits.len() >= limit {
                    break;
                }
            }
        }

        Ok(commits)
    }

    /// Diff two historical commits in time directly from the Merkle CAS.
    pub fn diff_historical_commits(
        &self,
        cas: &mut MerkleCasStore,
        commit_a: &TimelineCommit,
        commit_b: &TimelineCommit,
    ) -> Result<DiffReport, ApiSnapError> {
        let hash_a = parse_node_hash(&commit_a.response_root_hash)?;
        let hash_b = parse_node_hash(&commit_b.response_root_hash)?;

        let val_a = cas.reconstruct(hash_a).map_err(|e| ApiSnapError::Io {
            path: commit_a.response_root_hash.clone(),
            source: e,
        })?;
        let val_b = cas.reconstruct(hash_b).map_err(|e| ApiSnapError::Io {
            path: commit_b.response_root_hash.clone(),
            source: e,
        })?;

        let diffs = compare_json_ast(&val_a, &val_b, &DiffOptions::default());
        let is_match = diffs.is_empty();
        Ok(DiffReport {
            endpoint_name: format!("{} (Timeline Diff)", commit_a.endpoint_name),
            differences: diffs,
            is_match,
            expected_status: commit_a.status_code,
            actual_status: commit_b.status_code,
            trace_context: None,
            trace_link: None,
        })
    }

    fn append_commit(&self, commit: &TimelineCommit) -> Result<(), ApiSnapError> {
        let file_path = self.timeline_file(&commit.endpoint_name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| ApiSnapError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }

        let json_line = serde_json::to_string(commit).map_err(|e| ApiSnapError::InvalidConfig {
            location: file_path.display().to_string(),
            reason: e.to_string(),
        })?;

        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| ApiSnapError::Io {
                path: file_path.display().to_string(),
                source: e,
            })?;

        writeln!(file, "{json_line}").map_err(|e| ApiSnapError::Io {
            path: file_path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }

    fn timeline_file(&self, endpoint_name: &str) -> PathBuf {
        self.base_dir.join("timeline").join(format!("{endpoint_name}.jsonl"))
    }
}

fn parse_node_hash(hex_str: &str) -> Result<NodeHash, ApiSnapError> {
    let bytes = hex::decode(hex_str).map_err(|e| ApiSnapError::InvalidConfig {
        location: hex_str.to_string(),
        reason: format!("Invalid hex hash: {e}"),
    })?;

    if bytes.len() != 32 {
        return Err(ApiSnapError::InvalidConfig {
            location: hex_str.to_string(),
            reason: "Hash must be 32 bytes".into(),
        });
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(NodeHash(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_timeline_recording_and_historical_diff() {
        let temp = tempdir().unwrap();
        let cas_dir = temp.path().join(".cas");
        let mut cas = MerkleCasStore::new(&cas_dir).unwrap();
        let timeline = TimelineStore::new(&cas_dir);

        let ast_v1 = serde_json::json!({ "version": "1.0", "status": "ok" });
        let hash_v1 = cas.ingest(&ast_v1).unwrap();
        let c1 = timeline
            .record_observation(
                "user_api",
                hash_v1,
                12.5,
                200,
                ObservationSource::ManualRecord,
                &mut cas,
            )
            .unwrap();

        let ast_v2 = serde_json::json!({ "version": "2.0", "status": "ok", "new_field": true });
        let hash_v2 = cas.ingest(&ast_v2).unwrap();
        let c2 = timeline
            .record_observation(
                "user_api",
                hash_v2,
                15.0,
                200,
                ObservationSource::CiPipeline { pr_id: Some("101".into()) },
                &mut cas,
            )
            .unwrap();

        assert_eq!(c2.parent_commit, Some(c1.commit_id.clone()));
        assert_eq!(c2.structural_delta_summary.fields_added, 1);

        let diff_report = timeline.diff_historical_commits(&mut cas, &c1, &c2).unwrap();
        assert!(!diff_report.differences.is_empty());
    }
}
