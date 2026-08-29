use crate::error::ApiSnapError;
use bumpalo::Bump;
use serde_json::Value;

/// SIMD-accelerated and Arena-optimized JSON parsing and processing pipeline.
pub struct FastJsonEngine {
    threshold_bytes: usize,
}

impl Default for FastJsonEngine {
    fn default() -> Self {
        Self {
            threshold_bytes: 1024 * 1024, // 1MB threshold for SIMD switch
        }
    }
}

impl FastJsonEngine {
    pub fn new(threshold_bytes: usize) -> Self {
        Self { threshold_bytes }
    }

    /// Parse bytes using SIMD acceleration when payload exceeds size threshold.
    pub fn parse_slice(&self, bytes: &mut [u8]) -> Result<Value, ApiSnapError> {
        if bytes.len() >= self.threshold_bytes {
            // SIMD-accelerated path
            simd_json::from_slice(bytes).map_err(|e| ApiSnapError::MalformedJson {
                context: format!("simd-json fast parse ({} bytes)", bytes.len()),
                source: serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            })
        } else {
            // Standard path
            serde_json::from_slice(bytes).map_err(|e| ApiSnapError::MalformedJson {
                context: "serde_json parse".into(),
                source: e,
            })
        }
    }

    /// Run an operation within a thread-local scoped Bumpalo Arena allocator.
    pub fn with_arena<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Bump) -> R,
    {
        let bump = Bump::new();
        let result = f(&bump);
        // Arena is fully deallocated in one single pointer reset operation upon drop
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_fast_parse() {
        let engine = FastJsonEngine::new(10); // Low threshold for test
        let mut json_bytes = br#"{"large_array": [1, 2, 3, 4, 5], "status": "ok"}"#.to_vec();

        let val = engine.parse_slice(&mut json_bytes).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["large_array"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn test_arena_scope_execution() {
        let engine = FastJsonEngine::default();
        let sum = engine.with_arena(|bump| {
            let vec = bumpalo::vec![in bump; 10, 20, 30, 40];
            vec.iter().sum::<i32>()
        });
        assert_eq!(sum, 100);
    }
}
