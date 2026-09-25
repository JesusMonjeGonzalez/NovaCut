# Instalar NovaCut

Descarga siempre desde la página de **Releases** del repositorio:
<https://github.com/JesusMonjeGonzalez/NovaCut/releases>

NovaCut es una alpha de ingeniería: guarda copia de tus originales.

## Windows 10 / 11 (64 bits)

**Requisitos:** Windows 10 u 11 de 64 bits, un controlador de tarjeta gráfica
actualizado (OpenGL 2.0 o superior; cualquier Intel, AMD o NVIDIA de los
últimos diez años) y conexión a internet la primera vez, para descargar FFmpeg
(unos 100 MB).

1. Descarga **`NovaCut-Windows-Setup.exe`**.
2. Ábrelo. Windows mostrará **«Windows protegió tu PC»**: el instalador no está
   firmado con un certificado de pago, no porque tenga nada raro. Pulsa
   **«Más información» → «Ejecutar de todas formas»**.
3. Sigue el asistente. No pide permisos de administrador: se instala solo para
   tu usuario. Deja marcada la casilla **«Motor multimedia FFmpeg»**: la
   descarga, comprueba su suma SHA-256 y la deja junto a NovaCut.
4. Al terminar, NovaCut se abre. También lo tienes en el menú Inicio y, si lo
   marcaste, en el escritorio. Los proyectos `.ncrough` se abren con doble clic.

Si FFmpeg no se pudo descargar (sin conexión, proxy de empresa…), NovaCut se
instala igualmente y, al abrirlo, ofrece un botón **«Instalar FFmpeg
automáticamente»**. También puedes copiar a mano `ffmpeg.exe`, `ffprobe.exe` y
`ffplay.exe` (de <https://www.gyan.dev/ffmpeg/builds/>) junto a
`novacut-windows.exe`.

**Versión portable:** descomprime `NovaCut-Windows-x64.zip` donde quieras y
abre `novacut-windows.exe`. No toca el registro; la primera vez te ofrecerá
instalar FFmpeg en esa misma carpeta.

**Desinstalar:** Configuración → Aplicaciones → NovaCut → Desinstalar. Tus
proyectos no se borran.

**Si la ventana no se abre:** NovaCut te dirá que falta OpenGL. Actualiza el
controlador de la tarjeta gráfica desde la web del fabricante o Windows Update;
en máquinas virtuales o escritorio remoto, activa la aceleración 3D.

### Transcripción con Whisper (opcional)

La edición por texto y los subtítulos automáticos usan Whisper en tu equipo.
No viene incluido por el tamaño de los modelos:

1. Descarga `whisper-bin-x64.zip` de
   <https://github.com/ggml-org/whisper.cpp/releases> y copia `whisper-cli.exe`
   (y sus `.dll`) en `%LOCALAPPDATA%\NovaCut\Whisper`.
2. Descarga un modelo, por ejemplo `ggml-base.bin` (148 MB), de
   <https://huggingface.co/ggerganov/whisper.cpp> y déjalo en la misma carpeta.
   Para español conviene al menos `base`; `small` acierta más y es más lento.

## macOS 14 o superior (Apple Silicon e Intel)

1. Descarga **`NovaCut-macOS.zip`** y descomprímelo.
2. Arrastra **NovaCut** a la carpeta **Aplicaciones**.
3. La primera vez, macOS avisa de que no puede comprobar el desarrollador (la
   app está firmada localmente, no notarizada por Apple):
   - **macOS 14 Sonoma:** clic derecho sobre NovaCut → **Abrir** → **Abrir**.
   - **macOS 15 o posterior:** intenta abrirla, luego ve a **Ajustes del
     Sistema → Privacidad y seguridad** y pulsa **«Abrir igualmente»**.
   - Alternativa en Terminal: `xattr -dr com.apple.quarantine /Applications/NovaCut.app`

La app de macOS no necesita FFmpeg: usa AVFoundation. Solo pide permiso de
reconocimiento de voz si usas la transcripción.

## Comprobar la descarga

Cada release incluye `SHA256SUMS.txt`. En Windows (PowerShell):

```powershell
Get-FileHash .\NovaCut-Windows-Setup.exe -Algorithm SHA256
```

En macOS: `shasum -a 256 NovaCut-macOS.zip`. El resultado debe coincidir con
la línea correspondiente del fichero.

## Linux

No hay paquete todavía. El editor de la versión Windows compila en Linux
(`cargo build --release --features windows-host --bin novacut-windows`) si
tienes Rust y FFmpeg instalados, pero no se prueba en cada versión.
