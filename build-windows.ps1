param([switch]$Installer)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root

cargo test --locked
cargo test --locked --features windows-host --bin novacut-windows
cargo build --release --locked --features windows-host --bin novacut-windows

$output = Join-Path $root "build\NovaCut-Windows"
if (Test-Path $output) {
    Remove-Item $output -Recurse -Force
}
New-Item $output -ItemType Directory | Out-Null

Copy-Item "target\release\novacut-windows.exe" $output
Copy-Item "target\release\editorcito.dll" $output -ErrorAction SilentlyContinue
Copy-Item "docs\GUIA-WINDOWS.md" (Join-Path $output "LEEME-WINDOWS.md")

Write-Host "NovaCut Windows ready: $output"

if ($Installer) {
    $makensis = Get-Command "makensis.exe" -ErrorAction SilentlyContinue
    if (-not $makensis) {
        throw "NSIS is required to build the installer"
    }
    & $makensis.Source -WX "installer\NovaCut.nsi"
    Write-Host "NovaCut installer ready: build\installer\NovaCut-Windows-Setup.exe"
}
