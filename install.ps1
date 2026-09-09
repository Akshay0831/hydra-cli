# Hydra CLI Windows Installation Script
$ErrorActionPreference = "Stop"

Write-Host "=== Installing Hydra CLI for Windows ===" -ForegroundColor Cyan

# Check git
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Error "git is required but was not found in PATH."
    exit 1
}

# Check cargo
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo (Rust) is required but was not found in PATH."
    exit 1
}

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Definition

Write-Host "1. Initializing and updating Git submodules..." -ForegroundColor Yellow
Set-Location $RepoRoot
git submodule update --init --recursive

Write-Host "2. Building release binary..." -ForegroundColor Yellow
cargo build --release --bin hydra-cli

$TargetExe = Join-Path $RepoRoot "target\release\hydra-cli.exe"
if (-not (Test-Path $TargetExe)) {
    Write-Error "Build finished but $TargetExe was not found."
    exit 1
}

$InstallDir = Join-Path $HOME ".cargo\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

Copy-Item -Path $TargetExe -Destination (Join-Path $InstallDir "hydra-cli.exe") -Force

Write-Host "`n=== Hydra CLI successfully installed! ===" -ForegroundColor Green
Write-Host "Installed location: $(Join-Path $InstallDir 'hydra-cli.exe')"
Write-Host "Run 'hydra-cli.exe --help' to verify."
