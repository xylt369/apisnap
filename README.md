# 📸 ApiSnap

> **The Jest Snapshot for Backend APIs.**
>
> Language-agnostic, zero-SDK CLI written in Rust for deterministic API snapshot regression testing, bidirectional OpenAPI synchronization, smart resilience fuzzing, and CI contract governance.

---

## Key Superpowers

- **Zero-SDK Dependency**: Works out-of-the-box with Go, Python, Rust, Node.js, Java, Ruby, PHP, Elixir, and C#.
- **Deterministic Smart Auto-Masker**: Heuristic auto-sanitization of volatile noise (UUIDv4, ISO-8601 timestamps, JWT tokens, Mongo ObjectIds, Luhn-verified credit cards, SSNs, and emails).
- **Semantic AST Differ**: Order-insensitive JSON object key comparison, array Set/Ordered modes, Unicode NFC normalization, and float epsilon tolerance.
- **100MB+ Large Payload Acceleration**: Powered by `simd-json` AVX2/NEON vector instructions, `bumpalo` arena allocation, and pattern pre-compilation caches.
- **Bidirectional OpenAPI 3.1 Sync**:
  - `apisnap openapi generate`: Synthesizes valid OpenAPI 3.1 YAML specifications directly from golden snapshots.
  - `apisnap openapi verify`: Validates live API responses against existing OpenAPI documentation to detect contract drift.
- **Smart Resilience Fuzzing (`apisnap fuzz`)**: Boundary mutation engine generating SQLi, XSS, integer overflows, and missing-key variations to uncover HTTP 500 server crashes and stack trace leaks.
- **Enterprise Security & Encryption at Rest**: AES-256-GCM authenticated encryption (`APISNAP_MASTER_KEY`) and pre-write secret defense scanning.
- **Enterprise AuthProvider**: Auto-refreshing OAuth2 Client Credentials flow, API Key headers, Bearer tokens, and per-endpoint auth overrides.
- **Interactive Review TUI**: Single-keystroke interactive review (`a`ccept / `r`eject / `s`kip).
- **Pro Ecosystem**: Official VS Code Extension (Route CodeLens & Snapshot Explorer), GitHub Action CI Bot, and Next.js Landing Page.

---

## 1-Line Installation

```bash
curl -sSL https://raw.githubusercontent.com/xylt369/apisnap/main/install.sh | bash
```

Or via Cargo:
```bash
cargo install apisnap
```

---

## 🛠️ Quickstart

### 1. Scaffold Configuration
```bash
apisnap init
```

### 2. Record Golden Snapshots
```bash
apisnap record
```

### 3. Run Regression Tests in CI
```bash
apisnap test --ci
```

### 4. Interactively Review Regressions
```bash
apisnap review
```

### 5. Run Smart Resilience Fuzzing
```bash
apisnap fuzz
```

### 6. Bidirectional OpenAPI Generation & Drift Verification
```bash
apisnap openapi generate --output openapi.yaml
apisnap openapi verify --spec openapi.yaml
```

---

## 📄 License
Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE).
