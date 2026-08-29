#!/usr/bin/env bash
set -e

# ApiSnap 1-Line Installer for Linux & macOS
# Usage: curl -sSL https://raw.githubusercontent.com/xylt369/apisnap/main/install.sh | bash

REPO="xylt369/apisnap"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

echo "📸 Installing ApiSnap..."

# Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux*)
    TARGET="linux-x86_64"
    ;;
  Darwin*)
    if [ "$ARCH" = "arm64" ]; then
      TARGET="macos-arm64"
    else
      TARGET="macos-x86_64"
    fi
    ;;
  *)
    echo "Unsupported OS: $OS. Please install via 'cargo install apisnap'."
    exit 1
    ;;
esac

LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST_RELEASE" ]; then
  LATEST_RELEASE="v0.1.0"
fi

URL="https://github.com/$REPO/releases/download/$LATEST_RELEASE/apisnap-$TARGET.tar.gz"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading ApiSnap ($LATEST_RELEASE) for $TARGET..."
if curl -sSL "$URL" -o "$TMP_DIR/apisnap.tar.gz"; then
  tar -xzf "$TMP_DIR/apisnap.tar.gz" -C "$TMP_DIR"
  
  if [ -w "$INSTALL_DIR" ]; then
    cp "$TMP_DIR/apisnap-$TARGET" "$INSTALL_DIR/apisnap"
  else
    echo "Installing to $INSTALL_DIR requires sudo:"
    sudo cp "$TMP_DIR/apisnap-$TARGET" "$INSTALL_DIR/apisnap"
  fi
  chmod +x "$INSTALL_DIR/apisnap"
  echo "✅ ApiSnap installed successfully to $INSTALL_DIR/apisnap!"
  echo "Run 'apisnap --help' to get started."
else
  echo "Release asset not found yet. Falling back to cargo install..."
  if command -v cargo >/dev/null 2>&1; then
    cargo install apisnap
  else
    echo "Cargo not found. Please install Rust from https://rustup.rs."
    exit 1
  fi
fi
