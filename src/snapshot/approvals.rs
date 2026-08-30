use crate::error::ApiSnapError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Entry in the intentional diff approval ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffApproval {
    pub endpoint_name: String,
    pub approved_by: String,
    pub reason: String,
    pub approved_at: String,
}

/// Ledger storing approved intentional schema changes.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ApprovalLedger {
    pub approvals: HashMap<String, DiffApproval>,
}

impl ApprovalLedger {
    pub fn load_from_dir(base_dir: &Path) -> Result<Self, ApiSnapError> {
        let path = Self::ledger_path(base_dir);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).map_err(|e| ApiSnapError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

        serde_json::from_str(&content).map_err(|e| ApiSnapError::InvalidConfig {
            location: path.display().to_string(),
            reason: format!("Failed to parse approvals ledger: {e}"),
        })
    }

    pub fn approve(
        &mut self,
        endpoint_name: &str,
        author: &str,
        reason: &str,
        base_dir: &Path,
    ) -> Result<(), ApiSnapError> {
        let approval = DiffApproval {
            endpoint_name: endpoint_name.to_string(),
            approved_by: author.to_string(),
            reason: reason.to_string(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        };

        self.approvals.insert(endpoint_name.to_string(), approval);
        self.save(base_dir)?;
        Ok(())
    }

    pub fn is_approved(&self, endpoint_name: &str) -> bool {
        self.approvals.contains_key(endpoint_name)
    }

    pub fn consume_approval(&mut self, endpoint_name: &str, base_dir: &Path) -> Option<DiffApproval> {
        let removed = self.approvals.remove(endpoint_name);
        if removed.is_some() {
            let _ = self.save(base_dir);
        }
        removed
    }

    fn save(&self, base_dir: &Path) -> Result<(), ApiSnapError> {
        let path = Self::ledger_path(base_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ApiSnapError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }

        let json_str = serde_json::to_string_pretty(self).map_err(|e| ApiSnapError::InvalidConfig {
            location: path.display().to_string(),
            reason: e.to_string(),
        })?;

        fs::write(&path, json_str).map_err(|e| ApiSnapError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }

    fn ledger_path(base_dir: &Path) -> PathBuf {
        base_dir.join(".approved_diffs.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_approval_ledger_lifecycle() {
        let temp = tempdir().unwrap();
        let mut ledger = ApprovalLedger::load_from_dir(temp.path()).unwrap();

        assert!(!ledger.is_approved("get_user"));

        ledger
            .approve("get_user", "dev@apisnap.io", "Refactored user response", temp.path())
            .unwrap();

        assert!(ledger.is_approved("get_user"));

        // Re-load from disk
        let loaded = ApprovalLedger::load_from_dir(temp.path()).unwrap();
        assert!(loaded.is_approved("get_user"));
        assert_eq!(
            loaded.approvals.get("get_user").unwrap().reason,
            "Refactored user response"
        );
    }
}
