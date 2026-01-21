# Build CLI sidecar for Tauri bundling
# This script builds the dybur CLI and copies it to the tray app's binaries folder

$ErrorActionPreference = "Stop"

# Get script directory and project root
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$CliDir = Join-Path $ProjectRoot "apps\cli"
$BinariesDir = Join-Path $ProjectRoot "apps\tray\src-tauri\binaries"

# Detect target triple
$Target = rustc -vV | Select-String "host:" | ForEach-Object { $_.Line.Split(":")[1].Trim() }
Write-Host "Building for target: $Target"

# Build CLI in release mode
Write-Host "Building dybur CLI..."
Push-Location $CliDir
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed"
    }
} finally {
    Pop-Location
}

# Create binaries directory if it doesn't exist
if (-not (Test-Path $BinariesDir)) {
    New-Item -ItemType Directory -Path $BinariesDir -Force | Out-Null
}

# Copy binary with target-specific name
$SourceBinary = Join-Path $CliDir "target\release\dybur.exe"
$DestBinary = Join-Path $BinariesDir "dybur-$Target.exe"

Write-Host "Copying $SourceBinary to $DestBinary"
Copy-Item -Path $SourceBinary -Destination $DestBinary -Force

Write-Host "Sidecar built successfully!"
Write-Host "Binary location: $DestBinary"
