use crate::config::ApiSnapConfig;
use crate::error::ApiSnapError;
use crate::openapi::schema_infer::infer_schema_from_value;
use crate::snapshot::SnapshotStore;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

/// Generate an OpenAPI 3.1.0 specification YAML file from existing snapshots.
pub fn generate_openapi_spec(
    config: &ApiSnapConfig,
    output_path: &str,
) -> Result<String, ApiSnapError> {
    let store = SnapshotStore::new(&config.snapshot_dir);
    let mut paths_map = Map::new();

    for endpoint in &config.endpoints {
        if !store.exists(&endpoint.name) {
            continue;
        }

        let snapshot = store.read_snapshot(&endpoint.name)?;
        let method_key = endpoint.method.to_string().to_lowercase();
        let path_key = if endpoint.path.starts_with('/') {
            endpoint.path.clone()
        } else {
            format!("/{}", endpoint.path)
        };

        let response_schema = infer_schema_from_value(&snapshot.masked_body);
        let status_str = snapshot.metadata.status_code.to_string();

        let mut operation_map = Map::new();
        operation_map.insert("summary".into(), json!(endpoint.name));
        operation_map.insert("operationId".into(), json!(endpoint.name));

        // 1. Query Parameters
        if !endpoint.query_params.is_empty() {
            let mut params_vec = Vec::new();
            for (k, v) in &endpoint.query_params {
                params_vec.push(json!({
                    "name": k,
                    "in": "query",
                    "required": false,
                    "schema": {
                        "type": "string",
                        "example": v
                    }
                }));
            }
            operation_map.insert("parameters".into(), Value::Array(params_vec));
        }

        // 2. Request Body
        if let Some(body_val) = &endpoint.body {
            let body_schema = infer_schema_from_value(body_val);
            operation_map.insert(
                "requestBody".into(),
                json!({
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": body_schema
                        }
                    }
                }),
            );
        }

        // 3. Response Schema
        operation_map.insert(
            "responses".into(),
            json!({
                status_str: {
                    "description": format!("HTTP {} response recorded by ApiSnap", snapshot.metadata.status_code),
                    "content": {
                        "application/json": {
                            "schema": response_schema
                        }
                    }
                }
            }),
        );

        let path_item = paths_map
            .entry(path_key.clone())
            .or_insert_with(|| Value::Object(Map::new()));

        if let Value::Object(ref mut item_obj) = path_item {
            item_obj.insert(method_key, Value::Object(operation_map));
        }
    }

    let openapi_doc = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "ApiSnap Generated API Specification",
            "version": "1.0.0",
            "description": "Automatically synthesized from golden snapshot regression fixtures by ApiSnap."
        },
        "servers": [
            {
                "url": config.base_url,
                "description": "Target server"
            }
        ],
        "paths": paths_map
    });

    let yaml_str = serde_yaml::to_string(&openapi_doc).map_err(|e| {
        ApiSnapError::InvalidConfig {
            location: "openapi.generation".into(),
            reason: format!("failed to serialize openapi yaml: {e}"),
        }
    })?;

    if let Some(parent) = Path::new(output_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }

    fs::write(output_path, &yaml_str).map_err(|e| ApiSnapError::Io {
        path: output_path.to_string(),
        source: e,
    })?;

    Ok(yaml_str)
}
