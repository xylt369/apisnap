# Contributing to ApiSnap

Thank you for your interest in contributing to **ApiSnap**! We welcome bug fixes, documentation improvements, new masking heuristics, and feature contributions.

---

## 🛠 Development Workflow

### Prerequisites
- [Rust 1.75+](https://rustup.rs/) (stable toolchain)
- Git

### Building from Source
```bash
git clone https://github.com/xylt369/apisnap.git
cd apisnap

# Run tests
cargo test

# Build debug binary
cargo build

# Run local CLI
cargo run -- --help
```

---

## 🧪 Testing Guidelines
- All new features and bug fixes must include corresponding tests in `tests/integration_test.rs` or module unit tests.
- Run `cargo test` to ensure all tests pass.
- Run `cargo fmt --check` and `cargo clippy` before opening a pull request.

---

## 🚀 Submitting a Pull Request
1. Fork the repository and create your branch from `main`: `git checkout -b feature/my-new-feature`.
2. Commit your changes with clear, descriptive commit messages.
3. Push to your fork and submit a Pull Request.

---

## 📜 Code of Conduct
Please review our [Code of Conduct](CODE_OF_CONDUCT.md) before participating in the community.
