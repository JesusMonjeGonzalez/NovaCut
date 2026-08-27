param(
    [string]$InstallDir
)

# Descarga el build "release essentials" de gyan.dev y copia los binarios de
# FFmpeg junto a la aplicacion. No depende de WinGet ni de la Store.

$ErrorActionPreference = 'Stop'

$zip = Join-Path $env:TEMP 'novacut-ffmpeg.zip'
$unzip = Join-Path $env:TEMP 'novacut-ffmpeg'
if (Test-Path $unzip) { Remove-Item $unzip -Recurse -Force }
if (Test-Path $zip) { Remove-Item $zip -Force }

Invoke-WebRequest -UseBasicParsing -Uri 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile $zip

Expand-Archive -Path $zip -DestinationPath $unzip -Force

$bin = Get-ChildItem $unzip -Recurse -Filter 'ffmpeg.exe' | Select-Object -First 1
if (-not $bin) { throw 'El paquete descargado no contiene ffmpeg.exe' }

Copy-Item (Join-Path $bin.DirectoryName '*') $InstallDir -Force

Remove-Item $unzip -Recurse -Force
Remove-Item $zip -Force