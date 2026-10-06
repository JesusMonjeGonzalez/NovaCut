param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,
    [ValidateSet('precisa', 'rapida')]
    [string]$Model = 'precisa'
)

# Instala la transcripcion local (whisper.cpp) para la edicion por texto y los
# subtitulos automaticos: el ejecutable oficial de whisper.cpp y un modelo,
# ambos con version y SHA-256 fijados, en una carpeta propia.
#
# Lo usan el instalador y la propia app (boton "Instalar transcripcion"), asi
# que tiene que funcionar en el PowerShell 5.1 de cualquier Windows 10/11.
# Solo ASCII: PowerShell 5.1 lee los scripts sin BOM como ANSI.

$ErrorActionPreference = 'Stop'
# Si PowerShell 5.1 arranca desde pwsh 7 (o desde algo que lo hizo), hereda
# su PSModulePath, carga los modulos de la 7 y Get-FileHash, Expand-Archive o
# Get-AuthenticodeSignature dejan de existir. Usar solo los de Windows.
$env:PSModulePath = "$PSHOME\Modules;$env:ProgramFiles\WindowsPowerShell\Modules"
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = `
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$release = 'v1.9.2'
$binaries = "https://github.com/ggml-org/whisper.cpp/releases/download/$release/whisper-bin-x64.zip"
# Digest publicado por GitHub para ese asset, consultado el 2026-09-28.
$binariesHash = '49dcc16de826f20bd53d44f947a1ae49dfa81f86cad67a64d80820cb192d674a'
# Commit fijo del repositorio de modelos: la URL no puede cambiar por debajo.
$modelCommit = '5359861c739e955e79d9a303bcbc70fb988958b1'
$models = @{
    precisa = @{ Name = 'ggml-small-q5_1.bin'; Hash = 'ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb'; Size = '190 MB' }
    rapida  = @{ Name = 'ggml-base-q5_1.bin'; Hash = '422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898'; Size = '60 MB' }
}
$chosen = $models[$Model]
$modelUrl = "https://huggingface.co/ggerganov/whisper.cpp/resolve/$modelCommit/$($chosen.Name)"
# whisper.cpp se compila con MSVC: necesita el runtime de Visual C++.
$redist = 'https://aka.ms/vs/17/release/vc_redist.x64.exe'
$runtime = @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'vcomp140.dll')

$work = Join-Path $env:TEMP ('novacut-whisper-' + [guid]::NewGuid())
$staging = $null
$backup = $null
$installed = $false

function Get-WithRetry([string]$Uri, [string]$OutFile) {
    # Como en ffmpeg-install.ps1: curl.exe (Windows 10 1803+) con limites de
    # conexion, inactividad y total, y progreso visible. Sin limites, una
    # conexion atascada dejaba el instalador "descargando" para siempre.
    $curl = Get-Command curl.exe -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    $name = Split-Path -Leaf $OutFile
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Write-Host "Descargando $name, intento $attempt de 3 (limite de 10 minutos)..."
            if (Test-Path -LiteralPath $OutFile) {
                Remove-Item -LiteralPath $OutFile -Force
            }
            if ($curl) {
                $arguments = @('--fail', '--location', '--silent', '--show-error',
                    '--connect-timeout', '20', '--max-time', '600',
                    '--speed-time', '30', '--speed-limit', '1024',
                    '--output', ('"{0}"' -f $OutFile), ('"{0}"' -f $Uri))
                $download = Start-Process -FilePath $curl.Source -ArgumentList $arguments -NoNewWindow -PassThru
                $clock = [Diagnostics.Stopwatch]::StartNew()
                try {
                    $null = $download.Handle
                    while (-not $download.WaitForExit(2000)) {
                        $size = if (Test-Path -LiteralPath $OutFile) {
                            (Get-Item -LiteralPath $OutFile).Length / 1MB
                        } else { 0 }
                        Write-Host ('{0}: {1:N1} MB descargados ({2:N0} s)' -f $name, $size, $clock.Elapsed.TotalSeconds)
                        if ($clock.Elapsed.TotalSeconds -gt 630) {
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
                Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $OutFile -TimeoutSec 300
            }
            return
        } catch {
            if ($attempt -eq 3) { throw "No se pudo descargar $Uri : $($_.Exception.Message)" }
            Write-Host "Descarga interrumpida: $($_.Exception.Message). Reintentando..."
            Start-Sleep -Seconds (3 * $attempt)
        }
    }
}

function Test-Sha256([string]$Path, [string]$Expected, [string]$What) {
    $actual = (Get-FileHash $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $Expected) { throw "La descarga de $What esta corrupta (SHA-256 distinto); vuelve a intentarlo" }
}

function Test-VcRuntime {
    $system = Join-Path $env:SystemRoot 'System32'
    foreach ($dll in $runtime) {
        if (-not (Test-Path -LiteralPath (Join-Path $system $dll) -PathType Leaf)) { return $false }
    }
    return $true
}

try {
    New-Item $work -ItemType Directory | Out-Null

    if (-not (Test-VcRuntime)) {
        Write-Host 'Instalando el runtime de Visual C++ de Microsoft...'
        $exe = Join-Path $work 'vc_redist.x64.exe'
        Get-WithRetry $redist $exe
        # Microsoft publica versiones nuevas en la misma URL: en vez de una
        # huella fija se exige su firma Authenticode.
        $signature = Get-AuthenticodeSignature -FilePath $exe
        if ($signature.Status -ne 'Valid' -or
            $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
            throw 'El instalador de Visual C++ descargado no lleva una firma valida de Microsoft'
        }
        $process = Start-Process -FilePath $exe -ArgumentList '/install', '/quiet', '/norestart' -Verb RunAs -Wait -PassThru
        if ($process.ExitCode -notin @(0, 1638, 3010)) {
            throw "El runtime de Visual C++ no se instalo (codigo $($process.ExitCode))"
        }
        if (-not (Test-VcRuntime)) { throw 'El runtime de Visual C++ sigue sin estar disponible' }
    }

    Write-Host "Descargando whisper.cpp $release..."
    $zip = Join-Path $work 'whisper.zip'
    Get-WithRetry $binaries $zip
    Test-Sha256 $zip $binariesHash 'whisper.cpp'
    $unzip = Join-Path $work 'x'
    Expand-Archive -Path $zip -DestinationPath $unzip -Force
    $root = Join-Path $unzip 'Release'
    $files = @('whisper-cli.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll')
    foreach ($name in $files) {
        $path = Join-Path $root $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -eq 0) {
            throw "El paquete de whisper.cpp no contiene un archivo valido: $name"
        }
    }
    # Un backend de CPU por familia de procesador; ggml carga el mejor.
    $backends = @(Get-ChildItem -LiteralPath $root -Filter 'ggml-cpu-*.dll' | Where-Object { $_.Length -gt 0 })
    if ($backends.Count -eq 0) { throw 'El paquete de whisper.cpp no trae ningun backend de CPU' }

    Write-Host "Descargando el modelo $($chosen.Name) ($($chosen.Size))..."
    $modelFile = Join-Path $work $chosen.Name
    Get-WithRetry $modelUrl $modelFile
    Test-Sha256 $modelFile $chosen.Hash 'el modelo de Whisper'

    # Se prepara al lado del destino y se sustituye la carpeta entera: nunca
    # queda un Whisper a medias ni dos modelos compitiendo.
    $parent = Split-Path -Parent $InstallDir
    New-Item $parent -ItemType Directory -Force | Out-Null
    $staging = "$InstallDir.nuevo-" + [guid]::NewGuid()
    New-Item $staging -ItemType Directory | Out-Null
    foreach ($name in $files) { Copy-Item -LiteralPath (Join-Path $root $name) -Destination $staging }
    foreach ($backend in $backends) { Copy-Item -LiteralPath $backend.FullName -Destination $staging }
    Copy-Item -LiteralPath $modelFile -Destination $staging
    $process = Start-Process -FilePath (Join-Path $staging 'whisper-cli.exe') -ArgumentList '-h' -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -ne 0) { throw "whisper-cli no arranca (codigo $($process.ExitCode))" }

    if (Test-Path -LiteralPath $InstallDir) {
        $backup = "$InstallDir.anterior-" + [guid]::NewGuid()
        Move-Item -LiteralPath $InstallDir -Destination $backup
    }
    Move-Item -LiteralPath $staging -Destination $InstallDir
    $staging = $null
    $installed = $true
    Write-Host "Transcripcion instalada en $InstallDir"
} catch {
    $failure = $_
    if ($backup -and -not (Test-Path -LiteralPath $InstallDir) -and (Test-Path -LiteralPath $backup)) {
        try {
            Move-Item -LiteralPath $backup -Destination $InstallDir
            $backup = $null
        } catch {
            throw "Instalacion fallida: $($failure.Exception.Message). La instalacion anterior quedo en: $backup"
        }
    }
    throw $failure
} finally {
    if ($staging -and (Test-Path -LiteralPath $staging)) {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($installed -and $backup) {
        Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
