# 📸 ApiSnap

> **Language-Agnostic, Zero-SDK CLI for HTTP & gRPC API Snapshot Regression Testing with Deterministic Auto-Masking.**
> *"The Jest Snapshot for Backend APIs — Eliminate thousands of handwritten assertion lines."*

---

## ⚡ Features

- **Zero-SDK / Zero-Code Dependency**: Works with any backend language (Go, Python, Java, Rust, Node, PHP).
- **Deterministic Smart Auto-Masking**: Automatically sanitizes dynamic UUIDs, ISO-8601 timestamps, JWT tokens, and Unix epoch timestamps before saving snapshots.
- **Order-Insensitive Semantic Diffing**: Compares JSON ASTs mathematically (keys as sets), eliminating spurious false-positives from key reordering.
- **Interactive Review Workflow**: Like `cargo-insta`, review API changes and accept/reject them with a single keystroke (`a`/`r`/`s`/`q`).
- **Atomic Disk Writes**: Snapshot files (`.snap.json`) are written atomically to avoid corruption on unexpected process crashes.
- **CI / CD Ready**: Returns standard exit codes (`0` for pass, `1` for diff mismatch, `2` for network errors) and supports `--ci` machine-readable JSON output.

---

## 🚀 Quick Start

### 1. Initialize Configuration
```bash
apisnap init
```
This generates `apisnap.toml` in your project root.

### 2. Configure Endpoints
```toml
base_url = "http://localhost:8000"
timeout = "30s"
concurrency = 10
snapshot_dir = "__snapshots__"

[global_headers]
"Accept" = "application/json"
"Authorization" = "Bearer token123"

[masking]
enable_builtin_heuristics = true

[[endpoints]]
name = "get_user_profile"
method = "GET"
path = "/api/v1/users/1"
expected_status = 200

[[endpoints]]
name = "create_order"
method = "POST"
path = "/api/v1/orders"
expected_status = 201

[endpoints.body]
item_id = "SKU-998"
quantity = 2
```

### 3. Record Initial Snapshots
```bash
apisnap record
```
This hits your live/local API, masks volatile fields, and creates readable `.snap.json` files in `__snapshots__/`.

### 4. Run Regression Tests
```bash
apisnap test
```
Runs in $<100\text{ms}$ in your CI/CD pipeline!

### 5. Interactively Review Changes
```bash
apisnap review
```
Review detected diffs and accept new API shapes with `[a]`.

---

## 📄 License
MIT OR Apache-2.0
