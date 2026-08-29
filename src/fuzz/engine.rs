use crate::client::{RequestExecutor, ReqwestExecutor};
use crate::config::{ApiSnapConfig, EndpointConfig};
use crate::error::ApiSnapError;
use crate::fuzz::mutator::{generate_mutations, MutationCase};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Instant;

/// Individual outcome of a single fuzz mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzResult {
    pub mutation_name: String,
    pub description: String,
    pub status_code: u16,
    pub duration_ms: u64,
    pub is_anomaly: bool,
    pub anomaly_reason: Option<String>,
}

/// Aggregated report for endpoint fuzzing execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzReport {
    pub endpoint_name: String,
    pub total_mutations: usize,
    pub resilient_count: usize,
    pub anomaly_count: usize,
    pub results: Vec<FuzzResult>,
}

pub struct FuzzEngine {
    executor: Arc<dyn RequestExecutor>,
}

impl FuzzEngine {
    pub fn new(executor: Arc<dyn RequestExecutor>) -> Self {
        Self { executor }
    }

    pub async fn run_fuzz(
        &self,
        config: &ApiSnapConfig,
        endpoint: &EndpointConfig,
    ) -> Result<FuzzReport, ApiSnapError> {
        let baseline_body = endpoint.body.clone().unwrap_or(Value::Object(Default::default()));
        let mutations = generate_mutations(&baseline_body);

        let mut results = Vec::new();
        let mut anomaly_count = 0;
        let mut resilient_count = 0;

        for case in mutations {
            let mut mutated_endpoint = endpoint.clone();
            mutated_endpoint.body = Some(case.mutated_body);

            let res_result = self
                .executor
                .execute(&mutated_endpoint, &config.base_url, &config.global_headers, None)
                .await;

            match res_result {
                Ok(raw_res) => {
                    let body_str = raw_res.body.to_string();
                    let has_stack_trace = body_str.contains("Traceback (most recent call last)")
                        || body_str.contains("panic: runtime error")
                        || body_str.contains("NullPointerException")
                        || body_str.contains("FATAL EXCEPTION");

                    let is_anomaly = raw_res.status_code >= 500 || has_stack_trace;
                    let anomaly_reason = if raw_res.status_code >= 500 {
                        Some(format!("Server crashed with HTTP {}", raw_res.status_code))
                    } else if has_stack_trace {
                        Some("Unhandled exception stack trace leaked in response body".into())
                    } else {
                        None
                    };

                    if is_anomaly {
                        anomaly_count += 1;
                    } else {
                        resilient_count += 1;
                    }

                    results.push(FuzzResult {
                        mutation_name: case.name,
                        description: case.description,
                        status_code: raw_res.status_code,
                        duration_ms: raw_res.duration_ms,
                        is_anomaly,
                        anomaly_reason,
                    });
                }
                Err(e) => {
                    anomaly_count += 1;
                    results.push(FuzzResult {
                        mutation_name: case.name,
                        description: case.description,
                        status_code: 0,
                        duration_ms: 0,
                        is_anomaly: true,
                        anomaly_reason: Some(format!("Network / Connection Failure: {e}")),
                    });
                }
            }
        }

        Ok(FuzzReport {
            endpoint_name: endpoint.name.clone(),
            total_mutations: results.len(),
            resilient_count,
            anomaly_count,
            results,
        })
    }
}

pub fn render_fuzz_report(report: &FuzzReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n{} {}\n",
        "ApiSnap Fuzz Resilience Report:".bold(),
        report.endpoint_name.cyan().bold()
    ));
    out.push_str("============================================================\n");

    for r in &report.results {
        if r.is_anomaly {
            out.push_str(&format!(
                "  {} {:<25} HTTP {:<3} ({}ms) - {}\n",
                "[ANOMALY]".red().bold(),
                r.mutation_name.red(),
                r.status_code.to_string().red(),
                r.duration_ms,
                r.anomaly_reason.as_deref().unwrap_or("")
            ));
        } else {
            out.push_str(&format!(
                "  {} {:<25} HTTP {:<3} ({}ms) - Handled gracefully\n",
                "[OK]".green().bold(),
                r.mutation_name.green(),
                r.status_code,
                r.duration_ms
            ));
        }
    }

    out.push_str("------------------------------------------------------------\n");
    out.push_str(&format!(
        "Summary: {} total mutations | {} resilient | {} anomalies\n\n",
        report.total_mutations.to_string().bold(),
        report.resilient_count.to_string().green().bold(),
        report.anomaly_count.to_string().red().bold()
    ));

    out
}
