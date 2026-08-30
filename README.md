# 📸 ApiSnap

> **The Jest Snapshot for Backend APIs.**
>
> Language-agnostic, zero-SDK CLI written in Rust for deterministic API behavioral baseline testing, adaptive noise learning, multi-branch CAS diffing, behavioral timeline audit, cross-service blast radius analysis, and bidirectional OpenAPI synchronization.

---

## ⚡ Why ApiSnap?

Unlike traditional testing frameworks that require writing boilerplate unit tests, consumer contracts (Pact), or eBPF kernel instrumentation (Keploy), **ApiSnap** establishes zero-pre-requisite **Behavioral Baselines** for legacy, microservice, or newly built APIs in under 60 seconds.

---

## 🚀 Key Superpowers

### 📥 Phase 1: Zero-Prerequisite Ingestion & Adaptive Noise Learning
- **Multi-Format Ingestion**: Convert raw cURL commands, Postman Collections (v2.0/v2.1), and browser HAR exports directly into testable `apisnap.toml` endpoints.
  ```bash
  apisnap import --curl 'curl -X POST https://api.io/v1/orders -H "Authorization: Bearer tok" -d "{\"sku\":\"A-101\"}"'
  apisnap import --postman postman_collection.json
  apisnap import --har network_traffic.har
  ```
- **Adaptive Noise Learner (`apisnap record --learn 5`)**: Automatically sends $N$ probe requests, statistically detects high-variance dynamic tokens (e.g. sequence IDs, microsecond clocks, random nonces), and generates optimal JSONPath mask rules with zero manual regex tuning.
- **Local Transparent Capture Proxy (`apisnap capture`)**: Start a local non-kernel reverse proxy on `:9090` to record live HTTP/JSON traffic into golden baselines as you click through your frontend or mobile app.
  ```bash
  apisnap capture --proxy 127.0.0.1:9090 --target http://localhost:8080
  ```

### 🌿 Phase 2: Multi-Branch Pointer Storage & Team Governance
- **Merkle CAS Pointers**: ASTs are chunked and deduplicated in content-addressable BLAKE3 Merkle trees. Branch snapshots are lightweight `.ptr` pointer manifests with zero file duplication.
- **Branch-to-Branch Offline Regression Diffing**:
  ```bash
  apisnap test --baseline main --candidate pr-402
  ```
- **Intentional Breaking Change Approval Ledger (`apisnap approve-diff`)**: Team leads can whitelist intentional schema modifications with cryptographic author audit trails without breaking CI pipelines.
  ```bash
  apisnap approve-diff --endpoint user_service --author "@tech-lead" --reason "Migrated auth token schema"
  ```

### 🛰️ Phase 3: Behavioral Timeline Engine & Cross-Service Blast Radius
- **API Behavioral Time Machine (`apisnap timeline`)**: Immutable append-only historical audit log tracking schema evolutions, latency drifts, and structural deltas across releases.
  ```bash
  apisnap timeline show --endpoint billing_service --limit 10
  apisnap timeline diff --endpoint billing_service --commit-a 4f8b2c1 --commit-b 9a1e7d3
  ```
- **Cross-Service Blast Radius Radar (`apisnap blast-radius`)**: Automatically maps API modifications against declared downstream consumers, pinpointing breaking changes before deploying to staging.
  ```bash
  apisnap blast-radius --endpoint auth_service.verify_token
  ```

### 🛡️ Core Engine: Deterministic AST Differ & Security
- **Deterministic Smart Auto-Masker**: Built-in sanitization for UUIDv4, ISO-8601 timestamps, JWT tokens, Mongo ObjectIds, Luhn credit cards, SSNs, and emails.
- **Semantic AST Differ**: Order-insensitive JSON object key comparison, array Set/Ordered modes, Unicode NFC normalization, and float epsilon tolerance.
- **Bidirectional OpenAPI 3.1 Sync**: `apisnap openapi generate` & `apisnap openapi verify`.
- **Smart Resilience Fuzzing (`apisnap fuzz`)**: Boundary mutation engine uncovering HTTP 500 server crashes and secret leaks.
- **Enterprise Security**: AES-256-GCM authenticated encryption at rest (`APISNAP_MASTER_KEY`).

---

## 📦 Installation

```bash
# Cargo
cargo install apisnap

# One-Line Bash Installer
curl -sSL https://raw.githubusercontent.com/xylt369/apisnap/main/install.sh | bash
```

---

## 🛠️ Complete CLI Cheat Sheet

```bash
# Initialize project configuration
apisnap init

# Record snapshots with adaptive noise learning
apisnap record --learn 5

# Run regression tests in CI
apisnap test --ci

# Diff candidate branch pointer directly against baseline branch pointer
apisnap test --baseline main --candidate feature-branch

# Inspect behavioral timeline history
apisnap timeline show --endpoint order_api

# Calculate downstream blast radius
apisnap blast-radius --endpoint user_api

# Start transparent capture reverse proxy
apisnap capture --proxy 127.0.0.1:9090 --target http://localhost:3000

# Interactively review and approve drifts
apisnap review
```

---

## 📄 License
Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE).
