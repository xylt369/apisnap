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

        let operation = json!({
            "summary": endpoint.name,
            "operationId": endpoint.name,
            "responses": {
                status_str: {
                    "description": format!("HTTP {} response recorded by ApiSnap", snapshot.metadata.status_code),
                    "content": {
                        "application/json": {
                            "schema": response_schema
                        }
                    }
                }
            }
        });

        let path_item = paths_map
            .entry(path_key.clone())
            .or_insert_with(|| Value::Object(Map::new()));

        if let Value::Object(ref mut item_obj) = path_item {
            item_obj.insert(method_key, operation);
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
