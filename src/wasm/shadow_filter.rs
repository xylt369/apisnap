use serde::{Deserialize, Serialize};
use simd_json::BorrowedValue;
use std::collections::BTreeSet;

/// Configuration for the Shadow Compare Wasm Filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowCompareConfig {
    pub baseline_cluster: String,
    pub candidate_cluster: String,
    pub max_body_buffer_bytes: usize,
}

impl Default for ShadowCompareConfig {
    fn default() -> Self {
        Self {
            baseline_cluster: "upstream_baseline".into(),
            candidate_cluster: "upstream_candidate".into(),
            max_body_buffer_bytes: 4 * 1024 * 1024, // 4MB buffer limit
        }
    }
}

/// In-Memory Buffer for asynchronous streaming accumulation in Wasm runtime.
pub struct ShadowSession {
    pub context_id: u32,
    pub baseline_body: Vec<u8>,
    pub candidate_body: Vec<u8>,
    pub streaming_complete: bool,
}

impl ShadowSession {
    pub fn new(context_id: u32) -> Self {
        Self {
            context_id,
            baseline_body: Vec::new(),
            candidate_body: Vec::new(),
            streaming_complete: false,
        }
    }

    /// Ingest a stream chunk based on `x-apisnap-shadow-role`.
    pub fn on_body_chunk(&mut self, role: &str, chunk: &[u8], end_of_stream: bool) {
        if role == "candidate" {
            self.candidate_body.extend_from_slice(chunk);
        } else {
            self.baseline_body.extend_from_slice(chunk);
        }

        if end_of_stream {
            self.streaming_complete = true;
        }
    }

    /// Perform sub-millisecond AST structural comparison.
    pub fn check_structural_drift(&mut self) -> Result<bool, String> {
        if self.baseline_body.is_empty() || self.candidate_body.is_empty() {
            return Ok(false); // Incomplete pairing, skip
        }

        let mut baseline_buf = self.baseline_body.clone();
        let mut candidate_buf = self.candidate_body.clone();

        let baseline_parsed = simd_json::to_borrowed_value(&mut baseline_buf)
            .map_err(|e| format!("baseline json parse error: {e}"))?;
        let candidate_parsed = simd_json::to_borrowed_value(&mut candidate_buf)
            .map_err(|e| format!("candidate json parse error: {e}"))?;

        Ok(structurally_drifted(&baseline_parsed, &candidate_parsed))
    }
}

/// 简化版结构漂移检测：比较 Object 的键集合与 Array 长度是否一致，
/// 避免在 Wasm 沙箱内构造完整 DiffReport 带来的额外分配开销。
pub fn structurally_drifted(
    a: &BorrowedValue,
    b: &BorrowedValue,
) -> bool {
    match (a, b) {
        (BorrowedValue::Object(ao), BorrowedValue::Object(bo)) => {
            let ak: BTreeSet<_> = ao.keys().collect();
            let bk: BTreeSet<_> = bo.keys().collect();
            if ak != bk {
                return true;
            }
            ao.iter().any(|(k, av)| {
                bo.get(k.as_ref())
                    .map_or(true, |bv| structurally_drifted(av, bv))
            })
        }
        (BorrowedValue::Object(_), _) | (_, BorrowedValue::Object(_)) => true,
        (BorrowedValue::Array(aa), BorrowedValue::Array(ba)) => {
            aa.len() != ba.len()
                || aa
                    .iter()
                    .zip(ba.iter())
                    .any(|(x, y)| structurally_drifted(x, y))
        }
        (BorrowedValue::Array(_), _) | (_, BorrowedValue::Array(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structural_drift_matching() {
        let mut base_raw = br#"{"id": 1, "name": "Alice", "tags": ["a", "b"]}"#.to_vec();
        let mut cand_raw = br#"{"name": "Bob", "id": 2, "tags": ["c", "d"]}"#.to_vec();

        let base_val = simd_json::to_borrowed_value(&mut base_raw).unwrap();
        let cand_val = simd_json::to_borrowed_value(&mut cand_raw).unwrap();

        // Same structure (same keys, array length = 2), values differ
        assert!(!structurally_drifted(&base_val, &cand_val));
    }

    #[test]
    fn test_structural_drift_detected_on_missing_key() {
        let mut base_raw = br#"{"id": 1, "profile": {"age": 25, "role": "admin"}}"#.to_vec();
        let mut cand_raw = br#"{"id": 1, "profile": {"age": 25}}"#.to_vec(); // missing role

        let base_val = simd_json::to_borrowed_value(&mut base_raw).unwrap();
        let cand_val = simd_json::to_borrowed_value(&mut cand_raw).unwrap();

        assert!(structurally_drifted(&base_val, &cand_val), "must detect missing key 'role'");
    }
}
