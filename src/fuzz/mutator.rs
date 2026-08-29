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
    mutate_recursive(baseline, "$", baseline, &mut cases);

    cases
}

fn mutate_recursive(current: &Value, current_path: &str, root: &Value, out: &mut Vec<MutationCase>) {
    match current {
        Value::Object(map) => {
            // A. Missing each key
            for key in map.keys() {
                let target_path = if current_path == "$" {
                    format!("$.{key}")
                } else {
                    format!("{current_path}.{key}")
                };
                let mut mutated_root = root.clone();
                remove_path_in_ast(&mut mutated_root, &target_path);
                out.push(MutationCase {
                    name: format!("missing_key_{key}"),
                    description: format!("Omit required key '{target_path}'"),
                    mutated_body: mutated_root,
                });
            }

            // B. Null each key
            for key in map.keys() {
                let target_path = if current_path == "$" {
                    format!("$.{key}")
                } else {
                    format!("{current_path}.{key}")
                };
                let mutated_root = replace_path_in_ast(root, &target_path, Value::Null);
                out.push(MutationCase {
                    name: format!("null_key_{key}"),
                    description: format!("Set key '{target_path}' to null"),
                    mutated_body: mutated_root,
                });
            }

            // Recurse into children
            for (key, child) in map {
                let child_path = if current_path == "$" {
                    format!("$.{key}")
                } else {
                    format!("{current_path}.{key}")
                };
                mutate_recursive(child, &child_path, root, out);
            }
        }
        Value::String(_) => {
            // SQL Injection probe
            out.push(MutationCase {
                name: format!("sqli_{current_path}"),
                description: format!("SQL injection boundary probe at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::String("' OR '1'='1 --".into())),
            });

            // XSS Probe
            out.push(MutationCase {
                name: format!("xss_{current_path}"),
                description: format!("XSS script injection probe at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::String("<script>alert('xss')</script>".into())),
            });

            // Empty String
            out.push(MutationCase {
                name: format!("empty_str_{current_path}"),
                description: format!("Empty string at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::String("".into())),
            });

            // Oversized buffer (16KB)
            let large_str = "A".repeat(16384);
            out.push(MutationCase {
                name: format!("oversized_{current_path}"),
                description: format!("Oversized 16KB buffer at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::String(large_str)),
            });
        }
        Value::Number(_) => {
            // Integer Overflow / Boundary
            out.push(MutationCase {
                name: format!("max_int_{current_path}"),
                description: format!("Max signed 64-bit integer at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::Number(i64::MAX.into())),
            });

            out.push(MutationCase {
                name: format!("negative_{current_path}"),
                description: format!("Negative value (-1) at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::Number((-1).into())),
            });

            out.push(MutationCase {
                name: format!("zero_{current_path}"),
                description: format!("Zero (0) at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::Number(0.into())),
            });
        }
        Value::Array(arr) => {
            // Empty Array
            out.push(MutationCase {
                name: format!("empty_array_{current_path}"),
                description: format!("Empty array `[]` at '{current_path}'"),
                mutated_body: replace_path_in_ast(root, current_path, Value::Array(Vec::new())),
            });

            for (idx, item) in arr.iter().enumerate() {
                let child_path = format!("{current_path}[{idx}]");
                mutate_recursive(item, &child_path, root, out);
            }
        }
        _ => {}
    }
}

/// Clones `root` and substitutes the value at `path` with `replacement`.
pub fn replace_path_in_ast(root: &Value, path: &str, replacement: Value) -> Value {
    let mut cloned = root.clone();
    let segments = parse_json_path(path);
    if segments.is_empty() {
        return replacement;
    }

    let mut current = &mut cloned;
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        if is_last {
            match seg {
                PathSeg::Key(k) => {
                    if let Value::Object(map) = current {
                        map.insert(k.clone(), replacement);
                    }
                }
                PathSeg::Index(idx) => {
                    if let Value::Array(arr) = current {
                        if *idx < arr.len() {
                            arr[*idx] = replacement;
                        }
                    }
                }
            }
            break;
        } else {
            match seg {
                PathSeg::Key(k) => {
                    if let Value::Object(map) = current {
                        if let Some(next) = map.get_mut(k) {
                            current = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                PathSeg::Index(idx) => {
                    if let Value::Array(arr) = current {
                        if let Some(next) = arr.get_mut(*idx) {
                            current = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }

    cloned
}

/// Removes the field at `path` from `root`.
pub fn remove_path_in_ast(root: &mut Value, path: &str) {
    let segments = parse_json_path(path);
    if segments.is_empty() {
        return;
    }

    let mut current = root;
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        if is_last {
            match seg {
                PathSeg::Key(k) => {
                    if let Value::Object(map) = current {
                        map.remove(k);
                    }
                }
                PathSeg::Index(idx) => {
                    if let Value::Array(arr) = current {
                        if *idx < arr.len() {
                            arr.remove(*idx);
                        }
                    }
                }
            }
            break;
        } else {
            match seg {
                PathSeg::Key(k) => {
                    if let Value::Object(map) = current {
                        if let Some(next) = map.get_mut(k) {
                            current = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                PathSeg::Index(idx) => {
                    if let Value::Array(arr) = current {
                        if let Some(next) = arr.get_mut(*idx) {
                            current = next;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }
}

enum PathSeg {
    Key(String),
    Index(usize),
}

fn parse_json_path(path: &str) -> Vec<PathSeg> {
    let clean = path.trim_start_matches('$');
    let mut segments = Vec::new();

    let mut current_key = String::new();
    let mut chars = clean.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '.' {
            if !current_key.is_empty() {
                segments.push(PathSeg::Key(current_key.clone()));
                current_key.clear();
            }
        } else if ch == '[' {
            if !current_key.is_empty() {
                segments.push(PathSeg::Key(current_key.clone()));
                current_key.clear();
            }
            let mut num_str = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                num_str.push(c);
            }
            if let Ok(idx) = num_str.parse::<usize>() {
                segments.push(PathSeg::Index(idx));
            }
        } else {
            current_key.push(ch);
        }
    }

    if !current_key.is_empty() {
        segments.push(PathSeg::Key(current_key));
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deep_ast_mutation() {
        let baseline = json!({
            "user": {
                "profile": {
                    "age": 25,
                    "name": "Bob"
                }
            }
        });

        let mutated = replace_path_in_ast(&baseline, "$.user.profile.age", json!(100));
        assert_eq!(mutated["user"]["profile"]["age"], 100);
        assert_eq!(mutated["user"]["profile"]["name"], "Bob"); // Sibling preserved!
    }

    #[test]
    fn test_deep_ast_removal() {
        let mut baseline = json!({
            "user": {
                "name": "Alice",
                "email": "alice@example.com"
            }
        });

        remove_path_in_ast(&mut baseline, "$.user.email");
        assert!(baseline["user"].get("email").is_none());
        assert_eq!(baseline["user"]["name"], "Alice");
    }
}
