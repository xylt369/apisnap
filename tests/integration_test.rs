use apisnap::client::auth::{ApiKeyAuth, AuthProvider, StaticBearerAuth};
use apisnap::config::{
    ApiSnapConfig, ArrayDiffMode, CustomMaskRule, EndpointConfig, HttpMethod, MaskingConfig,
};
use apisnap::crypto::SnapshotEncryptor;
use apisnap::engine::{
    compare_json_ast, mask_value, scan_unmasked_secrets, DiffKind, DiffOptions, FastJsonEngine,
    MaskContext,
};
use apisnap::fuzz::generate_mutations;
use apisnap::openapi::{generate_openapi_spec, verify_openapi_spec};
use apisnap::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
use apisnap::ui::generate_pr_comment_markdown;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::fs;
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
        auth_override: None,
        mask_overrides: vec![CustomMaskRule {
            json_path: "$.data.secret".to_string(),
            replacement: "<ENDPOINT_MASKED>".to_string(),
            pattern: None,
        }],
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
            grpc_status_code: None,
            response_headers: [("content-type".to_string(), "application/json".to_string())]
                .into_iter()
                .collect(),
            apisnap_version: "1.0.0".to_string(),
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

/// 7. v0.8.0 AES-256-GCM Encrypted Snapshot Store Test
#[test]
fn test_v080_aes_gcm_encrypted_store() {
    let tmp_dir = tempdir().unwrap();
    let key = [0x99u8; 32];
    let encryptor = SnapshotEncryptor::new(&key);

    let store = SnapshotStore::new(tmp_dir.path()).with_encryptor(Some(encryptor.clone()));

    let snapshot = SnapshotFile {
        endpoint_name: "financial_report".to_string(),
        metadata: SnapshotMetadata {
            recorded_at: "2026-08-30T00:00:00Z".to_string(),
            duration_ms: 15,
            status_code: 200,
            grpc_status_code: None,
            response_headers: Default::default(),
            apisnap_version: "1.0.0".to_string(),
        },
        masked_body: json!({
            "balance": 1500000,
            "currency": "USD"
        }),
    };

    let enc_path = store.write_snapshot_atomic(&snapshot).unwrap();
    assert!(enc_path.exists());

    // Ensure raw file is NOT plaintext JSON
    let raw_bytes = fs::read(&enc_path).unwrap();
    assert!(!String::from_utf8_lossy(&raw_bytes).contains("financial_report"));

    // Ensure decrypting with key reads back exact struct
    let read_back = store.read_snapshot("financial_report").unwrap();
    assert_eq!(snapshot, read_back);
}

/// 8. v0.7.0 Fuzzing Mutation Generator Test
#[test]
fn test_v070_fuzz_mutator() {
    let baseline = json!({
        "order_id": "ORD-1234",
        "amount": 99.5
    });

    let cases = generate_mutations(&baseline);
    assert!(cases.len() >= 5);
    let descriptions: Vec<String> = cases.iter().map(|c| c.description.clone()).collect();
    assert!(descriptions.iter().any(|d| d.contains("SQL injection")));
    assert!(descriptions.iter().any(|d| d.contains("Omit required key")));
}

/// 9. v0.9.0 PR Visual Diff Markdown Formatter Test
#[test]
fn test_v090_pr_comment_generator() {
    let reports = vec![apisnap::engine::DiffReport {
        endpoint_name: "get_users".to_string(),
        differences: vec![],
        is_match: true,
        expected_status: 200,
        actual_status: 200,
    }];

    let markdown = generate_pr_comment_markdown(&reports, 25);
    assert!(markdown.contains("## 📸 ApiSnap Regression Test Summary"));
    assert!(markdown.contains("🟢 PASS"));
}
