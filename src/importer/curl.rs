use crate::config::{EndpointConfig, Method, Protocol};
use crate::error::ApiSnapError;
use serde_json::Value;
use std::collections::HashMap;

/// Robust cURL command line parser that converts raw cURL commands into ApiSnap `EndpointConfig`.
pub struct CurlImporter;

impl CurlImporter {
    pub fn parse(curl_cmd: &str) -> Result<EndpointConfig, ApiSnapError> {
        let trimmed = curl_cmd.trim();
        if !trimmed.starts_with("curl") {
            return Err(ApiSnapError::InvalidConfig {
                location: "curl_import".into(),
                reason: "Command must start with 'curl'".into(),
            });
        }

        let tokens = tokenize_command(trimmed);
        let mut method = Method::Get;
        let mut url_str: Option<String> = None;
        let mut headers = HashMap::new();
        let mut body_str: Option<String> = None;

        let mut i = 1;
        while i < tokens.len() {
            let token = &tokens[i];
            match token.as_str() {
                "-X" | "--request" => {
                    if i + 1 < tokens.len() {
                        method = parse_method(&tokens[i + 1]);
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "-H" | "--header" => {
                    if i + 1 < tokens.len() {
                        let header_line = &tokens[i + 1];
                        if let Some((k, v)) = header_line.split_once(':') {
                            headers.insert(k.trim().to_string(), v.trim().to_string());
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                    if i + 1 < tokens.len() {
                        body_str = Some(tokens[i + 1].clone());
                        if method == Method::Get {
                            method = Method::Post;
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "-u" | "--user" => {
                    if i + 1 < tokens.len() {
                        let user_pass = &tokens[i + 1];
                        let encoded = base64_simple(user_pass.as_bytes());
                        headers.insert("Authorization".into(), format!("Basic {encoded}"));
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                _ => {
                    if !token.starts_with('-') && url_str.is_none() {
                        url_str = Some(token.clone());
                    }
                    i += 1;
                }
            }
        }

        let raw_url = url_str.ok_or_else(|| ApiSnapError::InvalidConfig {
            location: "curl_import".into(),
            reason: "No URL found in cURL command".into(),
        })?;

        let parsed_url = url::Url::parse(&raw_url).map_err(|e| ApiSnapError::InvalidConfig {
            location: "curl_import".into(),
            reason: format!("Invalid URL '{raw_url}': {e}"),
        })?;

        let path = if parsed_url.query().is_some() {
            format!("{}?{}", parsed_url.path(), parsed_url.query().unwrap())
        } else {
            parsed_url.path().to_string()
        };

        let endpoint_name = generate_endpoint_slug(&method, &path);

        let body_json: Option<Value> = body_str.and_then(|b| serde_json::from_str(&b).ok());

        Ok(EndpointConfig {
            name: endpoint_name,
            protocol: Protocol::Http,
            method,
            path,
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
}

fn parse_method(s: &str) -> Method {
    match s.to_uppercase().as_str() {
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => Method::Get,
    }
}

fn generate_endpoint_slug(method: &Method, path: &str) -> String {
    let method_str = format!("{method:?}").to_lowercase();
    let sanitized_path = path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let clean_path = sanitized_path.trim_matches('_');
    if clean_path.is_empty() {
        format!("{method_str}_root")
    } else {
        format!("{method_str}_{clean_path}")
    }
}

fn tokenize_command(cmd: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for c in cmd.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }

        match c {
            '\\' => {
                escaped = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' | '\r' | '\n' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn base64_simple(input: &[u8]) -> String {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(SET[((triple >> 18) & 63) as usize] as char);
        result.push(SET[((triple >> 12) & 63) as usize] as char);
        if i + 1 < input.len() {
            result.push(SET[((triple >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if i + 2 < input.len() {
            result.push(SET[(triple & 63) as usize] as char);
        } else {
            result.push('=');
        }
        i += 3;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get_curl() {
        let cmd = "curl https://api.example.com/v1/users";
        let ep = CurlImporter::parse(cmd).unwrap();
        assert_eq!(ep.method, Method::Get);
        assert_eq!(ep.path, "/v1/users");
        assert_eq!(ep.name, "get_v1_users");
    }

    #[test]
    fn test_parse_post_curl_with_headers_and_body() {
        let cmd = r#"curl -X POST https://api.example.com/v1/orders -H "Content-Type: application/json" -H "Authorization: Bearer token123" -d '{"item_id": 42, "qty": 1}'"#;
        let ep = CurlImporter::parse(cmd).unwrap();
        assert_eq!(ep.method, Method::Post);
        assert_eq!(ep.headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(ep.headers.get("Authorization").unwrap(), "Bearer token123");
        assert!(ep.body.is_some());
        assert_eq!(ep.body.unwrap()["item_id"], 42);
    }
}
