use crate::config::ApiSnapConfig;
use crate::engine::DiffReport;
use crate::error::ApiSnapError;
use crate::snapshot::SnapshotStore;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;
use std::path::Path;

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
    let spec_content = fs::read_to_string(spec_path).map_err(|e| ApiSnapError::Io {
        path: spec_path.to_string(),
        source: e,
    })?;

    let spec_val: Value = if spec_path.ends_with(".yaml") || spec_path.ends_with(".yml") {
        serde_yaml::from_str(&spec_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: spec_path.to_string(),
            reason: format!("invalid OpenAPI YAML syntax: {e}"),
        })?
    } else {
        serde_json::from_str(&spec_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: spec_path.to_string(),
            reason: format!("invalid OpenAPI JSON syntax: {e}"),
        })?
    };

    let store = SnapshotStore::new(&config.snapshot_dir);
    let mut total_checked = 0;
    let mut matched = 0;
    let mut drift = 0;
    let mut errors = Vec::new();

    let paths_obj = match spec_val.get("paths").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => {
            return Err(ApiSnapError::InvalidConfig {
                location: spec_path.to_string(),
                reason: "missing 'paths' object in OpenAPI specification".into(),
            });
        }
    };

    for endpoint in &config.endpoints {
        if !store.exists(&endpoint.name) {
            continue;
        }

        total_checked += 1;
        let snapshot = store.read_snapshot(&endpoint.name)?;
        let method_key = endpoint.method.to_string().to_lowercase();
        let path_key = if endpoint.path.starts_with('/') {
            endpoint.path.clone()
        } else {
            format!("/{}", endpoint.path)
        };

        let schema_val = paths_obj
            .get(&path_key)
            .and_then(|p| p.get(&method_key))
            .and_then(|op| op.get("responses"))
            .and_then(|r| r.get(snapshot.metadata.status_code.to_string().as_str()))
            .and_then(|status_resp| status_resp.get("content"))
            .and_then(|content| content.get("application/json"))
            .and_then(|app_json| app_json.get("schema"));

        if let Some(schema) = schema_val {
            match JSONSchema::compile(schema) {
                Ok(compiled_schema) => {
                    let validation = compiled_schema.validate(&snapshot.masked_body);
                    if let Err(err_iter) = validation {
                        drift += 1;
                        for err in err_iter {
                            errors.push(format!(
                                "Contract Drift in '{}' ({} {}): {} at '{}'",
                                endpoint.name,
                                endpoint.method,
                                endpoint.path,
                                err,
                                err.instance_path
                            ));
                        }
                    } else {
                        matched += 1;
                    }
                }
                Err(compile_err) => {
                    errors.push(format!(
                        "Schema compilation error in OpenAPI for '{}': {}",
                        endpoint.name, compile_err
                    ));
                    drift += 1;
                }
            }
        } else {
            errors.push(format!(
                "Undocumented endpoint in OpenAPI spec: {} {} (HTTP {})",
                endpoint.method, path_key, snapshot.metadata.status_code
            ));
            drift += 1;
        }
    }

    Ok(OpenApiVerifyResult {
        total_endpoints_checked: total_checked,
        matched_count: matched,
        drift_count: drift,
        errors,
    })
}
