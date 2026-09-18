# PowerShell build script for MySQL TurboLoad
$ErrorActionPreference = "Stop"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host " Building MySQL TurboLoad Enterprise (Release)" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Error "Build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

if (-not (Test-Path "bin")) {
    New-Item -ItemType Directory -Path "bin" | Out-Null
}

$source = "target\release\mysql-turboload.exe"
$dest = "bin\mysql-turboload.exe"

if (Test-Path $source) {
    Copy-Item -Path $source -Destination $dest -Force
    $fileInfo = Get-Item $dest
    $sizeMb = [math]::Round($fileInfo.Length / 1MB, 2)
    Write-Host ""
    Write-Host "[SUCCESS] Release binary built and copied to $dest ($sizeMb MB)" -ForegroundColor Green
} else {
    Write-Error "Release binary not found at $source"
    exit 1
}
