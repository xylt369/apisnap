use crate::config::{CustomMaskRule, MaskingConfig};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

pub static ISO8601_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?$").unwrap()
});

pub static UUID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});

pub static JWT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$").unwrap());

pub static UNIX_EPOCH_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d{10}$|^\d{13}$").unwrap());

pub static KEY_TIME_HINT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(_at$|_time$|^time|timestamp|expires|issued|created|updated)").unwrap()
});

pub static MONGODB_OBJECT_ID_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[0-9a-fA-F]{24}$").unwrap());

pub const MASKED_UUID: &str = "<MASKED_UUID>";
pub const MASKED_JWT: &str = "<MASKED_JWT>";
pub const MASKED_OBJECT_ID: &str = "<MASKED_OBJECT_ID>";
pub const MASKED_TIMESTAMP: &str = "<MASKED_TIMESTAMP>";
pub const MASKED_EPOCH: &str = "<MASKED_EPOCH>";

/// Resolved masking context passed during recursive AST traversal.
#[derive(Debug, Clone)]
pub struct MaskContext {
    pub enable_builtin_heuristics: bool,
    /// Exact JSONPath -> CustomMaskRule (e.g. "$.data.token" or "$.items[*].id")
    pub path_rules: HashMap<String, CustomMaskRule>,
}

impl MaskContext {
    pub fn new(global_config: &MaskingConfig, overrides: &[CustomMaskRule]) -> Self {
        let mut path_rules = HashMap::new();

        // 1. Insert global custom rules
        for rule in &global_config.custom_rules {
            path_rules.insert(rule.json_path.clone(), rule.clone());
        }

        // 2. Insert endpoint-level overrides (shadows global rules on collision)
        for rule in overrides {
            path_rules.insert(rule.json_path.clone(), rule.clone());
        }

        Self {
            enable_builtin_heuristics: global_config.enable_builtin_heuristics,
            path_rules,
        }
    }
}

/// Recursively masks a JSON AST value in-place according to the given context.
pub fn mask_value(val: &mut Value, ctx: &MaskContext) {
    mask_recursive(val, ctx, "$", None);
}

fn mask_recursive(
    val: &mut Value,
    ctx: &MaskContext,
    current_path: &str,
    parent_key: Option<&str>,
) {
    // 1. Check if there is an exact or wildcard custom JSONPath rule matching this path
    if let Some(rule) = match_custom_rule(ctx, current_path) {
        apply_custom_rule(val, rule);
        return;
    }

    match val {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let child_path = format!("{current_path}.{key}");
                mask_recursive(child, ctx, &child_path, Some(key));
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter_mut().enumerate() {
                let child_path = format!("{current_path}[{idx}]");
                mask_recursive(child, ctx, &child_path, parent_key);
            }
        }
        Value::String(s) => {
            if !ctx.enable_builtin_heuristics {
                return;
            }
            // Skip already masked tokens to preserve idempotency
            if s.starts_with("<MASKED_") && s.ends_with('>') {
                return;
            }

            if UUID_REGEX.is_match(s) {
                *s = MASKED_UUID.to_string();
            } else if JWT_REGEX.is_match(s) {
                *s = MASKED_JWT.to_string();
            } else if MONGODB_OBJECT_ID_REGEX.is_match(s) {
                *s = MASKED_OBJECT_ID.to_string();
            } else if ISO8601_REGEX.is_match(s) {
                *s = MASKED_TIMESTAMP.to_string();
            }
        }
        Value::Number(num) => {
            if !ctx.enable_builtin_heuristics {
                return;
            }
            // Check heuristic for epoch integers
            if let Some(key) = parent_key {
                if KEY_TIME_HINT.is_match(key) {
                    let num_str = num.to_string();
                    if UNIX_EPOCH_REGEX.is_match(&num_str) {
                        *val = Value::String(MASKED_EPOCH.to_string());
                    }
                }
            }
        }
        Value::Bool(_) | Value::Null => {}
    }
}

/// Match an exact path or wildcard path pattern (e.g. `$.items[0].id` matches `$.items[*].id`).
fn match_custom_rule<'a>(ctx: &'a MaskContext, path: &str) -> Option<&'a CustomMaskRule> {
    if let Some(rule) = ctx.path_rules.get(path) {
        return Some(rule);
    }

    // Check wildcard array syntax: e.g. "$.items[0].id" matches rule for "$.items[*].id"
    let wildcard_path = normalize_wildcard_path(path);
    if let Some(rule) = ctx.path_rules.get(&wildcard_path) {
        return Some(rule);
    }

    None
}

fn normalize_wildcard_path(path: &str) -> String {
    static ARRAY_INDEX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\d+\]").unwrap());
    ARRAY_INDEX_RE.replace_all(path, "[*]").to_string()
}

fn apply_custom_rule(val: &mut Value, rule: &CustomMaskRule) {
    if let Some(pattern_str) = &rule.pattern {
        if let Ok(regex) = Regex::new(pattern_str) {
            if let Value::String(s) = val {
                let replaced = regex.replace_all(s, &rule.replacement).to_string();
                *s = replaced;
                return;
            }
        }
    }

    // Default: replace entire leaf with replacement string
    *val = Value::String(rule.replacement.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_nested_json_masking() {
        let mut input = json!({
            "user": {
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "profile": {
                    "created_at": "2024-01-15T10:30:00Z"
                }
            }
        });

        let ctx = MaskContext::new(&MaskingConfig::default(), &[]);
        mask_value(&mut input, &ctx);

        let expected = json!({
            "user": {
                "id": "<MASKED_UUID>",
                "profile": {
                    "created_at": "<MASKED_TIMESTAMP>"
                }
            }
        });

        assert_eq!(input, expected);
    }

    #[test]
    fn test_custom_rule_precedence() {
        let mut input = json!({
            "data": {
                "token": "abc.def.ghi"
            }
        });

        let rule = CustomMaskRule {
            json_path: "$.data.token".to_string(),
            replacement: "<CUSTOM_TOKEN>".to_string(),
            pattern: None,
        };

        let ctx = MaskContext::new(&MaskingConfig::default(), &[rule]);
        mask_value(&mut input, &ctx);

        let expected = json!({
            "data": {
                "token": "<CUSTOM_TOKEN>"
            }
        });

        assert_eq!(input, expected);
    }

    #[test]
    fn test_idempotency() {
        let mut input = json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "time": "2024-01-15T10:30:00Z",
            "epoch_created_at": 1705314600
        });

        let ctx = MaskContext::new(&MaskingConfig::default(), &[]);
        mask_value(&mut input, &ctx);
        let first_pass = input.clone();

        mask_value(&mut input, &ctx);
        assert_eq!(input, first_pass);
    }
}
