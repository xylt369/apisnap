use crate::config::MaskingConfig;
use crate::engine::{mask_value, MaskContext};
use crate::error::ApiSnapError;
use crate::snapshot::{SnapshotFile, SnapshotMetadata};
use colored::Colorize;
use reqwest::header::{HeaderName, HeaderValue};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// Configuration for the local transparent capture proxy.
#[derive(Debug, Clone)]
pub struct ProxyCaptureConfig {
    pub listen_addr: SocketAddr,
    pub target_upstream: String,
    pub snapshot_dir: PathBuf,
    pub masking: MaskingConfig,
}

/// Local reverse proxy that records live HTTP traffic into golden snapshots.
pub struct ProxyCaptureEngine {
    config: ProxyCaptureConfig,
    client: reqwest::Client,
}

impl ProxyCaptureEngine {
    pub fn new(config: ProxyCaptureConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { config, client }
    }

    pub async fn start(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ApiSnapError> {
        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .map_err(|e| ApiSnapError::Io {
                path: self.config.listen_addr.to_string(),
                source: e,
            })?;

        println!(
            "{} Capture Proxy active on http://{} -> forwarding to {}",
            "[PROXY]".green().bold(),
            self.config.listen_addr.to_string().cyan().bold(),
            self.config.target_upstream.cyan()
        );

        let config = Arc::new(self.config.clone());
        let client = self.client.clone();

        loop {
            tokio::select! {
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, peer_addr)) => {
                            let cfg = Arc::clone(&config);
                            let cl = client.clone();

                            tokio::spawn(async move {
                                if let Err(err) = handle_connection(stream, peer_addr, cfg, cl).await {
                                    eprintln!("  {} Proxy session error: {err}", "[WARN]".yellow());
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("  {} Accept error: {e}", "[ERROR]".red());
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    println!("\n{} Proxy Capture Engine stopped gracefully.", "[INFO]".cyan());
                    break;
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    config: Arc<ProxyCaptureConfig>,
    client: reqwest::Client,
) -> Result<(), ApiSnapError> {
    let mut buf = [0u8; 65536];
    let n = stream.read(&mut buf).await.map_err(|e| ApiSnapError::Io {
        path: format!("tcp_read:{peer_addr}"),
        source: e,
    })?;

    if n == 0 {
        return Ok(());
    }

    let raw_req = &buf[..n];
    let (method, path_and_query, headers, body_bytes) = parse_incoming_http_request(raw_req)?;

    let target_url = format!(
        "{}{}",
        config.target_upstream.trim_end_matches('/'),
        path_and_query
    );

    let start_t = std::time::Instant::now();
    let mut req_builder = match method.as_str() {
        "POST" => client.post(&target_url),
        "PUT" => client.put(&target_url),
        "DELETE" => client.delete(&target_url),
        "PATCH" => client.patch(&target_url),
        "HEAD" => client.head(&target_url),
        _ => client.get(&target_url),
    };

    for (k, v) in &headers {
        if !k.eq_ignore_ascii_case("host") {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                req_builder = req_builder.header(name, val);
            }
        }
    }

    if let Some(body) = body_bytes {
        req_builder = req_builder.body(body.to_vec());
    }

    let upstream_res = match req_builder.send().await {
        Ok(r) => r,
        Err(e) => {
            let err_resp = format!("HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{e}", e.to_string().len());
            let _ = stream.write_all(err_resp.as_bytes()).await;
            return Ok(());
        }
    };

    let duration_ms = start_t.elapsed().as_millis() as u64;
    let status = upstream_res.status();
    let status_code = status.as_u16();

    let mut response_headers = HashMap::new();
    let mut header_bytes = format!("HTTP/1.1 {} {}\r\n", status.as_str(), status.canonical_reason().unwrap_or("OK"));

    for (k, v) in upstream_res.headers() {
        let val_str = v.to_str().unwrap_or("");
        response_headers.insert(k.as_str().to_string(), val_str.to_string());
        header_bytes.push_str(&format!("{}: {}\r\n", k.as_str(), val_str));
    }
    header_bytes.push_str("\r\n");

    let res_bytes = upstream_res.bytes().await.map_err(|e| ApiSnapError::Execution(format!("HTTP {status_code} error: {e}")))?;

    // Respond back to client
    let _ = stream.write_all(header_bytes.as_bytes()).await;
    let _ = stream.write_all(&res_bytes).await;

    // Record snapshot if JSON
    if let Ok(mut json_val) = serde_json::from_slice::<serde_json::Value>(&res_bytes) {
        let mask_ctx = MaskContext::new(&config.masking, &[]);
        mask_value(&mut json_val, &mask_ctx);

        let endpoint_slug = format!(
            "{}_{}",
            method.to_lowercase(),
            path_and_query.replace('/', "_").trim_matches('_')
        );

        let snapshot = SnapshotFile {
            endpoint_name: endpoint_slug.clone(),
            metadata: SnapshotMetadata {
                recorded_at: chrono::Utc::now().to_rfc3339(),
                duration_ms,
                status_code,
                grpc_status_code: None,
                response_headers,
                apisnap_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            masked_body: json_val,
        };

        let file_path = config.snapshot_dir.join(format!("{endpoint_slug}.snap.json"));
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json_str) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(&file_path, json_str);
            println!(
                "  {} {:<30} -> {}",
                "[CAPTURED]".green().bold(),
                endpoint_slug.bold(),
                file_path.display().to_string().dimmed()
            );
        }
    }

    Ok(())
}

fn parse_incoming_http_request(
    raw: &[u8],
) -> Result<(String, String, HashMap<String, String>, Option<&[u8]>), ApiSnapError> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| ApiSnapError::InvalidConfig {
            location: "tcp_parse".into(),
            reason: "Incomplete HTTP request headers".into(),
        })?;

    let header_part = std::str::from_utf8(&raw[..header_end]).map_err(|e| ApiSnapError::InvalidConfig {
        location: "tcp_utf8".into(),
        reason: e.to_string(),
    })?;

    let mut lines = header_part.lines();
    let request_line = lines.next().ok_or_else(|| ApiSnapError::InvalidConfig {
        location: "tcp_req_line".into(),
        reason: "Missing HTTP request line".into(),
    })?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path_and_query = parts.next().unwrap_or("/").to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    let body_bytes = if raw.len() > header_end + 4 {
        Some(&raw[header_end + 4..])
    } else {
        None
    };

    Ok((method, path_and_query, headers, body_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_incoming_http_request() {
        let raw = b"POST /api/v1/orders HTTP/1.1\r\nHost: localhost:9090\r\nContent-Type: application/json\r\n\r\n{\"item\": 1}";
        let (method, path, headers, body) = parse_incoming_http_request(raw).unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/v1/orders");
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(body, Some(b"{\"item\": 1}".as_slice()));
    }
}
