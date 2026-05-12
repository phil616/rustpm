# build_msi.ps1
# Build rustpm MSI installer for Windows
#
# Prerequisites:
#   WiX Toolset v5: https://wixtoolset.org/
#   Rust: https://rustup.rs/
#
# Quick WiX setup:
#   Invoke-WebRequest "https://github.com/wixtoolset/wix/releases/download/v5.0.2/wix-cli-x64.msi" -OutFile "$env:TEMP\wix.msi"
#   msiexec /i "$env:TEMP\wix.msi" /quiet
#
# Usage:
#   .\scripts\build_msi.ps1              # Build release MSI
#   .\scripts\build_msi.ps1 -Debug       # Build debug MSI
#   .\scripts\build_msi.ps1 -SkipBuild   # Skip cargo build

param(
    [switch]$Debug,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Split-Path -Parent $PSScriptRoot)

# ---- Version ----
$version = (Get-Content "$RepoRoot\Cargo.toml" | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
if (-not $version) { $version = "0.1.0" }
Write-Host "Version: $version"

# ---- Build Rust binaries ----
if (-not $SkipBuild) {
    Write-Host "Building Rust binaries..."
    Push-Location $RepoRoot
    try {
        if ($Debug) {
            cargo build
        } else {
            cargo build --release
        }
    } finally {
        Pop-Location
    }
}
$TargetDir = if ($Debug) { "target\debug" } else { "target\release" }

if (-not (Test-Path "$RepoRoot\$TargetDir\rustpmd.exe")) {
    throw "rustpmd.exe not found. Build first or use -SkipBuild."
}

# ---- Locate WiX ----
$wixExe = $null
$searchPaths = @(
    "wix",
    "$env:ProgramFiles\WiX Toolset v5.0\bin",
    "${env:ProgramFiles(x86)}\WiX Toolset v5.0\bin",
    "$env:ProgramFiles\WiX Toolset v5\bin",
    "${env:ProgramFiles(x86)}\WiX Toolset v5\bin",
    "C:\wix\PFiles64\WiX Toolset v5.0\bin",
    "C:\wix\PFiles64\WiX Toolset v5\bin",
    "$env:LOCALAPPDATA\WiX"
)

foreach ($p in $searchPaths) {
    $test = Join-Path $p "wix.exe"
    if (Test-Path $test) {
        $wixExe = $test
        break
    }
}

if (-not $wixExe) {
    Write-Host "WiX Toolset v5 not found." -ForegroundColor Red
    Write-Host "Install from: https://github.com/wixtoolset/wix/releases" -ForegroundColor Red
    exit 1
}

Write-Host "WiX: $wixExe"

# ---- Output directory ----
$distDir = Join-Path $RepoRoot "dist"
New-Item -ItemType Directory -Path $distDir -Force | Out-Null

# ---- Build MSI ----
$msiFile = Join-Path $distDir "rustpm-$version-x64.msi"
Write-Host "Building MSI..."
Push-Location "$RepoRoot\wix"
try {
    & $wixExe build main.wxs -arch x64 -o $msiFile 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "wix build failed (exit code $LASTEXITCODE)"
    }
} finally {
    Pop-Location
}

# ---- Done ----
$msiSize = (Get-Item $msiFile).Length
Write-Host ""
Write-Host "==============================================" -ForegroundColor Green
Write-Host " MSI built successfully! ($($msiSize / 1KB) KB)" -ForegroundColor Green
Write-Host " Output: $msiFile" -ForegroundColor Green
Write-Host "==============================================" -ForegroundColor Green
Write-Host ""
Write-Host "Install:   msiexec /i `"$msiFile`"" -ForegroundColor Cyan
Write-Host "Quiet:     msiexec /i `"$msiFile`" /quiet" -ForegroundColor Cyan
Write-Host "Uninstall: msiexec /x `"$msiFile`"" -ForegroundColor Cyan