use crate::config::{EndpointConfig, Method, Protocol};
use crate::error::ApiSnapError;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Importer for HTTP Archive (HAR 1.2) JSON files exported from DevTools.
pub struct HarImporter;

impl HarImporter {
    pub fn parse_har(har_content: &str) -> Result<Vec<EndpointConfig>, ApiSnapError> {
        let root: Value = serde_json::from_str(har_content).map_err(|e| ApiSnapError::InvalidConfig {
            location: "har_import".into(),
            reason: format!("Invalid HAR JSON: {e}"),
        })?;

        let entries = root
            .get("log")
            .and_then(|l| l.get("entries"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| ApiSnapError::InvalidConfig {
                location: "har_import".into(),
                reason: "Missing 'log.entries' array in HAR file".into(),
            })?;

        let mut endpoints = Vec::new();
        let mut seen_routes = HashSet::new();

        for entry in entries {
            if let Some(req) = entry.get("request") {
                if let Some(ep) = parse_har_entry(req) {
                    let route_key = format!("{:?}:{}", ep.method, ep.path);
                    if !seen_routes.contains(&route_key) {
                        seen_routes.insert(route_key);
                        endpoints.push(ep);
                    }
                }
            }
        }

        if endpoints.is_empty() {
            return Err(ApiSnapError::InvalidConfig {
                location: "har_import".into(),
                reason: "No API requests found in HAR file".into(),
            });
        }

        Ok(endpoints)
    }
}

fn parse_har_entry(req: &Value) -> Option<EndpointConfig> {
    let method_str = req.get("method").and_then(|m| m.as_str())?;
    let method = match method_str.to_uppercase().as_str() {
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => Method::Get,
    };

    let url_str = req.get("url").and_then(|u| u.as_str())?;
    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
        return None;
    }

    let parsed_url = url::Url::parse(url_str).ok()?;
    let path = parsed_url.path();

    // Skip static assets (.js, .css, .png, .jpg, .svg, .woff, etc.)
    let static_extensions = [".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".woff", ".woff2", ".ttf"];
    if static_extensions.iter().any(|ext| path.ends_with(ext)) {
        return None;
    }

    let mut headers = HashMap::new();
    if let Some(header_arr) = req.get("headers").and_then(|h| h.as_array()) {
        for h in header_arr {
            if let (Some(name), Some(val)) = (
                h.get("name").and_then(|n| n.as_str()),
                h.get("value").and_then(|v| v.as_str()),
            ) {
                // Filter out standard noisy browser headers
                let noisy = ["sec-ch-ua", "user-agent", "cookie", "sec-fetch-", "origin", "referer"];
                if !noisy.iter().any(|n| name.to_lowercase().starts_with(n)) {
                    headers.insert(name.to_string(), val.to_string());
                }
            }
        }
    }

    let mut body_json = None;
    if let Some(post_data) = req.get("postData") {
        if let Some(text) = post_data.get("text").and_then(|t| t.as_str()) {
            body_json = serde_json::from_str(text).ok();
        }
    }

    let clean_path = path.trim_matches('/').replace('/', "_");
    let slug = format!("{}_{}", method_str.to_lowercase(), clean_path)
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();

    Some(EndpointConfig {
        name: if slug.is_empty() { "har_endpoint".into() } else { slug },
        protocol: Protocol::Http,
        method,
        path: path.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_har_file() {
        let sample = r#"{
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "POST",
                            "url": "https://api.internal/v1/checkout",
                            "headers": [
                                { "name": "Content-Type", "value": "application/json" },
                                { "name": "user-agent", "value": "Mozilla/5.0" }
                            ],
                            "postData": {
                                "text": "{\"amount\": 199.95}"
                            }
                        }
                    }
                ]
            }
        }"#;

        let endpoints = HarImporter::parse_har(sample).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].method, Method::Post);
        assert_eq!(endpoints[0].path, "/v1/checkout");
        assert_eq!(endpoints[0].headers.get("Content-Type").unwrap(), "application/json");
        assert!(!endpoints[0].headers.contains_key("user-agent")); // filtered noisy
        assert_eq!(endpoints[0].body.as_ref().unwrap()["amount"], 199.95);
    }
}
