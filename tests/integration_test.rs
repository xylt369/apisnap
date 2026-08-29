use apisnap::client::auth::{ApiKeyAuth, AuthProvider, StaticBearerAuth};
use apisnap::config::{
    ApiSnapConfig, ArrayDiffMode, CustomMaskRule, EndpointConfig, HttpMethod, MaskingConfig,
};
use apisnap::engine::{
    compare_json_ast, mask_value, scan_unmasked_secrets, DiffKind, DiffOptions, MaskContext,
};
use apisnap::openapi::{generate_openapi_spec, verify_openapi_spec};
use apisnap::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
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
            response_headers: [("content-type".to_string(), "application/json".to_string())]
                .into_iter()
                .collect(),
            apisnap_version: "0.3.0".to_string(),
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

/// 7. v0.3.0 AuthProvider Header Injection Test
#[tokio::test]
async fn test_v030_auth_provider_headers() {
    let client = reqwest::Client::new();
    let bearer = StaticBearerAuth {
        token: "jwt_token_12345".to_string(),
    };

    let req = client.get("http://localhost/test");
    let req = bearer.apply(req).await.unwrap();
    let built = req.build().unwrap();

    let auth_header = built.headers().get("authorization").unwrap().to_str().unwrap();
    assert_eq!(auth_header, "Bearer jwt_token_12345");

    let api_key = ApiKeyAuth {
        header_name: "X-Secret-Key".to_string(),
        key: "secret_api_val".to_string(),
    };
    let req2 = client.get("http://localhost/test");
    let req2 = api_key.apply(req2).await.unwrap();
    let built2 = req2.build().unwrap();
    assert_eq!(
        built2.headers().get("x-secret-key").unwrap().to_str().unwrap(),
        "secret_api_val"
    );
}

/// 8. v0.5.0 Bidirectional OpenAPI Generation and Round-Trip Verification Test
#[test]
fn test_v050_openapi_generate_and_verify_roundtrip() {
    let tmp_dir = tempdir().unwrap();
    let snapshot_dir = tmp_dir.path().join("__snapshots__");
    fs::create_dir_all(&snapshot_dir).unwrap();

    let store = SnapshotStore::new(&snapshot_dir);
    let snapshot = SnapshotFile {
        endpoint_name: "get_user".to_string(),
        metadata: SnapshotMetadata {
            recorded_at: "2026-08-30T00:00:00Z".to_string(),
            duration_ms: 12,
            status_code: 200,
            response_headers: Default::default(),
            apisnap_version: "0.3.0".to_string(),
        },
        masked_body: json!({
            "user_id": "<MASKED_UUID>",
            "created_at": "<MASKED_TIMESTAMP>",
            "email": "<MASKED_EMAIL>",
            "age": 30
        }),
    };
    store.write_snapshot_atomic(&snapshot).unwrap();

    let config = ApiSnapConfig {
        base_url: "https://api.example.com".to_string(),
        timeout: std::time::Duration::from_secs(10),
        concurrency: 5,
        max_depth: 512,
        float_epsilon: 0.0,
        normalize_unicode_keys: true,
        auth: None,
        global_headers: Default::default(),
        masking: MaskingConfig::default(),
        endpoints: vec![EndpointConfig {
            name: "get_user".to_string(),
            method: HttpMethod::Get,
            path: "/api/v1/users/123".to_string(),
            headers: Default::default(),
            query_params: Default::default(),
            body: None,
            expected_status: 200,
            timeout_override: None,
            float_epsilon_override: None,
            auth_override: None,
            mask_overrides: vec![],
            array_modes: Default::default(),
        }],
        snapshot_dir: snapshot_dir.display().to_string(),
    };

    let spec_path = tmp_dir.path().join("openapi.yaml");
    let spec_str = generate_openapi_spec(&config, spec_path.to_str().unwrap()).unwrap();
    assert!(spec_str.contains("openapi: 3.1.0"));
    assert!(spec_str.contains("/api/v1/users/123"));

    // Verify round-trip has zero drift
    let verify_res = verify_openapi_spec(&config, spec_path.to_str().unwrap()).unwrap();
    assert_eq!(verify_res.total_endpoints_checked, 1);
    assert_eq!(verify_res.matched_count, 1);
    assert_eq!(verify_res.drift_count, 0);
}
