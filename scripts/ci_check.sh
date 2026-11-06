#!/usr/bin/env bash
# scripts/ci_check.sh — Local CI/CD verification script for Linux / macOS
set -euo pipefail

echo -e "\033[1;36m============================================================\033[0m"
echo -e "\033[1;36m Running Aven CI/CD Pre-Flight Checks\033[0m"
echo -e "\033[1;36m============================================================\033[0m"

echo -e "\n\033[1;33m[1/4] Checking code formatting (cargo fmt)...\033[0m"
cargo fmt --check
echo -e "\033[1;32mFormatting check PASSED.\033[0m"

echo -e "\n\033[1;33m[2/4] Running strict Clippy linter (cargo clippy)...\033[0m"
cargo clippy --all-targets -- -D warnings
echo -e "\033[1;32mClippy linter PASSED (zero warnings).\033[0m"

echo -e "\n\033[1;33m[3/4] Running release test suite (cargo test)...\033[0m"
cargo test --release
echo -e "\033[1;32mRelease test suite PASSED.\033[0m"

echo -e "\n\033[1;33m[4/4] Verifying avenc release binary & demo...\033[0m"
cargo build --release
./target/release/avenc demo
echo -e "\033[1;32mavenc release build & core verification PASSED.\033[0m"

echo -e "\n\033[1;36m============================================================\033[0m"
echo -e "\033[1;32m ALL CI/CD CHECKS PASSED — Clean to commit and push!\033[0m"
echo -e "\033[1;36m============================================================\033[0m"
