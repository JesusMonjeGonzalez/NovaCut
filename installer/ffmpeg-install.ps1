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

$source = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip'
$work = Join-Path $env:TEMP ('novacut-ffmpeg-' + [guid]::NewGuid())
New-Item $work -ItemType Directory -Force | Out-Null
New-Item $InstallDir -ItemType Directory -Force | Out-Null

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
    $zip = Join-Path $work 'ffmpeg.zip'
    Write-Host 'Descargando FFmpeg (~100 MB)...'
    Get-WithRetry $source $zip | Out-Null

    # Integridad: gyan.dev publica el SHA-256 de cada paquete. Detecta
    # descargas cortadas o corruptas antes de instalar nada.
    $published = (Get-WithRetry "$source.sha256" $null).Content
    if ($published -is [byte[]]) { $published = [Text.Encoding]::ASCII.GetString($published) }
    $expected = ($published.Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw 'La suma SHA-256 publicada no es valida' }
    if ($actual -ne $expected) { throw 'La descarga de FFmpeg esta corrupta (SHA-256 distinto); vuelve a intentarlo' }

    $unzip = Join-Path $work 'x'
    Expand-Archive -Path $zip -DestinationPath $unzip -Force
    $bin = Get-ChildItem $unzip -Recurse -Filter 'ffmpeg.exe' | Select-Object -First 1
    if (-not $bin) { throw 'El paquete descargado no contiene ffmpeg.exe' }

    foreach ($name in 'ffmpeg.exe', 'ffprobe.exe', 'ffplay.exe') {
        Copy-Item (Join-Path $bin.DirectoryName $name) $InstallDir -Force
    }
    # FFmpeg es GPL: su licencia viaja con los binarios.
    $license = Get-ChildItem $unzip -Recurse -Filter 'LICENSE*' | Select-Object -First 1
    if ($license) { Copy-Item $license.FullName (Join-Path $InstallDir 'FFmpeg-LICENSE.txt') -Force }

    # Se recoge la salida entera: cortar la tuberia (Select-Object -First)
    # mata el proceso y deja un codigo de salida de error aunque funcione.
    $version = & (Join-Path $InstallDir 'ffmpeg.exe') -hide_banner -version
    if ($LASTEXITCODE -ne 0) { throw 'FFmpeg se copio pero no arranca' }
    Write-Host ($version | Select-Object -First 1)
    Write-Host "FFmpeg instalado en $InstallDir"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
