use crate::client::RequestExecutor;
use crate::config::ApiSnapConfig;
use crate::engine::{mask_value, MaskContext};
use crate::error::ApiSnapError;
use crate::snapshot::SnapshotStore;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;

/// Result of OpenAPI contract drift verification.
#[derive(Debug, Clone)]
pub struct OpenApiVerifyResult {
    pub total_endpoints_checked: usize,
    pub matched_count: usize,
    pub drift_count: usize,
    pub errors: Vec<String>,
}

/// Verify snapshots against an existing OpenAPI 3.x specification file.
pub fn verify_openapi_spec(
    config: &ApiSnapConfig,
    spec_path: &str,
) -> Result<OpenApiVerifyResult, ApiSnapError> {
    let spec_val = load_openapi_spec(spec_path)?;
    let store = SnapshotStore::new(&config.snapshot_dir);
    let mut total_checked = 0;
    let mut matched = 0;
    let mut drift = 0;
    let mut errors = Vec::new();

    let paths_obj = extract_paths_object(&spec_val, spec_path)?;

    for endpoint in &config.endpoints {
        if !store.exists(&endpoint.name) {
            continue;
        }

        total_checked += 1;
        let snapshot = store.read_snapshot(&endpoint.name)?;
        let method_key = endpoint.method.to_string().to_lowercase();
        let path_key = normalize_path_key(&endpoint.path);

        validate_ast_against_openapi(
            &snapshot.masked_body,
            snapshot.metadata.status_code,
            &endpoint.name,
            &method_key,
            &path_key,
            paths_obj,
            &mut matched,
            &mut drift,
            &mut errors,
        );
    }

    Ok(OpenApiVerifyResult {
        total_endpoints_checked: total_checked,
        matched_count: matched,
        drift_count: drift,
        errors,
    })
}

/// Live endpoint verification against an OpenAPI specification.
pub async fn verify_openapi_live(
    config: &ApiSnapConfig,
    spec_path: &str,
    executor: &dyn RequestExecutor,
) -> Result<OpenApiVerifyResult, ApiSnapError> {
    let spec_val = load_openapi_spec(spec_path)?;
    let mut total_checked = 0;
    let mut matched = 0;
    let mut drift = 0;
    let mut errors = Vec::new();

    let paths_obj = extract_paths_object(&spec_val, spec_path)?;

    for endpoint in &config.endpoints {
        total_checked += 1;

        let raw_res = match executor
            .execute(endpoint, &config.base_url, &config.global_headers, None)
            .await
        {
            Ok(res) => res,
            Err(e) => {
                drift += 1;
                errors.push(format!(
                    "Live network failure querying '{}' ({} {}): {}",
                    endpoint.name, endpoint.method, endpoint.path, e
                ));
                continue;
            }
        };

        // 1. Mask dynamic noise in live response
        let mut masked_live = raw_res.body.clone();
        let mask_ctx = MaskContext::new(&config.masking, &endpoint.mask_overrides)
            .with_max_depth(config.max_depth);
        mask_value(&mut masked_live, &mask_ctx);

        let method_key = endpoint.method.to_string().to_lowercase();
        let path_key = normalize_path_key(&endpoint.path);

        // 2. Validate live masked payload against OpenAPI contract
        validate_ast_against_openapi(
            &masked_live,
            raw_res.status_code,
            &endpoint.name,
            &method_key,
            &path_key,
            paths_obj,
            &mut matched,
            &mut drift,
            &mut errors,
        );
    }

    Ok(OpenApiVerifyResult {
        total_endpoints_checked: total_checked,
        matched_count: matched,
        drift_count: drift,
        errors,
    })
}

fn load_openapi_spec(spec_path: &str) -> Result<Value, ApiSnapError> {
    let spec_content = fs::read_to_string(spec_path).map_err(|e| ApiSnapError::Io {
        path: spec_path.to_string(),
        source: e,
    })?;

    if spec_path.ends_with(".yaml") || spec_path.ends_with(".yml") {
        serde_yaml::from_str(&spec_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: spec_path.to_string(),
            reason: format!("invalid OpenAPI YAML syntax: {e}"),
        })
    } else {
        serde_json::from_str(&spec_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: spec_path.to_string(),
            reason: format!("invalid OpenAPI JSON syntax: {e}"),
        })
    }
}

fn extract_paths_object<'a>(spec_val: &'a Value, spec_path: &str) -> Result<&'a serde_json::Map<String, Value>, ApiSnapError> {
    match spec_val.get("paths").and_then(|p| p.as_object()) {
        Some(p) => Ok(p),
        None => Err(ApiSnapError::InvalidConfig {
            location: spec_path.to_string(),
            reason: "missing 'paths' object in OpenAPI specification".into(),
        }),
    }
}

fn normalize_path_key(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn validate_ast_against_openapi(
    body: &Value,
    status_code: u16,
    endpoint_name: &str,
    method: &str,
    path_key: &str,
    paths_obj: &serde_json::Map<String, Value>,
    matched: &mut usize,
    drift: &mut usize,
    errors: &mut Vec<String>,
) {
    let method_lower = method.to_lowercase();
    let schema_val = paths_obj
        .get(path_key)
        .and_then(|p| p.get(&method_lower))
        .and_then(|op| op.get("responses"))
        .and_then(|resps| {
            resps
                .get(&status_code.to_string())
                .or_else(|| resps.get("default"))
        })
        .and_then(|resp| resp.get("content"))
        .and_then(|c| {
            c.get("application/json")
                .or_else(|| c.get("application/grpc+json"))
                .or_else(|| c.get("*/*"))
        })
        .and_then(|media| media.get("schema"));

    let schema = match schema_val {
        Some(s) => s,
        None => {
            *drift += 1;
            errors.push(format!(
                "Endpoint '{}' ({} {}): Missing schema definition in OpenAPI spec for status {}",
                endpoint_name,
                method.to_uppercase(),
                path_key,
                status_code
            ));
            return;
        }
    };

    match JSONSchema::compile(schema) {
        Ok(compiled) => {
            let validation_res = compiled.validate(body);
            if let Err(schema_errors) = validation_res {
                *drift += 1;
                let error_msgs: Vec<String> = schema_errors
                    .map(|e| format!("  - at '{}': {}", e.instance_path, e))
                    .collect();
                errors.push(format!(
                    "Endpoint '{}' schema contract violation:\n{}",
                    endpoint_name,
                    error_msgs.join("\n")
                ));
            } else {
                *matched += 1;
            }
        }
        Err(e) => {
            *drift += 1;
            errors.push(format!(
                "Endpoint '{}' invalid OpenAPI JSONSchema definition: {}",
                endpoint_name, e
            ));
        }
    }
}
