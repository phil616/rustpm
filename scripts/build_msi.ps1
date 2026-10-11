# build_msi.ps1
# Build rustpm MSI installer for Windows
#
# Prerequisites:
#   WiX Toolset v5: https://wixtoolset.org/
#   Rust: https://rustup.rs/
#
# Quick WiX setup:
#   dotnet tool install --global wix --version 5.0.2
#
# Usage:
#   .\scripts\build_msi.ps1              # Build release MSI
#   .\scripts\build_msi.ps1 -Debug       # Build debug MSI
#   .\scripts\build_msi.ps1 -SkipBuild   # Skip cargo build
#   .\scripts\build_msi.ps1 -Target x86_64-pc-windows-msvc # Explicit amd64 target

param(
    [switch]$Debug,
    [switch]$SkipBuild,
    [ValidateSet("x86_64-pc-windows-msvc")]
    [string]$Target
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Split-Path -Parent $PSScriptRoot)

# ---- Version ----
$version = (Get-Content "$RepoRoot\Cargo.toml" | Select-String '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Cargo.toml must contain a numeric major.minor.patch MSI version."
}
Write-Host "Version: $version"

# ---- Build Rust binaries ----
if (-not $SkipBuild) {
    Write-Host "Building Rust binaries..."
    Push-Location $RepoRoot
    try {
        $buildArgs = @("build", "--locked", "--bins")
        if (-not $Debug) { $buildArgs += "--release" }
        if ($Target) { $buildArgs += @("--target", $Target) }
        & cargo @buildArgs
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed (exit code $LASTEXITCODE)"
        }
    } finally {
        Pop-Location
    }
}
$buildProfile = if ($Debug) { "debug" } else { "release" }
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot "target" }
if (-not [System.IO.Path]::IsPathRooted($targetRoot)) {
    $targetRoot = Join-Path $RepoRoot $targetRoot
}
if ($Target) { $targetRoot = Join-Path $targetRoot $Target }
$TargetDir = Join-Path $targetRoot $buildProfile

foreach ($binary in @("rustpmd.exe", "rustpm-privd.exe", "rustpmctl.exe")) {
    if (-not (Test-Path (Join-Path $TargetDir $binary))) {
        throw "$binary not found in $TargetDir. Build first."
    }
}
$TargetDir = (Resolve-Path $TargetDir).Path

# ---- Locate WiX ----
$wixExe = (Get-Command wix.exe -ErrorAction SilentlyContinue).Source
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
    if ($wixExe) { break }
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

# Pin the extension to the tool's version so local and CI builds use matching
# custom actions. The WiX cache makes repeated adds inexpensive.
$wixVersionOutput = & $wixExe --version
if ($LASTEXITCODE -ne 0) { throw "Unable to determine WiX version" }
$wixVersion = ([string]$wixVersionOutput).Trim().Split('+')[0]
if ($wixVersion -notmatch '^5\.\d+\.\d+$') {
    throw "WiX Toolset v5 is required; found '$wixVersion'."
}
& $wixExe extension add -g "WixToolset.Util.wixext/$wixVersion"
if ($LASTEXITCODE -ne 0) { throw "WiX Util extension installation failed" }

# ---- Output directory ----
$distDir = Join-Path $RepoRoot "dist"
New-Item -ItemType Directory -Path $distDir -Force | Out-Null

# ---- Build MSI ----
$msiFile = Join-Path $distDir "rustpm-$version-x64.msi"
Write-Host "Building MSI..."
Push-Location "$RepoRoot\wix"
try {
    & $wixExe build main.wxs -arch x64 -ext WixToolset.Util.wixext -d "ProductVersion=$version" -d "BinaryDir=$TargetDir" -o $msiFile 2>&1
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
