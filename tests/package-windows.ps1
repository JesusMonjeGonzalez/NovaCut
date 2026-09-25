param(
    # Only on a disposable GitHub Actions Windows VM: HKCU and shortcuts change.
    [switch]$DisposableVM,
    [string]$Installer = (Join-Path $PSScriptRoot "..\build\installer\NovaCut-Windows-Setup.exe"),
    [string]$Portable = (Join-Path $PSScriptRoot "..\build\NovaCut-Windows")
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -cne "true" -or $env:RUNNER_OS -cne "Windows" -or
    [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not $DisposableVM) {
    throw "REFUSED: requires GITHUB_ACTIONS=true, RUNNER_OS=Windows and -DisposableVM on a disposable VM."
}

# Do not overwrite an existing user's installation, associations or shortcuts.
foreach ($path in @(
    "HKCU:\Software\NovaCut",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut",
    "HKCU:\Software\Classes\.ncrough",
    "HKCU:\Software\Classes\NovaCut.RoughProject",
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'NovaCut.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'NovaCut'),
    (Join-Path $env:LOCALAPPDATA 'Programs\NovaCut')
)) {
    if (Test-Path -LiteralPath $path) { throw "REFUSED: existing NovaCut state: $path" }
}
if (Get-Process -Name 'novacut-windows' -ErrorAction SilentlyContinue) {
    throw "REFUSED: NovaCut is already running."
}
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$root = Split-Path -Parent $PSScriptRoot
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ("novacut-package-" + [guid]::NewGuid())
$installDir = Join-Path $sandbox "Installed NovaCut"
$uninstaller = Join-Path $installDir 'Desinstalar-NovaCut.exe'
$uninstalled = $false

function Invoke-BoundedProcess([string]$File, [string]$Arguments, [switch]$Smoke) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WorkingDirectory (Split-Path -Parent $File)
    try {
        if ($Smoke) {
            if ($process.WaitForExit(10000)) {
                throw "Application exited before 10 seconds: $File (exit $($process.ExitCode))"
            }
            $process.Refresh()
            if ($process.HasExited) { throw "Application is not alive: $File" }
        } else {
            if (-not $process.WaitForExit(120000)) { throw "Timed out: $File" }
            if ($process.ExitCode -ne 0) { throw "Exit $($process.ExitCode): $File" }
        }
    } finally {
        # Kill only the process we started, never all processes with its name.
        if (-not $process.HasExited) {
            $process.Kill()
            if (-not $process.WaitForExit(5000)) { throw "Could not stop PID $($process.Id)" }
        }
        $process.Dispose()
    }
}

function Assert-Package([string]$Directory) {
    foreach ($name in 'novacut-windows.exe', 'LEEME-WINDOWS.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_LICENSES-Windows.html') {
        $file = Join-Path $Directory $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -eq 0) {
            throw "Missing or empty package file: $file"
        }
    }
    foreach ($name in 'LICENSE', 'THIRD_PARTY_NOTICES.md') {
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $name)).Hash -ne
            (Get-FileHash -LiteralPath (Join-Path $root $name)).Hash) {
            throw "Stale package notice: $name"
        }
    }
    if ((Get-FileHash -LiteralPath (Join-Path $Directory 'THIRD_PARTY_LICENSES-Windows.html')).Hash -ne
        (Get-FileHash -LiteralPath (Join-Path $root 'docs/licenses/THIRD_PARTY_LICENSES-Windows.html')).Hash) {
        throw 'Stale package license report: THIRD_PARTY_LICENSES-Windows.html'
    }
}

New-Item $sandbox -ItemType Directory | Out-Null
try {
    # NSIS requires /D last, with no quotes even when the directory has spaces.
    Invoke-BoundedProcess $Installer "/S /NOFFMPEG /D=$installDir"
    Assert-Package $installDir
    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw 'Missing uninstaller' }
    foreach ($name in 'ffmpeg.exe', 'ffprobe.exe', 'ffplay.exe', 'FFmpeg-LICENSE.txt') {
        if (Test-Path -LiteralPath (Join-Path $installDir $name)) { throw "/NOFFMPEG installed $name" }
    }
    Invoke-BoundedProcess (Join-Path $installDir 'novacut-windows.exe') ' ' -Smoke

    $fixture = Join-Path $installDir 'user-project.ncrough'
    Set-Content -LiteralPath $fixture -Value '{"version":2,"name":"User project smoke fixture","clips":[]}' -Encoding ASCII
    $fixtureHash = (Get-FileHash -LiteralPath $fixture).Hash
    # _?= keeps NSIS in this process so exit/timeout is observable; it must be last.
    Invoke-BoundedProcess $uninstaller "/S _?=$installDir"
    $uninstalled = $true
    if (Test-Path -LiteralPath (Join-Path $installDir 'novacut-windows.exe')) { throw 'Uninstall left the executable' }
    if (-not (Test-Path -LiteralPath $fixture -PathType Leaf) -or
        (Get-FileHash -LiteralPath $fixture).Hash -ne $fixtureHash) {
        throw 'Uninstall removed or changed the user project'
    }
    Write-Host 'PASS: silent install without FFmpeg, 10s process smoke, uninstall preserves user project'

    if (Test-Path -LiteralPath $Portable -PathType Container) {
        $portableDir = Join-Path $sandbox 'Portable NovaCut'
        Copy-Item -LiteralPath $Portable -Destination $portableDir -Recurse
        Assert-Package $portableDir
        Invoke-BoundedProcess (Join-Path $portableDir 'novacut-windows.exe') ' ' -Smoke
        Write-Host 'PASS: portable package and 10s process smoke'
    } else {
        Write-Host "SKIP: portable directory not found: $Portable"
    }
} finally {
    try {
        if (-not $uninstalled -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
            Invoke-BoundedProcess $uninstaller "/S _?=$installDir"
        }
    } finally {
        # Only the uniquely created sandbox; never remove a general install/temp path.
        Remove-Item -LiteralPath $sandbox -Recurse -Force
    }
}
