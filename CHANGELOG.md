# Changelog

All notable changes to **ApiSnap** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
