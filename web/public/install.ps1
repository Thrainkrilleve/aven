# Aven Windows Installer (PowerShell)
# https://avenc.dev
$ErrorActionPreference = "Stop"

$Version = "0.9.0"
Write-Host "==> Installing Aven Relational Compiler (avenc v$Version)..." -ForegroundColor Cyan

$InstallDir = "$env:USERPROFILE\.cargo\bin"
if (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Host "==> Rust toolchain detected. Building from pure-std source..." -ForegroundColor Green
    cargo install --git https://github.com/Thrainkrilleve/aven.git --tag "v$Version" --force
} else {
    $Url = "https://github.com/Thrainkrilleve/aven/releases/download/v$Version/avenc-x86_64-pc-windows-msvc.zip"
    Write-Host "==> Downloading binary from $Url..." -ForegroundColor Green
    $ZipPath = "$env:TEMP\avenc.zip"
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath
    Expand-Archive -Path $ZipPath -DestinationPath $InstallDir -Force
    Remove-Item -Path $ZipPath -Force
}

Write-Host "==> Success! avenc installed to $InstallDir\avenc.exe" -ForegroundColor Green
Write-Host "==> Run: avenc --help" -ForegroundColor Yellow
