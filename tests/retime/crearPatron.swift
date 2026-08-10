import AVFoundation
import CoreVideo
import Foundation

// Crea el material de prueba del retime: un archivo real (H.264) donde cada
// frame es un gris distinto —la luminancia sube de 0 a 255 a lo largo del
// clip—. Un gris es robusto a la subsamplicación de croma del codec, así que
// se puede leer la luminancia decodificada y traducirla a frame exacto: a 1×
// dos segundos avanzan 50 niveles, a 2× avanzan 100, y un congelado mantiene
// el nivel.
//
// Uso: crearPatron directorioDeSalida

func traza(_ s: String) { print(s); fflush(stdout) }

let directorio = URL(fileURLWithPath: CommandLine.arguments[1])
let salida = directorio.appendingPathComponent("patron-retime.m4v")
try? FileManager.default.removeItem(at: salida)

let fps = 25.0
let ancho = 160
let alto = 90
let totalFrames = 500 // 20 segundos a 25 fps; cada 50 frames un escalón de gris (10 escalones).

let escritor = try AVAssetWriter(outputURL: salida, fileType: .m4v)
let entradaVideo = AVAssetWriterInput(
    mediaType: .video,
    outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: ancho,
        AVVideoHeightKey: alto,
    ]
)
entradaVideo.expectsMediaDataInRealTime = false
let adaptador = AVAssetWriterInputPixelBufferAdaptor(
    assetWriterInput: entradaVideo,
    sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: ancho,
        kCVPixelBufferHeightKey as String: alto,
    ]
)
guard escritor.canAdd(entradaVideo) else { fatalError("no se puede añadir el vídeo") }
escritor.add(entradaVideo)
guard escritor.startWriting() else {
    fatalError("no se pudo arrancar el escritor: \(escritor.error?.localizedDescription ?? "?")")
}
escritor.startSession(atSourceTime: .zero)

let duracionDeMarco = CMTime(value: 1, timescale: CMTimeScale(fps))

final class Avance {
    var pendiente = totalFrames
    let semaforo = DispatchSemaphore(value: 0)
}
let avance = Avance()

entradaVideo.requestMediaDataWhenReady(on: DispatchQueue(label: "editorcito.patron")) {
    while entradaVideo.isReadyForMoreMediaData && avance.pendiente > 0 {
        let f = totalFrames - avance.pendiente
        var pixel: CVPixelBuffer?
        guard CVPixelBufferCreate(
            kCFAllocatorDefault, ancho, alto, kCVPixelFormatType_32BGRA, nil, &pixel
        ) == kCVReturnSuccess, let pixel else { break }
        CVPixelBufferLockBaseAddress(pixel, [])
        if let base = CVPixelBufferGetBaseAddress(pixel)?.assumingMemoryBound(to: UInt8.self) {
            // Diez escalones de gris bien separados (28 niveles), uno cada 50
            // frames: el codec desplaza los grises continuos, pero un escalón
            // de 28 niveles sobrevive a cualquier cuantización realista.
            let paso = 28
            let nivel = UInt8(min(9, f / 50) * paso)
            for i in 0..<(ancho * alto) {
                base[i * 4 + 0] = nivel
                base[i * 4 + 1] = nivel
                base[i * 4 + 2] = nivel
                base[i * 4 + 3] = 255
            }
        }
        CVPixelBufferUnlockBaseAddress(pixel, [])
        adaptador.append(pixel, withPresentationTime: CMTimeMultiply(duracionDeMarco, multiplier: Int32(f)))
        avance.pendiente -= 1
    }
    if avance.pendiente == 0 {
        entradaVideo.markAsFinished()
        avance.semaforo.signal()
    }
}

avance.semaforo.wait()
let semaforo = DispatchSemaphore(value: 0)
escritor.finishWriting { semaforo.signal() }
semaforo.wait()
guard escritor.status == .completed else {
    fatalError("el patrón no se completó: \(escritor.error?.localizedDescription ?? "?")")
}
traza("patrón retime creado: \(totalFrames) frames · \(totalFrames / Int(fps)) s · luminancia por frame")
