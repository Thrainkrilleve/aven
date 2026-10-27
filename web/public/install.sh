#!/bin/sh
# Aven Universal Installer for Linux, macOS & BSD
# https://avenc.dev
set -e

AVEN_VERSION="0.9.0"
echo "==> Installing Aven Relational Compiler (avenc v${AVEN_VERSION})..."

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "Error: Unsupported architecture '$ARCH'"; exit 1 ;;
esac

TARGET="${ARCH}-unknown-${OS}-musl"
if [ "$OS" = "darwin" ]; then
  TARGET="${ARCH}-apple-darwin"
fi

INSTALL_DIR="${HOME}/.cargo/bin"
mkdir -p "${INSTALL_DIR}"

if command -v cargo >/dev/null 2>&1; then
  echo "==> Rust toolchain detected. Building from pure-std source..."
  cargo install --git https://github.com/Thrainkrilleve/aven.git --tag "v${AVEN_VERSION}" --force
else
  DOWNLOAD_URL="https://github.com/Thrainkrilleve/aven/releases/download/v${AVEN_VERSION}/avenc-${TARGET}.tar.gz"
  echo "==> Downloading pre-compiled binary from ${DOWNLOAD_URL}..."
  curl -sSfL "${DOWNLOAD_URL}" | tar -xz -C "${INSTALL_DIR}"
  chmod +x "${INSTALL_DIR}/avenc"
fi

echo ""
echo "==> Success! avenc has been installed to ${INSTALL_DIR}/avenc"
echo "==> Verify installation with: avenc --help"
