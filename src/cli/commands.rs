use crate::client::{RawResponse, RequestExecutor, ReqwestExecutor};
use crate::config::{ApiSnapConfig, EndpointConfig};
use crate::engine::{compare_json_ast, mask_value, DiffOptions, DiffReport, MaskContext};
use crate::error::ApiSnapError;
use crate::snapshot::{SnapshotFile, SnapshotMetadata, SnapshotStore};
use crate::ui::{print_summary_report, run_interactive_review, ReviewItem};
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
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan);
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));

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
        &config.base_url,
        &config.global_headers,
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
) -> Result<(), ApiSnapError> {
    let start_instant = Instant::now();
    let config = ApiSnapConfig::load_from_file(config_path)?;
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan);
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));

    let filtered_endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_filter);
    if filtered_endpoints.is_empty() {
        println!("{}", "No matching endpoints found to test.".yellow());
        return Ok(());
    }

    let concurrency = concurrency_override.unwrap_or(config.concurrency);
    let progress = if is_ci {
        ProgressBar::hidden()
    } else {
        create_progress_bar(filtered_endpoints.len() as u64)
    };

    let results = dispatch_requests(
        executor,
        &config.base_url,
        &config.global_headers,
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

        // 2. Diff against stored snapshot with v0.2.0 tolerance and normalization
        let float_epsilon = endpoint.float_epsilon_override.unwrap_or(config.float_epsilon);
        let diff_options = DiffOptions {
            float_epsilon,
            normalize_unicode_keys: config.normalize_unicode_keys,
            max_depth: config.max_depth,
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
    print_summary_report(&reports, total_duration, is_ci);

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
    let store = SnapshotStore::new(&config.snapshot_dir)
        .with_secret_scan(config.masking.pre_write_secret_scan);
    let executor = Arc::new(ReqwestExecutor::new(config.timeout));

    let filtered_endpoints: Vec<EndpointConfig> = filter_endpoints(&config.endpoints, endpoint_filter);
    if filtered_endpoints.is_empty() {
        println!("{}", "No matching endpoints found to review.".yellow());
        return Ok(());
    }

    println!("\n{} Running review pass over endpoints...", "ApiSnap".cyan().bold());
    let progress = create_progress_bar(filtered_endpoints.len() as u64);

    let results = dispatch_requests(
        executor,
        &config.base_url,
        &config.global_headers,
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
    base_url: &str,
    global_headers: &std::collections::HashMap<String, String>,
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
            let b_url = base_url.clone();
            let g_headers = global_headers.clone();

            join_set.spawn(async move {
                let res = exec.execute(&endpoint, &b_url, &g_headers).await;
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
                let b_url = base_url.clone();
                let g_headers = global_headers.clone();

                join_set.spawn(async move {
                    let res = exec.execute(&next_ep, &b_url, &g_headers).await;
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
