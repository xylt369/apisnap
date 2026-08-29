use apisnap::config::{ArrayDiffMode, CustomMaskRule, EndpointConfig, HttpMethod, MaskingConfig};
use apisnap::engine::{
    compare_json_ast, mask_value, scan_unmasked_secrets, DiffKind, DiffOptions, MaskContext,
};
use apisnap::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// 1. Nested JSON Masking Test (RFC Section 6.1 #1)
#[test]
fn test_mandatory_1_nested_json_masking() {
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

    assert_eq!(
        input, expected,
        "Masking must be applied at arbitrary nesting depth across independent sibling branches"
    );
}

/// 2. Array Reordering Test (Set Mode vs Ordered Mode) (RFC Section 6.1 #2)
#[test]
fn test_mandatory_2_array_reordering() {
    let expected = json!({"tags": ["a", "b", "c"]});
    let actual = json!({"tags": ["c", "a", "b"]});

    // A. Set Mode: Reordering alone must not produce differences
    let mut set_options = DiffOptions::default();
    set_options
        .array_modes
        .insert("$.tags".to_string(), ArrayDiffMode::Set);

    let set_diffs = compare_json_ast(&expected, &actual, &set_options);
    assert!(
        set_diffs.is_empty(),
        "Set mode must ignore element reordering"
    );

    // B. Ordered Mode: Must report index-level modified diffs
    let ordered_options = DiffOptions::default();
    let ordered_diffs = compare_json_ast(&expected, &actual, &ordered_options);
    assert_eq!(
        ordered_diffs.len(),
        2,
        "Ordered mode must detect index differences"
    );

    let modified_paths: Vec<&str> = ordered_diffs
        .iter()
        .map(|d| match d {
            DiffKind::Modified { json_path, .. } => json_path.as_str(),
            _ => "",
        })
        .collect();

    assert!(modified_paths.contains(&"$.tags[0]"));
    assert!(modified_paths.contains(&"$.tags[2]"));
}

/// 3. Custom Regex Rule Overrides Test (RFC Section 6.1 #3)
#[test]
fn test_mandatory_3_custom_regex_rule_overrides() {
    let mut input = json!({
        "data": {
            "token": "abc.def.ghi"
        }
    });

    let custom_rule = CustomMaskRule {
        json_path: "$.data.token".to_string(),
        replacement: "<CUSTOM_TOKEN>".to_string(),
        pattern: None,
    };

    let ctx = MaskContext::new(&MaskingConfig::default(), &[custom_rule]);
    mask_value(&mut input, &ctx);

    let expected = json!({
        "data": {
            "token": "<CUSTOM_TOKEN>"
        }
    });

    assert_eq!(
        input, expected,
        "Custom rule at exact path must take precedence over builtin heuristics"
    );
}

/// 4. Type Mismatches Test (RFC Section 6.1 #4)
#[test]
fn test_mandatory_4_type_mismatches() {
    let expected = json!({"count": 5});
    let actual = json!({"count": "5"});

    let diffs = compare_json_ast(&expected, &actual, &DiffOptions::default());
    assert_eq!(diffs.len(), 1, "Must produce exactly one diff entry");

    match &diffs[0] {
        DiffKind::TypeMismatch {
            json_path,
            expected_type,
            actual_type,
            old_value,
            new_value,
        } => {
            assert_eq!(json_path, "$.count");
            assert_eq!(*expected_type, "number");
            assert_eq!(*actual_type, "string");
            assert_eq!(old_value, &json!(5));
            assert_eq!(new_value, &json!("5"));
        }
        other => panic!("Expected TypeMismatch, got {:?}", other),
    }
}

/// 5. Endpoint-Level Mask Override Precedence Test (RFC Section 6.1 #5)
#[test]
fn test_mandatory_5_endpoint_level_mask_override_precedence() {
    let global_config = MaskingConfig {
        enable_builtin_heuristics: true,
        strict_pii_mode: false,
        unmask_allow_list: vec![],
        pre_write_secret_scan: true,
        custom_rules: vec![],
    };

    let endpoint_a = EndpointConfig {
        name: "endpoint_a".to_string(),
        method: HttpMethod::Get,
        path: "/api/a".to_string(),
        headers: Default::default(),
        query_params: Default::default(),
        body: None,
        expected_status: 200,
        timeout_override: None,
        float_epsilon_override: None,
        mask_overrides: vec![CustomMaskRule {
            json_path: "$.data.secret".to_string(),
            replacement: "<ENDPOINT_MASKED>".to_string(),
            pattern: None,
        }],
        array_modes: Default::default(),
    };

    let endpoint_b = EndpointConfig {
        name: "endpoint_b".to_string(),
        method: HttpMethod::Get,
        path: "/api/b".to_string(),
        headers: Default::default(),
        query_params: Default::default(),
        body: None,
        expected_status: 200,
        timeout_override: None,
        float_epsilon_override: None,
        mask_overrides: vec![],
        array_modes: Default::default(),
    };

    let mut payload_a = json!({"data": {"secret": "my-plain-secret-value"}});
    let ctx_a = MaskContext::new(&global_config, &endpoint_a.mask_overrides);
    mask_value(&mut payload_a, &ctx_a);
    assert_eq!(
        payload_a,
        json!({"data": {"secret": "<ENDPOINT_MASKED>"}}),
        "Endpoint A override must mask secret"
    );

    let mut payload_b = json!({"data": {"secret": "my-plain-secret-value"}});
    let ctx_b = MaskContext::new(&global_config, &endpoint_b.mask_overrides);
    mask_value(&mut payload_b, &ctx_b);
    assert_eq!(
        payload_b,
        json!({"data": {"secret": "my-plain-secret-value"}}),
        "Endpoint B without override must leave plain string untouched"
    );
}

/// 6. Snapshot Store Atomic Write & Read Round-Trip Test
#[test]
fn test_snapshot_store_atomic_roundtrip() {
    let tmp_dir = tempdir().unwrap();
    let store = SnapshotStore::new(tmp_dir.path());

    let snapshot = SnapshotFile {
        endpoint_name: "users_list".to_string(),
        metadata: SnapshotMetadata {
            recorded_at: "2026-08-29T22:00:00Z".to_string(),
            duration_ms: 42,
            status_code: 200,
            response_headers: [("content-type".to_string(), "application/json".to_string())]
                .into_iter()
                .collect(),
            apisnap_version: "0.2.0".to_string(),
        },
        masked_body: json!({
            "users": [
                {"id": "<MASKED_UUID>", "name": "Alice"}
            ]
        }),
    };

    let path = store.write_snapshot_atomic(&snapshot).unwrap();
    assert!(path.exists());

    let read_back = store.read_snapshot("users_list").unwrap();
    assert_eq!(snapshot, read_back);
}

/// 7. v0.2.0 Hardening: Float Epsilon Tolerance Test
#[test]
fn test_v020_float_epsilon_tolerance() {
    let expected = json!({"rate": 1.2345601});
    let actual = json!({"rate": 1.2345609});

    let mut options = DiffOptions::default();
    options.float_epsilon = 0.0001;

    let diffs = compare_json_ast(&expected, &actual, &options);
    assert!(
        diffs.is_empty(),
        "Float values within epsilon tolerance must not trigger modified diffs"
    );
}

/// 8. v0.2.0 Hardening: Unicode NFC Key Normalization Test
#[test]
fn test_v020_unicode_nfc_key_normalization() {
    let nfd_key = "cafe\u{301}"; // e + combining acute accent
    let nfc_key = "caf\u{e9}";  // single precomposed character

    let mut exp_map = serde_json::Map::new();
    exp_map.insert(nfd_key.to_string(), json!("latte"));
    let expected = Value::Object(exp_map);

    let mut act_map = serde_json::Map::new();
    act_map.insert(nfc_key.to_string(), json!("latte"));
    let actual = Value::Object(act_map);

    let options = DiffOptions {
        normalize_unicode_keys: true,
        ..Default::default()
    };

    let diffs = compare_json_ast(&expected, &actual, &options);
    assert!(
        diffs.is_empty(),
        "Unicode NFC key normalization must seamlessly equate NFD and NFC keys"
    );
}

/// 9. v0.2.0 Hardening: Credit Card Luhn & SSN PII Masking
#[test]
fn test_v020_pii_credit_card_and_ssn() {
    let mut payload = json!({
        "payment": {
            "card_number": "4532015112830366", // Valid Visa test number
            "holder_ssn": "987-65-4321",
            "contact_email": "alice@security.io"
        }
    });

    let ctx = MaskContext::new(&MaskingConfig::default(), &[]);
    mask_value(&mut payload, &ctx);

    assert_eq!(
        payload,
        json!({
            "payment": {
                "card_number": "<MASKED_CREDIT_CARD>",
                "holder_ssn": "<MASKED_SSN>",
                "contact_email": "<MASKED_EMAIL>"
            }
        })
    );
}

/// 10. v0.2.0 Hardening: Strict PII Deny-By-Default Allow-List Mode
#[test]
fn test_v020_strict_pii_allow_list() {
    let mut payload = json!({
        "status": "healthy",
        "private_token": "secret_user_blob_991823"
    });

    let config = MaskingConfig {
        enable_builtin_heuristics: true,
        strict_pii_mode: true,
        unmask_allow_list: vec!["$.status".to_string()],
        pre_write_secret_scan: true,
        custom_rules: vec![],
    };

    let ctx = MaskContext::new(&config, &[]);
    mask_value(&mut payload, &ctx);

    assert_eq!(
        payload,
        json!({
            "status": "healthy",
            "private_token": "<REDACTED>"
        }),
        "Strict PII mode must redact any leaf not explicitly in unmask_allow_list"
    );
}

/// 11. v0.2.0 Hardening: Pre-Write Secret Guard
#[test]
fn test_v020_pre_write_secret_guard_blocks_leak() {
    let leaked_payload = json!({
        "aws_key": "AKIAIOSFODNN7EXAMPLE"
    });

    let result = scan_unmasked_secrets(&leaked_payload);
    assert!(
        result.is_err(),
        "Pre-write secret guard must detect unmasked AWS access keys"
    );
}
