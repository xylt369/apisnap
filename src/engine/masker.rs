use crate::config::{CustomMaskRule, MaskingConfig};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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

pub static SSN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d{3}-\d{2}-\d{4}$").unwrap());

pub static EMAIL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap());

pub static AWS_KEY_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^AKIA[0-9A-Z]{16}$").unwrap());

pub static PRIVATE_KEY_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----").unwrap());

pub const MASKED_UUID: &str = "<MASKED_UUID>";
pub const MASKED_JWT: &str = "<MASKED_JWT>";
pub const MASKED_OBJECT_ID: &str = "<MASKED_OBJECT_ID>";
pub const MASKED_TIMESTAMP: &str = "<MASKED_TIMESTAMP>";
pub const MASKED_EPOCH: &str = "<MASKED_EPOCH>";
pub const MASKED_CREDIT_CARD: &str = "<MASKED_CREDIT_CARD>";
pub const MASKED_SSN: &str = "<MASKED_SSN>";
pub const MASKED_EMAIL: &str = "<MASKED_EMAIL>";
pub const REDACTED: &str = "<REDACTED>";

/// Resolved masking context passed during recursive AST traversal.
#[derive(Debug, Clone)]
pub struct MaskContext {
    pub enable_builtin_heuristics: bool,
    pub strict_pii_mode: bool,
    pub max_depth: usize,
    pub unmask_allow_list: HashSet<String>,
    pub path_rules: HashMap<String, CustomMaskRule>,
}

impl MaskContext {
    pub fn new(global_config: &MaskingConfig, overrides: &[CustomMaskRule]) -> Self {
        let mut path_rules = HashMap::new();

        // 1. Insert global custom rules
        for rule in &global_config.custom_rules {
            path_rules.insert(rule.json_path.clone(), rule.clone());
        }

        // 2. Insert endpoint-level overrides
        for rule in overrides {
            path_rules.insert(rule.json_path.clone(), rule.clone());
        }

        let unmask_allow_list: HashSet<String> = global_config.unmask_allow_list.iter().cloned().collect();

        Self {
            enable_builtin_heuristics: global_config.enable_builtin_heuristics,
            strict_pii_mode: global_config.strict_pii_mode,
            max_depth: 512,
            unmask_allow_list,
            path_rules,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

/// Recursively masks a JSON AST value in-place according to the given context.
pub fn mask_value(val: &mut Value, ctx: &MaskContext) {
    mask_recursive(val, ctx, "$", None, 0);
}

fn mask_recursive(
    val: &mut Value,
    ctx: &MaskContext,
    current_path: &str,
    parent_key: Option<&str>,
    depth: usize,
) {
    if depth > ctx.max_depth {
        *val = Value::String("<DEPTH_EXCEEDED>".to_string());
        return;
    }

    // 1. Check if there is an exact or wildcard custom JSONPath rule matching this path
    if let Some(rule) = match_custom_rule(ctx, current_path) {
        apply_custom_rule(val, rule);
        return;
    }

    // 2. Check strict PII mode allowlist
    if ctx.strict_pii_mode && is_leaf(val) {
        if !ctx.unmask_allow_list.contains(current_path) {
            *val = Value::String(REDACTED.to_string());
            return;
        }
    }

    match val {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let child_path = format!("{current_path}.{key}");
                mask_recursive(child, ctx, &child_path, Some(key), depth + 1);
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter_mut().enumerate() {
                let child_path = format!("{current_path}[{idx}]");
                mask_recursive(child, ctx, &child_path, parent_key, depth + 1);
            }
        }
        Value::String(s) => {
            if !ctx.enable_builtin_heuristics {
                return;
            }
            if s.starts_with("<MASKED_") && s.ends_with('>') {
                return;
            }

            if UUID_REGEX.is_match(s) {
                *s = MASKED_UUID.to_string();
            } else if JWT_REGEX.is_match(s) {
                *s = MASKED_JWT.to_string();
            } else if MONGODB_OBJECT_ID_REGEX.is_match(s) {
                *s = MASKED_OBJECT_ID.to_string();
            } else if SSN_REGEX.is_match(s) {
                *s = MASKED_SSN.to_string();
            } else if is_credit_card_luhn(s) {
                *s = MASKED_CREDIT_CARD.to_string();
            } else if EMAIL_REGEX.is_match(s) {
                *s = MASKED_EMAIL.to_string();
            } else if ISO8601_REGEX.is_match(s) {
                *s = MASKED_TIMESTAMP.to_string();
            }
        }
        Value::Number(num) => {
            if !ctx.enable_builtin_heuristics {
                return;
            }
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

fn is_leaf(val: &Value) -> bool {
    matches!(val, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

/// Pre-write secret scanner that aborts writes if raw unmasked secrets (e.g. AWS Keys) exist.
pub fn scan_unmasked_secrets(val: &Value) -> Result<(), String> {
    scan_secrets_recursive(val, "$")
}

fn scan_secrets_recursive(val: &Value, path: &str) -> Result<(), String> {
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                scan_secrets_recursive(v, &format!("{path}.{k}"))?;
            }
        }
        Value::Array(arr) => {
            for (idx, v) in arr.iter().enumerate() {
                scan_secrets_recursive(v, &format!("{path}[{idx}]"))?;
            }
        }
        Value::String(s) => {
            if AWS_KEY_REGEX.is_match(s) {
                return Err(format!("Unmasked AWS Access Key detected at '{path}': {s}"));
            }
            if PRIVATE_KEY_HEADER.is_match(s) {
                return Err(format!("Unmasked Private Key Header detected at '{path}'"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Luhn algorithm validation for credit card numbers.
fn is_credit_card_luhn(s: &str) -> bool {
    let sanitized: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if sanitized.len() < 13 || sanitized.len() > 19 {
        return false;
    }

    let mut sum = 0;
    let mut alternate = false;
    for ch in sanitized.chars().rev() {
        let mut digit = ch.to_digit(10).unwrap();
        if alternate {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
        alternate = !alternate;
    }

    sum % 10 == 0
}

fn match_custom_rule<'a>(ctx: &'a MaskContext, path: &str) -> Option<&'a CustomMaskRule> {
    if let Some(rule) = ctx.path_rules.get(path) {
        return Some(rule);
    }
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
    *val = Value::String(rule.replacement.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_credit_card_luhn_masking() {
        // Valid Visa test card
        let mut input = json!({
            "card": "4532015112830366"
        });
        let ctx = MaskContext::new(&MaskingConfig::default(), &[]);
        mask_value(&mut input, &ctx);
        assert_eq!(input, json!({"card": "<MASKED_CREDIT_CARD>"}));
    }

    #[test]
    fn test_ssn_masking() {
        let mut input = json!({
            "ssn": "123-45-6789"
        });
        let ctx = MaskContext::new(&MaskingConfig::default(), &[]);
        mask_value(&mut input, &ctx);
        assert_eq!(input, json!({"ssn": "<MASKED_SSN>"}));
    }

    #[test]
    fn test_strict_pii_mode() {
        let mut input = json!({
            "status": "active",
            "secret_note": "internal internal memo"
        });
        let config = MaskingConfig {
            enable_builtin_heuristics: true,
            strict_pii_mode: true,
            unmask_allow_list: vec!["$.status".to_string()],
            pre_write_secret_scan: true,
            custom_rules: vec![],
        };
        let ctx = MaskContext::new(&config, &[]);
        mask_value(&mut input, &ctx);
        assert_eq!(input, json!({"status": "active", "secret_note": "<REDACTED>"}));
    }

    #[test]
    fn test_pre_write_secret_guard() {
        let payload = json!({
            "aws": "AKIAIOSFODNN7EXAMPLE"
        });
        let res = scan_unmasked_secrets(&payload);
        assert!(res.is_err());
    }
}
