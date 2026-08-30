use crate::config::{EndpointConfig, Method, Protocol};
use crate::error::ApiSnapError;
use serde_json::Value;
use std::collections::HashMap;

/// Importer for Postman Collection v2.0 & v2.1 JSON specifications.
pub struct PostmanImporter;

impl PostmanImporter {
    pub fn parse_collection(json_content: &str) -> Result<Vec<EndpointConfig>, ApiSnapError> {
        let root: Value = serde_json::from_str(json_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: "postman_import".into(),
            reason: format!("Invalid Postman JSON: {e}"),
        })?;

        let items = root.get("item").and_then(|i| i.as_array()).ok_or_else(|| {
            ApiSnapError::InvalidConfig {
                location: "postman_import".into(),
                reason: "Missing 'item' array in Postman collection".into(),
            }
        })?;

        let mut endpoints = Vec::new();
        extract_items_recursive(items, &mut endpoints)?;

        if endpoints.is_empty() {
            return Err(ApiSnapError::InvalidConfig {
                location: "postman_import".into(),
                reason: "No valid HTTP requests found in Postman collection".into(),
            });
        }

        Ok(endpoints)
    }
}

fn extract_items_recursive(
    items: &[Value],
    out: &mut Vec<EndpointConfig>,
) -> Result<(), ApiSnapError> {
    for item in items {
        if let Some(sub_items) = item.get("item").and_then(|i| i.as_array()) {
            extract_items_recursive(sub_items, out)?;
        } else if let Some(req) = item.get("request") {
            let name = item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unnamed_endpoint");

            if let Some(ep) = parse_postman_request(name, req) {
                out.push(ep);
            }
        }
    }
    Ok(())
}

fn parse_postman_request(name: &str, req: &Value) -> Option<EndpointConfig> {
    let method_str = req.get("method").and_then(|m| m.as_str()).unwrap_or("GET");
    let method = match method_str.to_uppercase().as_str() {
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => Method::Get,
    };

    let url_raw = extract_postman_url(req.get("url")?)?;
    let url_converted = convert_postman_variables(&url_raw);

    let mut headers = HashMap::new();
    if let Some(header_arr) = req.get("header").and_then(|h| h.as_array()) {
        for h in header_arr {
            if let (Some(k), Some(v)) = (
                h.get("key").and_then(|k| k.as_str()),
                h.get("value").and_then(|v| v.as_str()),
            ) {
                headers.insert(k.to_string(), convert_postman_variables(v));
            }
        }
    }

    let mut body_json = None;
    if let Some(body_obj) = req.get("body") {
        if let Some(raw_text) = body_obj.get("raw").and_then(|r| r.as_str()) {
            let converted_raw = convert_postman_variables(raw_text);
            body_json = serde_json::from_str(&converted_raw).ok();
        }
    }

    let sanitized_name = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();

    Some(EndpointConfig {
        name: if sanitized_name.is_empty() {
            "postman_endpoint".into()
        } else {
            sanitized_name
        },
        protocol: Protocol::Http,
        method,
        path: url_converted,
        grpc: None,
        headers,
        query_params: HashMap::new(),
        body: body_json,
        expected_status: 200,
        timeout_override: Some(std::time::Duration::from_secs(30)),
        float_epsilon_override: None,
        auth_override: None,
        mask_overrides: Vec::new(),
        array_modes: HashMap::new(),
        upstream_dependencies: Vec::new(),
    })
}

fn extract_postman_url(url_val: &Value) -> Option<String> {
    if let Some(s) = url_val.as_str() {
        return Some(s.to_string());
    }
    if let Some(raw) = url_val.get("raw").and_then(|r| r.as_str()) {
        return Some(raw.to_string());
    }
    None
}

/// Converts Postman `{{variable}}` syntax to ApiSnap environment variable syntax `${VARIABLE}`.
fn convert_postman_variables(input: &str) -> String {
    let mut out = input.to_string();
    let re = regex::Regex::new(r"\{\{([a-zA-Z0-9_-]+)\}\}").unwrap();
    out = re.replace_all(&out, "$${$1}").to_string();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_postman_v2_collection() {
        let sample = r#"{
            "info": { "name": "Sample API" },
            "item": [
                {
                    "name": "Get User",
                    "request": {
                        "method": "GET",
                        "url": "{{BASE_URL}}/api/v1/users/1",
                        "header": [{ "key": "Authorization", "value": "Bearer {{TOKEN}}" }]
                    }
                }
            ]
        }"#;

        let endpoints = PostmanImporter::parse_collection(sample).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].name, "Get_User");
        assert_eq!(endpoints[0].path, "${BASE_URL}/api/v1/users/1");
        assert_eq!(endpoints[0].headers.get("Authorization").unwrap(), "Bearer ${TOKEN}");
    }
}
