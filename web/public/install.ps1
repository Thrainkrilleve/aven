# Aven installer for Windows (PowerShell) — https://avenc.dev
#
# Builds avenc from source with cargo. Prebuilt binaries are not published yet,
# so a Rust toolchain is required (https://rustup.rs). Uses return/throw rather than
# exit, because this runs inside the caller's session via `irm ... | iex`.

$Repo = "https://github.com/Thrainkrilleve/aven.git"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "error: cargo was not found." -ForegroundColor Red
    Write-Host "Install a Rust toolchain from https://rustup.rs, then re-run this installer."
    return
}

Write-Host "==> Building avenc from $Repo (this takes about a minute)..." -ForegroundColor Cyan
cargo install --git $Repo avenc --locked
if ($LASTEXITCODE -ne 0) { throw "cargo install failed with exit code $LASTEXITCODE" }

Write-Host ""
Write-Host "==> Installed. Check it with: avenc --version" -ForegroundColor Green
