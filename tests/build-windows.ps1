$ErrorActionPreference = "Stop"
$source = Join-Path (Split-Path -Parent $PSScriptRoot) "build-windows.ps1"
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("novacut-build-test-" + [guid]::NewGuid())
$originalLocation = Get-Location
$originalExitCode = $global:LASTEXITCODE

try {
    # Each case runs in isolation, including a stale executable and package.
    foreach ($failAt in 1, 2, 3, 0) {
        $case = Join-Path $sandbox "case-$failAt"
        New-Item (Join-Path $case "target/release") -ItemType Directory -Force | Out-Null
        New-Item (Join-Path $case "docs") -ItemType Directory -Force | Out-Null
        New-Item (Join-Path $case "build/NovaCut-Windows") -ItemType Directory -Force | Out-Null
        Copy-Item $source (Join-Path $case "build-windows.ps1")
        Set-Content (Join-Path $case "target/release/novacut-windows.exe") "stale executable"
        Set-Content (Join-Path $case "docs/GUIA-WINDOWS.md") "test guide"
        $sentinel = Join-Path $case "build/NovaCut-Windows/previous-package.txt"
        Set-Content $sentinel "keep previous package"

        # Globales, no `$script:`: el simulador de cargo se ejecuta desde
        # build-windows.ps1 y ahí `$script:` apuntaría a otro ámbito, así que
        # el fallo simulado no llegaba a producirse nunca.
        $global:NovaCutTestCalls = 0
        $global:NovaCutTestFailAt = $failAt
        function cargo {
            $global:NovaCutTestCalls++
            $global:LASTEXITCODE = 0
            if ($global:NovaCutTestCalls -eq $global:NovaCutTestFailAt) {
                $global:LASTEXITCODE = 37
            } elseif ($global:NovaCutTestCalls -eq 3) {
                Set-Content "target/release/novacut-windows.exe" "fresh executable"
            }
        }

        $failure = $null
        try {
            & (Join-Path $case "build-windows.ps1")
        } catch {
            $failure = $_
        }
        if ($failAt -ne 0) {
            if (-not $failure -or $failure.Exception.Message -notlike "*failed with exit code 37") {
                throw "Case ${failAt}: expected the Cargo failure, got '$failure'"
            }
            if ($global:NovaCutTestCalls -ne $failAt -or -not (Test-Path $sentinel)) {
                throw "Case ${failAt}: continued after failure or replaced the previous package"
            }
            if (Test-Path (Join-Path $case "build/NovaCut-Windows/novacut-windows.exe")) {
                throw "Case ${failAt}: packaged a stale executable"
            }
        } else {
            if ($failure) { throw $failure }
            $packaged = Get-Content (Join-Path $case "build/NovaCut-Windows/novacut-windows.exe")
            if ($global:NovaCutTestCalls -ne 3 -or $packaged -ne "fresh executable" -or (Test-Path $sentinel)) {
                throw "Successful build did not replace the package with the fresh executable"
            }
            if (-not (Test-Path (Join-Path $case "build/NovaCut-Windows/LEEME-WINDOWS.md"))) {
                throw "Successful package is missing its guide"
            }
        }
        Write-Host "PASS: Cargo failure stage $failAt (0 = successful build)"
    }
} finally {
    Set-Location $originalLocation
    Remove-Item Function:\cargo -ErrorAction SilentlyContinue
    Remove-Variable NovaCutTestCalls, NovaCutTestFailAt -Scope Global -ErrorAction SilentlyContinue
    $global:LASTEXITCODE = $originalExitCode
    if (Test-Path $sandbox) { Remove-Item $sandbox -Recurse -Force }
}
