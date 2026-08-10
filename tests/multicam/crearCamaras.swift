import AVFoundation
import CoreMedia
import CoreVideo
import Darwin
import Foundation

// Crea las «cámaras» de prueba de la multicámara: cada una es un archivo real
// (H.264 + AAC) con un color sólido y un tono de 1 kHz a un nivel conocido,
// con un pad de silencio distinto al principio. La prueba conoce los pads, así
// que puede calcular los desfases del grupo sin medir nada.
//
// Uso: crearCamaras directorioDeSalida

func traza(_ s: String) { print(s); fflush(stdout) }

enum ErrorDeCreacion: LocalizedError {
    case fallo(String)
    var errorDescription: String? {
        "No se pudo crear la cámara: \(detalle)"
    }
    private var detalle: String {
        if case .fallo(let m) = self { return m } else { return "" }
    }
}

struct Camara {
    let nombre: String
    let rojo: Double
    let verde: Double
    let azul: Double
    /// Cresta del tono en dBFS (el tono de 1 kHz mide su cresta en LUFS).
    let nivelEndBFS: Double
    /// Silencio al principio del archivo, en segundos.
    let pad: Double
}

func crearCamara(_ camara: Camara, en directorio: URL) async throws {
    let muestreo = 48_000.0
    let duracionTotal = 5.0
    let canales = 2
    let fps = 25.0
    let ancho = 160
    let alto = 90

    let salida = directorio.appendingPathComponent("\(camara.nombre).m4v")
    try? FileManager.default.removeItem(at: salida)
    let escritor = try AVAssetWriter(outputURL: salida, fileType: .m4v)

    let entradaVideo = AVAssetWriterInput(
        mediaType: .video,
        outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
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
    guard escritor.canAdd(entradaVideo) else { throw ErrorDeCreacion.fallo("vídeo") }
    escritor.add(entradaVideo)

    let entradaAudio = AVAssetWriterInput(
        mediaType: .audio,
        outputSettings: [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: muestreo,
            AVNumberOfChannelsKey: canales,
            AVEncoderBitRateKey: 96_000,
        ]
    )
    guard escritor.canAdd(entradaAudio) else { throw ErrorDeCreacion.fallo("audio") }
    escritor.add(entradaAudio)

    guard escritor.startWriting() else { throw ErrorDeCreacion.fallo(escritor.error?.localizedDescription ?? "escritor") }
    escritor.startSession(atSourceTime: .zero)
    traza("\(camara.nombre): escritor arrancado")

    // Vídeo: un color sólido por cámara, inconfundible al decodificar.
    let marcosDeVideo = Int(duracionTotal * fps)
    let duracionDeMarco = CMTime(value: 1, timescale: CMTimeScale(fps))
    var colores = [UInt8](repeating: 0, count: ancho * alto * 4)
    for i in 0..<(ancho * alto) {
        // BGRA: la cámara llena el canal que la identifica.
        colores[i * 4 + 0] = UInt8(camara.azul * 255)
        colores[i * 4 + 1] = UInt8(camara.verde * 255)
        colores[i * 4 + 2] = UInt8(camara.rojo * 255)
        colores[i * 4 + 3] = 255
    }
    // El audio se prepara entero primero (el tono con su pad) y el vídeo se
    // entrega con `requestMediaDataWhenReady`: con dos entradas, esperar a que
    // una esté lista mientras la otra no se alimenta atasca al escritor, que
    // coordina la disponibilidad entre pistas y no vuelve a decir «listo».
    var cliente = AudioStreamBasicDescription(
        mSampleRate: muestreo,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        mBytesPerPacket: UInt32(4 * canales),
        mFramesPerPacket: 1,
        mBytesPerFrame: UInt32(4 * canales),
        mChannelsPerFrame: UInt32(canales),
        mBitsPerChannel: 32,
        mReserved: 0
    )
    var descripcionAudio: CMAudioFormatDescription?
    CMAudioFormatDescriptionCreate(
        allocator: kCFAllocatorDefault, asbd: &cliente, layoutSize: 0, layout: nil,
        magicCookieSize: 0, magicCookie: nil, extensions: nil, formatDescriptionOut: &descripcionAudio
    )

    let amplitud = pow(10.0, camara.nivelEndBFS / 20)
    let muestrasDePad = Int(camara.pad * muestreo)
    let marcosPorLote = 4096
    let duracionDeMuestra = CMTime(value: 1, timescale: CMTimeScale(muestreo))
    let totalDeMarcos = Int(duracionTotal * muestreo)

    final class Avance {
        var pendiente = 0
        let semaforo = DispatchSemaphore(value: 0)
    }

    let avanceAudio = Avance()
    avanceAudio.pendiente = totalDeMarcos
    let avanceVideo = Avance()
    avanceVideo.pendiente = marcosDeVideo

    entradaAudio.requestMediaDataWhenReady(on: DispatchQueue(label: "editorcito.audio")) {
        while entradaAudio.isReadyForMoreMediaData && avanceAudio.pendiente > 0 {
            let yaEscritos = totalDeMarcos - avanceAudio.pendiente
            let marcos = min(marcosPorLote, avanceAudio.pendiente)
            var muestras = [Float](repeating: 0, count: marcos * canales)
            for m in 0..<marcos {
                let n = yaEscritos + m
                if n >= muestrasDePad {
                    let v = Float(amplitud * sin(2.0 * .pi * 1000 * Double(n) / muestreo))
                    muestras[m * canales] = v
                    muestras[m * canales + 1] = v
                }
            }
            let bytes = marcos * canales * MemoryLayout<Float>.size
            var bloque: CMBlockBuffer?
            guard CMBlockBufferCreateWithMemoryBlock(
                allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: bytes,
                blockAllocator: nil, customBlockSource: nil, offsetToData: 0,
                dataLength: bytes, flags: 0, blockBufferOut: &bloque
            ) == kCMBlockBufferNoErr, let bloque else { break }
            _ = muestras.withUnsafeBytes { puntero in
                CMBlockBufferReplaceDataBytes(with: puntero.baseAddress!, blockBuffer: bloque, offsetIntoDestination: 0, dataLength: bytes)
            }
            let tiempo = CMTimeMultiply(duracionDeMuestra, multiplier: Int32(yaEscritos))
            let info = CMSampleTimingInfo(duration: duracionDeMuestra, presentationTimeStamp: tiempo, decodeTimeStamp: .invalid)
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

    entradaVideo.requestMediaDataWhenReady(on: DispatchQueue(label: "editorcito.video")) {
        while entradaVideo.isReadyForMoreMediaData && avanceVideo.pendiente > 0 {
            let f = marcosDeVideo - avanceVideo.pendiente
            var pixel: CVPixelBuffer?
            guard CVPixelBufferCreate(
                kCFAllocatorDefault, ancho, alto, kCVPixelFormatType_32BGRA, nil, &pixel
            ) == kCVReturnSuccess, let pixel else { break }
            CVPixelBufferLockBaseAddress(pixel, [])
            if let base = CVPixelBufferGetBaseAddress(pixel) {
                memcpy(base, colores, colores.count)
            }
            CVPixelBufferUnlockBaseAddress(pixel, [])
            adaptador.append(pixel, withPresentationTime: CMTimeMultiply(duracionDeMarco, multiplier: Int32(f)))
            avanceVideo.pendiente -= 1
        }
        if avanceVideo.pendiente == 0 {
            entradaVideo.markAsFinished()
            avanceVideo.semaforo.signal()
        }
    }

    avanceAudio.semaforo.wait()
    avanceVideo.semaforo.wait()
    await escritor.finishWriting()
    guard escritor.status == .completed else {
        throw ErrorDeCreacion.fallo(escritor.error?.localizedDescription ?? "fin")
    }
    traza("Cámara \(camara.nombre): \(ancho)×\(alto) \(Int(fps)) fps · tono \(camara.nivelEndBFS) dBFS · pad \(camara.pad) s")
}

let directorio = URL(fileURLWithPath: CommandLine.arguments[1])
let camaras = [
    Camara(nombre: "camaraA", rojo: 1, verde: 0, azul: 0, nivelEndBFS: -20, pad: 0.5),
    Camara(nombre: "camaraB", rojo: 0, verde: 1, azul: 0, nivelEndBFS: -26, pad: 1.5),
    Camara(nombre: "camaraC", rojo: 0, verde: 0, azul: 1, nivelEndBFS: -32, pad: 1.0),
]
for camara in camaras {
    try await crearCamara(camara, en: directorio)
}
