use bumpalo::Bump;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

/// High-throughput JSON parser leveraging SIMD hardware acceleration
/// and Scoped Bumpalo Arena Allocation.
pub struct FastJsonEngine {
    simd_threshold_bytes: usize,
    arena_capacity: usize,
    fast_parse_hits: AtomicUsize,
}

impl Default for FastJsonEngine {
    fn default() -> Self {
        Self {
            simd_threshold_bytes: 1024 * 1024, // 1MB payload threshold
            arena_capacity: 512 * 1024,        // 512KB pre-allocated chunk
            fast_parse_hits: AtomicUsize::new(0),
        }
    }
}

impl FastJsonEngine {
    pub fn new(simd_threshold_bytes: usize) -> Self {
        Self {
            simd_threshold_bytes,
            ..Default::default()
        }
    }

    /// Parses a mutable byte buffer using SIMD-JSON if size exceeds threshold.
    pub fn parse_slice(&self, buffer: &mut [u8]) -> Result<Value, String> {
        if buffer.len() >= self.simd_threshold_bytes {
            self.fast_parse_hits.fetch_add(1, Ordering::Relaxed);
            simd_json::from_slice(buffer)
                .map_err(|e| format!("SIMD-JSON parsing failed: {e}"))
        } else {
            serde_json::from_slice(buffer).map_err(|e| format!("Serde JSON parsing failed: {e}"))
        }
    }

    pub fn fast_parse_count(&self) -> usize {
        self.fast_parse_hits.load(Ordering::Relaxed)
    }

    /// Allocates an isolated arena for zero-cost per-request scratch memory.
    pub fn with_arena<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&Bump) -> T,
    {
        let bump = Bump::with_capacity(self.arena_capacity);
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
            let slice = bump.alloc([10, 20, 30, 40]);
            slice.iter().sum::<i32>()
        });
        assert_eq!(sum, 100);
    }
}
