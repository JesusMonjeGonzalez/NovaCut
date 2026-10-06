param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,
    [string]$LogPath = ''
)

# Descarga el build "release essentials" de gyan.dev y copia los binarios de
# FFmpeg junto a la aplicacion. No depende de WinGet ni de la Store.
#
# Lo usan el instalador y la propia app (boton "Instalar FFmpeg"), asi que
# tiene que funcionar en el PowerShell 5.1 que trae cualquier Windows 10/11.

$ErrorActionPreference = 'Stop'
# Si PowerShell 5.1 arranca desde pwsh 7 (o desde algo que lo hizo), hereda
# su PSModulePath, carga los modulos de la 7 y Get-FileHash, Expand-Archive o
# Get-AuthenticodeSignature dejan de existir. Usar solo los de Windows.
$env:PSModulePath = "$PSHOME\Modules;$env:ProgramFiles\WindowsPowerShell\Modules"
# Con la barra de progreso, Invoke-WebRequest de PowerShell 5.1 es diez veces
# mas lento: 80 MB pasaban de segundos a minutos.
$ProgressPreference = 'SilentlyContinue'
# Windows 10 antiguos negocian TLS 1.0 por defecto y gyan.dev lo rechaza.
[Net.ServicePointManager]::SecurityProtocol = `
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$release = '9.0.2'
$source = "https://github.com/GyanD/codexffmpeg/releases/download/$release/ffmpeg-$release-essentials_build.zip"
$fallbackSource = "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-$release-essentials_build.zip"
# SHA256 upstream consultado el 2026-09-25:
# https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip.sha256
# No descargar el hash durante la instalacion: forma parte de la version revisada.
$expected = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
$work = Join-Path $env:TEMP ('novacut-ffmpeg-' + [guid]::NewGuid())
$backup = $null
$keepBackup = $false
$touched = @()
$originals = @{}
$transcribing = $false

function Get-WithRetry([string[]]$Sources, [string]$OutFile) {
    # curl.exe viene con Windows 10 1803+ y Windows 11. Evita la descarga
    # lenta de Invoke-WebRequest 5.1 y limita conexion, inactividad y total.
    # Puede haber varios curl.exe (Windows, Git, etc.) en PATH. Start-Process
    # necesita una ruta, no el array que devuelve Get-Command en ese caso.
    $curl = Get-Command curl.exe -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        $Uri = $Sources[($attempt - 1) % $Sources.Count]
        try {
            Write-Host "Descargando FFmpeg, intento $attempt de 3 (limite de 5 minutos)..."
            if (Test-Path -LiteralPath $OutFile) {
                Remove-Item -LiteralPath $OutFile -Force
            }
            if ($curl) {
                $arguments = @('--fail', '--location', '--silent', '--show-error',
                    '--connect-timeout', '20', '--max-time', '300',
                    '--speed-time', '30', '--speed-limit', '1024',
                    '--output', ('"{0}"' -f $OutFile), ('"{0}"' -f $Uri))
                $download = Start-Process -FilePath $curl.Source -ArgumentList $arguments -NoNewWindow -PassThru
                $clock = [Diagnostics.Stopwatch]::StartNew()
                try {
                    # En PowerShell 5.1, conservar el handle antes de esperar:
                    # sin el, ExitCode puede ser null aunque curl haya terminado
                    # correctamente (Start-Process sin -Wait).
                    $null = $download.Handle
                    while (-not $download.WaitForExit(2000)) {
                        $size = if (Test-Path -LiteralPath $OutFile) {
                            (Get-Item -LiteralPath $OutFile).Length / 1MB
                        } else { 0 }
                        Write-Host ('FFmpeg: {0:N1} MB descargados ({1:N0} s)' -f $size, $clock.Elapsed.TotalSeconds)
                        if ($clock.Elapsed.TotalSeconds -gt 330) {
                            throw 'La descarga ha superado el tiempo limite'
                        }
                    }
                    if ($download.ExitCode -ne 0) {
                        throw "La descarga fallo (curl $($download.ExitCode)); revisa la conexion o el proxy"
                    }
                } finally {
                    if (-not $download.HasExited) { $download.Kill(); $download.WaitForExit() }
                    $download.Dispose()
                }
            } else {
                Write-Host 'Usando PowerShell (Windows sin curl.exe), limite de 120 segundos...'
                Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $OutFile -TimeoutSec 120
            }
            return
        } catch {
            if ($attempt -eq 3) { throw "No se pudo descargar $Uri : $($_.Exception.Message)" }
            Write-Host "Descarga interrumpida: $($_.Exception.Message). Reintentando..."
            Start-Sleep -Seconds (3 * $attempt)
        }
    }
}

try {
    if ($LogPath) {
        Start-Transcript -Path $LogPath -Force | Out-Null
        $transcribing = $true
    }
    New-Item $work -ItemType Directory | Out-Null
    $zip = Join-Path $work 'ffmpeg.zip'
    Write-Host 'Descargando FFmpeg (~115 MB). El progreso aparece en los detalles.'
    Get-WithRetry -Sources @($source, $fallbackSource) -OutFile $zip | Out-Null

    Write-Host 'Verificando SHA-256 de FFmpeg...'
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw 'La descarga de FFmpeg esta corrupta (SHA-256 distinto); vuelve a intentarlo' }

    $unzip = Join-Path $work 'x'
    Write-Host 'Descomprimiendo FFmpeg. Puede tardar unos minutos...'
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
    Write-Host 'Comprobando los ejecutables de FFmpeg...'
    foreach ($name in $names[0..2]) {
        $process = Start-Process -FilePath $staged[$name] -ArgumentList '-hide_banner', '-version' -Wait -PassThru -NoNewWindow
        if ($process.ExitCode -ne 0) { throw "$name no arranca en staging" }
    }

    Write-Host 'Copiando FFmpeg a la carpeta de NovaCut...'
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
    Write-Host "Error al instalar FFmpeg: $($failure.Exception.Message)"
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
    if ($transcribing) { Stop-Transcript | Out-Null }
}
