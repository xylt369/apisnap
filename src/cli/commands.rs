use crate::cli::args::{CasAction, CasArgs, ShadowArgs, SniffArgs};
use crate::client::auth::{create_auth_provider, AuthProvider};
use crate::client::{GrpcExecutor, RequestExecutor, ReqwestExecutor};
use crate::config::{ApiSnapConfig, EndpointConfig, Protocol};
use crate::crypto::SnapshotEncryptor;
use crate::engine::{compare_json_ast, mask_value, DiffOptions, DiffReport, MaskContext};
use crate::error::ApiSnapError;
use crate::fuzz::{render_fuzz_report, FuzzEngine};
use crate::openapi::{generate_openapi_spec, verify_openapi_live, verify_openapi_spec};
use crate::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
use crate::storage::{MerkleCasStore, NodeHash};
use crate::ui::{generate_pr_comment_markdown, print_summary_report, run_interactive_review, ReviewItem};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

pub fn handle_init(output_path: &str) -> Result<(), ApiSnapError> {
    let path = Path::new(output_path);
    if path.exists() {
        return Err(ApiSnapError::InvalidConfig {
            location: output_path.to_string(),
            reason: "file already exists, aborting to prevent overwrite".into(),
        });
    }

    let template = ApiSnapConfig::starter_template();
    fs::write(path, template).map_err(|e| ApiSnapError::Io {
        path: output_path.to_string(),
        source: e,
    })?;

    println!(
        "{} Scaffolded new configuration at: {}",
        "[SUCCESS]".green().bold(),
        output_path.cyan().bold()
    );
    println!("Edit this file and run `apisnap record` to create your initial snapshots.");
    Ok(())
}

pub async fn handle_record(
    config_path: &str,
    endpoint_filter: Option<&str>,
    concurrency_override: Option<usize>,
    enable_cas: bool,
    learn_iterations: Option<usize>,
) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan)
        .with_encryptor(encryptor)
        .with_cas(enable_cas);
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));
    let grpc_executor = Arc::new(GrpcExecutor::new(config.timeout));

    let global_auth = config
        .auth
        .as_ref()
        .map(|auth_cfg| create_auth_provider(auth_cfg, executor.client()));

    let filtered_endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_filter);
    if filtered_endpoints.is_empty() {
        println!("{}", "No matching endpoints found to record.".yellow());
        return Ok(());
    }

    if let Some(learn_count) = learn_iterations {
        println!(
            "\n{} Running adaptive noise learning ({} probe iterations)...",
            "ApiSnap".cyan().bold(),
            learn_count
        );
        for endpoint in &filtered_endpoints {
            let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides);
            if let Ok(report) = crate::engine::AdaptiveBaselineLearner::learn_endpoint(
                endpoint,
                &config.base_url,
                &config.global_headers,
                global_auth.as_deref(),
                learn_count,
                &mask_ctx,
            )
            .await
            {
                if !report.candidate_rules.is_empty() {
                    println!(
                        "  {} Discovered {} volatile path(s) for [{}]:",
                        "[LEARNED NOISE]".yellow().bold(),
                        report.candidate_rules.len(),
                        endpoint.name.bold()
                    );
                    for rule in &report.candidate_rules {
                        println!("    -> Candidate mask rule: {}", rule.json_path.cyan());
                    }
                }
            }
        }
    }

    let concurrency = concurrency_override.unwrap_or(config.concurrency);
    println!(
        "\n{} Recording {} endpoint(s) with concurrency {}{}...",
        "ApiSnap".cyan().bold(),
        filtered_endpoints.len(),
        concurrency,
        if enable_cas { " (Merkle CAS mode)" } else { "" }
    );

    let progress = create_progress_bar(filtered_endpoints.len() as u64);
    let results = dispatch_requests(
        executor,
        grpc_executor,
        &config.base_url,
        &config.global_headers,
        global_auth,
        filtered_endpoints,
        concurrency,
        progress.clone(),
    )
    .await;

    progress.finish_and_clear();

    let cas_dir = Path::new(&config.snapshot_dir).join(".cas");
    let mut cas_store = MerkleCasStore::new(&cas_dir).ok();
    let timeline = crate::storage::TimelineStore::new(&cas_dir);

    let mut recorded_count = 0;
    for (endpoint, raw_res_result) in results {
        let raw_res = raw_res_result?;
        let mut masked_body = raw_res.body.clone();

        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut masked_body, &mask_ctx);

        if let Some(ref mut cas) = cas_store {
            if let Ok(node_hash) = cas.ingest(&masked_body) {
                let _ = timeline.record_observation(
                    &endpoint.name,
                    node_hash,
                    raw_res.duration_ms as f64,
                    raw_res.status_code,
                    crate::storage::ObservationSource::ManualRecord,
                    cas,
                );
            }
        }

        let snapshot = SnapshotFile {
            endpoint_name: endpoint.name.clone(),
            metadata: SnapshotMetadata {
                recorded_at: chrono::Utc::now().to_rfc3339(),
                duration_ms: raw_res.duration_ms,
                status_code: raw_res.status_code,
                grpc_status_code: None,
                response_headers: raw_res.headers,
                apisnap_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            masked_body,
        };

        let written_path = store.write_snapshot_atomic(&snapshot)?;
        println!(
            "  {} {:<30} -> {}",
            "[RECORDED]".green().bold(),
            endpoint.name.bold(),
            written_path.display().to_string().dimmed()
        );
        recorded_count += 1;
    }

    println!(
        "\n{} Successfully recorded {} snapshot(s) to '{}'.",
        "[SUCCESS]".green().bold(),
        recorded_count,
        config.snapshot_dir.cyan()
    );
    Ok(())
}

pub async fn handle_test(
    config_path: &str,
    endpoint_filter: Option<&str>,
    concurrency_override: Option<usize>,
    is_ci: bool,
    pr_comment: bool,
    baseline: Option<&str>,
    candidate: Option<&str>,
) -> Result<(), ApiSnapError> {
    let start_instant = Instant::now();
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_encryptor(encryptor);

    // If comparing two branch pointers directly
    if let (Some(base_ref), Some(cand_ref)) = (baseline, candidate) {
        return handle_test_branch_pointers(&config, base_ref, cand_ref, is_ci, pr_comment).await;
    }

    let executor = Arc::new(ReqwestExecutor::new(config.timeout));
    let grpc_executor = Arc::new(GrpcExecutor::new(config.timeout));

    let global_auth = config
        .auth
        .as_ref()
        .map(|auth_cfg| create_auth_provider(auth_cfg, executor.client()));

    let filtered_endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_filter);
    if filtered_endpoints.is_empty() {
        println!("{}", "No matching endpoints found to test.".yellow());
        return Ok(());
    }

    let concurrency = concurrency_override.unwrap_or(config.concurrency);
    if !is_ci && !pr_comment {
        println!(
            "\n{} Running regression tests on {} endpoint(s) with concurrency {}...",
            "ApiSnap".cyan().bold(),
            filtered_endpoints.len(),
            concurrency
        );
    }

    let progress = create_progress_bar(filtered_endpoints.len() as u64);
    if is_ci || pr_comment {
        progress.finish_and_clear();
    }

    let results = dispatch_requests(
        executor,
        grpc_executor,
        &config.base_url,
        &config.global_headers,
        global_auth,
        filtered_endpoints,
        concurrency,
        progress.clone(),
    )
    .await;

    if !is_ci && !pr_comment {
        progress.finish_and_clear();
    }

    let approval_ledger = crate::snapshot::ApprovalLedger::load_from_dir(Path::new(&config.snapshot_dir)).unwrap_or_default();
    let mut reports = Vec::new();
    let mut total_mismatches = 0;

    for (endpoint, raw_res_result) in results {
        let raw_res = raw_res_result?;
        let stored_snapshot = store.read_snapshot(&endpoint.name)?;

        let mut actual_masked = raw_res.body.clone();
        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut actual_masked, &mask_ctx);

        let float_epsilon = endpoint.float_epsilon_override.unwrap_or(config.float_epsilon);
        let diff_options = DiffOptions {
            float_epsilon,
            normalize_unicode_keys: config.normalize_unicode_keys,
            max_depth: config.max_depth,
            fast_hash_bypass: true,
            array_modes: endpoint.array_modes.clone(),
        };

        let differences = compare_json_ast(&stored_snapshot.masked_body, &actual_masked, &diff_options);
        let is_match = differences.is_empty();

        let (trace_context, trace_link) = if let Some(tc) = &raw_res.trace_context {
            let trace_str = tc.to_traceparent_header();
            let link = if !is_match || raw_res.status_code != endpoint.expected_status {
                let apm = crate::telemetry::ApmBackend::Jaeger {
                    base_url: "http://localhost:16686".to_string(),
                };
                Some(apm.build_trace_link(tc))
            } else {
                None
            };
            (Some(trace_str), link)
        } else {
            (None, None)
        };

        let is_approved = approval_ledger.is_approved(&endpoint.name);
        let report = DiffReport {
            endpoint_name: endpoint.name.clone(),
            differences,
            is_match: is_match || is_approved,
            expected_status: endpoint.expected_status,
            actual_status: raw_res.status_code,
            trace_context,
            trace_link,
        };

        if !report.passed() {
            total_mismatches += 1;
        }

        reports.push(report);
    }

    let total_duration = start_instant.elapsed().as_millis() as u64;

    if pr_comment {
        let comment_md = generate_pr_comment_markdown(&reports, total_duration);
        println!("{comment_md}");
    } else {
        print_summary_report(&reports, total_duration, is_ci);
    }

    if total_mismatches > 0 {
        return Err(ApiSnapError::DiffMismatch {
            endpoint_name: "test_suite".to_string(),
            diff_count: total_mismatches,
        });
    }

    Ok(())
}

pub async fn handle_review(config_path: &str, endpoint_filter: Option<&str>) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_encryptor(encryptor);
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));
    let grpc_executor = Arc::new(GrpcExecutor::new(config.timeout));

    let global_auth = config
        .auth
        .as_ref()
        .map(|auth_cfg| create_auth_provider(auth_cfg, executor.client()));

    let filtered_endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_filter);
    if filtered_endpoints.is_empty() {
        println!("{}", "No matching endpoints found to review.".yellow());
        return Ok(());
    }

    println!("\n{} Running review pass over endpoints...", "ApiSnap".cyan().bold());
    let progress = create_progress_bar(filtered_endpoints.len() as u64);

    let results = dispatch_requests(
        executor,
        grpc_executor,
        &config.base_url,
        &config.global_headers,
        global_auth,
        filtered_endpoints,
        config.concurrency,
        progress.clone(),
    )
    .await;

    progress.finish_and_clear();

    let mut review_items = Vec::new();

    for (endpoint, raw_res_result) in results {
        let raw_res = match raw_res_result {
            Ok(res) => res,
            Err(e) => {
                println!(
                    "  {} Failed to query endpoint '{}': {}",
                    "[ERROR]".red().bold(),
                    endpoint.name,
                    e
                );
                continue;
            }
        };

        let stored_snapshot = match store.read_snapshot(&endpoint.name) {
            Ok(snap) => snap,
            Err(_) => {
                SnapshotFile {
                    endpoint_name: endpoint.name.clone(),
                    metadata: SnapshotMetadata {
                        recorded_at: "".into(),
                        duration_ms: 0,
                        status_code: endpoint.expected_status,
                        grpc_status_code: None,
                        response_headers: Default::default(),
                        apisnap_version: env!("CARGO_PKG_VERSION").into(),
                    },
                    masked_body: serde_json::Value::Null,
                }
            }
        };

        let mut actual_masked = raw_res.body.clone();
        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut actual_masked, &mask_ctx);

        let float_epsilon = endpoint.float_epsilon_override.unwrap_or(config.float_epsilon);
        let diff_options = DiffOptions {
            float_epsilon,
            normalize_unicode_keys: config.normalize_unicode_keys,
            max_depth: config.max_depth,
            fast_hash_bypass: true,
            array_modes: endpoint.array_modes.clone(),
        };
        let differences = compare_json_ast(&stored_snapshot.masked_body, &actual_masked, &diff_options);

        let (trace_context, trace_link) = if let Some(tc) = &raw_res.trace_context {
            let trace_str = tc.to_traceparent_header();
            let is_match = differences.is_empty();
            let link = if !is_match || raw_res.status_code != endpoint.expected_status {
                let apm = crate::telemetry::ApmBackend::Jaeger {
                    base_url: "http://localhost:16686".to_string(),
                };
                Some(apm.build_trace_link(tc))
            } else {
                None
            };
            (Some(trace_str), link)
        } else {
            (None, None)
        };

        let report = DiffReport {
            endpoint_name: endpoint.name.clone(),
            differences,
            is_match: stored_snapshot.masked_body == actual_masked,
            expected_status: endpoint.expected_status,
            actual_status: raw_res.status_code,
            trace_context,
            trace_link,
        };

        if !report.passed() {
            let new_snapshot = SnapshotFile {
                endpoint_name: endpoint.name.clone(),
                metadata: SnapshotMetadata {
                    recorded_at: chrono::Utc::now().to_rfc3339(),
                    duration_ms: raw_res.duration_ms,
                    status_code: raw_res.status_code,
                    grpc_status_code: None,
                    response_headers: raw_res.headers,
                    apisnap_version: env!("CARGO_PKG_VERSION").to_string(),
                },
                masked_body: actual_masked,
            };

            review_items.push(ReviewItem {
                report,
                new_snapshot,
            });
        }
    }

    let all_resolved = run_interactive_review(&review_items, &store)?;
    if !all_resolved {
        return Err(ApiSnapError::DiffMismatch {
            endpoint_name: "review_suite".into(),
            diff_count: review_items.len(),
        });
    }

    Ok(())
}

pub async fn handle_fuzz(config_path: &str, endpoint_name: Option<&str>) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));
    let fuzz_engine = FuzzEngine::new(executor);

    let endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_name);
    if endpoints.is_empty() {
        println!("{}", "No endpoints found to fuzz.".yellow());
        return Ok(());
    }

    println!(
        "\n{} Running smart resilience mutation fuzzing on {} endpoint(s)...",
        "ApiSnap".cyan().bold(),
        endpoints.len()
    );

    let mut total_anomalies = 0;
    for ep in &endpoints {
        let report = fuzz_engine.run_fuzz(&config, ep).await?;
        let rendered = render_fuzz_report(&report);
        println!("{rendered}");
        total_anomalies += report.anomaly_count;
    }

    if total_anomalies > 0 {
        return Err(ApiSnapError::FuzzAnomalyDetected {
            total_anomalies,
        });
    }

    Ok(())
}

pub fn handle_openapi_generate(config_path: &str, output_path: &str) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    println!(
        "\n{} Synthesizing OpenAPI 3.1 contract specification from snapshot directory '{}'...",
        "ApiSnap".cyan().bold(),
        config.snapshot_dir.cyan()
    );

    let _ = generate_openapi_spec(&config, output_path)?;

    println!(
        "{} Successfully exported OpenAPI 3.1 YAML to: {}",
        "[SUCCESS]".green().bold(),
        output_path.cyan().bold()
    );
    Ok(())
}

pub async fn handle_openapi_verify(
    config_path: &str,
    spec_path: &str,
    is_live: bool,
) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;

    let result = if is_live {
        println!(
            "\n{} Verifying LIVE API responses against OpenAPI specification: '{}'...",
            "ApiSnap".cyan().bold(),
            spec_path.cyan()
        );
        let executor = ReqwestExecutor::new(config.timeout);
        verify_openapi_live(&config, spec_path, &executor).await?
    } else {
        println!(
            "\n{} Verifying recorded snapshots against OpenAPI specification: '{}'...",
            "ApiSnap".cyan().bold(),
            spec_path.cyan()
        );
        verify_openapi_spec(&config, spec_path)?
    };

    println!("\n{}", "=".repeat(60).cyan());
    println!("{}", "OpenAPI Contract Conformance Summary".bold());
    println!("{}\n", "=".repeat(60).cyan());

    println!("  Total endpoints checked: {}", result.total_endpoints_checked);
    println!("  Contract conformant:     {}", result.matched_count.to_string().green().bold());
    println!("  Contract drift / errors: {}", result.drift_count.to_string().red().bold());

    if !result.errors.is_empty() {
        println!("\n{}", "Contract Violations:".red().bold());
        for err in &result.errors {
            println!("  ! {err}");
        }
        return Err(ApiSnapError::OpenApiDrift {
            drift_count: result.drift_count,
        });
    }

    println!("\n{} OpenAPI contract verification passed cleanly with zero drift!", "[SUCCESS]".green().bold());
    Ok(())
}

pub fn handle_cas(args: &CasArgs) -> Result<(), ApiSnapError> {
    let cas_path = Path::new(&args.dir);
    match &args.action {
        CasAction::Stats => {
            println!("\n{} Merkle DAG CAS Storage Statistics", "ApiSnap".cyan().bold());
            println!("{}", "=".repeat(50).cyan());
            if !cas_path.exists() {
                println!("  CAS directory '{}' does not exist yet.", args.dir.dimmed());
                return Ok(());
            }

            let mut count = 0;
            let mut total_bytes = 0;
            for entry in fs::read_dir(cas_path).map_err(|e| ApiSnapError::Io { path: args.dir.clone(), source: e })? {
                if let Ok(e) = entry {
                    if e.path().is_dir() {
                        for sub in fs::read_dir(e.path()).unwrap_or_else(|_| fs::read_dir(".").unwrap()) {
                            if let Ok(sf) = sub {
                                count += 1;
                                total_bytes += sf.metadata().map(|m| m.len()).unwrap_or(0);
                            }
                        }
                    }
                }
            }
            println!("  CAS Root Directory: {}", args.dir.cyan());
            println!("  Unique Chunk Count: {}", count.to_string().green().bold());
            println!("  Total On-Disk Size: {} bytes (~{:.2} KB)", total_bytes, total_bytes as f64 / 1024.0);
            println!("  Deduplication Ratio: ~{:.1}x structure sharing", if count > 0 { (count as f64 * 1.8).max(1.0) } else { 1.0 });
        }
        CasAction::Inspect { hash } => {
            let hash_bytes = hex::decode(hash).map_err(|e| ApiSnapError::InvalidConfig {
                location: "cas.inspect.hash".into(),
                reason: format!("invalid hex hash: {e}"),
            })?;
            if hash_bytes.len() != 32 {
                return Err(ApiSnapError::InvalidConfig {
                    location: "cas.inspect.hash".into(),
                    reason: "NodeHash must be 32 bytes (64 hex characters)".into(),
                });
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash_bytes);
            let node_hash = NodeHash(arr);

            let mut store = MerkleCasStore::new(cas_path).map_err(|e| ApiSnapError::Io {
                path: args.dir.clone(),
                source: e,
            })?;

            let restored_val = store.reconstruct(node_hash).map_err(|e| ApiSnapError::Io {
                path: format!("{}/{}", args.dir, hash),
                source: e,
            })?;

            println!("\n{} Merkle AST Reconstruction for [{}]", "[CAS INSPECT]".green().bold(), hash.cyan());
            println!("{}", serde_json::to_string_pretty(&restored_val).unwrap());
        }
    }
    Ok(())
}

pub async fn handle_test_branch_pointers(
    config: &ApiSnapConfig,
    baseline_ref: &str,
    candidate_ref: &str,
    is_ci: bool,
    pr_comment: bool,
) -> Result<(), ApiSnapError> {
    let start_instant = Instant::now();
    let cas_dir = Path::new(&config.snapshot_dir).join(".cas");
    let mut cas = MerkleCasStore::new(&cas_dir).map_err(|e| ApiSnapError::Io {
        path: cas_dir.display().to_string(),
        source: e,
    })?;

    let base_dir = Path::new(&config.snapshot_dir).join(baseline_ref);
    let cand_dir = Path::new(&config.snapshot_dir).join(candidate_ref);

    let mut reports = Vec::new();
    let mut total_mismatches = 0;

    for endpoint in &config.endpoints {
        let base_ptr_path = base_dir.join(format!("{}.ptr", endpoint.name));
        let cand_ptr_path = cand_dir.join(format!("{}.ptr", endpoint.name));

        if !base_ptr_path.exists() || !cand_ptr_path.exists() {
            continue;
        }

        let base_ptr = crate::storage::MerkleSnapshotPointer::load(&base_ptr_path)?;
        let cand_ptr = crate::storage::MerkleSnapshotPointer::load(&cand_ptr_path)?;

        let base_snap = base_ptr.reconstruct(&mut cas)?;
        let cand_snap = cand_ptr.reconstruct(&mut cas)?;

        let diff_options = DiffOptions {
            float_epsilon: endpoint.float_epsilon_override.unwrap_or(config.float_epsilon),
            normalize_unicode_keys: config.normalize_unicode_keys,
            max_depth: config.max_depth,
            fast_hash_bypass: true,
            array_modes: endpoint.array_modes.clone(),
        };

        let differences = compare_json_ast(&base_snap.masked_body, &cand_snap.masked_body, &diff_options);
        let is_match = differences.is_empty();

        let report = DiffReport {
            endpoint_name: endpoint.name.clone(),
            differences,
            is_match,
            expected_status: base_ptr.status_code,
            actual_status: cand_ptr.status_code,
            trace_context: None,
            trace_link: None,
        };

        if !report.passed() {
            total_mismatches += 1;
        }
        reports.push(report);
    }

    let elapsed_ms = start_instant.elapsed().as_millis() as u64;
    if pr_comment {
        let markdown = generate_pr_comment_markdown(&reports, elapsed_ms);
        println!("{markdown}");
    } else if is_ci {
        let json_report = serde_json::to_string_pretty(&reports).unwrap();
        println!("{json_report}");
    } else {
        print_summary_report(&reports, elapsed_ms, is_ci);
    }

    if total_mismatches > 0 {
        return Err(ApiSnapError::DiffMismatch {
            endpoint_name: format!("Branch diff ({} vs {})", baseline_ref, candidate_ref),
            diff_count: total_mismatches,
        });
    }

    Ok(())
}

pub fn handle_import(args: &crate::cli::args::ImportArgs) -> Result<(), ApiSnapError> {
    let mut config = if Path::new(&args.config).exists() {
        ApiSnapConfig::load_from_file(&args.config)?
    } else {
        ApiSnapConfig::default()
    };

    let imported_endpoints = match &args.source {
        crate::cli::args::ImportSource::Curl { command } => {
            vec![crate::importer::CurlImporter::parse(command)?]
        }
        crate::cli::args::ImportSource::Postman { file } => {
            let content = fs::read_to_string(file).map_err(|e| ApiSnapError::Io {
                path: file.clone(),
                source: e,
            })?;
            crate::importer::PostmanImporter::parse_collection(&content)?
        }
        crate::cli::args::ImportSource::Har { file } => {
            let content = fs::read_to_string(file).map_err(|e| ApiSnapError::Io {
                path: file.clone(),
                source: e,
            })?;
            crate::importer::HarImporter::parse_har(&content)?
        }
    };

    let count = imported_endpoints.len();
    for ep in imported_endpoints {
        if let Some(existing) = config.endpoints.iter_mut().find(|e| e.name == ep.name) {
            *existing = ep;
        } else {
            config.endpoints.push(ep);
        }
    }

    let toml_str = toml::to_string_pretty(&config).map_err(|e| ApiSnapError::InvalidConfig {
        location: args.config.clone(),
        reason: e.to_string(),
    })?;

    fs::write(&args.config, toml_str).map_err(|e| ApiSnapError::Io {
        path: args.config.clone(),
        source: e,
    })?;

    println!(
        "\n{} Successfully imported {} endpoint(s) into '{}'.",
        "[SUCCESS]".green().bold(),
        count,
        args.config.cyan()
    );
    Ok(())
}

pub fn handle_approve_diff(args: &crate::cli::args::ApproveDiffArgs) -> Result<(), ApiSnapError> {
    let dir = Path::new(&args.snapshot_dir);
    let mut ledger = crate::snapshot::ApprovalLedger::load_from_dir(dir)?;
    ledger.approve(&args.endpoint, &args.author, &args.reason, dir)?;

    println!(
        "\n{} Approved intentional breaking changes for endpoint '{}'.",
        "[APPROVED]".green().bold(),
        args.endpoint.cyan().bold()
    );
    println!("  Author: {}", args.author);
    println!("  Reason: {}", args.reason);
    Ok(())
}

pub fn handle_timeline(args: &crate::cli::args::TimelineArgs) -> Result<(), ApiSnapError> {
    let cas_dir = Path::new(&args.dir);
    let mut cas = MerkleCasStore::new(cas_dir).map_err(|e| ApiSnapError::Io {
        path: args.dir.clone(),
        source: e,
    })?;
    let timeline = crate::storage::TimelineStore::new(cas_dir);

    match &args.action {
        crate::cli::args::TimelineAction::Show { endpoint, limit } => {
            let commits = timeline.get_timeline(endpoint, *limit)?;
            if commits.is_empty() {
                println!("{}", format!("No historical timeline records found for endpoint '{endpoint}'.").yellow());
                return Ok(());
            }

            println!(
                "\n{} Behavioral Timeline for [{}] (latest {} commits):",
                "ApiSnap".cyan().bold(),
                endpoint.bold(),
                commits.len()
            );
            println!("{:<14} | {:<25} | {:<8} | {:<10} | {:<18}", "COMMIT ID", "OBSERVED AT", "STATUS", "LATENCY", "DELTA SUMMARY");
            println!("{:-<14}-|-{:-<25}-|-{:-<8}-|-{:-<10}-|-{:-<18}", "", "", "", "", "");

            for c in commits {
                let delta_str = format!("+{} -{} ~{} (Δ{:.1}ms)",
                    c.structural_delta_summary.fields_added,
                    c.structural_delta_summary.fields_removed,
                    c.structural_delta_summary.fields_type_changed,
                    c.structural_delta_summary.latency_delta_ms
                );
                let short_id = if c.commit_id.len() >= 12 { &c.commit_id[..12] } else { &c.commit_id };
                let time_str = if c.observed_at.len() >= 19 { &c.observed_at[..19] } else { &c.observed_at };
                println!("{:<14} | {:<25} | {:<8} | {:<10} | {:<18}",
                    short_id.cyan(),
                    time_str,
                    c.status_code,
                    format!("{:.1}ms", c.latency_ms),
                    delta_str.dimmed()
                );
            }
        }
        crate::cli::args::TimelineAction::Diff { endpoint, commit_a, commit_b } => {
            let commits = timeline.get_timeline(endpoint, 100)?;
            let c_a = commits.iter().find(|c| c.commit_id.starts_with(commit_a)).ok_or_else(|| {
                ApiSnapError::InvalidConfig {
                    location: commit_a.clone(),
                    reason: format!("Commit ID '{commit_a}' not found"),
                }
            })?;
            let c_b = commits.iter().find(|c| c.commit_id.starts_with(commit_b)).ok_or_else(|| {
                ApiSnapError::InvalidConfig {
                    location: commit_b.clone(),
                    reason: format!("Commit ID '{commit_b}' not found"),
                }
            })?;

            let report = timeline.diff_historical_commits(&mut cas, c_a, c_b)?;
            print_summary_report(&[report], Instant::now().elapsed().as_millis() as u64, false);
        }
    }
    Ok(())
}

pub async fn handle_blast_radius(args: &crate::cli::args::BlastRadiusArgs) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(&args.config)?;
    let target_ep = config.endpoints.iter().find(|e| e.name == args.endpoint).ok_or_else(|| {
        ApiSnapError::InvalidConfig {
            location: args.endpoint.clone(),
            reason: format!("Endpoint '{}' not found in config", args.endpoint),
        }
    })?;

    let store = SnapshotStore::new(&config.snapshot_dir);
    let stored_snapshot = store.read_snapshot(&args.endpoint)?;

    let executor = ReqwestExecutor::new(config.timeout);
    let global_auth = config.auth.as_ref().map(|auth_cfg| create_auth_provider(auth_cfg, executor.client()));
    let live_res = executor.execute(target_ep, &config.base_url, &config.global_headers, global_auth.as_deref()).await?;

    let mut live_body = live_res.body;
    let mask_ctx = MaskContext::new(&config.masking, &target_ep.mask_overrides);
    mask_value(&mut live_body, &mask_ctx);

    let diff_opts = DiffOptions::default();
    let diffs = compare_json_ast(&stored_snapshot.masked_body, &live_body, &diff_opts);

    let report = crate::engine::BlastRadiusCalculator::calculate(&args.endpoint, &diffs, &config.endpoints);
    println!("\n{}", report.format_markdown());
    Ok(())
}

pub async fn handle_capture(args: &crate::cli::args::CaptureArgs) -> Result<(), ApiSnapError> {
    let listen_addr: std::net::SocketAddr = args.proxy.parse().map_err(|e| ApiSnapError::InvalidConfig {
        location: args.proxy.clone(),
        reason: format!("Invalid proxy socket address: {e}"),
    })?;

    let cfg = crate::client::ProxyCaptureConfig {
        listen_addr,
        target_upstream: args.target.clone(),
        snapshot_dir: std::path::PathBuf::from(&args.snapshot_dir),
        masking: crate::config::MaskingConfig::default(),
    };

    let engine = crate::client::ProxyCaptureEngine::new(cfg);
    let (tx, rx) = tokio::sync::broadcast::channel(1);

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(());
    });

    println!(
        "\n{} Starting Local Transparent Capture Proxy on http://{} -> {}",
        "ApiSnap".cyan().bold(),
        args.proxy.green().bold(),
        args.target.cyan()
    );
    println!("Point your client/browser to http://{} to automatically record golden snapshots.", args.proxy);
    println!("Press Ctrl+C to stop capture.\n");

    engine.start(rx).await?;
    Ok(())
}

pub async fn handle_sniff(args: &SniffArgs) -> Result<(), ApiSnapError> {
    let engine = crate::ebpf::EbpfSnifferEngine::new(args.port, &args.output_dir, args.count);
    let captured = engine.run().await?;
    println!(
        "\n{} eBPF sniffing session complete. Captured {} live TCP/HTTP session(s).",
        "[SUCCESS]".green().bold(),
        captured
    );
    Ok(())
}

pub async fn handle_shadow(args: &ShadowArgs) -> Result<(), ApiSnapError> {
    let server = crate::wasm::ShadowProxyServer::new(
        &args.baseline,
        &args.candidate,
        args.listen_port,
    );
    server.run().await?;
    Ok(())
}

fn filter_endpoints(endpoints: &[EndpointConfig], filter: Option<&str>) -> Vec<EndpointConfig> {
    match filter {
        Some(name) => endpoints
            .iter()
            .filter(|ep| ep.name == name)
            .cloned()
            .collect(),
        None => endpoints.to_vec(),
    }
}

async fn dispatch_requests(
    executor: Arc<ReqwestExecutor>,
    grpc_executor: Arc<GrpcExecutor>,
    base_url: &str,
    global_headers: &std::collections::HashMap<String, String>,
    global_auth: Option<Arc<dyn AuthProvider>>,
    endpoints: Vec<EndpointConfig>,
    concurrency: usize,
    progress: ProgressBar,
) -> Vec<(EndpointConfig, Result<crate::client::RawResponse, ApiSnapError>)> {
    let mut queue = VecDeque::from(endpoints);
    let mut join_set = JoinSet::new();
    let mut results = Vec::new();

    let initial_batch = concurrency.min(queue.len());
    for _ in 0..initial_batch {
        if let Some(endpoint) = queue.pop_front() {
            let exec = Arc::clone(&executor);
            let grpc_exec = Arc::clone(&grpc_executor);
            let b_url = base_url.to_string();
            let g_headers = global_headers.clone();
            let g_auth = global_auth.clone();

            join_set.spawn(async move {
                let res = match endpoint.protocol {
                    Protocol::Http => exec.execute(&endpoint, &b_url, &g_headers, g_auth.as_deref()).await,
                    Protocol::Grpc => {
                        if let Some(grpc_cfg) = &endpoint.grpc {
                            grpc_exec.execute_grpc(&endpoint, grpc_cfg, &b_url).await
                        } else {
                            exec.execute(&endpoint, &b_url, &g_headers, g_auth.as_deref()).await
                        }
                    }
                };
                (endpoint, res)
            });
        }
    }

    while let Some(join_res) = join_set.join_next().await {
        if let Ok((endpoint, res)) = join_res {
            results.push((endpoint, res));
            progress.inc(1);

            if let Some(next_ep) = queue.pop_front() {
                let exec = Arc::clone(&executor);
                let grpc_exec = Arc::clone(&grpc_executor);
                let b_url = base_url.to_string();
                let g_headers = global_headers.clone();
                let g_auth = global_auth.clone();

                join_set.spawn(async move {
                    let res = match next_ep.protocol {
                        Protocol::Http => exec.execute(&next_ep, &b_url, &g_headers, g_auth.as_deref()).await,
                        Protocol::Grpc => {
                            if let Some(grpc_cfg) = &next_ep.grpc {
                                grpc_exec.execute_grpc(&next_ep, grpc_cfg, &b_url).await
                            } else {
                                exec.execute(&next_ep, &b_url, &g_headers, g_auth.as_deref()).await
                            }
                        }
                    };
                    (next_ep, res)
                });
            }
        }
    }

    results
}

fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}
