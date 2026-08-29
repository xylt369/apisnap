# Changelog

All notable changes to **ApiSnap** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.4.0] - 2026-08-30

### Added
- **High-Performance SIMD-JSON & Arena Allocation Subsystem (Milestone v0.4.0)**:
  - **SIMD-JSON Zero-Copy Accelerated Parser (`simd-json`)**: Automatically switches to vector instruction parsing (AVX2/NEON) for payloads exceeding 1MB threshold (`FastJsonEngine`), providing up to 3x throughput improvement on 100MB+ large payloads.
  - **Bumpalo Scoped Arena Allocator (`bumpalo`)**: Scopes AST scratch memory to single-request lifetimes, freeing memory via one single pointer reset operation rather than paying individual per-node recursive `Drop` costs across deep JSON trees.
  - **Pre-Compiled Pattern Automata Cache**: Pre-compiles custom rule regexes at config-load time into `Arc<Regex>` pools, eliminating runtime recompilations across 5,000+ endpoints.
  - **Fast-Hash Equality Bypass**: Identical AST subtrees short-circuit in 0.01ms before executing recursive tree diffing.

---

## [0.3.0] - 2026-08-30

### Added
- **Enterprise Authentication Subsystem (`src/client/auth.rs`)**:
  - `AuthProvider` trait with asynchronous request enrichment.
  - **Static Bearer Token (`Bearer`)** and **API Key Header (`ApiKey`)** auth providers.
  - **HTTP Basic Authentication (`Basic`)** provider.
  - **OAuth2 Client Credentials Flow (`OAuth2ClientCredentials`)**: Auto-refreshing access tokens with expiry-aware read/write locking (`tokio::sync::RwLock`), preventing credential expirations during test suite execution.
  - **Per-Endpoint `auth_override`**: Compose distinct auth mechanisms (e.g. internal mTLS vs public OAuth2) within the same test suite.
- **Bidirectional OpenAPI / Swagger Synchronization Engine (`src/openapi/`)**:
  - **JSON Schema Inference (`schema_infer.rs`)**: Reconstructs OpenAPI 3.1 types from AST and re-derives format hints from mask tokens (`<MASKED_UUID>` $\to$ `format: uuid`, `<MASKED_TIMESTAMP>` $\to$ `format: date-time`, `<MASKED_EMAIL>` $\to$ `format: email`).
  - **OpenAPI 3.1 Spec Generator (`apisnap openapi generate`)**: Synthesizes clean `openapi.yaml` from golden snapshots.
  - **Contract Drift Verifier (`apisnap openapi verify`)**: Compiles and validates recorded snapshot payloads against official OpenAPI schemas with `jsonschema`.

---

## [0.2.0] - 2026-08-30

### Added
- **Enterprise Hardening (Milestone v0.2.0)**:
  - **Recursion Depth Guard (`max_depth = 512`)**: Protects against stack overflow on adversarial or deeply nested JSON payloads.
  - **Float Epsilon Tolerance (`float_epsilon = 0.0001`)**: Eliminates spurious `Modified` diffs on non-deterministic floating-point serializations.
  - **Unicode NFC Key Normalization**: Equates decomposed NFD and precomposed NFC JSON object keys seamlessly.
  - **Enterprise Strict PII Mode (`strict_pii_mode`)**: Deny-by-default redaction model where all leaf data is masked with `<REDACTED>` unless explicitly allowlisted in `unmask_allow_list`.
  - **High-Confidence PII Detectors**:
    - Credit Card numbers with **Luhn Checksum Algorithm** (`<MASKED_CREDIT_CARD>`).
    - US Social Security Numbers (`<MASKED_SSN>`).
    - Email Addresses (`<MASKED_EMAIL>`).
  - **Pre-Write Secret Guard**: Final safety barrier scanning the AST for leaked credentials (AWS Access Keys, PEM Private Key Headers) before disk write, preventing git leaks.

---

## [0.1.0] - 2026-08-29

### Added
- **Core CLI**: Initial release of `apisnap` CLI written in pure Rust.
- **Deterministic Auto-Masker**: Builtin heuristic support for UUIDv4, ISO-8601 timestamps, JWT tokens, Unix epoch timestamps, and MongoDB ObjectIds.
- **Semantic AST Differ**: Order-insensitive JSON object key diffing and configurable array Ordered/Set comparison modes.
- **Interactive Review Workflow**: Single-keystroke interactive TUI review (`a`ccept, `r`eject, `s`kip, `q`uit).
- **Atomic File Store**: Safe `.snap.json` writes via temporary file fsync and atomic rename.
- **Bounded Concurrency**: Parallel request dispatcher powered by `tokio::task::JoinSet`.
- **GitHub Action**: Official `action.yml` for automated CI regression checks.
- **Official VS Code Extension**: Snapshot explorer, inline route CodeLens, and Pro license activation.
- **Official Website**: Next.js + Tailwind dark-mode landing page with interactive in-browser masking playground.
