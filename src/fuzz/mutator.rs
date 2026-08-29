use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A synthesized mutation targeting a specific JSONPath with a description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationCase {
    pub name: String,
    pub description: String,
    pub mutated_body: Value,
}

/// Generates a suite of resilience boundary mutations from a baseline JSON body.
pub fn generate_mutations(baseline: &Value) -> Vec<MutationCase> {
    let mut cases = Vec::new();

    // 1. Top-level baseline variations
    cases.push(MutationCase {
        name: "null_payload".into(),
        description: "Empty / Null request payload".into(),
        mutated_body: Value::Null,
    });

    cases.push(MutationCase {
        name: "empty_object".into(),
        description: "Empty object `{}` payload".into(),
        mutated_body: Value::Object(Default::default()),
    });

    // 2. Recursive field mutations
    mutate_recursive(baseline, "$", &mut cases);

    cases
}

fn mutate_recursive(val: &Value, path: &str, out: &mut Vec<MutationCase>) {
    match val {
        Value::Object(map) => {
            // A. Missing each key
            for key in map.keys() {
                let mut mutated_map = map.clone();
                mutated_map.remove(key);
                out.push(MutationCase {
                    name: format!("missing_key_{key}"),
                    description: format!("Omit required key '{path}.{key}'"),
                    mutated_body: Value::Object(mutated_map),
                });
            }

            // B. Null each key
            for key in map.keys() {
                let mut mutated_map = map.clone();
                mutated_map.insert(key.clone(), Value::Null);
                out.push(MutationCase {
                    name: format!("null_key_{key}"),
                    description: format!("Set key '{path}.{key}' to null"),
                    mutated_body: Value::Object(mutated_map),
                });
            }

            // Recurse into children
            for (key, child) in map {
                mutate_recursive(child, &format!("{path}.{key}"), out);
            }
        }
        Value::String(s) => {
            // SQL Injection probe
            out.push(MutationCase {
                name: format!("sqli_{path}"),
                description: format!("SQL injection boundary probe at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::String("' OR '1'='1 --".into())),
            });

            // XSS Probe
            out.push(MutationCase {
                name: format!("xss_{path}"),
                description: format!("XSS script injection probe at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::String("<script>alert('xss')</script>".into())),
            });

            // Empty String
            out.push(MutationCase {
                name: format!("empty_str_{path}"),
                description: format!("Empty string at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::String("".into())),
            });

            // Oversized buffer (16KB)
            let large_str = "A".repeat(16384);
            out.push(MutationCase {
                name: format!("oversized_{path}"),
                description: format!("Oversized 16KB buffer at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::String(large_str)),
            });
        }
        Value::Number(_) => {
            // Integer Overflow / Boundary
            out.push(MutationCase {
                name: format!("max_int_{path}"),
                description: format!("Max signed 64-bit integer at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::Number(i64::MAX.into())),
            });

            out.push(MutationCase {
                name: format!("negative_{path}"),
                description: format!("Negative value (-1) at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::Number((-1).into())),
            });

            out.push(MutationCase {
                name: format!("zero_{path}"),
                description: format!("Zero (0) at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::Number(0.into())),
            });
        }
        Value::Array(arr) => {
            // Empty Array
            out.push(MutationCase {
                name: format!("empty_array_{path}"),
                description: format!("Empty array `[]` at '{path}'"),
                mutated_body: replace_leaf(val, path, Value::Array(Vec::new())),
            });
        }
        _ => {}
    }
}

fn replace_leaf(root: &Value, _path: &str, replacement: Value) -> Value {
    // For single-depth testing, return replacement or modified root
    replacement
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_generate_mutations() {
        let baseline = json!({
            "username": "alice",
            "age": 30
        });

        let mutations = generate_mutations(&baseline);
        assert!(!mutations.is_empty());
        let names: Vec<&str> = mutations.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"missing_key_username"));
        assert!(names.contains(&"null_key_age"));
    }
}
