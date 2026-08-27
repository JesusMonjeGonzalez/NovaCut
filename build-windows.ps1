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
        # Chocolatey instala NSIS fuera del PATH (sin shim), así que también
        # se buscan las ubicaciones típicas de instalación.
        $candidates = @(
            "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
            "$env:ProgramFiles\NSIS\makensis.exe",
            "$env:LOCALAPPDATA\Programs\NSIS\makensis.exe"
        )
        $found = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
        if ($found) {
            $makensis = Get-Item $found
        }
    }
    if (-not $makensis) {
        throw "NSIS is required to build the installer (install with: choco install nsis -y, or download from https://nsis.sourceforge.io)"
    }
    $nsisPath = if ($makensis -is [System.Management.Automation.CommandInfo]) {
        $makensis.Source
    } else {
        $makensis.FullName
    }
    & $nsisPath -WX "installer\NovaCut.nsi"
    if ($LASTEXITCODE -ne 0) {
        throw "NSIS failed with exit code $LASTEXITCODE"
    }
    Write-Host "NovaCut installer ready: build\installer\NovaCut-Windows-Setup.exe"
}
