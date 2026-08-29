use crate::config::{CustomMaskRule, MaskingConfig};
use crate::engine::jit_rule::{fnv1a_hash, CompiledMatchFn, CraneliftRuleEngine};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

/// Pre-parsed token in a JSONPath expression tree (e.g. `$.items[*].id`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Root,
    Key(String),
    Index(usize),
    WildcardArray,
}

/// Tokenized rule compiled once at configuration load time.
#[derive(Debug, Clone)]
pub struct ParsedMaskRule {
    pub segments: Vec<PathSegment>,
    pub rule: CustomMaskRule,
}

/// Resolved masking context passed during recursive AST traversal.
#[derive(Clone)]
pub struct MaskContext {
    pub enable_builtin_heuristics: bool,
    pub strict_pii_mode: bool,
    pub max_depth: usize,
    pub unmask_allow_list: HashSet<String>,
    pub tokenized_rules: Vec<ParsedMaskRule>,
    pub precompiled_patterns: HashMap<String, Arc<Regex>>,
    pub jit_match_fn: Option<CompiledMatchFn>,
}

impl MaskContext {
    pub fn new(global_config: &MaskingConfig, overrides: &[CustomMaskRule]) -> Self {
        let mut tokenized_rules = Vec::new();
        let mut precompiled_patterns = HashMap::new();

        let mut add_rule = |rule: &CustomMaskRule| {
            let segments = parse_json_path_to_segments(&rule.json_path);
            tokenized_rules.push(ParsedMaskRule {
                segments,
                rule: rule.clone(),
            });

            if let Some(pattern_str) = &rule.pattern {
                if !precompiled_patterns.contains_key(pattern_str) {
                    if let Ok(re) = Regex::new(pattern_str) {
                        precompiled_patterns.insert(pattern_str.clone(), Arc::new(re));
                    }
                }
            }
        };

        // 1. Insert global custom rules
        for rule in &global_config.custom_rules {
            add_rule(rule);
        }

        // 2. Insert endpoint-level overrides
        for rule in overrides {
            add_rule(rule);
        }

        let unmask_allow_list: HashSet<String> = global_config.unmask_allow_list.iter().cloned().collect();

        // 3. Compile JIT rule matcher for linear key paths (RFC-002 Cranelift Engine)
        let mut jit_match_fn = None;
        let paths: Vec<String> = tokenized_rules
            .iter()
            .map(|r| r.rule.json_path.clone())
            .collect();

        if !paths.is_empty() {
            let mut engine = CraneliftRuleEngine::new();
            jit_match_fn = Some(engine.compile_rules(&paths));
        }

        Self {
            enable_builtin_heuristics: global_config.enable_builtin_heuristics,
            strict_pii_mode: global_config.strict_pii_mode,
            max_depth: 512,
            unmask_allow_list,
            tokenized_rules,
            precompiled_patterns,
            jit_match_fn,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

/// Parse a raw JSONPath string (e.g. `$.data.items[*].id`) into structured tokens once.
pub fn parse_json_path_to_segments(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    segments.push(PathSegment::Root);

    let clean = path.trim_start_matches('$');
    let mut current_key = String::new();
    let mut chars = clean.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '.' {
            if !current_key.is_empty() {
                segments.push(PathSegment::Key(current_key.clone()));
                current_key.clear();
            }
        } else if ch == '[' {
            if !current_key.is_empty() {
                segments.push(PathSegment::Key(current_key.clone()));
                current_key.clear();
            }
            let mut num_str = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                num_str.push(c);
            }
            if num_str == "*" {
                segments.push(PathSegment::WildcardArray);
            } else if let Ok(idx) = num_str.parse::<usize>() {
                segments.push(PathSegment::Index(idx));
            }
        } else {
            current_key.push(ch);
        }
    }

    if !current_key.is_empty() {
        segments.push(PathSegment::Key(current_key));
    }

    segments
}

/// Recursively masks a JSON AST value in-place according to the given context.
pub fn mask_value(val: &mut Value, ctx: &MaskContext) {
    let mut current_segments = vec![PathSegment::Root];
    mask_recursive(val, ctx, &mut current_segments, None, 0);
}

fn mask_recursive(
    val: &mut Value,
    ctx: &MaskContext,
    current_segments: &mut Vec<PathSegment>,
    parent_key: Option<&str>,
    depth: usize,
) {
    if depth > ctx.max_depth {
        *val = Value::String("<DEPTH_EXCEEDED>".to_string());
        return;
    }

    // 1. Check pre-tokenized / JIT compiled JSONPath rules
    if let Some(rule) = match_tokenized_rule(ctx, current_segments) {
        apply_custom_rule(val, rule, &ctx.precompiled_patterns);
        return;
    }

    // 2. Check strict PII mode allowlist
    if ctx.strict_pii_mode && is_leaf(val) {
        let string_path = segments_to_string_path(current_segments);
        if !ctx.unmask_allow_list.contains(&string_path) {
            *val = Value::String(REDACTED.to_string());
            return;
        }
    }

    match val {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                current_segments.push(PathSegment::Key(key.clone()));
                mask_recursive(child, ctx, current_segments, Some(key), depth + 1);
                current_segments.pop();
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter_mut().enumerate() {
                current_segments.push(PathSegment::Index(idx));
                mask_recursive(child, ctx, current_segments, parent_key, depth + 1);
                current_segments.pop();
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

fn match_tokenized_rule<'a>(ctx: &'a MaskContext, current: &[PathSegment]) -> Option<&'a CustomMaskRule> {
    // Fast path: Cranelift JIT match for simple object paths (e.g. $.user.token)
    if let Some(jit_fn) = ctx.jit_match_fn {
        let is_key_only = current.iter().skip(1).all(|s| matches!(s, PathSegment::Key(_)));
        if is_key_only && current.len() > 1 {
            let hashes: Vec<u64> = current
                .iter()
                .skip(1)
                .map(|s| match s {
                    PathSegment::Key(k) => fnv1a_hash(k),
                    _ => 0,
                })
                .collect();
            let match_idx = unsafe { jit_fn(hashes.as_ptr(), hashes.len() as u64) };
            if match_idx != u32::MAX && (match_idx as usize) < ctx.tokenized_rules.len() {
                return Some(&ctx.tokenized_rules[match_idx as usize].rule);
            }
        }
    }

    // Standard fallback matching (with array wildcard support)
    for parsed in &ctx.tokenized_rules {
        if segments_match(&parsed.segments, current) {
            return Some(&parsed.rule);
        }
    }
    None
}

fn segments_match(rule_segments: &[PathSegment], current: &[PathSegment]) -> bool {
    if rule_segments.len() != current.len() {
        return false;
    }
    for (r, c) in rule_segments.iter().zip(current.iter()) {
        match (r, c) {
            (PathSegment::Root, PathSegment::Root) => {}
            (PathSegment::Key(k1), PathSegment::Key(k2)) if k1 == k2 => {}
            (PathSegment::Index(i1), PathSegment::Index(i2)) if i1 == i2 => {}
            (PathSegment::WildcardArray, PathSegment::Index(_)) => {}
            _ => return false,
        }
    }
    true
}

fn segments_to_string_path(segments: &[PathSegment]) -> String {
    let mut out = String::from("$");
    for seg in segments.iter().skip(1) {
        match seg {
            PathSegment::Key(k) => out.push_str(&format!(".{k}")),
            PathSegment::Index(i) => out.push_str(&format!("[{i}]")),
            PathSegment::WildcardArray => out.push_str("[*]"),
            PathSegment::Root => {}
        }
    }
    out
}

fn apply_custom_rule(
    val: &mut Value,
    rule: &CustomMaskRule,
    patterns: &HashMap<String, Arc<Regex>>,
) {
    if let Some(pattern_str) = &rule.pattern {
        if let Some(re) = patterns.get(pattern_str) {
            if let Value::String(s) = val {
                *s = re.replace_all(s, rule.replacement.as_str()).to_string();
            }
        }
    } else {
        *val = Value::String(rule.replacement.clone());
    }
}

pub fn is_credit_card_luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0;
    let mut alternate = false;
    for &d in digits.iter().rev() {
        let mut val = d;
        if alternate {
            val *= 2;
            if val > 9 {
                val -= 9;
            }
        }
        sum += val;
        alternate = !alternate;
    }
    sum % 10 == 0
}

pub fn scan_unmasked_secrets(val: &Value) -> Result<(), String> {
    scan_recursive(val, "$")
}

fn scan_recursive(val: &Value, path: &str) -> Result<(), String> {
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                let child_path = format!("{path}.{k}");
                scan_recursive(v, &child_path)?;
            }
        }
        Value::Array(arr) => {
            for (idx, v) in arr.iter().enumerate() {
                let child_path = format!("{path}[{idx}]");
                scan_recursive(v, &child_path)?;
            }
        }
        Value::String(s) => {
            if s.starts_with("<MASKED_") && s.ends_with('>') {
                return Ok(());
            }
            if AWS_KEY_REGEX.is_match(s) {
                return Err(format!("Unmasked AWS Access Key detected at '{path}'"));
            }
            if PRIVATE_KEY_HEADER.is_match(s) {
                return Err(format!("Unmasked RSA/EC Private Key block detected at '{path}'"));
            }
        }
        _ => {}
    }
    Ok(())
}
