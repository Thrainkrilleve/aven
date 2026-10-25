# scripts/ci_check.ps1 - Local CI/CD verification script for Windows PowerShell
$ErrorActionPreference = "Stop"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " Running Aven CI/CD Pre-Flight Checks" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

# 1. Format check
Write-Host ""
Write-Host "[1/4] Checking code formatting (cargo fmt)..." -ForegroundColor Yellow
cargo fmt --check
if ($LASTEXITCODE -ne 0) {
    Write-Host "Formatting check failed! Run 'cargo fmt' to fix." -ForegroundColor Red
    exit 1
}
Write-Host "Formatting check PASSED." -ForegroundColor Green

# 2. Strict Clippy Linter
Write-Host ""
Write-Host "[2/4] Running strict Clippy linter (cargo clippy)..." -ForegroundColor Yellow
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host "Clippy check failed!" -ForegroundColor Red
    exit 1
}
Write-Host "Clippy linter PASSED (zero warnings)." -ForegroundColor Green

# 3. Unit Test Suite
Write-Host ""
Write-Host "[3/4] Running release test suite (cargo test)..." -ForegroundColor Yellow
cargo test --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Test suite failed!" -ForegroundColor Red
    exit 1
}
Write-Host "All unit tests PASSED." -ForegroundColor Green

# 4. Multi-Pillar Demo
Write-Host ""
Write-Host "[4/4] Verifying avenc release binary and demo..." -ForegroundColor Yellow
cargo build --release
& .\target\release\avenc.exe demo
if ($LASTEXITCODE -ne 0) {
    Write-Host "avenc demo failed!" -ForegroundColor Red
    exit 1
}
Write-Host "avenc release build and core verification PASSED." -ForegroundColor Green

Write-Host ""
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " ALL CI/CD CHECKS PASSED - Clean to commit and push!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan
