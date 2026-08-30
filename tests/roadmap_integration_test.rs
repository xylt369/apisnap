use apisnap::config::{EndpointConfig, HttpMethod, MaskingConfig, Protocol, UpstreamDependency};
use apisnap::engine::{
    AdaptiveBaselineLearner, BlastRadiusCalculator, BlastSeverity, DiffKind, MaskContext,
};
use apisnap::importer::{CurlImporter, HarImporter, PostmanImporter};
use apisnap::snapshot::ApprovalLedger;
use apisnap::storage::{
    MerkleCasStore, MerkleSnapshotPointer, ObservationSource, TimelineStore,
};
use std::collections::HashMap;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_curl_importer_integration() {
    let curl_cmd = r#"curl -X POST https://api.production.io/v2/payments -H "Authorization: Bearer sec_tok_99" -H "Content-Type: application/json" -d '{"amount": 49.99, "currency": "USD"}'"#;
    let endpoint = CurlImporter::parse(curl_cmd).expect("Should parse valid cURL command");

    assert_eq!(endpoint.name, "post_v2_payments");
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.path, "/v2/payments");
    assert_eq!(
        endpoint.headers.get("Authorization").unwrap(),
        "Bearer sec_tok_99"
    );
    assert_eq!(
        endpoint.headers.get("Content-Type").unwrap(),
        "application/json"
    );
    assert_eq!(endpoint.body.as_ref().unwrap()["amount"], 49.99);
}

#[test]
fn test_postman_collection_importer_integration() {
    let postman_json = r#"{
        "info": { "name": "E-Commerce API Suite" },
        "item": [
            {
                "name": "Cart Operations",
                "item": [
                    {
                        "name": "Add Item To Cart",
                        "request": {
                            "method": "POST",
                            "url": "{{API_HOST}}/api/cart/items",
                            "header": [{ "key": "X-Session-ID", "value": "{{SESSION_ID}}" }],
                            "body": { "mode": "raw", "raw": "{\"sku\": \"A-901\", \"qty\": 2}" }
                        }
                    }
                ]
            }
        ]
    }"#;

    let endpoints =
        PostmanImporter::parse_collection(postman_json).expect("Should parse Postman collection");
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].name, "Add_Item_To_Cart");
    assert_eq!(endpoints[0].method, HttpMethod::Post);
    assert_eq!(endpoints[0].path, "${API_HOST}/api/cart/items");
    assert_eq!(
        endpoints[0].headers.get("X-Session-ID").unwrap(),
        "${SESSION_ID}"
    );
    assert_eq!(endpoints[0].body.as_ref().unwrap()["sku"], "A-901");
}

#[test]
fn test_har_archive_importer_integration() {
    let har_json = r#"{
        "log": {
            "entries": [
                {
                    "request": {
                        "method": "GET",
                        "url": "https://service.internal/v1/inventory/items",
                        "headers": [{ "name": "Accept", "value": "application/json" }]
                    }
                },
                {
                    "request": {
                        "method": "GET",
                        "url": "https://service.internal/static/bundle.js",
                        "headers": []
                    }
                }
            ]
        }
    }"#;

    let endpoints = HarImporter::parse_har(har_json).expect("Should parse HAR archive");
    // Bundle.js must be filtered out as static asset
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].name, "get_v1_inventory_items");
    assert_eq!(endpoints[0].path, "/v1/inventory/items");
}

#[tokio::test]
async fn test_adaptive_noise_learner_integration() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mock_server = MockServer::start().await;

    // Server returns dynamic counter that changes per request
    let seq = Arc::new(AtomicUsize::new(1));
    let seq_clone = Arc::clone(&seq);
    Mock::given(method("GET"))
        .and(path("/api/counter"))
        .respond_with(move |_req: &wiremock::Request| {
            let current_seq = seq_clone.fetch_add(1, Ordering::SeqCst);
            let body = serde_json::json!({
                "static_name": "worker_pool",
                "dynamic_seq_id": current_seq,
                "created_at": "2026-08-30T00:00:00Z"
            });
            ResponseTemplate::new(200).set_body_json(body)
        })
        .mount(&mock_server)
        .await;

    let endpoint = EndpointConfig {
        name: "get_counter".into(),
        protocol: Protocol::Http,
        method: HttpMethod::Get,
        path: "/api/counter".into(),
        grpc: None,
        headers: HashMap::new(),
        query_params: HashMap::new(),
        body: None,
        expected_status: 200,
        timeout_override: None,
        float_epsilon_override: None,
        auth_override: None,
        mask_overrides: Vec::new(),
        array_modes: HashMap::new(),
        upstream_dependencies: Vec::new(),
    };

    let mask_config = MaskingConfig::default();
    let mask_ctx = MaskContext::new(&mask_config, &[]);

    let report = AdaptiveBaselineLearner::learn_endpoint(
        &endpoint,
        &mock_server.uri(),
        &HashMap::new(),
        None,
        3,
        &mask_ctx,
    )
    .await
    .expect("Should learn noise across 3 iterations");

    assert!(report.unstable_paths.contains(&"$.dynamic_seq_id".to_string()));
    assert!(!report.unstable_paths.contains(&"$.static_name".to_string()));
    assert_eq!(report.candidate_rules.len(), 1);
    assert_eq!(report.candidate_rules[0].json_path, "$.dynamic_seq_id");
}

#[test]
fn test_merkle_snapshot_pointer_multi_branch_workflow() {
    let temp = tempdir().unwrap();
    let cas_dir = temp.path().join(".cas");
    let main_branch_dir = temp.path().join("main");
    let pr_branch_dir = temp.path().join("pr-402");

    let mut cas = MerkleCasStore::new(&cas_dir).expect("Init CAS");

    let main_ast = serde_json::json!({
        "status": "ACTIVE",
        "role": "admin",
        "limits": { "max_ops": 1000 }
    });
    let pr_ast = serde_json::json!({
        "status": "ACTIVE",
        "role": "super_admin", // Changed in PR
        "limits": { "max_ops": 2000 }
    });

    let main_hash = cas.ingest(&main_ast).expect("Ingest main");
    let pr_hash = cas.ingest(&pr_ast).expect("Ingest pr");

    let main_ptr = MerkleSnapshotPointer::new("user_service", main_hash, 200, 24);
    let pr_ptr = MerkleSnapshotPointer::new("user_service", pr_hash, 200, 21);

    main_ptr.save(&main_branch_dir).expect("Save main pointer");
    pr_ptr.save(&pr_branch_dir).expect("Save PR pointer");

    // Load and reconstruct
    let loaded_main_ptr =
        MerkleSnapshotPointer::load(&main_branch_dir.join("user_service.ptr")).unwrap();
    let loaded_pr_ptr =
        MerkleSnapshotPointer::load(&pr_branch_dir.join("user_service.ptr")).unwrap();

    let reconstructed_main = loaded_main_ptr.reconstruct(&mut cas).unwrap();
    let reconstructed_pr = loaded_pr_ptr.reconstruct(&mut cas).unwrap();

    assert_eq!(reconstructed_main.masked_body["role"], "admin");
    assert_eq!(reconstructed_pr.masked_body["role"], "super_admin");
}

#[test]
fn test_approval_ledger_workflow() {
    let temp = tempdir().unwrap();
    let mut ledger = ApprovalLedger::load_from_dir(temp.path()).unwrap();

    assert!(!ledger.is_approved("legacy_auth"));

    ledger
        .approve(
            "legacy_auth",
            "security-team",
            "Migrated from MD5 to Argon2 hash token",
            temp.path(),
        )
        .expect("Approve diff");

    assert!(ledger.is_approved("legacy_auth"));
}

#[test]
fn test_api_behavioral_timeline_and_historical_diff() {
    let temp = tempdir().unwrap();
    let cas_dir = temp.path().join(".cas");
    let mut cas = MerkleCasStore::new(&cas_dir).expect("Init CAS");
    let timeline = TimelineStore::new(&cas_dir);

    // Day 1
    let ast_day1 = serde_json::json!({ "api_version": "1.0", "rate_limit": 100 });
    let hash_day1 = cas.ingest(&ast_day1).unwrap();
    let commit1 = timeline
        .record_observation(
            "orders_api",
            hash_day1,
            12.0,
            200,
            ObservationSource::ManualRecord,
            &mut cas,
        )
        .unwrap();

    // Day 30 (rate limit increased, new field added)
    let ast_day30 = serde_json::json!({ "api_version": "1.1", "rate_limit": 500, "region": "us-east-1" });
    let hash_day30 = cas.ingest(&ast_day30).unwrap();
    let commit2 = timeline
        .record_observation(
            "orders_api",
            hash_day30,
            18.5,
            200,
            ObservationSource::CiPipeline {
                pr_id: Some("881".into()),
            },
            &mut cas,
        )
        .unwrap();

    assert_eq!(commit2.parent_commit, Some(commit1.commit_id.clone()));
    assert_eq!(commit2.structural_delta_summary.fields_added, 1);

    // Query timeline
    let commits = timeline.get_timeline("orders_api", 10).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].commit_id, commit2.commit_id);

    // Historical Diff between Day 1 and Day 30
    let diff_report = timeline
        .diff_historical_commits(&mut cas, &commit1, &commit2)
        .unwrap();
    assert!(!diff_report.differences.is_empty());
    assert!(!diff_report.passed());
}

#[test]
fn test_cross_service_blast_radius_radar_integration() {
    let upstream_service = "auth_service.verify_token";

    // Breaking change: field "permissions" is removed from auth_service response
    let diffs = vec![DiffKind::Removed {
        json_path: "$.auth.permissions".into(),
        old_value: serde_json::json!(["read:reports", "write:reports"]),
    }];

    let reports_service = EndpointConfig {
        name: "reports_service.generate_pdf".into(),
        protocol: Protocol::Http,
        method: HttpMethod::Post,
        path: "/reports/pdf".into(),
        grpc: None,
        headers: HashMap::new(),
        query_params: HashMap::new(),
        body: None,
        expected_status: 200,
        timeout_override: None,
        float_epsilon_override: None,
        auth_override: None,
        mask_overrides: Vec::new(),
        array_modes: HashMap::new(),
        upstream_dependencies: vec![UpstreamDependency {
            upstream_endpoint: "auth_service.verify_token".into(),
            consumed_json_paths: vec!["$.auth.permissions".into()],
            owning_team: Some("analytics-squad".into()),
        }],
    };

    let billing_service = EndpointConfig {
        name: "billing_service.invoice".into(),
        protocol: Protocol::Http,
        method: HttpMethod::Post,
        path: "/billing/invoice".into(),
        grpc: None,
        headers: HashMap::new(),
        query_params: HashMap::new(),
        body: None,
        expected_status: 200,
        timeout_override: None,
        float_epsilon_override: None,
        auth_override: None,
        mask_overrides: Vec::new(),
        array_modes: HashMap::new(),
        upstream_dependencies: vec![UpstreamDependency {
            upstream_endpoint: "auth_service.verify_token".into(),
            consumed_json_paths: vec!["$.auth.user_id".into()], // does NOT consume permissions
            owning_team: Some("billing-squad".into()),
        }],
    };

    let all_endpoints = vec![reports_service, billing_service];
    let report = BlastRadiusCalculator::calculate(upstream_service, &diffs, &all_endpoints);

    // Only reports_service is impacted by permissions removal
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].affected_endpoint,
        "reports_service.generate_pdf"
    );
    assert_eq!(report.findings[0].severity, BlastSeverity::Critical);
    assert_eq!(
        report.findings[0].affected_team.as_deref(),
        Some("analytics-squad")
    );
    assert_eq!(
        report.findings[0].triggering_paths,
        vec!["$.auth.permissions"]
    );
}
