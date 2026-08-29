# Changelog

All notable changes to **ApiSnap** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0] - 2026-08-30 — General Availability (GA)

### Added
- **Full Architecture Roadmap Complete (v0.1.0 $\to$ v1.0.0)**:
  - **gRPC & Protobuf Dynamic Subsystem (`src/client/grpc.rs`)**: RPC method invocation with status code mapping and reflection support.
  - **Smart Resilience Fuzzing Engine (`src/fuzz/` & `apisnap fuzz`)**: Boundary mutation engine (SQLi, XSS, integer boundaries, missing keys) classifying anomalies, 500 server crashes, and stack trace leaks.
  - **AES-256-GCM Snapshot Encryption at Rest (`src/crypto/` & `APISNAP_MASTER_KEY`)**: Authenticated encryption of `.snap.enc` files preventing secrets exposure in repositories.
  - **GitHub PR Visual Diff Bot Formatter (`--pr-comment`)**: Generates rich Markdown tables and collapsible diffs for automated PR comments in CI.
  - **SIMD-JSON Zero-Copy Accelerated Parser (`simd-json`)**: Vector instruction acceleration (AVX2/NEON) for payloads $\ge$ 1MB.
  - **Bumpalo Scoped Arena Allocator (`bumpalo`)**: Scopes scratch memory to single requests for zero-cost single-pointer deallocation.
  - **Bidirectional OpenAPI / Swagger Synchronization Engine (`src/openapi/`)**:
    - `apisnap openapi generate`: Synthesizes OpenAPI 3.1 YAML from golden snapshots.
    - `apisnap openapi verify`: Validates recorded snapshots against OpenAPI schemas using `jsonschema`.
  - **Enterprise Authentication Subsystem (`src/client/auth.rs`)**: OAuth2 client credentials auto-refresh, Bearer, ApiKey, and per-endpoint auth overrides.
  - **Enterprise Hardening**: Max recursion depth guards (`max_depth = 512`), float epsilon tolerance (`float_epsilon = 0.0001`), Unicode NFC key normalization, strict PII deny-by-default allowlists, Luhn credit card detection, and pre-write secret scanning defense.
  - **Official VS Code Pro Extension (`vscode-extension/`)** & **Next.js Landing Page (`website/`)**.

---

## [0.4.0] - 2026-08-30
- Added SIMD-JSON zero-copy parsing, Bumpalo arena allocation, pattern pre-compilation cache, and fast-hash equality bypass.

## [0.3.0] - 2026-08-30
- Added enterprise `AuthProvider` (OAuth2 auto-refreshing token cache, ApiKey, Bearer) and bidirectional OpenAPI 3.1 generation and verification.

## [0.2.0] - 2026-08-30
- Added recursion depth guards, float epsilon tolerance, Unicode NFC key normalization, strict PII mode, Luhn credit card detection, and pre-write secret defense.

## [0.1.0] - 2026-08-29
- Initial release of core CLI, auto-masker, AST differ, interactive review TUI, atomic store, and GitHub Action.
