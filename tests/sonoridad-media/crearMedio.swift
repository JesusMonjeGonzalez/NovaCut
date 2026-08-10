import AVFoundation
import CoreMedia
import Foundation

// Envuelve un WAV en un M4A (AAC) para poder probar el pipeline de sonoridad
// contra un archivo real de AVFoundation partiendo de las señales de la EBU:
//   crearMedio entrada.wav salida.m4a
//
// El WAV se lee a mano y no con ExtAudioFile por una razón concreta: pedirle
// un formato cliente en float32 le obliga a abrir un AudioConverter, y el
// convertidor AAC del escritor comparte ese recurso escaso — con los dos
// abiertos a la vez, la lectura devuelve cero marcos sin ningún error. El
// parseo directo de PCM de 16/24 bits no necesita conversor y es exactamente
// el mismo que usa la suite de conformidad contra las señales de la EBU.

enum ErrorDeCreacion: LocalizedError {
    case fallo(String)
    var errorDescription: String? {
        "No se pudo crear el medio: \(detalle)"
    }
    private var detalle: String {
        if case .fallo(let m) = self { return m } else { return "" }
    }
}

struct WavLeido {
    let frecuencia: Double
    let canales: Int
    let entrelazado: [Float]
}

func leerWav(_ ruta: String) throws -> WavLeido {
    guard let datos = FileManager.default.contents(atPath: ruta) else {
        throw ErrorDeCreacion.fallo("leer \(ruta)")
    }
    guard datos.count > 44, Array(datos[0..<4]) == Array("RIFF".utf8) else {
        throw ErrorDeCreacion.fallo("no es un WAV RIFF")
    }

    var canales = 0
    var frecuencia = 0.0
    var bits = 0
    var entrelazado: [Float] = []

    var offset = 12
    while offset + 8 <= datos.count {
        let id = String(data: datos[offset..<offset + 4], encoding: .ascii)
        let tamano = datos.withUnsafeBytes {
            $0.loadUnaligned(fromByteOffset: offset + 4, as: UInt32.self)
        }
        guard offset + 8 + Int(tamano) <= datos.count else { break }

        if id == "fmt " {
            canales = Int(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 10, as: UInt16.self)
            })
            frecuencia = Double(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 12, as: UInt32.self)
            })
            bits = Int(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 22, as: UInt16.self)
            })
        } else if id == "data" {
            let pcm = datos.subdata(in: (offset + 8)..<(offset + 8 + Int(tamano)))
            let bytesPorMuestra = bits / 8
            let marcos = pcm.count / (canales * bytesPorMuestra)
            entrelazado = [Float](repeating: 0, count: marcos * canales)
            pcm.withUnsafeBytes { bytes in
                let crudos = bytes.bindMemory(to: UInt8.self)
                for m in 0..<marcos {
                    for c in 0..<canales {
                        let base = (m * canales + c) * bytesPorMuestra
                        if bits == 16 {
                            let crudo = Int16(bitPattern: UInt16(crudos[base]) | (UInt16(crudos[base + 1]) << 8))
                            entrelazado[m * canales + c] = Float(crudo) / 32_768
                        } else {
                            let u = UInt32(crudos[base]) | (UInt32(crudos[base + 1]) << 8) | (UInt32(crudos[base + 2]) << 16)
                            let signo = Int32(bitPattern: (u & 0x80_0000) != 0 ? u | 0xFF00_0000 : u)
                            entrelazado[m * canales + c] = Float(signo) / 8_388_608
                        }
                    }
                }
            }
        }
        offset += 8 + Int(tamano) + Int(tamano) % 2
    }

    guard canales > 0, frecuencia > 0, (bits == 16 || bits == 24), !entrelazado.isEmpty else {
        throw ErrorDeCreacion.fallo("WAV sin PCM de 16/24 bits")
    }
    return WavLeido(frecuencia: frecuencia, canales: canales, entrelazado: entrelazado)
}

func crearM4a(desde wav: URL, hacia salida: URL) async throws {
    let audio = try leerWav(wav.path)
    let canales = audio.canales
    let frecuencia = audio.frecuencia

    var cliente = AudioStreamBasicDescription(
        mSampleRate: frecuencia,
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
        allocator: kCFAllocatorDefault,
        asbd: &cliente,
        layoutSize: 0,
        layout: nil,
        magicCookieSize: 0,
        magicCookie: nil,
        extensions: nil,
        formatDescriptionOut: &descripcionAudio
    )

    try? FileManager.default.removeItem(at: salida)
    let escritor = try AVAssetWriter(outputURL: salida, fileType: .m4a)
    let entrada = AVAssetWriterInput(
        mediaType: .audio,
        outputSettings: [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: frecuencia,
            AVNumberOfChannelsKey: canales,
            AVEncoderBitRateKey: 192_000,
        ]
    )
    guard escritor.canAdd(entrada) else { throw ErrorDeCreacion.fallo("entrada AAC") }
    escritor.add(entrada)
    guard escritor.startWriting() else {
        throw ErrorDeCreacion.fallo(escritor.error?.localizedDescription ?? "escritor")
    }
    escritor.startSession(atSourceTime: .zero)

    let marcosPorLote = 4096
    let duracionMuestra = CMTime(value: 1, timescale: CMTimeScale(frecuencia))
    var tiempo = CMTime.zero
    var pendiente = 0
    let totalDeMarcos = audio.entrelazado.count / canales

    while pendiente < totalDeMarcos {
        let marcos = min(marcosPorLote, totalDeMarcos - pendiente)
        let bytes = marcos * canales * MemoryLayout<Float>.size

        var bloque: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault,
            memoryBlock: nil,
            blockLength: bytes,
            blockAllocator: nil,
            customBlockSource: nil,
            offsetToData: 0,
            dataLength: bytes,
            flags: 0,
            blockBufferOut: &bloque
        ) == kCMBlockBufferNoErr, let bloque else {
            throw ErrorDeCreacion.fallo("bloque de audio")
        }

        // Copiar el lote a un CMBlockBuffer y enviarlo como sample buffer de
        // PCM: el escritor convierte a AAC al anexarlo.
        _ = audio.entrelazado.withUnsafeBytes { puntero in
            CMBlockBufferReplaceDataBytes(
                with: puntero.baseAddress!.advanced(by: pendiente * canales * 4),
                blockBuffer: bloque,
                offsetIntoDestination: 0,
                dataLength: bytes
            )
        }

        let info = CMSampleTimingInfo(
            duration: duracionMuestra,
            presentationTimeStamp: tiempo,
            decodeTimeStamp: .invalid
        )
        var muestra: CMSampleBuffer?
        guard CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: bloque,
            formatDescription: descripcionAudio,
            sampleCount: marcos,
            sampleTimingEntryCount: 1,
            sampleTimingArray: [info],
            sampleSizeEntryCount: 0,
            sampleSizeArray: nil,
            sampleBufferOut: &muestra
        ) == noErr, let muestra else {
            throw ErrorDeCreacion.fallo("sample buffer")
        }

        while !entrada.isReadyForMoreMediaData {
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        guard entrada.append(muestra) else {
            throw ErrorDeCreacion.fallo(escritor.error?.localizedDescription ?? "anexar audio")
        }
        tiempo = CMTimeAdd(tiempo, CMTimeMultiply(duracionMuestra, multiplier: Int32(marcos)))
        pendiente += marcos
    }

    entrada.markAsFinished()
    await escritor.finishWriting()
    guard escritor.status == .completed else {
        throw ErrorDeCreacion.fallo(escritor.error?.localizedDescription ?? "fin de escritura")
    }
    print("Creado \(salida.lastPathComponent): \(canales) ch, \(Int(frecuencia)) Hz, AAC, \(audio.entrelazado.count / canales) marcos")
}

let entrada = URL(fileURLWithPath: CommandLine.arguments[1])
let salida = URL(fileURLWithPath: CommandLine.arguments[2])
try await crearM4a(desde: entrada, hacia: salida)
