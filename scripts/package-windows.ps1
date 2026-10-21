<#
.SYNOPSIS
    Builds Nori for Windows and packages the NSIS standalone installer.
.DESCRIPTION
    1. Builds the `nori-cli` release binary (the background indexing & search helper).
    2. Stages `nori.exe` into `crates/nori-gui/helper/nori.exe`.
    3. Runs Tauri 2 build to create the NSIS installer for Windows x64.
    The resulting installer installs to %LOCALAPPDATA%\Programs\Nori (single user, no UAC admin required).
#>

[CmdletBinding()]
param (
    [switch]$SkipGuiBuild
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir

Write-Host "==> Packaging Nori for Windows (x64)" -ForegroundColor Cyan
Set-Location $RootDir

# 1. Build nori CLI helper
Write-Host "==> Step 1/3: Building nori CLI release binary..." -ForegroundColor Yellow
cargo build --release -p nori-cli
$CliExe = Join-Path $RootDir "target\release\nori.exe"
if (-not (Test-Path $CliExe)) {
    throw "nori.exe not found at $CliExe"
}
Write-Host "--> CLI helper built: $CliExe" -ForegroundColor Green

# 2. Stage helper into crates/nori-gui/helper
Write-Host "==> Step 2/3: Staging helper into crates/nori-gui/helper/..." -ForegroundColor Yellow
$HelperDir = Join-Path $RootDir "crates\nori-gui\helper"
if (-not (Test-Path $HelperDir)) {
    New-Item -ItemType Directory -Path $HelperDir -Force | Out-Null
}
Copy-Item -Path $CliExe -Destination (Join-Path $HelperDir "nori.exe") -Force
Write-Host "--> Staged: $(Join-Path $HelperDir "nori.exe")" -ForegroundColor Green

if ($SkipGuiBuild) {
    Write-Host "Skipping GUI build as requested." -ForegroundColor Yellow
    return
}

# 3. Build GUI and NSIS bundle
Write-Host "==> Step 3/3: Building Nori GUI and packaging NSIS installer..." -ForegroundColor Yellow
Set-Location (Join-Path $RootDir "crates\nori-gui")

# Check if cargo-tauri or npx tauri is available
$tauriCmd = Get-Command "cargo-tauri" -ErrorAction SilentlyContinue
if ($tauriCmd) {
    cargo tauri build --bundles nsis --config '{"bundle":{"resources":{"helper/":"helper/"}}}'
} else {
    npx --yes @tauri-apps/cli@2.12 build --bundles nsis --config '{"bundle":{"resources":{"helper/":"helper/"}}}'
}

$BundleDir = Join-Path $RootDir "target\release\bundle\nsis"
Write-Host ""
Write-Host "==> Build complete!" -ForegroundColor Green
Write-Host "Installer package: $BundleDir" -ForegroundColor Cyan
