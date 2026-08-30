use crate::config::ArrayDiffMode;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use unicode_normalization::UnicodeNormalization;

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
        expected_type: String,
        actual_type: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
    /// Arrays compared in ordered mode with differing lengths.
    ArrayLengthMismatch {
        json_path: String,
        expected_len: usize,
        actual_len: usize,
    },
    /// Traversal recursion exceeded maximum configured depth limit.
    DepthExceeded {
        json_path: String,
        max_depth: usize,
    },
}

impl DiffKind {
    pub fn json_path(&self) -> &str {
        match self {
            DiffKind::Added { json_path, .. } => json_path,
            DiffKind::Removed { json_path, .. } => json_path,
            DiffKind::Modified { json_path, .. } => json_path,
            DiffKind::TypeMismatch { json_path, .. } => json_path,
            DiffKind::ArrayLengthMismatch { json_path, .. } => json_path,
            DiffKind::DepthExceeded { json_path, .. } => json_path,
        }
    }
}

/// Aggregated result of diffing an endpoint's actual response against its snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffReport {
    pub endpoint_name: String,
    pub differences: Vec<DiffKind>,
    pub is_match: bool,
    pub expected_status: u16,
    pub actual_status: u16,
    #[serde(default)]
    pub trace_context: Option<String>,
    #[serde(default)]
    pub trace_link: Option<String>,
}

impl DiffReport {
    pub fn passed(&self) -> bool {
        self.is_match && self.expected_status == self.actual_status
    }

    pub fn render_colored(&self) -> String {
        let mut out = String::new();
        if self.expected_status != self.actual_status {
            out.push_str(&format!(
                "    {} HTTP Status: expected {}, got {}\n",
                "!".yellow().bold(),
                self.expected_status.to_string().cyan(),
                self.actual_status.to_string().red()
            ));
        }
        for diff in &self.differences {
            match diff {
                DiffKind::Added { json_path, new_value } => {
                    out.push_str(&format!(
                        "    {} Added {}: {}\n",
                        "+".green().bold(),
                        json_path.cyan(),
                        new_value.to_string().green()
                    ));
                }
                DiffKind::Removed { json_path, old_value } => {
                    out.push_str(&format!(
                        "    {} Removed {}: {}\n",
                        "-".red().bold(),
                        json_path.cyan(),
                        old_value.to_string().red()
                    ));
                }
                DiffKind::Modified { json_path, old_value, new_value } => {
                    out.push_str(&format!(
                        "    {} Modified {}: {} -> {}\n",
                        "~".yellow().bold(),
                        json_path.cyan(),
                        old_value.to_string().red(),
                        new_value.to_string().green()
                    ));
                }
                DiffKind::TypeMismatch { json_path, expected_type, actual_type, .. } => {
                    out.push_str(&format!(
                        "    {} Type Mismatch {}: expected {}, got {}\n",
                        "!".yellow().bold(),
                        json_path.cyan(),
                        expected_type.cyan(),
                        actual_type.red()
                    ));
                }
                DiffKind::ArrayLengthMismatch { json_path, expected_len, actual_len } => {
                    out.push_str(&format!(
                        "    {} Array Length Mismatch {}: expected {}, got {}\n",
                        "!".yellow().bold(),
                        json_path.cyan(),
                        expected_len,
                        actual_len
                    ));
                }
                DiffKind::DepthExceeded { json_path, max_depth } => {
                    out.push_str(&format!(
                        "    {} Recursion depth exceeded at {} (max: {})\n",
                        "!".red().bold(),
                        json_path.cyan(),
                        max_depth
                    ));
                }
            }
        }
        if let Some(link) = &self.trace_link {
            out.push_str(&format!(
                "    {} APM Trace Link: {}\n",
                "*".cyan().bold(),
                link.cyan().underline()
            ));
        }
        out
    }
}

/// Options controlling AST comparison behavior.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub float_epsilon: f64,
    pub normalize_unicode_keys: bool,
    pub max_depth: usize,
    pub fast_hash_bypass: bool,
    pub array_modes: HashMap<String, ArrayDiffMode>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            float_epsilon: 0.0,
            normalize_unicode_keys: true,
            max_depth: 512,
            fast_hash_bypass: true,
            array_modes: HashMap::new(),
        }
    }
}

/// Compare two JSON ASTs recursively with semantic rules.
pub fn compare_json_ast(
    expected: &Value,
    actual: &Value,
    options: &DiffOptions,
) -> Vec<DiffKind> {
    let mut differences = Vec::new();

    // Fast-Hash bypass: If raw serialized structures are byte-for-byte identical, return immediately
    if options.fast_hash_bypass && expected == actual {
        return differences;
    }

    diff_recursive(expected, actual, "$", options, 0, &mut differences);
    differences
}

fn diff_recursive(
    expected: &Value,
    actual: &Value,
    path: &str,
    options: &DiffOptions,
    depth: usize,
    out: &mut Vec<DiffKind>,
) {
    if depth > options.max_depth {
        out.push(DiffKind::DepthExceeded {
            json_path: path.to_string(),
            max_depth: options.max_depth,
        });
        return;
    }

    // 1. Variant Type Mismatch
    let (e_type, a_type) = (value_type_name(expected), value_type_name(actual));
    if e_type != a_type {
        out.push(DiffKind::TypeMismatch {
            json_path: path.to_string(),
            expected_type: e_type.to_string(),
            actual_type: a_type.to_string(),
            old_value: expected.clone(),
            new_value: actual.clone(),
        });
        return;
    }

    // 2. Structural matching by variant
    match (expected, actual) {
        (Value::Object(e_map), Value::Object(a_map)) => {
            let e_normalized: BTreeMap<String, &Value> = if options.normalize_unicode_keys {
                e_map.iter().map(|(k, v)| (k.nfc().collect::<String>(), v)).collect()
            } else {
                e_map.iter().map(|(k, v)| (k.clone(), v)).collect()
            };

            let a_normalized: BTreeMap<String, &Value> = if options.normalize_unicode_keys {
                a_map.iter().map(|(k, v)| (k.nfc().collect::<String>(), v)).collect()
            } else {
                a_map.iter().map(|(k, v)| (k.clone(), v)).collect()
            };

            let e_keys: BTreeSet<&str> = e_normalized.keys().map(|k| k.as_str()).collect();
            let a_keys: BTreeSet<&str> = a_normalized.keys().map(|k| k.as_str()).collect();

            // Removed keys
            for &key in e_keys.difference(&a_keys) {
                out.push(DiffKind::Removed {
                    json_path: format!("{path}.{key}"),
                    old_value: e_normalized[key].clone(),
                });
            }

            // Added keys
            for &key in a_keys.difference(&e_keys) {
                out.push(DiffKind::Added {
                    json_path: format!("{path}.{key}"),
                    new_value: a_normalized[key].clone(),
                });
            }

            // Common keys (order-insensitive)
            for &key in e_keys.intersection(&a_keys) {
                let child_path = format!("{path}.{key}");
                diff_recursive(e_normalized[key], a_normalized[key], &child_path, options, depth + 1, out);
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

                    let min_len = e_arr.len().min(a_arr.len());
                    for i in 0..min_len {
                        let child_path = format!("{path}[{i}]");
                        diff_recursive(&e_arr[i], &a_arr[i], &child_path, options, depth + 1, out);
                    }

                    if a_arr.len() > e_arr.len() {
                        for (idx, item) in a_arr.iter().enumerate().skip(e_arr.len()) {
                            out.push(DiffKind::Added {
                                json_path: format!("{path}[{idx}]"),
                                new_value: item.clone(),
                            });
                        }
                    }
                }
                ArrayDiffMode::Set => {
                    diff_array_as_set(e_arr, a_arr, path, options, depth, out);
                }
            }
        }
        (Value::String(e_str), Value::String(a_str)) => {
            if e_str != a_str {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: Value::String(e_str.clone()),
                    new_value: Value::String(a_str.clone()),
                });
            }
        }
        (Value::Number(e_num), Value::Number(a_num)) => {
            let is_match = if options.float_epsilon > 0.0 {
                if let (Some(e_f), Some(a_f)) = (e_num.as_f64(), a_num.as_f64()) {
                    (e_f - a_f).abs() <= options.float_epsilon
                } else {
                    e_num == a_num
                }
            } else {
                e_num == a_num
            };

            if !is_match {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: Value::Number(e_num.clone()),
                    new_value: Value::Number(a_num.clone()),
                });
            }
        }
        (Value::Bool(e_bool), Value::Bool(a_bool)) => {
            if e_bool != a_bool {
                out.push(DiffKind::Modified {
                    json_path: path.to_string(),
                    old_value: Value::Bool(*e_bool),
                    new_value: Value::Bool(*a_bool),
                });
            }
        }
        (Value::Null, Value::Null) => {}
        _ => unreachable!("variant type mismatch already handled above"),
    }
}

fn diff_array_as_set(
    expected: &[Value],
    actual: &[Value],
    path: &str,
    options: &DiffOptions,
    depth: usize,
    out: &mut Vec<DiffKind>,
) {
    let mut actual_matched = vec![false; actual.len()];

    for e_item in expected {
        let mut matched = false;
        for (a_idx, a_item) in actual.iter().enumerate() {
            if !actual_matched[a_idx] {
                let mut temp_diffs = Vec::new();
                diff_recursive(e_item, a_item, path, options, depth + 1, &mut temp_diffs);
                if temp_diffs.is_empty() {
                    actual_matched[a_idx] = true;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            out.push(DiffKind::Removed {
                json_path: format!("{path}[*]"),
                old_value: e_item.clone(),
            });
        }
    }

    for (a_idx, &matched) in actual_matched.iter().enumerate() {
        if !matched {
            out.push(DiffKind::Added {
                json_path: format!("{path}[*]"),
                new_value: actual[a_idx].clone(),
            });
        }
    }
}

fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
