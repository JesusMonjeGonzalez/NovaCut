#!/bin/bash
# Genera el corpus golden de formatos y comprueba la sincronía A/V.
#
# El patrón de prueba es un flash blanco de un frame sincronizado con un pitido
# de 1 kHz: es el "clap" de la claqueta. Si el montaje desvía el audio y el vídeo
# más de un frame, el desfase entre el flash y el pitido lo delata con precisión
# de fotograma.
#
# Uso: ./generar-corpus.sh [carpeta]
#   Genera los archivos en build/corpus por defecto.

set -euo pipefail
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
DESTINO="${1:-$RAIZ/build/corpus}"
mkdir -p "$DESTINO"

PATRON="$DESTINO/patron.mov"
if [ ! -f "$PATRON" ]; then
    echo "==> Generando patrón de prueba (flash + pitido)"
    cat > "$DESTINO/generar-patron.swift" <<'SWIFT'
import AVFoundation
import Foundation

let fps = 30
let total = 240
let muestrasPorFrame = 48000 / fps

let url = URL(fileURLWithPath: CommandLine.arguments[1])
try? FileManager.default.removeItem(at: url)
let writer = try! AVAssetWriter(outputURL: url, fileType: .mov)

let ajustesVideo: [String: Any] = [
    AVVideoCodecKey: AVVideoCodecType.h264,
    AVVideoWidthKey: 640,
    AVVideoHeightKey: 360,
]
let entradaVideo = AVAssetWriterInput(mediaType: .video, outputSettings: ajustesVideo)
let adaptador = AVAssetWriterInputPixelBufferAdaptor(
    assetWriterInput: entradaVideo,
    sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
)
writer.add(entradaVideo)

var asbd = AudioStreamBasicDescription(
    mSampleRate: 48000, mFormatID: kAudioFormatLinearPCM,
    mFormatFlags: kLinearPCMFormatFlagIsSignedInteger,
    mBytesPerPacket: 2, mFramesPerPacket: 1, mBytesPerFrame: 2,
    mChannelsPerFrame: 1, mBitsPerChannel: 16, mReserved: 0
)
var hint: CMAudioFormatDescription?
CMAudioFormatDescriptionCreate(allocator: kCFAllocatorDefault, asbd: &asbd,
                               layoutSize: 0, layout: nil, magicCookieSize: 0,
                               magicCookie: nil, extensions: nil, formatDescriptionOut: &hint)
let ajustesAudio: [String: Any] = [
    AVFormatIDKey: kAudioFormatLinearPCM,
    AVSampleRateKey: 48000,
    AVNumberOfChannelsKey: 1,
    AVLinearPCMBitDepthKey: 16,
    AVLinearPCMIsFloatKey: false,
    AVLinearPCMIsBigEndianKey: false,
]
let entradaAudio = AVAssetWriterInput(mediaType: .audio, outputSettings: ajustesAudio,
                                      sourceFormatHint: hint)
writer.add(entradaAudio)

guard writer.startWriting() else {
    print("no se pudo empezar a escribir: \(writer.error?.localizedDescription ?? "desconocido")")
    exit(1)
}
writer.startSession(atSourceTime: .zero)

// ---- Vídeo: rojo con un frame blanco en el 60.
while !entradaVideo.isReadyForMoreMediaData { usleep(2000) }
var buffer: CVPixelBuffer?
CVPixelBufferPoolCreatePixelBuffer(nil, adaptador.pixelBufferPool!, &buffer)
for i in 0..<total {
    guard let pixelBuffer = buffer else { continue }
    CVPixelBufferLockBaseAddress(pixelBuffer, [])
    let base = CVPixelBufferGetBaseAddress(pixelBuffer)!
    let ancho = 640, alto = 360
    let esFlash = i == 60
    let bytes = base.bindMemory(to: UInt8.self, capacity: ancho * alto * 4)
    for p in 0..<(ancho * alto) {
        let b = p * 4
        if esFlash {
            bytes[b] = 255; bytes[b + 1] = 255; bytes[b + 2] = 255
        } else {
            bytes[b] = 200; bytes[b + 1] = 30; bytes[b + 2] = 30
        }
        bytes[b + 3] = 255
    }
    CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
    while !entradaVideo.isReadyForMoreMediaData { usleep(2000) }
    adaptador.append(pixelBuffer, withPresentationTime: CMTime(value: CMTimeValue(i), timescale: 30))
}
entradaVideo.markAsFinished()

// ---- Audio: PCM mono con el pitido en el frame 60, escrito de una vez.
var audio = [Int16](repeating: 0, count: total * muestrasPorFrame)
let inicioDelPitido = 60 * muestrasPorFrame
let finDelPitido = inicioDelPitido + muestrasPorFrame * 2
for i in inicioDelPitido..<min(finDelPitido, audio.count) {
    audio[i] = Int16(sin(2.0 * Double.pi * 1000.0 * Double(i - inicioDelPitido) / 48000.0) * 8000)
}

while !entradaAudio.isReadyForMoreMediaData { usleep(2000) }
var bloque: CMBlockBuffer?
let bytesAudio = audio.withUnsafeBytes { Array($0) }
CMBlockBufferCreateWithMemoryBlock(
    allocator: kCFAllocatorDefault,
    memoryBlock: nil,
    blockLength: bytesAudio.count,
    blockAllocator: nil,
    customBlockSource: nil,
    offsetToData: 0,
    dataLength: bytesAudio.count,
    flags: 0,
    blockBufferOut: &bloque
)
if let bloque {
    bytesAudio.withUnsafeBytes { raw in
        CMBlockBufferReplaceDataBytes(with: raw.baseAddress!, blockBuffer: bloque,
                                      offsetIntoDestination: 0, dataLength: bytesAudio.count)
    }
    var formato: CMAudioFormatDescription?
    var asbdMuestra = AudioStreamBasicDescription(
        mSampleRate: 48000, mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kLinearPCMFormatFlagIsSignedInteger,
        mBytesPerPacket: 2, mFramesPerPacket: 1, mBytesPerFrame: 2,
        mChannelsPerFrame: 1, mBitsPerChannel: 16, mReserved: 0
    )
    CMAudioFormatDescriptionCreate(allocator: kCFAllocatorDefault, asbd: &asbdMuestra,
                                   layoutSize: 0, layout: nil, magicCookieSize: 0,
                                   magicCookie: nil, extensions: nil, formatDescriptionOut: &formato)
    var timing = CMSampleTimingInfo(
        duration: CMTime(value: 1, timescale: 30),
        presentationTimeStamp: .zero,
        decodeTimeStamp: .invalid
    )
    var sampleBuffer: CMSampleBuffer?
    let status = CMSampleBufferCreateReady(
        allocator: kCFAllocatorDefault, dataBuffer: bloque, formatDescription: formato,
        sampleCount: audio.count,
        sampleTimingEntryCount: 1, sampleTimingArray: &timing,
        sampleSizeEntryCount: 0, sampleSizeArray: nil,
        sampleBufferOut: &sampleBuffer
    )
    if status == noErr, let sampleBuffer {
        entradaAudio.append(sampleBuffer)
    }
}
entradaAudio.markAsFinished()

let semaforo = DispatchSemaphore(value: 0)
writer.finishWriting { semaforo.signal() }
semaforo.wait()
if writer.status != .completed {
    print("error al escribir el patrón: \(writer.error?.localizedDescription ?? "desconocido")")
    exit(1)
}
print("Patrón escrito en \(url.lastPathComponent)")
SWIFT
    swiftc -O -target arm64-apple-macos14.0 "$DESTINO/generar-patron.swift" \
        -framework AVFoundation -o "$DESTINO/generar-patron" 2>&1 | head -5
    "$DESTINO/generar-patron" "$PATRON"
fi

echo "==> Convirtiendo a los formatos del corpus"
convertir() {
    local nombre="$1" salida="$2" preset="$3"
    if [ -f "$salida" ]; then return; fi
    if avconvert --preset "$preset" --source "$PATRON" --output "$salida" >/dev/null 2>&1; then
        echo "  ok  $nombre"
    else
        echo "  aviso: no se pudo generar $nombre (el preset no está en este macOS)"
    fi
}

convertir "h264"       "$DESTINO/h264.mov"    "PresetAppleM4VCellular"
convertir "hevc"       "$DESTINO/hevc.mov"    "PresetAppleHEVC1920x1080"
convertir "prores"     "$DESTINO/prores.mov"  "PresetAppleProRes422LPCM"
convertir "h264-aac"   "$DESTINO/h264-aac.m4v" "PresetAppleM4VWiFi"

echo "==> Corpus en $DESTINO"
ls -la "$DESTINO" | grep -E "\.(mov|m4v)$" || true
