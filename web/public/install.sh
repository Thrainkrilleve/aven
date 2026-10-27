#!/bin/sh
# Aven installer for Linux and macOS — https://avenc.dev
#
# Builds avenc from source with cargo. Prebuilt binaries are not published yet,
# so a Rust toolchain is required (https://rustup.rs).
set -eu

REPO="https://github.com/Thrainkrilleve/aven.git"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo was not found." >&2
  echo "Install a Rust toolchain first:" >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  echo "then re-run this installer." >&2
  exit 1
fi

echo "==> Building avenc from ${REPO} (this takes about a minute)..."
cargo install --git "${REPO}" avenc --locked

echo ""
echo "==> Installed. Check it with: avenc --version"
