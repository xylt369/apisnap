use crate::config::ArrayDiffMode;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Classification of a single detected difference between expected and actual JSON ASTs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiffKind {
    /// Key/index present in `actual` but not in `expected`.
    Added {
        json_path: String,
        new_value: serde_json::Value,
    },
    /// Key/index present in `expected` but not in `actual`.
    Removed {
        json_path: String,
        old_value: serde_json::Value,
    },
    /// Same key/index, same variant type, but different value.
    Modified {
        json_path: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
    /// Same key/index, but `serde_json::Value` type variant differs (e.g. String vs Number).
    TypeMismatch {
        json_path: String,
        expected_type: &'static str,
        actual_type: &'static str,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
    /// Arrays compared in ordered mode with differing lengths.
    ArrayLengthMismatch {
        json_path: String,
        expected_len: usize,
        actual_len: usize,
    },
}

/// Aggregated result of diffing an endpoint's actual response against its snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffReport {
    pub endpoint_name: String,
    pub differences: Vec<DiffKind>,
    pub is_match: bool,
    pub expected_status: u16,
    pub actual_status: u16,
}

impl DiffReport {
    pub fn passed(&self) -> bool {
        self.is_match && self.expected_status == self.actual_status
    }

    /// Render a human-readable, ANSI-colored diff output.
    pub fn render_colored(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "\n{} {}\n",
            "Snapshot Diff Report for:".bold(),
            self.endpoint_name.cyan().bold()
        ));

        if self.expected_status != self.actual_status {
            out.push_str(&format!(
                "  {} Status code mismatch: expected {}, got {}\n",
                "[!]".red().bold(),
                self.expected_status.to_string().green(),
                self.actual_status.to_string().red()
            ));
        }

        if self.differences.is_empty() && self.expected_status == self.actual_status {
            out.push_str(&format!("  {} 100% Match\n", "[PASS]".green().bold()));
            return out;
        }

        for diff in &self.differences {
            match diff {
                DiffKind::Added { json_path, new_value } => {
                    out.push_str(&format!(
                        "  {} {}: {}\n",
                        "+".green().bold(),
                        json_path.green(),
                        format_json_compact(new_value).green()
                    ));
                }
                DiffKind::Removed { json_path, old_value } => {
                    out.push_str(&format!(
                        "  {} {}: {}\n",
                        "-".red().bold(),
                        json_path.red(),
                        format_json_compact(old_value).red()
                    ));
                }
                DiffKind::Modified { json_path, old_value, new_value } => {
                    out.push_str(&format!(
                        "  {} {}\n    {} {}\n    {} {}\n",
                        "~".yellow().bold(),
                        json_path.yellow().bold(),
                        "-".red(),
                        format_json_compact(old_value).red(),
                        "+".green(),
                        format_json_compact(new_value).green()
                    ));
                }
                DiffKind::TypeMismatch {
                    json_path,
                    expected_type,
                    actual_type,
                    old_value,
                    new_value,
                } => {
                    out.push_str(&format!(
                        "  {} {} (type mismatch: expected {}, got {})\n    {} {}\n    {} {}\n",
                        "x".red().bold(),
                        json_path.red().bold(),
                        expected_type.yellow(),
                        actual_type.yellow(),
                        "-".red(),
                        format_json_compact(old_value).red(),
                        "+".green(),
                        format_json_compact(new_value).green()
                    ));
                }
                DiffKind::ArrayLengthMismatch {
                    json_path,
                    expected_len,
                    actual_len,
                } => {
                    out.push_str(&format!(
                        "  {} {} (array length changed: {} -> {})\n",
                        "~".yellow().bold(),
                        json_path.yellow(),
                        expected_len.to_string().red(),
                        actual_len.to_string().green()
                    ));
                }
            }
        }

        out.push_str(&format!(
            "\n  Total differences: {}\n",
            self.differences.len().to_string().bold()
        ));

        out
    }
}

fn format_json_compact(val: &Value) -> String {
    match val {
        Value::String(s) => format!("\"{s}\""),
        _ => val.to_string(),
    }
}

/// Configuration for semantic comparison options.
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    pub array_modes: HashMap<String, ArrayDiffMode>,
}

/// Compare expected and actual JSON ASTs semantically.
pub fn compare_json_ast(
    expected: &Value,
    actual: &Value,
    options: &DiffOptions,
) -> Vec<DiffKind> {
    let mut differences = Vec::new();
    diff_recursive(expected, actual, "$", options, &mut differences);
    differences
}

fn diff_recursive(
    expected: &Value,
    actual: &Value,
    path: &str,
    options: &DiffOptions,
    out: &mut Vec<DiffKind>,
) {
    let exp_type = json_type_name(expected);
    let act_type = json_type_name(actual);

    // 1. Type mismatch check
    if exp_type != act_type {
        out.push(DiffKind::TypeMismatch {
            json_path: path.to_string(),
            expected_type: exp_type,
            actual_type: act_type,
            old_value: expected.clone(),
            new_value: actual.clone(),
        });
        return;
    }

    // 2. Structural matching by variant
    match (expected, actual) {
        (Value::Object(e_map), Value::Object(a_map)) => {
            let e_keys: BTreeSet<&str> = e_map.keys().map(|k| k.as_str()).collect();
            let a_keys: BTreeSet<&str> = a_map.keys().map(|k| k.as_str()).collect();

            // Removed keys
            for &key in e_keys.difference(&a_keys) {
                out.push(DiffKind::Removed {
                    json_path: format!("{path}.{key}"),
                    old_value: e_map[key].clone(),
                });
            }

            // Added keys
            for &key in a_keys.difference(&e_keys) {
                out.push(DiffKind::Added {
                    json_path: format!("{path}.{key}"),
                    new_value: a_map[key].clone(),
                });
            }

            // Common keys (order-insensitive)
            for &key in e_keys.intersection(&a_keys) {
                let child_path = format!("{path}.{key}");
                diff_recursive(&e_map[key], &a_map[key], &child_path, options, out);
            }
        }
        (Value::Array(e_arr), Value::Array(a_arr)) => {
            let mode = options
                .array_modes
                .get(path)
                .copied()
                .unwrap_or(ArrayDiffMode::Ordered);

            match mode {
                ArrayDiffMode::Ordered => {
                    if e_arr.len() != a_arr.len() {
                        out.push(DiffKind::ArrayLengthMismatch {
                            json_path: path.to_string(),
                            expected_len: e_arr.len(),
                            actual_len: a_arr.len(),
                        });
                    }

                    let min_len = std::cmp::min(e_arr.len(), a_arr.len());
                    for i in 0..min_len {
                        let child_path = format!("{path}[{i}]");
                        diff_recursive(&e_arr[i], &a_arr[i], &child_path, options, out);
                    }

                    // Extra actual elements
                    for (i, extra) in a_arr.iter().enumerate().skip(min_len) {
                        out.push(DiffKind::Added {
                            json_path: format!("{path}[{i}]"),
                            new_value: extra.clone(),
                        });
                    }
                }
                ArrayDiffMode::Set => {
                    diff_array_as_set(e_arr, a_arr, path, out);
                }
            }
        }
        (Value::String(e_str), Value::String(a_str)) => {
            if e_str != a_str {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: expected.clone(),
                    new_value: actual.clone(),
                });
            }
        }
        (Value::Number(e_num), Value::Number(a_num)) => {
            if e_num != a_num {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: expected.clone(),
                    new_value: actual.clone(),
                });
            }
        }
        (Value::Bool(e_bool), Value::Bool(a_bool)) => {
            if e_bool != a_bool {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: expected.clone(),
                    new_value: actual.clone(),
                });
            }
        }
        (Value::Null, Value::Null) => {}
        _ => unreachable!("Variant mismatch already handled by json_type_name check"),
    }
}

/// Compares array elements as an unordered multiset.
fn diff_array_as_set(
    expected: &[Value],
    actual: &[Value],
    path: &str,
    out: &mut Vec<DiffKind>,
) {
    let mut e_canonical: BTreeMap<String, usize> = BTreeMap::new();
    let mut a_canonical: BTreeMap<String, usize> = BTreeMap::new();

    for item in expected {
        let key = canonical_json_string(item);
        *e_canonical.entry(key).or_insert(0) += 1;
    }

    for item in actual {
        let key = canonical_json_string(item);
        *a_canonical.entry(key).or_insert(0) += 1;
    }

    // Removed in actual
    for (key, &count) in &e_canonical {
        let actual_count = a_canonical.get(key).copied().unwrap_or(0);
        if count > actual_count {
            let val: Value = serde_json::from_str(key).unwrap_or(Value::Null);
            for _ in 0..(count - actual_count) {
                out.push(DiffKind::Removed {
                    json_path: format!("{path}[*]"),
                    old_value: val.clone(),
                });
            }
        }
    }

    // Added in actual
    for (key, &count) in &a_canonical {
        let expected_count = e_canonical.get(key).copied().unwrap_or(0);
        if count > expected_count {
            let val: Value = serde_json::from_str(key).unwrap_or(Value::Null);
            for _ in 0..(count - expected_count) {
                out.push(DiffKind::Added {
                    json_path: format!("{path}[*]"),
                    new_value: val.clone(),
                });
            }
        }
    }
}

/// Recursively canonicalize JSON so key ordering within elements is consistent.
fn canonical_json_string(val: &Value) -> String {
    match val {
        Value::Object(map) => {
            let sorted_map: BTreeMap<&str, Value> = map.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            serde_json::to_string(&sorted_map).unwrap_or_default()
        }
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

fn json_type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_order_insensitive_object_match() {
        let expected = json!({"a": 1, "b": "hello"});
        let actual = json!({"b": "hello", "a": 1});

        let diffs = compare_json_ast(&expected, &actual, &DiffOptions::default());
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_type_mismatch() {
        let expected = json!({"count": 5});
        let actual = json!({"count": "5"});

        let diffs = compare_json_ast(&expected, &actual, &DiffOptions::default());
        assert_eq!(diffs.len(), 1);
        match &diffs[0] {
            DiffKind::TypeMismatch {
                json_path,
                expected_type,
                actual_type,
                ..
            } => {
                assert_eq!(json_path, "$.count");
                assert_eq!(*expected_type, "number");
                assert_eq!(*actual_type, "string");
            }
            _ => panic!("Expected TypeMismatch"),
        }
    }

    #[test]
    fn test_array_set_mode() {
        let expected = json!({"tags": ["a", "b", "c"]});
        let actual = json!({"tags": ["c", "a", "b"]});

        let mut options = DiffOptions::default();
        options
            .array_modes
            .insert("$.tags".to_string(), ArrayDiffMode::Set);

        let diffs = compare_json_ast(&expected, &actual, &options);
        assert!(diffs.is_empty(), "Set mode should ignore element reordering");

        // Ordered mode should detect differences
        let ordered_diffs = compare_json_ast(&expected, &actual, &DiffOptions::default());
        assert_eq!(ordered_diffs.len(), 2, "Ordered mode must detect index modifications");
    }
}
