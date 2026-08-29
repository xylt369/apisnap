use apisnap::config::{ArrayDiffMode, CustomMaskRule, MaskingConfig};
use apisnap::ebpf::{extract_http_json_body, parse_captured_event, CapturedPacket};
use apisnap::engine::{
    compare_json_ast, fnv1a_hash, mask_value, CraneliftRuleEngine, DiffKind, DiffOptions,
    MaskContext,
};
use apisnap::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
use apisnap::storage::{MerkleCasStore, MerkleNode};
use apisnap::telemetry::{ApmBackend, TraceContext};
use apisnap::wasm::shadow_filter::ShadowSession;
use serde_json::json;
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
        3,
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
            assert_eq!(expected_type, "number");
            assert_eq!(actual_type, "string");
            assert_eq!(old_value, &json!(5));
            assert_eq!(new_value, &json!("5"));
        }
        other => panic!("Expected TypeMismatch, got {:?}", other),
    }
}

/// 5. Snapshot Store Atomic Write & Read Round-Trip Test
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

/// 6. RFC-002 Module 1: Merkle DAG CAS Subtree Deduplication Test
#[test]
fn test_rfc002_merkle_cas_storage() {
    let tmp = tempdir().unwrap();
    let mut store = MerkleCasStore::new(tmp.path()).unwrap();

    let val1 = json!({
        "account": {
            "id": 1001,
            "owner": "Charlie",
            "tier": "enterprise"
        },
        "version": 1
    });

    let hash1 = store.ingest(&val1).unwrap();
    let restored1 = store.reconstruct(hash1).unwrap();
    assert_eq!(val1, restored1);

    // Val2 modifies version -> 2
    let val2 = json!({
        "account": {
            "id": 1001,
            "owner": "Charlie",
            "tier": "enterprise"
        },
        "version": 2
    });

    let hash2 = store.ingest(&val2).unwrap();
    assert_ne!(hash1, hash2);

    let restored2 = store.reconstruct(hash2).unwrap();
    assert_eq!(val2, restored2);

    // Account subtree is identical and shared
    let acc_hash1 = match store.load(hash1).unwrap() {
        MerkleNode::Object { entries } => entries.iter().find(|(k, _)| k == "account").unwrap().1,
        _ => panic!("expected object"),
    };
    let acc_hash2 = match store.load(hash2).unwrap() {
        MerkleNode::Object { entries } => entries.iter().find(|(k, _)| k == "account").unwrap().1,
        _ => panic!("expected object"),
    };
    assert_eq!(acc_hash1, acc_hash2, "Account subtree must be shared in CAS");
}

/// 7. RFC-002 Module 2: Cranelift JIT Rule Compilation & Evaluation Test
#[test]
fn test_rfc002_cranelift_jit_matching() {
    let mut engine = CraneliftRuleEngine::new();
    let rules = vec![
        "$.user.credentials.api_key".to_string(),
        "$.payment.card_number".to_string(),
    ];

    let compiled_fn = engine.compile_rules(&rules);

    let path_creds = vec![
        fnv1a_hash("user"),
        fnv1a_hash("credentials"),
        fnv1a_hash("api_key"),
    ];
    let match_idx = unsafe { compiled_fn(path_creds.as_ptr(), path_creds.len() as u64) };
    assert_eq!(match_idx, 0, "Must match rule index 0");

    let path_payment = vec![fnv1a_hash("payment"), fnv1a_hash("card_number")];
    let match_idx_pay = unsafe { compiled_fn(path_payment.as_ptr(), path_payment.len() as u64) };
    assert_eq!(match_idx_pay, 1, "Must match rule index 1");

    let path_unknown = vec![fnv1a_hash("public"), fnv1a_hash("info")];
    let mismatch = unsafe { compiled_fn(path_unknown.as_ptr(), path_unknown.len() as u64) };
    assert_eq!(mismatch, u32::MAX, "Must return sentinel on mismatch");
}

/// 8. RFC-002 Module 5: OpenTelemetry Distributed Tracing & APM Link Test
#[test]
fn test_rfc002_otel_tracing() {
    let root_ctx = TraceContext::new_root();
    let header_str = root_ctx.to_traceparent_header();
    assert!(header_str.starts_with("00-"));

    let child_ctx = root_ctx.new_child_span();
    assert_eq!(root_ctx.trace_id, child_ctx.trace_id);
    assert_ne!(root_ctx.span_id, child_ctx.span_id);

    let jaeger = ApmBackend::Jaeger {
        base_url: "https://jaeger.internal.net".into(),
    };
    let link = jaeger.build_trace_link(&child_ctx);
    assert!(link.contains("https://jaeger.internal.net/trace/"));
}

/// 9. RFC-002 Module 4: Proxy-Wasm Shadow Traffic Differ Test
#[test]
fn test_rfc002_proxy_wasm_shadow_diff() {
    let mut session = ShadowSession::new(42);
    session.on_body_chunk("baseline", br#"{"id": 100, "status": "ok"}"#, true);
    session.on_body_chunk("candidate", br#"{"id": 100, "status": "ok"}"#, true);

    let is_drifted = session.check_structural_drift().unwrap();
    assert!(!is_drifted, "Identical payloads must not drift");

    let mut session_drift = ShadowSession::new(43);
    session_drift.on_body_chunk("baseline", br#"{"id": 100, "status": "ok"}"#, true);
    session_drift.on_body_chunk("candidate", br#"{"id": 100}"#, true); // missing status

    let is_drifted2 = session_drift.check_structural_drift().unwrap();
    assert!(is_drifted2, "Missing key in candidate must trigger drift");
}

/// 10. RFC-002 Module 3: eBPF Extracted HTTP Packet Parser Test
#[test]
fn test_rfc002_ebpf_packet_parsing() {
    let raw_stream = b"POST /api/v1/orders HTTP/1.1\r\nHost: example.com\r\n\r\n{\"order_id\":\"ORD-99\"}";
    let body = extract_http_json_body(raw_stream).unwrap();
    assert_eq!(body, b"{\"order_id\":\"ORD-99\"}");

    let mut pkt = CapturedPacket {
        src_ip: 0x0100007f,
        dst_ip: 0x0100007f,
        src_port: 8080,
        dst_port: 54321,
        payload_len: raw_stream.len() as u32,
        payload: [0u8; 4096],
    };
    pkt.payload[..raw_stream.len()].copy_from_slice(raw_stream);

    let parsed = parse_captured_event(&pkt).unwrap();
    assert_eq!(parsed["order_id"], "ORD-99");
}
