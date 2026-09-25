param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDir
)

# Descarga el build "release essentials" de gyan.dev y copia los binarios de
# FFmpeg junto a la aplicacion. No depende de WinGet ni de la Store.
#
# Lo usan el instalador y la propia app (boton "Instalar FFmpeg"), asi que
# tiene que funcionar en el PowerShell 5.1 que trae cualquier Windows 10/11.

$ErrorActionPreference = 'Stop'
# Con la barra de progreso, Invoke-WebRequest de PowerShell 5.1 es diez veces
# mas lento: 80 MB pasaban de segundos a minutos.
$ProgressPreference = 'SilentlyContinue'
# Windows 10 antiguos negocian TLS 1.0 por defecto y gyan.dev lo rechaza.
[Net.ServicePointManager]::SecurityProtocol = `
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$release = '9.0.2'
$source = "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-$release-essentials_build.zip"
# SHA256 upstream consultado el 2026-09-25:
# https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip.sha256
# No descargar el hash durante la instalacion: forma parte de la version revisada.
$expected = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
$work = Join-Path $env:TEMP ('novacut-ffmpeg-' + [guid]::NewGuid())
$backup = $null
$keepBackup = $false
$touched = @()
$originals = @{}

function Get-WithRetry([string]$Uri, [string]$OutFile) {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            if ($OutFile) {
                Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $OutFile
                return $null
            }
            return Invoke-WebRequest -UseBasicParsing -Uri $Uri
        } catch {
            if ($attempt -eq 3) { throw "No se pudo descargar $Uri : $($_.Exception.Message)" }
            Start-Sleep -Seconds (3 * $attempt)
        }
    }
}

try {
    New-Item $work -ItemType Directory | Out-Null
    $zip = Join-Path $work 'ffmpeg.zip'
    Write-Host 'Descargando FFmpeg (~100 MB)...'
    Get-WithRetry $source $zip | Out-Null

    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw 'La descarga de FFmpeg esta corrupta (SHA-256 distinto); vuelve a intentarlo' }

    $unzip = Join-Path $work 'x'
    Expand-Archive -Path $zip -DestinationPath $unzip -Force
    $root = Join-Path $unzip "ffmpeg-$release-essentials_build"
    $names = @('ffmpeg.exe', 'ffprobe.exe', 'ffplay.exe', 'FFmpeg-LICENSE.txt')
    $staged = @{}
    foreach ($name in $names) {
        $relative = if ($name -eq 'FFmpeg-LICENSE.txt') { 'LICENSE' } else { "bin/$name" }
        $path = Join-Path $root $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-Item -LiteralPath $path).Length -eq 0) {
            throw "El paquete descargado no contiene un archivo valido: $relative"
        }
        $staged[$name] = $path
    }
    foreach ($name in $names[0..2]) {
        $process = Start-Process -FilePath $staged[$name] -ArgumentList '-hide_banner', '-version' -Wait -PassThru -NoNewWindow
        if ($process.ExitCode -ne 0) { throw "$name no arranca en staging" }
    }

    New-Item $InstallDir -ItemType Directory -Force | Out-Null
    # En el destino, no en TEMP: una restauracion fallida debe ser recuperable.
    $backup = Join-Path $InstallDir ('ffmpeg-backup-' + [guid]::NewGuid())
    New-Item $backup -ItemType Directory | Out-Null
    foreach ($name in $names) {
        $destination = Join-Path $InstallDir $name
        $originals[$name] = Test-Path -LiteralPath $destination
        if ($originals[$name]) {
            if (-not (Test-Path -LiteralPath $destination -PathType Leaf)) {
                throw "El destino no es un archivo: $destination"
            }
            Copy-Item -LiteralPath $destination -Destination (Join-Path $backup $name) -Force
        }
    }
    foreach ($name in $names) {
        # Registrar ANTES: Copy-Item puede truncar el destino antes de fallar.
        $touched += $name
        Copy-Item -LiteralPath $staged[$name] -Destination (Join-Path $InstallDir $name) -Force
    }
    $process = Start-Process -FilePath (Join-Path $InstallDir 'ffmpeg.exe') -ArgumentList '-hide_banner', '-version' -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -ne 0) { throw 'FFmpeg se copio pero no arranca' }
    Write-Host "FFmpeg instalado en $InstallDir"
} catch {
    $failure = $_
    foreach ($name in $touched) {
        try {
            $destination = Join-Path $InstallDir $name
            if ($originals[$name]) {
                Copy-Item -LiteralPath (Join-Path $backup $name) -Destination $destination -Force
            } elseif (Test-Path -LiteralPath $destination) {
                Remove-Item -LiteralPath $destination -Force
            }
        } catch {
            $keepBackup = $true
            Write-Warning "No se pudo restaurar ${name}: $($_.Exception.Message)" -WarningAction Continue
        }
    }
    if ($keepBackup) {
        throw "Instalacion fallida: $($failure.Exception.Message). Rollback incompleto; backups conservados en: $backup"
    }
    throw $failure
} finally {
    if ($backup -and -not $keepBackup) {
        Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
