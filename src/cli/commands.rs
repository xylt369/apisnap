use crate::client::auth::{create_auth_provider, AuthProvider};
use crate::client::{GrpcExecutor, RawResponse, RequestExecutor, ReqwestExecutor};
use crate::config::{ApiSnapConfig, EndpointConfig, Protocol};
use crate::crypto::SnapshotEncryptor;
use crate::engine::{compare_json_ast, mask_value, DiffOptions, DiffReport, MaskContext};
use crate::error::ApiSnapError;
use crate::fuzz::{render_fuzz_report, FuzzEngine};
use crate::openapi::{generate_openapi_spec, verify_openapi_spec};
use crate::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
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
) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan)
        .with_encryptor(encryptor);
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

    let concurrency = concurrency_override.unwrap_or(config.concurrency);
    println!(
        "\n{} Recording {} endpoint(s) with concurrency {}...",
        "ApiSnap".cyan().bold(),
        filtered_endpoints.len(),
        concurrency
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

    let mut recorded_count = 0;
    for (endpoint, raw_res_result) in results {
        let raw_res = raw_res_result?;
        let mut masked_body = raw_res.body.clone();

        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut masked_body, &mask_ctx);

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
        "\n{} Successfully recorded {} snapshot(s) in '{}'\n",
        "[DONE]".green().bold(),
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
) -> Result<(), ApiSnapError> {
    let start_instant = Instant::now();
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan)
        .with_encryptor(encryptor);
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
    let progress = if is_ci || pr_comment {
        ProgressBar::hidden()
    } else {
        create_progress_bar(filtered_endpoints.len() as u64)
    };

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

    let mut reports = Vec::new();
    let mut total_mismatches = 0;

    for (endpoint, raw_res_result) in results {
        let raw_res = raw_res_result?;
        let stored_snapshot = store.read_snapshot(&endpoint.name)?;

        // 1. Mask actual live response
        let mut actual_masked = raw_res.body.clone();
        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut actual_masked, &mask_ctx);

        // 2. Diff against stored snapshot
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

        let report = DiffReport {
            endpoint_name: endpoint.name.clone(),
            differences,
            is_match,
            expected_status: endpoint.expected_status,
            actual_status: raw_res.status_code,
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

pub async fn handle_review(
    config_path: &str,
    endpoint_filter: Option<&str>,
) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let encryptor = SnapshotEncryptor::from_env().transpose()?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan)
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

        let report = DiffReport {
            endpoint_name: endpoint.name.clone(),
            differences,
            is_match: stored_snapshot.masked_body == actual_masked,
            expected_status: endpoint.expected_status,
            actual_status: raw_res.status_code,
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
        print!("{rendered}");
        total_anomalies += report.anomaly_count;
    }

    if total_anomalies > 0 {
        return Err(ApiSnapError::DiffMismatch {
            endpoint_name: "fuzz_suite".into(),
            diff_count: total_anomalies,
        });
    }

    println!("{} All endpoints proved resilient under boundary mutations!\n", "[PASS]".green().bold());
    Ok(())
}

pub fn handle_openapi_generate(config_path: &str, output_path: &str) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    println!(
        "\n{} Synthesizing OpenAPI 3.1 schema from snapshots in '{}'...",
        "ApiSnap".cyan().bold(),
        config.snapshot_dir.cyan()
    );

    generate_openapi_spec(&config, output_path)?;

    println!(
        "{} Successfully generated OpenAPI 3.1 YAML at: {}\n",
        "[SUCCESS]".green().bold(),
        output_path.cyan().bold()
    );

    Ok(())
}

pub fn handle_openapi_verify(config_path: &str, spec_path: &str) -> Result<(), ApiSnapError> {
    let config = ApiSnapConfig::load_from_file(config_path)?;
    println!(
        "\n{} Verifying snapshots against OpenAPI specification: '{}'...",
        "ApiSnap".cyan().bold(),
        spec_path.cyan()
    );

    let result = verify_openapi_spec(&config, spec_path)?;

    println!(
        "Checked {} endpoint(s): {} matched, {} contract drift(s).",
        result.total_endpoints_checked,
        result.matched_count.to_string().green(),
        result.drift_count.to_string().red()
    );

    if result.drift_count > 0 {
        println!("\n{}", "Detected Schema Contract Drifts:".red().bold());
        for err in &result.errors {
            println!("  {} {}", "[DRIFT]".red().bold(), err);
        }
        return Err(ApiSnapError::DiffMismatch {
            endpoint_name: "openapi_verify".into(),
            diff_count: result.drift_count,
        });
    }

    println!("{} All snapshots fully match OpenAPI specification!\n", "[PASS]".green().bold());
    Ok(())
}

fn filter_endpoints(
    endpoints: &[EndpointConfig],
    filter: Option<&str>,
) -> Vec<EndpointConfig> {
    if let Some(target) = filter {
        endpoints
            .iter()
            .filter(|e| e.name.eq_ignore_ascii_case(target))
            .cloned()
            .collect()
    } else {
        endpoints.to_vec()
    }
}

async fn dispatch_requests(
    executor: Arc<dyn RequestExecutor>,
    grpc_executor: Arc<GrpcExecutor>,
    base_url: &str,
    global_headers: &std::collections::HashMap<String, String>,
    global_auth: Option<Arc<dyn AuthProvider>>,
    endpoints: Vec<EndpointConfig>,
    concurrency: usize,
    progress: ProgressBar,
) -> Vec<(EndpointConfig, Result<RawResponse, ApiSnapError>)> {
    let mut queue = VecDeque::from(endpoints);
    let mut join_set = JoinSet::new();
    let mut results = Vec::new();

    let base_url = base_url.to_string();
    let global_headers = global_headers.clone();

    while join_set.len() < concurrency && !queue.is_empty() {
        if let Some(endpoint) = queue.pop_front() {
            let exec = Arc::clone(&executor);
            let grpc_exec = Arc::clone(&grpc_executor);
            let b_url = base_url.clone();
            let g_headers = global_headers.clone();
            let g_auth = global_auth.clone();

            join_set.spawn(async move {
                let res = match endpoint.protocol {
                    Protocol::Http => exec.execute(&endpoint, &b_url, &g_headers, g_auth).await,
                    Protocol::Grpc => {
                        if let Some(grpc_cfg) = &endpoint.grpc {
                            grpc_exec.execute_grpc(&endpoint, grpc_cfg, &b_url).await
                        } else {
                            exec.execute(&endpoint, &b_url, &g_headers, g_auth).await
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
                let b_url = base_url.clone();
                let g_headers = global_headers.clone();
                let g_auth = global_auth.clone();

                join_set.spawn(async move {
                    let res = match next_ep.protocol {
                        Protocol::Http => exec.execute(&next_ep, &b_url, &g_headers, g_auth).await,
                        Protocol::Grpc => {
                            if let Some(grpc_cfg) = &next_ep.grpc {
                                grpc_exec.execute_grpc(&next_ep, grpc_cfg, &b_url).await
                            } else {
                                exec.execute(&next_ep, &b_url, &g_headers, g_auth).await
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
