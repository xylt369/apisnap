use crate::client::{RequestExecutor, ReqwestExecutor};
use crate::config::ApiSnapConfig;
use crate::engine::{mask_value, MaskContext};
use crate::error::ApiSnapError;
use crate::snapshot::SnapshotStore;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;
use std::sync::Arc;

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
            &endpoint.method.to_string(),
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

/// Live API Response Verification: Queries target endpoints in real time and validates against OpenAPI spec.
pub async fn verify_openapi_live(
    config: &ApiSnapConfig,
    spec_path: &str,
) -> Result<OpenApiVerifyResult, ApiSnapError> {
    let spec_val = load_openapi_spec(spec_path)?;
    let executor = ReqwestExecutor::new(config.timeout);
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
            &endpoint.method.to_string(),
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
        .and_then(|r| r.get(status_code.to_string().as_str()))
        .and_then(|status_resp| status_resp.get("content"))
        .and_then(|content| content.get("application/json"))
        .and_then(|app_json| app_json.get("schema"));

    if let Some(schema) = schema_val {
        match JSONSchema::compile(schema) {
            Ok(compiled_schema) => {
                let validation = compiled_schema.validate(body);
                if let Err(err_iter) = validation {
                    *drift += 1;
                    for err in err_iter {
                        errors.push(format!(
                            "Contract Drift in '{}' ({} {}): {} at '{}'",
                            endpoint_name, method, path_key, err, err.instance_path
                        ));
                    }
                } else {
                    *matched += 1;
                }
            }
            Err(compile_err) => {
                errors.push(format!(
                    "Schema compilation error in OpenAPI for '{}': {}",
                    endpoint_name, compile_err
                ));
                *drift += 1;
            }
        }
    } else {
        errors.push(format!(
            "Undocumented endpoint response in OpenAPI spec: {} {} (HTTP {})",
            method, path_key, status_code
        ));
        *drift += 1;
    }
}
