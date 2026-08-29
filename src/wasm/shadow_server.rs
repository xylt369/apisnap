use crate::engine::{compare_json_ast, DiffOptions};
use crate::error::ApiSnapError;
use crate::wasm::shadow_filter::ShadowSession;
use colored::Colorize;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Production-grade Live Shadow Traffic Differ Proxy Gateway.
pub struct ShadowProxyServer {
    baseline_url: String,
    candidate_url: String,
    listen_port: u16,
    http_client: reqwest::Client,
    session_counter: Arc<AtomicU64>,
}

impl ShadowProxyServer {
    pub fn new(baseline_url: &str, candidate_url: &str, listen_port: u16) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client for shadow proxy");

        Self {
            baseline_url: baseline_url.trim_end_matches('/').to_string(),
            candidate_url: candidate_url.trim_end_matches('/').to_string(),
            listen_port,
            http_client,
            session_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Start the live TCP reverse proxy listening loop.
    pub async fn run(&self) -> Result<(), ApiSnapError> {
        let addr = format!("0.0.0.0:{}", self.listen_port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| ApiSnapError::Io {
            path: addr.clone(),
            source: e,
        })?;

        println!(
            "\n{} Shadow Traffic Differ Gateway active on http://0.0.0.0:{}",
            "[SHADOW PROXY]".green().bold(),
            self.listen_port.to_string().cyan().bold()
        );
        println!("  ├─ Baseline  Upstream: {}", self.baseline_url.cyan());
        println!("  ├─ Candidate Upstream: {}", self.candidate_url.cyan());
        println!("  └─ Mode: Real-time async dual-forwarding + line-rate SIMD-JSON drift detection\n");

        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("  {} Failed to accept connection: {e}", "[WARN]".yellow());
                    continue;
                }
            };

            let session_id = self.session_counter.fetch_add(1, Ordering::Relaxed);
            let baseline_base = self.baseline_url.clone();
            let candidate_base = self.candidate_url.clone();
            let client = self.http_client.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(
                    stream,
                    peer_addr,
                    session_id,
                    &baseline_base,
                    &candidate_base,
                    client,
                )
                .await
                {
                    eprintln!("  {} Session [{session_id}] Error: {e}", "[ERROR]".red());
                }
            });
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: std::net::SocketAddr,
    session_id: u64,
    baseline_base: &str,
    candidate_base: &str,
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

    let baseline_url = format!("{baseline_base}{path_and_query}");
    let candidate_url = format!("{candidate_base}{path_and_query}");

    // 1. Concurrently dispatch to Baseline and Candidate backends
    let req_baseline = build_forward_request(&client, &method, &baseline_url, &headers, body_bytes);
    let req_candidate = build_forward_request(&client, &method, &candidate_url, &headers, body_bytes);

    let start_t = Instant::now();
    let (res_baseline, res_candidate) = tokio::join!(req_baseline.send(), req_candidate.send());

    let duration_ms = start_t.elapsed().as_millis() as u64;

    let resp_base = res_baseline.map_err(|e| ApiSnapError::Network {
        url: baseline_url.clone(),
        source: e,
    })?;
    let resp_cand = res_candidate.map_err(|e| ApiSnapError::Network {
        url: candidate_url.clone(),
        source: e,
    })?;

    let status_base = resp_base.status();
    let status_cand = resp_cand.status();

    let mut base_resp_headers = HashMap::new();
    for (k, v) in resp_base.headers() {
        if let Ok(val) = v.to_str() {
            base_resp_headers.insert(k.as_str().to_string(), val.to_string());
        }
    }

    let bytes_base = resp_base.bytes().await.map_err(|e| ApiSnapError::Network {
        url: baseline_url.clone(),
        source: e,
    })?;
    let bytes_cand = resp_cand.bytes().await.map_err(|e| ApiSnapError::Network {
        url: candidate_url.clone(),
        source: e,
    })?;

    // 2. Perform Real-time Shadow Streaming Comparison (SIMD-JSON Zero-Copy AST Differ)
    let mut shadow_session = ShadowSession::new((session_id % (u32::MAX as u64)) as u32);
    shadow_session.on_body_chunk("baseline", &bytes_base, true);
    shadow_session.on_body_chunk("candidate", &bytes_cand, true);

    let is_drifted = shadow_session.check_structural_drift().unwrap_or(false);

    if is_drifted || status_base != status_cand {
        println!(
            "  {} Session [{}] {} {} ({}ms) -> {}",
            "[DRIFT DETECTED]".red().bold(),
            session_id,
            method.to_string().bold(),
            path_and_query.cyan(),
            duration_ms,
            format!("Baseline: HTTP {} | Candidate: HTTP {}", status_base.as_u16(), status_cand.as_u16()).yellow()
        );

        if let (Ok(ast_base), Ok(ast_cand)) = (
            serde_json::from_slice::<Value>(&bytes_base),
            serde_json::from_slice::<Value>(&bytes_cand),
        ) {
            let diffs = compare_json_ast(&ast_base, &ast_cand, &DiffOptions::default());
            for d in diffs.iter().take(5) {
                println!("     ! Drift item: {d:?}");
            }
        }
    } else {
        println!(
            "  {} Session [{}] {} {} ({}ms) -> {}",
            "[MATCH]".green().bold(),
            session_id,
            method.to_string().bold(),
            path_and_query.cyan(),
            duration_ms,
            format!("HTTP {} (0 Structural Drift)", status_base.as_u16()).green()
        );
    }

    // 3. Transparently return the primary Baseline HTTP response back to the client
    let mut raw_response = Vec::new();
    let status_line = format!("HTTP/1.1 {} {}\r\n", status_base.as_u16(), status_base.canonical_reason().unwrap_or("OK"));
    raw_response.extend_from_slice(status_line.as_bytes());

    for (k, v) in &base_resp_headers {
        if k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        let h_line = format!("{k}: {v}\r\n");
        raw_response.extend_from_slice(h_line.as_bytes());
    }

    let content_len = format!("Content-Length: {}\r\n\r\n", bytes_base.len());
    raw_response.extend_from_slice(content_len.as_bytes());
    raw_response.extend_from_slice(&bytes_base);

    stream.write_all(&raw_response).await.map_err(|e| ApiSnapError::Io {
        path: format!("tcp_write:{peer_addr}"),
        source: e,
    })?;

    stream.flush().await.map_err(|e| ApiSnapError::Io {
        path: format!("tcp_flush:{peer_addr}"),
        source: e,
    })?;

    Ok(())
}

fn parse_incoming_http_request(
    raw: &[u8],
) -> Result<(reqwest::Method, String, HeaderMap, &[u8]), ApiSnapError> {
    let header_sep = b"\r\n\r\n";
    let sep_pos = raw
        .windows(header_sep.len())
        .position(|w| w == header_sep)
        .unwrap_or(raw.len());

    let header_text = std::str::from_utf8(&raw[..sep_pos]).map_err(|_| {
        ApiSnapError::InvalidConfig {
            location: "http_proxy_request".into(),
            reason: "invalid utf-8 in request headers".into(),
        }
    })?;

    let mut lines = header_text.lines();
    let request_line = lines.next().ok_or_else(|| ApiSnapError::InvalidConfig {
        location: "http_proxy_request".into(),
        reason: "empty request line".into(),
    })?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(ApiSnapError::InvalidConfig {
            location: "http_proxy_request".into(),
            reason: format!("malformed request line: '{request_line}'"),
        });
    }

    let method = reqwest::Method::from_str(parts[0]).unwrap_or(reqwest::Method::GET);
    let path_and_query = parts[1].to_string();

    let mut headers = HeaderMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            let v = v.trim();
            if let (Ok(h_name), Ok(h_val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                if !h_name.as_str().eq_ignore_ascii_case("host") {
                    headers.insert(h_name, h_val);
                }
            }
        }
    }

    let body_bytes = if sep_pos + header_sep.len() < raw.len() {
        &raw[sep_pos + header_sep.len()..]
    } else {
        &[]
    };

    Ok((method, path_and_query, headers, body_bytes))
}

fn build_forward_request<'a>(
    client: &reqwest::Client,
    method: &reqwest::Method,
    url: &str,
    headers: &HeaderMap,
    body: &'a [u8],
) -> reqwest::RequestBuilder {
    let mut req = client.request(method.clone(), url);
    for (k, v) in headers {
        req = req.header(k.clone(), v.clone());
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }
    req
}
