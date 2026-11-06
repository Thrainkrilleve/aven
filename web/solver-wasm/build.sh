#!/usr/bin/env sh
# Rebuild the browser solver and refresh web/public/wasm/solver.wasm.
# Requires: rustup target add wasm32-unknown-unknown  (wasm-opt optional).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
cargo build --release --target wasm32-unknown-unknown --manifest-path "$here/Cargo.toml"
out="$here/target/wasm32-unknown-unknown/release/aven_solver_wasm.wasm"
dest="$here/../public/wasm/solver.wasm"
mkdir -p "$(dirname "$dest")"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Os "$out" -o "$dest"
  echo "built + wasm-opt -> $dest ($(wc -c < "$dest") bytes)"
else
  cp "$out" "$dest"
  echo "built (no wasm-opt) -> $dest ($(wc -c < "$dest") bytes)"
fi
