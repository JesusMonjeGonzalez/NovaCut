#!/bin/bash
# Genera el corpus golden de formatos y cadencias para el gate P0.
#
# Cada archivo lleva el patrón de claqueta: flashes blancos de dos frames
# sincronizados con pitidos de 1 kHz, en cinco puntos del material. Si un editor
# desvía audio y vídeo más de un frame, el desfase entre flash y pitido lo
# delata. `probar-corpus.sh` mide eso y la cadencia de samples.
#
# Casos que cubre el corpus:
#   cfr-clap-h264.mov       H.264 CFR 30 + PCM (el caso base)
#   vfr-clap-h264.mov       H.264 VFR (cadencia real 30 ± 10 % + tirones) + PCM
#   vertical-clap-h264.mov  vídeo vertical 360×640 (orientación de móvil)
#   cfr-clap-hevc.mov       HEVC + PCM
#   cfr-clap-prores.mov     ProRes 422 + PCM
#   h264-aac.m4v            H.264 + AAC (el caso de teléfono real, con priming)
#   solo-audio.m4a          audio solo (importación sin vídeo)
#
# Uso: ./generar-corpus.sh [carpeta]
#   Genera los archivos en build/corpus por defecto.

set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
DESTINO="${1:-$RAIZ/build/corpus}"
mkdir -p "$DESTINO"

# El generador escribe vídeo y audio con `requestMediaDataWhenReady`: con dos
# entradas, esperar a que una esté lista mientras la otra no se alimenta atasca
# al escritor, que coordina la disponibilidad entre pistas. El patrón VFR elige
# una cadencia realista: duraciones que tiemblan ±10 % alrededor de 30 fps con
# tirones ocasionales, como un teléfono grabando; los PTS se acumulan y el
# pitido se coloca en el mismo tiempo de presentación que su flash.
cat > "$DESTINO/generar-patron.swift" <<'SWIFT'
import AVFoundation
import CoreMedia
import CoreVideo
import Darwin
import Foundation

// Crea un patrón de claqueta: fondo rojo con flashes blancos de dos frames y
// pitidos de 1 kHz sincronizados, en cinco puntos del material.
//
// Uso: generar-patron salida.mov cfr|vfr h264|hevc|prores422 ancho alto

func traza(_ s: String) { print(s); fflush(stdout) }

enum ErrorDeGeneracion: LocalizedError {
    case fallo(String)
    var errorDescription: String? {
        if case .fallo(let m) = self { return m } else { return "" }
    }
}

let argumentos = CommandLine.arguments
guard argumentos.count == 6 else {
    print("uso: generar-patron salida.mov cfr|vfr h264|hevc|prores422 ancho alto")
    exit(2)
}
let salida = URL(fileURLWithPath: argumentos[1])
let modo = argumentos[2]
let codec = argumentos[3]
let ancho = Int(argumentos[4])!
let alto = Int(argumentos[5])!

let fps = 30.0
let totalDeFrames = 480            // 16 segundos
let framesDeFlash = [96, 168, 240, 312, 384]
let muestreo = 48_000.0
let muestrasPorFrame = Int(muestreo / fps)
let indiceDePitido = 6            // 100 ms de pitido

// Cadencia VFR: un ciclo de duraciones que tiembla alrededor de 30 fps y con
// tirones ocasionales (un marco lento cada pocos), como una grabación de móvil.
let cicloVFR: [Double] = [1/30, 1/28, 1/31, 1/33, 1/27, 1/30, 1/29, 1/32, 1/22, 1/30]

// Los PTS del vídeo: para VFR se acumulan de verdad; el pitido se coloca en el
// mismo tiempo de presentación que su flash.
var ptsDeFrame = [Double]()
var tiempo = 0.0
for i in 0..<totalDeFrames {
    ptsDeFrame.append(tiempo)
    tiempo += (modo == "vfr") ? cicloVFR[i % cicloVFR.count] : (1.0 / fps)
}
let duracionTotal = tiempo

let codecDeVideo: AVVideoCodecType
switch codec {
case "hevc": codecDeVideo = .hevc
case "prores422": codecDeVideo = .proRes422
default: codecDeVideo = .h264
}

try? FileManager.default.removeItem(at: salida)
let escritor = try AVAssetWriter(outputURL: salida, fileType: .mov)

let entradaVideo = AVAssetWriterInput(
    mediaType: .video,
    outputSettings: [
        AVVideoCodecKey: codecDeVideo,
        AVVideoWidthKey: ancho,
        AVVideoHeightKey: alto,
    ]
)
let adaptador = AVAssetWriterInputPixelBufferAdaptor(
    assetWriterInput: entradaVideo,
    sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: ancho,
        kCVPixelBufferHeightKey as String: alto,
    ]
)
guard escritor.canAdd(entradaVideo) else { throw ErrorDeGeneracion.fallo("vídeo") }
escritor.add(entradaVideo)

let entradaAudio = AVAssetWriterInput(
    mediaType: .audio,
    outputSettings: [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVSampleRateKey: muestreo,
        AVNumberOfChannelsKey: 1,
        AVLinearPCMBitDepthKey: 16,
        AVLinearPCMIsFloatKey: false,
        AVLinearPCMIsBigEndianKey: false,
        AVLinearPCMIsNonInterleaved: false,
    ]
)
guard escritor.canAdd(entradaAudio) else { throw ErrorDeGeneracion.fallo("audio") }
escritor.add(entradaAudio)

guard escritor.startWriting() else {
    throw ErrorDeGeneracion.fallo(escritor.error?.localizedDescription ?? "escritor")
}
escritor.startSession(atSourceTime: .zero)
traza("generando \(salida.lastPathComponent) · \(modo) · \(codec) · \(ancho)×\(alto) · \(String(format: "%.1f", duracionTotal)) s")

final class Avance {
    var pendiente = 0
    let semaforo = DispatchSemaphore(value: 0)
}

// ---- Audio: PCM mono 48 kHz, pitido donde toque su flash. El audio dura
// exactamente lo mismo que el vídeo: una pista más larga alarga la duración del
// archivo y las ventanas de medida del arnés, calculadas sobre esa duración,
// dejan de apuntar a los flashes del final.
let totalDeMuestras = Int(duracionTotal * muestreo)
let avanceAudio = Avance()
avanceAudio.pendiente = totalDeMuestras
let marcosPorLote = 4096
let duracionDeMuestra = CMTime(value: 1, timescale: CMTimeScale(muestreo))

var descripcionAudio: CMAudioFormatDescription?
var formato = AudioStreamBasicDescription(
    mSampleRate: muestreo, mFormatID: kAudioFormatLinearPCM,
    mFormatFlags: kLinearPCMFormatFlagIsSignedInteger,
    mBytesPerPacket: 2, mFramesPerPacket: 1, mBytesPerFrame: 2,
    mChannelsPerFrame: 1, mBitsPerChannel: 16, mReserved: 0
)
CMAudioFormatDescriptionCreate(
    allocator: kCFAllocatorDefault, asbd: &formato, layoutSize: 0, layout: nil,
    magicCookieSize: 0, magicCookie: nil, extensions: nil, formatDescriptionOut: &descripcionAudio
)

let flashes = Set(framesDeFlash)
let pitidoPorFlash = Dictionary(uniqueKeysWithValues: framesDeFlash.map { ($0, ptsDeFrame[$0]) })

func muestrasDelPitido(_ n: Int) -> Int16 {
    // El pitido empieza en el PTS de su flash y dura `indiceDePitido` frames.
    for (_, inicio) in pitidoPorFlash {
        let inicioEnMuestras = Int(inicio * muestreo)
        let m = n - inicioEnMuestras
        if m >= 0 && m < muestrasPorFrame * indiceDePitido {
            return Int16(sin(2.0 * .pi * 1000 * Double(m) / muestreo) * 8000)
        }
    }
    return 0
}

entradaAudio.requestMediaDataWhenReady(on: DispatchQueue(label: "corpus.audio")) {
    while entradaAudio.isReadyForMoreMediaData && avanceAudio.pendiente > 0 {
        let yaEscritos = totalDeMuestras - avanceAudio.pendiente
        let marcos = min(marcosPorLote, avanceAudio.pendiente)
        var muestras = [Int16](repeating: 0, count: marcos)
        for m in 0..<marcos {
            muestras[m] = muestrasDelPitido(yaEscritos + m)
        }
        let bytes = marcos * MemoryLayout<Int16>.size
        var bloque: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: bytes,
            blockAllocator: nil, customBlockSource: nil, offsetToData: 0,
            dataLength: bytes, flags: 0, blockBufferOut: &bloque
        ) == kCMBlockBufferNoErr, let bloque else { break }
        _ = muestras.withUnsafeBytes { puntero in
            CMBlockBufferReplaceDataBytes(with: puntero.baseAddress!, blockBuffer: bloque, offsetIntoDestination: 0, dataLength: bytes)
        }
        let tiempoMuestra = CMTimeMultiply(duracionDeMuestra, multiplier: Int32(yaEscritos))
        let info = CMSampleTimingInfo(duration: duracionDeMuestra, presentationTimeStamp: tiempoMuestra, decodeTimeStamp: .invalid)
        var muestra: CMSampleBuffer?
        guard CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault, dataBuffer: bloque, formatDescription: descripcionAudio,
            sampleCount: marcos, sampleTimingEntryCount: 1, sampleTimingArray: [info],
            sampleSizeEntryCount: 0, sampleSizeArray: nil, sampleBufferOut: &muestra
        ) == noErr, let muestra else { break }
        entradaAudio.append(muestra)
        avanceAudio.pendiente -= marcos
    }
    if avanceAudio.pendiente == 0 {
        entradaAudio.markAsFinished()
        avanceAudio.semaforo.signal()
    }
}

// ---- Vídeo: rojo, con flashes blancos de dos frames.
let avanceVideo = Avance()
avanceVideo.pendiente = totalDeFrames
let duracionDeMarco = CMTime(value: 1, timescale: CMTimeScale(fps))
var colores = [UInt8](repeating: 0, count: ancho * alto * 4)
for i in 0..<(ancho * alto) {
    colores[i * 4 + 0] = 30    // BGRA: rojo apagado
    colores[i * 4 + 1] = 30
    colores[i * 4 + 2] = 200
    colores[i * 4 + 3] = 255
}
var blancos = [UInt8](repeating: 255, count: ancho * alto * 4)

entradaVideo.requestMediaDataWhenReady(on: DispatchQueue(label: "corpus.video")) {
    while entradaVideo.isReadyForMoreMediaData && avanceVideo.pendiente > 0 {
        let f = totalDeFrames - avanceVideo.pendiente
        var pixel: CVPixelBuffer?
        guard CVPixelBufferCreate(
            kCFAllocatorDefault, ancho, alto, kCVPixelFormatType_32BGRA, nil, &pixel
        ) == kCVReturnSuccess, let pixel else { break }
        CVPixelBufferLockBaseAddress(pixel, [])
        if let base = CVPixelBufferGetBaseAddress(pixel) {
            let esFlash = flashes.contains(f) || flashes.contains(f - 1)
            memcpy(base, esFlash ? blancos : colores, colores.count)
        }
        CVPixelBufferUnlockBaseAddress(pixel, [])
        let tiempoDeFrame = CMTime(seconds: ptsDeFrame[f], preferredTimescale: 600)
        adaptador.append(pixel, withPresentationTime: tiempoDeFrame)
        avanceVideo.pendiente -= 1
    }
    if avanceVideo.pendiente == 0 {
        entradaVideo.markAsFinished()
        avanceVideo.semaforo.signal()
    }
}

avanceAudio.semaforo.wait()
avanceVideo.semaforo.wait()
escritor.finishWriting {
    avanceVideo.semaforo.signal()
}
avanceVideo.semaforo.wait()
guard escritor.status == .completed else {
    throw ErrorDeGeneracion.fallo(escritor.error?.localizedDescription ?? "fin")
}
traza("ok \(salida.lastPathComponent)")
SWIFT

swiftc -O -target arm64-apple-macos14.0 \
    "$DESTINO/generar-patron.swift" \
    -framework AVFoundation -framework AudioToolbox -framework CoreVideo \
    -o "$DESTINO/generar-patron"

generar() {
    local nombre="$1" modo="$2" codec="$3" ancho="$4" alto="$5"
    local salida="$DESTINO/$nombre"
    if [ -f "$salida" ]; then
        echo "  ya existe  $nombre"
        return 0
    fi
    "$DESTINO/generar-patron" "$salida" "$modo" "$codec" "$ancho" "$alto"
    echo "  ok  $nombre"
}

echo "==> Generando patrones de claqueta"
generar "cfr-clap-h264.mov"     "cfr" "h264" "640" "360"
generar "vfr-clap-h264.mov"     "vfr" "h264" "640" "360"
generar "vertical-clap-h264.mov" "cfr" "h264" "360" "640"
generar "cfr-clap-hevc.mov"     "cfr" "hevc" "640" "360"
generar "cfr-clap-prores.mov"   "cfr" "prores422" "640" "360"

echo "==> Convirtiendo a los formatos del corpus"
convertir() {
    local nombre="$1" salida="$2" preset="$3"
    if [ -f "$salida" ]; then
        echo "  ya existe  $nombre"
        return 0
    fi
    if ! avconvert --preset "$preset" --source "$DESTINO/cfr-clap-h264.mov" --output "$salida" >/dev/null 2>&1; then
        echo "  no se pudo generar $nombre: el preset no está disponible en este macOS" >&2
        return 1
    fi
    echo "  ok  $nombre"
}

convertir "h264-aac" "$DESTINO/h264-aac.m4v" "PresetAppleM4VWiFi"
convertir "solo-audio" "$DESTINO/solo-audio.m4a" "PresetAppleM4A"

echo "==> Corpus en $DESTINO"
ls -la "$DESTINO" | grep -E "\.(mov|m4v|m4a)$" || true
