# Run with powershell.exe -NoProfile -File tests/whisper-install.ps1 (5.1).
# No network, downloads or native executables: every external step is mocked.
$ErrorActionPreference = 'Stop'
$installer = Join-Path (Split-Path -Parent $PSScriptRoot) 'installer/whisper-install.ps1'
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ('novacut-whisper-tests-' + [guid]::NewGuid())
$oldTemp = $env:TEMP
$oldSystemRoot = $env:SystemRoot
$oldTls = [Net.ServicePointManager]::SecurityProtocol
$zipUrl = 'https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip'
$modelBase = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/'
$redistUrl = 'https://aka.ms/vs/17/release/vc_redist.x64.exe'
$hashes = @{
    'whisper.zip' = '49dcc16de826f20bd53d44f947a1ae49dfa81f86cad67a64d80820cb192d674a'
    'ggml-small-q5_1.bin' = 'ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb'
    'ggml-base-q5_1.bin' = '422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898'
}
$cases = @('fresh', 'replace', 'rapida', 'hash-zip', 'hash-model', 'startup', 'runtime', 'runtime-unsigned')

try {
    New-Item $sandbox -ItemType Directory | Out-Null
    foreach ($scenario in $cases) {
        & {
            param($scenario)
            $caseDir = Join-Path $sandbox $scenario
            $destination = Join-Path $caseDir 'Whisper'
            $env:TEMP = Join-Path $caseDir 'temp'
            $env:SystemRoot = Join-Path $caseDir 'Windows'
            $system32 = Join-Path $env:SystemRoot 'System32'
            New-Item $env:TEMP, $system32 -ItemType Directory -Force | Out-Null
            if ($scenario -notlike 'runtime*') {
                foreach ($dll in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'vcomp140.dll') {
                    Set-Content -LiteralPath (Join-Path $system32 $dll) 'runtime'
                }
            }
            $existing = $scenario -in @('replace', 'hash-zip', 'hash-model', 'startup')
            if ($existing) {
                New-Item $destination -ItemType Directory | Out-Null
                Set-Content (Join-Path $destination 'ggml-base.bin') 'old model'
                Set-Content (Join-Path $destination 'whisper-cli.exe') 'old cli'
            }
            $state = @{ Downloads = @(); Elevated = 0; Runs = 0 }

            # Sin curl.exe: la ruta de PowerShell, acotada, se prueba aqui.
            function Get-Command {
                param($Name, $CommandType, $ErrorAction)
                if ($Name -ne 'curl.exe' -or $CommandType -ne 'Application') { throw 'Unexpected command lookup' }
            }
            function Invoke-WebRequest {
                param($Uri, $OutFile, [switch]$UseBasicParsing, $TimeoutSec)
                if (-not $TimeoutSec) { throw 'Unbounded PowerShell download' }
                $state.Downloads += $Uri
                if (-not $OutFile) { throw 'Download without destination' }
                if ($Uri -ne $zipUrl -and $Uri -ne $redistUrl -and -not $Uri.StartsWith($modelBase)) {
                    throw "Unexpected network request: $Uri"
                }
                Set-Content -LiteralPath $OutFile 'mock download'
            }
            function Start-Sleep { param($Seconds) throw 'Unexpected download retry' }
            function Get-FileHash {
                param($Path, $Algorithm)
                if ($Algorithm -ne 'SHA256') { throw 'Invalid hash request' }
                $name = Split-Path -Leaf $Path
                $hash = $hashes[$name]
                if (($scenario -eq 'hash-zip' -and $name -eq 'whisper.zip') -or
                    ($scenario -eq 'hash-model' -and $name -like 'ggml-*')) { $hash = '0' * 64 }
                [pscustomobject]@{ Hash = $hash.ToUpperInvariant() }
            }
            function Expand-Archive {
                param($Path, $DestinationPath, [switch]$Force)
                $release = Join-Path $DestinationPath 'Release'
                New-Item $release -ItemType Directory -Force | Out-Null
                foreach ($name in 'whisper-cli.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll',
                    'ggml-cpu-haswell.dll', 'ggml-cpu-x64.dll', 'whisper-server.exe', 'SDL2.dll') {
                    Set-Content -LiteralPath (Join-Path $release $name) "new-$name"
                }
            }
            function Get-AuthenticodeSignature {
                param($FilePath)
                $status = if ($scenario -eq 'runtime-unsigned') { 'HashMismatch' } else { 'Valid' }
                [pscustomobject]@{
                    Status = $status
                    SignerCertificate = [pscustomobject]@{ Subject = 'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond' }
                }
            }
            function Start-Process {
                param($FilePath, $ArgumentList, $Verb, [switch]$Wait, [switch]$PassThru, [switch]$NoNewWindow)
                if ((Split-Path -Leaf $FilePath) -eq 'vc_redist.x64.exe') {
                    if ($Verb -ne 'RunAs' -or ($ArgumentList -join ' ') -ne '/install /quiet /norestart') {
                        throw 'Invalid redistributable request'
                    }
                    $state.Elevated++
                    foreach ($dll in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'vcomp140.dll') {
                        Set-Content -LiteralPath (Join-Path $system32 $dll) 'runtime'
                    }
                    return [pscustomobject]@{ ExitCode = 3010 }
                }
                $state.Runs++
                if ((Split-Path -Leaf $FilePath) -ne 'whisper-cli.exe' -or ($ArgumentList -join ' ') -ne '-h') {
                    throw 'Invalid execution request'
                }
                if ((Split-Path -Parent $FilePath) -eq $destination) { throw 'Executed before staging' }
                $code = if ($scenario -eq 'startup') { 1 } else { 0 }
                [pscustomobject]@{ ExitCode = $code }
            }

            $model = if ($scenario -eq 'rapida') { 'rapida' } else { 'precisa' }
            $failure = $null
            try { & $installer -InstallDir $destination -Model $model } catch { $failure = $_ }
            $success = $scenario -in @('fresh', 'replace', 'rapida', 'runtime')
            if ($success -and $failure) { throw "${scenario}: $failure" }
            if (-not $success -and -not $failure) { throw "${scenario}: expected failure" }
            $expected = switch ($scenario) {
                'hash-zip' { '*whisper.cpp esta corrupta*' }
                'hash-model' { '*modelo de Whisper esta corrupta*' }
                'startup' { '*whisper-cli no arranca*' }
                'runtime-unsigned' { '*firma valida de Microsoft*' }
            }
            if (-not $success -and $failure.Exception.Message -notlike $expected) {
                throw "${scenario}: wrong failure: $failure"
            }
            if ($scenario -eq 'runtime-unsigned' -and ($state.Elevated -ne 0 -or $state.Downloads.Count -ne 1)) {
                throw 'Ran or downloaded more after an unsigned redistributable'
            }
            if ($scenario -eq 'runtime' -and $state.Elevated -ne 1) { throw 'Runtime was not installed' }
            if ($scenario -notlike 'runtime*' -and $state.Elevated -ne 0) { throw 'Installed the runtime needlessly' }
            if ($success) {
                $wanted = if ($model -eq 'rapida') { 'ggml-base-q5_1.bin' } else { 'ggml-small-q5_1.bin' }
                $installedNames = @(Get-ChildItem -LiteralPath $destination | ForEach-Object Name | Sort-Object)
                $expectedNames = @('ggml-base.dll', 'ggml-cpu-haswell.dll', 'ggml-cpu-x64.dll', 'ggml.dll',
                    $wanted, 'whisper-cli.exe', 'whisper.dll') | Sort-Object
                if (($installedNames -join ',') -ne ($expectedNames -join ',')) {
                    throw "${scenario}: installed $($installedNames -join ',')"
                }
                if ((Get-Content (Join-Path $destination 'whisper-cli.exe')) -ne 'new-whisper-cli.exe') {
                    throw "${scenario}: whisper-cli not replaced"
                }
            } elseif ($existing) {
                if ((Get-Content (Join-Path $destination 'ggml-base.bin')) -ne 'old model' -or
                    (Get-Content (Join-Path $destination 'whisper-cli.exe')) -ne 'old cli') {
                    throw "${scenario}: lost the previous installation"
                }
            } elseif (Test-Path -LiteralPath $destination) {
                throw "${scenario}: left a partial installation"
            }
            $leftovers = @(Get-ChildItem -LiteralPath $caseDir -Directory | Where-Object Name -like 'Whisper.*')
            if ($leftovers.Count -ne 0) { throw "${scenario}: left $($leftovers.Name -join ',')" }
            if (@(Get-ChildItem -LiteralPath $env:TEMP -Force).Count -ne 0) { throw "${scenario}: temp not cleaned" }
            Write-Host "PASS: $scenario"
        } $scenario
    }
} finally {
    $env:TEMP = $oldTemp
    $env:SystemRoot = $oldSystemRoot
    [Net.ServicePointManager]::SecurityProtocol = $oldTls
    if (Test-Path -LiteralPath $sandbox) { Remove-Item -LiteralPath $sandbox -Recurse -Force }
}
