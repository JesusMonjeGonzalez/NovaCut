# Run with powershell.exe -NoProfile -File tests/ffmpeg-install.ps1 (5.1).
# No Pester, network, downloads or native executables required.
$ErrorActionPreference = 'Stop'
$installer = Join-Path (Split-Path -Parent $PSScriptRoot) 'installer/ffmpeg-install.ps1'
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ('novacut-ffmpeg-tests-' + [guid]::NewGuid())
$oldTemp = $env:TEMP
$oldTls = [Net.ServicePointManager]::SecurityProtocol
$names = @('ffmpeg.exe', 'ffprobe.exe', 'ffplay.exe', 'FFmpeg-LICENSE.txt')
$cases = @('success', 'fresh', 'hash', 'backup', 'rollback', 'installed', 'fresh-failure', 'mixed-failure')
$cases += 'download-failure', 'download-retry', 'curl-success', 'curl-failure', 'curl-retry', 'curl-progress', 'curl-multiple'
foreach ($name in $names) { $cases += "missing-$name", "empty-$name", "replace-$name" }
foreach ($name in $names[0..2]) { $cases += "startup-$name" }

try {
    New-Item $sandbox -ItemType Directory | Out-Null
    foreach ($scenario in $cases) {
        # Child scope isolates the command mocks from other tests/callers.
        & {
            param($scenario)
            $caseDir = Join-Path $sandbox $scenario
            $destinationDir = Join-Path $caseDir 'installed'
            $env:TEMP = Join-Path $caseDir 'temp'
            New-Item $destinationDir -ItemType Directory -Force | Out-Null
            New-Item $env:TEMP -ItemType Directory -Force | Out-Null
            $before = @{}
            foreach ($name in $names) {
                if ($scenario -in @('fresh', 'fresh-failure') -or
                    ($scenario -eq 'mixed-failure' -and $name -eq 'ffprobe.exe')) { continue }
                $before[$name] = "old-$name"
                Set-Content -LiteralPath (Join-Path $destinationDir $name) -Value $before[$name]
            }
            Set-Content (Join-Path $destinationDir 'unrelated.txt') 'keep me'
            $state = @{ Downloads = 0; Expanded = 0; Writes = 0; Runs = 0; Backups = 0; Retries = 0; Disposed = 0 }

            function Get-Command {
                param($Name, $CommandType, $ErrorAction)
                if ($Name -ne 'curl.exe' -or $CommandType -ne 'Application') { throw 'Unexpected command lookup' }
                if ($scenario -like 'curl-*') { [pscustomobject]@{ Source = 'mock-curl.exe' } }
                if ($scenario -eq 'curl-multiple') { [pscustomobject]@{ Source = 'another-curl.exe' } }
            }

            function Invoke-WebRequest {
                param($Uri, $OutFile, [switch]$UseBasicParsing, $TimeoutSec)
                if ($TimeoutSec -ne 120) { throw 'Unbounded PowerShell download' }
                if (Test-Path -LiteralPath $OutFile) { throw 'Partial download not removed before retry' }
                $state.Downloads++
                $expectedUri = if ($state.Downloads -eq 2) {
                    'https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip'
                } else {
                    'https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip'
                }
                if ($Uri -cne $expectedUri -or -not $OutFile) {
                    throw "Unexpected network request: $Uri"
                }
                Set-Content -LiteralPath $OutFile 'mock archive'
                if ($scenario -eq 'download-failure' -or ($scenario -eq 'download-retry' -and $state.Downloads -eq 1)) {
                    throw 'mock network timeout'
                }
            }
            function Start-Sleep {
                param($Seconds)
                if ($scenario -notin @('download-failure', 'download-retry', 'curl-failure', 'curl-retry')) {
                    throw 'Unexpected download retry'
                }
                $state.Retries++
            }
            function Get-FileHash {
                param($Path, $Algorithm)
                if ($Algorithm -ne 'SHA256' -or -not (Test-Path -LiteralPath $Path)) { throw 'Invalid hash request' }
                $hash = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
                if ($scenario -eq 'hash') { $hash = '0' * 64 }
                [pscustomobject]@{ Hash = $hash.ToUpperInvariant() }
            }
            function Expand-Archive {
                param($Path, $DestinationPath, [switch]$Force)
                $state.Expanded++
                $root = Join-Path $DestinationPath 'ffmpeg-9.0.2-essentials_build'
                New-Item (Join-Path $root 'bin') -ItemType Directory -Force | Out-Null
                foreach ($name in $names) {
                    if ($scenario -eq "missing-$name") { continue }
                    $relative = if ($name -eq 'FFmpeg-LICENSE.txt') { 'LICENSE' } else { "bin/$name" }
                    $value = if ($scenario -eq "empty-$name") { '' } else { "new-$name" }
                    Set-Content -LiteralPath (Join-Path $root $relative) -Value $value -NoNewline
                }
            }
            function Start-Process {
                param($FilePath, $ArgumentList, [switch]$Wait, [switch]$PassThru, [switch]$NoNewWindow)
                if ($FilePath -eq 'mock-curl.exe') {
                    $state.Downloads++
                    $expectedHost = if ($state.Downloads -eq 2) { 'www.gyan.dev' } else { 'github.com' }
                    if (([uri]$ArgumentList[-1].Trim('"')).Host -ne $expectedHost) { throw 'Download did not switch mirrors' }
                    foreach ($limit in @('--connect-timeout', '--max-time', '--speed-time', '--speed-limit')) {
                        $i = [Array]::IndexOf($ArgumentList, $limit)
                        if ($i -lt 0 -or [int]$ArgumentList[$i + 1] -le 0) { throw "Missing curl limit: $limit" }
                    }
                    if ($Wait -or -not $PassThru) { throw 'Download process cannot report progress' }
                    $out = $ArgumentList[[Array]::IndexOf($ArgumentList, '--output') + 1].Trim('"')
                    if (Test-Path -LiteralPath $out) { throw 'Partial curl download not removed' }
                    Set-Content -LiteralPath $out 'mock archive'
                    $exit = if ($scenario -eq 'curl-failure' -or ($scenario -eq 'curl-retry' -and $state.Downloads -eq 1)) { 28 } else { 0 }
                    $process = [pscustomobject]@{ ExitCode = $exit; HasExited = $false; Waits = 0 }
                    $process | Add-Member ScriptMethod WaitForExit {
                        param($Milliseconds)
                        $this.Waits++
                        if ($scenario -eq 'curl-progress' -and $this.Waits -eq 1) { return $false }
                        $this.HasExited = $true
                        return $true
                    }
                    $process | Add-Member ScriptMethod Dispose { $state.Disposed++ }
                    return $process
                }
                $state.Runs++
                if (-not (Test-Path -LiteralPath $FilePath) -or -not $Wait -or -not $PassThru -or
                    ($ArgumentList -join ' ') -ne '-hide_banner -version') { throw 'Invalid execution request' }
                $installed = (Split-Path -Parent $FilePath) -eq $destinationDir
                $exitCode = 0
                if (($scenario -eq 'installed' -and $installed) -or
                    ($scenario -eq ('startup-' + (Split-Path -Leaf $FilePath)))) { $exitCode = 17 }
                [pscustomobject]@{ ExitCode = $exitCode }
            }
            function Copy-Item {
                param($LiteralPath, $Destination, [switch]$Force)
                $name = Split-Path -Leaf $Destination
                $replacing = (Split-Path -Parent $Destination) -eq $destinationDir
                $restoring = (Split-Path -Leaf (Split-Path -Parent $LiteralPath)) -like 'ffmpeg-backup-*'
                if (-not $replacing) {
                    $state.Backups++
                    if ($scenario -eq 'backup' -and $name -eq 'ffprobe.exe') { throw 'mock backup failure' }
                } elseif ($restoring) {
                    if ($scenario -eq 'rollback' -and $name -eq 'ffmpeg.exe') { throw 'mock rollback failure' }
                } else {
                    $state.Writes++
                    if ($state.Runs -ne 3) { throw 'Replacement before staging execution checks' }
                    if ($scenario -eq "replace-$name" -or
                        ($scenario -in @('rollback', 'fresh-failure', 'mixed-failure') -and $name -eq 'ffplay.exe')) {
                        Set-Content -LiteralPath $Destination 'partial write'
                        throw 'mock replacement failure'
                    }
                }
                Microsoft.PowerShell.Management\Copy-Item -LiteralPath $LiteralPath -Destination $Destination -Force
            }

            $failure = $null
            try { & $installer -InstallDir $destinationDir } catch { $failure = $_ }
            $success = $scenario -in @('success', 'fresh', 'download-retry', 'curl-success', 'curl-retry', 'curl-progress', 'curl-multiple')
            if ($success -and $failure) { throw $failure }
            if (-not $success -and -not $failure) { throw "${scenario}: expected failure" }
            $expectedError = switch -Wildcard ($scenario) {
                'download-failure' { '*No se pudo descargar*mock network timeout*' }
                'curl-failure' { '*No se pudo descargar*curl 28*' }
                'hash' { '*SHA-256 distinto*' }
                'missing-*' { '*archivo valido*' }
                'empty-*' { '*archivo valido*' }
                'startup-*' { '*no arranca en staging*' }
                'backup' { '*mock backup failure*' }
                'rollback' { '*Rollback incompleto*backups conservados*' }
                'installed' { '*se copio pero no arranca*' }
                'fresh-failure' { '*mock replacement failure*' }
                'mixed-failure' { '*mock replacement failure*' }
                'replace-*' { '*mock replacement failure*' }
            }
            if (-not $success -and $failure.Exception.Message -notlike $expectedError) {
                throw "${scenario}: wrong failure: $failure"
            }
            $expectedDownloads = if ($scenario -in @('download-failure', 'curl-failure')) { 3 }
                elseif ($scenario -in @('download-retry', 'curl-retry')) { 2 } else { 1 }
            if ($state.Downloads -ne $expectedDownloads -or $state.Retries -ne ($expectedDownloads - 1)) {
                throw "${scenario}: unexpected downloads/retries"
            }
            if ($scenario -like 'curl-*' -and $state.Disposed -ne $state.Downloads) { throw 'Leaked download process' }
            if ($scenario -in @('download-failure', 'curl-failure') -and ($state.Expanded -ne 0 -or $state.Writes -ne 0)) {
                throw 'Installed a partial download'
            }
            if ($scenario -eq 'hash' -and $state.Expanded -ne 0) { throw 'Expanded untrusted archive' }
            if (($scenario -match '^(hash|missing-|empty-|startup-|backup)') -and $state.Writes -ne 0) {
                throw "${scenario}: wrote destination before validation/backup completed"
            }
            if ($scenario -match '^(missing-|empty-)' -and $state.Runs -ne 0) { throw 'Executed incomplete staging' }
            foreach ($name in $names) {
                $path = Join-Path $destinationDir $name
                if ($scenario -eq 'rollback' -and $name -eq 'ffmpeg.exe') { continue }
                if ($success) {
                    if ((Get-Content -LiteralPath $path -Raw) -ne "new-$name") { throw "${scenario}: incorrect installed $name" }
                } elseif ($before.ContainsKey($name)) {
                    if ((Get-Content -LiteralPath $path).Trim() -ne $before[$name]) { throw "${scenario}: lost original $name" }
                } elseif (Test-Path -LiteralPath $path) { throw "${scenario}: left new file $name" }
            }
            $backups = @(Get-ChildItem -LiteralPath $destinationDir -Directory -Filter 'ffmpeg-backup-*')
            if ($scenario -eq 'rollback') {
                if ($backups.Count -ne 1 -or -not $failure.Exception.Message.Contains($backups[0].FullName)) {
                    throw 'Missing recovery directory or diagnostic path'
                }
                foreach ($name in $names) {
                    if ((Get-Content -LiteralPath (Join-Path $backups[0].FullName $name)).Trim() -ne $before[$name]) {
                        throw "Lost recovery copy: $name"
                    }
                }
            } elseif ($backups.Count -ne 0) { throw "${scenario}: unnecessary backup retained" }
            if (@(Get-ChildItem -LiteralPath $env:TEMP -Force).Count -ne 0) { throw "${scenario}: staging not cleaned" }
            if ((Get-Content (Join-Path $destinationDir 'unrelated.txt')) -ne 'keep me') { throw 'Unrelated file changed' }
            Write-Host "PASS: $scenario"
        } $scenario
    }
} finally {
    $env:TEMP = $oldTemp
    [Net.ServicePointManager]::SecurityProtocol = $oldTls
    if (Test-Path -LiteralPath $sandbox) { Remove-Item -LiteralPath $sandbox -Recurse -Force }
}
