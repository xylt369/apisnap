use crate::client::{RequestExecutor, ReqwestExecutor};
use crate::config::{CustomMaskRule, EndpointConfig};
use crate::engine::{mask_value, MaskContext};
use crate::error::ApiSnapError;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Result of statistical adaptive noise learning across N live probe runs.
#[derive(Debug, Clone)]
pub struct LearnedNoiseReport {
    pub endpoint_name: String,
    pub iterations: usize,
    pub candidate_rules: Vec<CustomMaskRule>,
    pub unstable_paths: Vec<String>,
}

/// Adaptive Noise Learning Engine that probes endpoints N times to automatically discover volatile fields.
pub struct AdaptiveBaselineLearner;

impl AdaptiveBaselineLearner {
    pub async fn learn_endpoint(
        endpoint: &EndpointConfig,
        base_url: &str,
        global_headers: &HashMap<String, String>,
        auth: Option<&dyn crate::client::AuthProvider>,
        iterations: usize,
        mask_ctx: &MaskContext,
    ) -> Result<LearnedNoiseReport, ApiSnapError> {
        let count = iterations.max(2);
        let executor = ReqwestExecutor::new(
            endpoint.timeout_override.unwrap_or(std::time::Duration::from_secs(30)),
        );
        let mut raw_responses: Vec<Value> = Vec::new();

        for _ in 0..count {
            let res = executor.execute(endpoint, base_url, global_headers, auth).await?;
            raw_responses.push(res.body);
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        if raw_responses.len() < 2 {
            return Ok(LearnedNoiseReport {
                endpoint_name: endpoint.name.clone(),
                iterations: raw_responses.len(),
                candidate_rules: Vec::new(),
                unstable_paths: Vec::new(),
            });
        }

        let mut masked_responses = raw_responses.clone();
        for resp in &mut masked_responses {
            mask_value(resp, mask_ctx);
        }

        let mut masked_path_values: HashMap<String, HashSet<String>> = HashMap::new();
        for res in &masked_responses {
            collect_path_values(res, "$", &mut masked_path_values);
        }

        let mut unstable_paths = Vec::new();
        let mut candidate_rules = Vec::new();

        for (path, val_set) in masked_path_values {
            if val_set.len() > 1 {
                // Still varies across runs after applying current masking rules!
                unstable_paths.push(path.clone());
                candidate_rules.push(CustomMaskRule {
                    json_path: path,
                    replacement: "<MASKED_LEARNED_TOKEN>".into(),
                    pattern: None,
                });
            }
        }

        // Sort for deterministic output
        unstable_paths.sort();
        candidate_rules.sort_by(|a, b| a.json_path.cmp(&b.json_path));

        Ok(LearnedNoiseReport {
            endpoint_name: endpoint.name.clone(),
            iterations: raw_responses.len(),
            candidate_rules,
            unstable_paths,
        })
    }
}

fn collect_path_values(
    val: &Value,
    current_path: &str,
    out: &mut HashMap<String, HashSet<String>>,
) {
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                let subpath = format!("{current_path}.{k}");
                collect_path_values(v, &subpath, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let subpath = format!("{current_path}[{i}]");
                collect_path_values(v, &subpath, out);
            }
        }
        _ => {
            let str_repr = val.to_string();
            out.entry(current_path.to_string())
                .or_default()
                .insert(str_repr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_path_variance() {
        let res1 = serde_json::json!({
            "status": "ok",
            "seq": 101,
            "user": "Alice"
        });
        let res2 = serde_json::json!({
            "status": "ok",
            "seq": 102,
            "user": "Alice"
        });

        let mut path_values = HashMap::new();
        collect_path_values(&res1, "$", &mut path_values);
        collect_path_values(&res2, "$", &mut path_values);

        assert_eq!(path_values.get("$.status").unwrap().len(), 1);
        assert_eq!(path_values.get("$.user").unwrap().len(), 1);
        assert_eq!(path_values.get("$.seq").unwrap().len(), 2); // Varied across runs
    }
}
