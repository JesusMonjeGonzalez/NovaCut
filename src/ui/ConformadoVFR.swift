import AVFoundation
import CryptoKit
import CoreVideo
import Foundation

/// Resultado de preparar los medios que necesitan una cadencia constante.
struct ResultadoDeConformadoVFR {
    let medios: [UUID: MedioResuelto]
    let conformados: Int
    let fallos: [String]
}

enum ErrorDeConformadoVFR: LocalizedError {
    case sinVideo
    case sinCache
    case exportacion(String)
    case cacheInvalida
    case incompleto([String])

    var errorDescription: String? {
        switch self {
        case .sinVideo:
            return "El medio VFR no tiene una pista de vídeo utilizable."
        case .sinCache:
            return "No se pudo crear la carpeta de conformado VFR."
        case .exportacion(let detalle):
            return "No se pudo conformar el medio VFR: \(detalle)"
        case .cacheInvalida:
            return "El intermediario CFR quedó incompleto o no es reproducible."
        case .incompleto(let fallos):
            return "La entrega se detuvo porque el conformado VFR no terminó: "
                + fallos.joined(separator: " · ")
        }
    }
}

/// Convierte VFR a un intermediario CFR antes de que el medio entre en una
/// composición de frames enteros. La salida se guarda en caché por identidad del
/// archivo y base de tiempo, y nunca sustituye al asset original.
enum ConformadorVFR {

    static func preparar(
        medios: [UUID: MedioResuelto],
        para timebase: Timebase
    ) async -> ResultadoDeConformadoVFR {
        var preparados = medios
        var conformados = 0
        var fallos: [String] = []

        for medio in medios.values.sorted(by: { $0.id.uuidString < $1.id.uuidString }) {
            guard medio.esVFR, medio.tieneVideo else { continue }
            if medio.timebaseDeMontaje == timebase,
               medio.estaConformado {
                continue
            }
            do {
                preparados[medio.id] = try await preparar(medio, para: timebase)
                conformados += 1
            } catch {
                fallos.append("\(medio.url.lastPathComponent): \(error.localizedDescription)")
            }
        }

        return ResultadoDeConformadoVFR(medios: preparados, conformados: conformados, fallos: fallos)
    }

    static func preparar(_ medio: MedioResuelto, para timebase: Timebase) async throws -> MedioResuelto {
        guard medio.esVFR, medio.tieneVideo else { return medio }
        if medio.timebaseDeMontaje == timebase,
           medio.estaConformado {
            return medio
        }

        let cache = try urlDeCache(para: medio, timebase: timebase)
        if FileManager.default.fileExists(atPath: cache.path),
           let preparado = try? await cargarCache(cache, sobre: medio, timebase: timebase) {
            return preparado
        }

        try await generarCache(en: cache, para: medio, timebase: timebase)
        return try await cargarCache(cache, sobre: medio, timebase: timebase)
    }

    private static func cargarCache(
        _ url: URL,
        sobre medio: MedioResuelto,
        timebase: Timebase
    ) async throws -> MedioResuelto {
        let asset = AVURLAsset(
            url: url,
            options: [AVURLAssetPreferPreciseDurationAndTimingKey: true]
        )
        guard try await asset.load(.isPlayable) else { throw ErrorDeConformadoVFR.cacheInvalida }
        let video = try await asset.loadTracks(withMediaType: .video).first
        let audio = try await asset.loadTracks(withMediaType: .audio).first
        let duration = try await asset.load(.duration)
        guard let video, duration.isNumeric, duration.seconds > 0 else {
            throw ErrorDeConformadoVFR.cacheInvalida
        }

        let toleranciaDeReloj = max(timebase.tiempo(1).seconds * 2, 0.05)
        let rangoDeVideo = try await video.load(.timeRange)
        let rangoOriginal = try await medio.pistaDeVideo?.load(.timeRange)
        guard rangoDeVideo.start.isNumeric,
              rangoDeVideo.duration.isNumeric,
              rangoOriginal?.start.isNumeric == true,
              rangoOriginal?.duration.isNumeric == true,
              abs(rangoDeVideo.start.seconds - (rangoOriginal?.start.seconds ?? 0)) <= toleranciaDeReloj,
              abs(rangoDeVideo.duration.seconds - (rangoOriginal?.duration.seconds ?? 0)) <= toleranciaDeReloj,
              abs(duration.seconds - medio.duracion.seconds) <= toleranciaDeReloj,
              let marcas = await MedioResuelto.marcasDePresentacion(pista: video),
              let resumen = MedioResuelto.resumenDePTS(marcas),
              resumen.esCFR(para: timebase) else {
            throw ErrorDeConformadoVFR.cacheInvalida
        }

        if let audioOriginal = medio.pistaDeAudio {
            guard let audio else { throw ErrorDeConformadoVFR.cacheInvalida }
            let rangoDeAudio = try await audio.load(.timeRange)
            let rangoOriginalDeAudio = try await audioOriginal.load(.timeRange)
            guard rangoDeAudio.start.isNumeric,
                  rangoDeAudio.duration.isNumeric,
                  rangoOriginalDeAudio.start.isNumeric,
                  rangoOriginalDeAudio.duration.isNumeric,
                  abs(rangoDeAudio.start.seconds - rangoOriginalDeAudio.start.seconds) <= toleranciaDeReloj,
                  abs(rangoDeAudio.duration.seconds - rangoOriginalDeAudio.duration.seconds) <= toleranciaDeReloj else {
                throw ErrorDeConformadoVFR.cacheInvalida
            }
        }
        return medio.conMontaje(
            asset: asset,
            video: video,
            audio: audio,
            duration: duration,
            timebase: timebase
        )
    }

    private static func generarCache(
        en url: URL,
        para medio: MedioResuelto,
        timebase: Timebase
    ) async throws {
        guard let video = medio.pistaDeVideo else { throw ErrorDeConformadoVFR.sinVideo }
        let duration = try await medio.asset.load(.duration)
        guard duration.isNumeric, duration.seconds > 0 else {
            throw ErrorDeConformadoVFR.exportacion("la duración no es válida")
        }
        let tamano = medio.tamanoNatural
        guard tamano.width > 0, tamano.height > 0 else {
            throw ErrorDeConformadoVFR.exportacion("el tamaño de vídeo no es válido")
        }
        let rangoDeVideo = try await video.load(.timeRange)
        guard rangoDeVideo.duration.isNumeric, rangoDeVideo.duration.seconds > 0 else {
            throw ErrorDeConformadoVFR.exportacion("la pista de vídeo no tiene duración válida")
        }
        let duracionDeVideo = rangoDeVideo.duration

        // El exportador de alto nivel puede conservar el FPS nominal del archivo
        // aunque la composición declare otro frameDuration. El lector de
        // composición sí materializa un frame en cada instante CFR; después el
        // writer recibe esos buffers con sus PTS explícitos. La matriz se deja
        // neutra: el montaje aplica la orientación original una sola vez.
        let videoComposition = AVMutableVideoComposition()
        videoComposition.renderSize = tamano
        videoComposition.frameDuration = timebase.tiempo(1)
        let instruccion = AVMutableVideoCompositionInstruction()
        instruccion.timeRange = CMTimeRange(start: .zero, duration: duracionDeVideo)
        let capa = AVMutableVideoCompositionLayerInstruction(assetTrack: video)
        capa.setTransform(.identity, at: .zero)
        instruccion.layerInstructions = [capa]
        videoComposition.instructions = [instruccion]

        let temporal = url.deletingLastPathComponent()
            .appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString).part")
        try? FileManager.default.removeItem(at: temporal)
        defer { try? FileManager.default.removeItem(at: temporal) }

        let lector = try AVAssetReader(asset: medio.asset)
        let salidaDeVideo = AVAssetReaderVideoCompositionOutput(
            videoTracks: [video],
            videoSettings: [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA
            ]
        )
        salidaDeVideo.videoComposition = videoComposition
        guard lector.canAdd(salidaDeVideo) else {
            throw ErrorDeConformadoVFR.exportacion("no se pudo preparar la lectura CFR")
        }
        lector.add(salidaDeVideo)

        let salidaDeAudio: AVAssetReaderTrackOutput?
        let formatoDeAudio: CMFormatDescription?
        if let audio = medio.pistaDeAudio {
            salidaDeAudio = AVAssetReaderTrackOutput(track: audio, outputSettings: nil)
            guard lector.canAdd(salidaDeAudio!) else {
                throw ErrorDeConformadoVFR.exportacion("no se pudo leer el audio original")
            }
            lector.add(salidaDeAudio!)
            formatoDeAudio = try await audio.load(.formatDescriptions).first
        } else {
            salidaDeAudio = nil
            formatoDeAudio = nil
        }

        let escritor = try AVAssetWriter(outputURL: temporal, fileType: .mov)
        let ancho = Int(tamano.width.rounded())
        let alto = Int(tamano.height.rounded())
        let entradaDeVideo = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [
                AVVideoCodecKey: AVVideoCodecType.h264,
                AVVideoWidthKey: ancho,
                AVVideoHeightKey: alto,
            ]
        )
        guard escritor.canAdd(entradaDeVideo) else {
            throw ErrorDeConformadoVFR.exportacion("el codificador de vídeo no admite el tamaño del medio")
        }
        escritor.add(entradaDeVideo)
        let adaptador = AVAssetWriterInputPixelBufferAdaptor(
            assetWriterInput: entradaDeVideo,
            sourcePixelBufferAttributes: [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
                kCVPixelBufferWidthKey as String: ancho,
                kCVPixelBufferHeightKey as String: alto,
            ]
        )

        let entradaDeAudio: AVAssetWriterInput?
        if let formatoDeAudio {
            let entrada = AVAssetWriterInput(
                mediaType: .audio,
                outputSettings: nil,
                sourceFormatHint: formatoDeAudio
            )
            guard escritor.canAdd(entrada) else {
                throw ErrorDeConformadoVFR.exportacion("el contenedor no admite el audio original")
            }
            escritor.add(entrada)
            entradaDeAudio = entrada
        } else {
            entradaDeAudio = nil
        }

        guard escritor.startWriting() else {
            throw ErrorDeConformadoVFR.exportacion(
                escritor.error?.localizedDescription ?? "no se pudo iniciar el escritor"
            )
        }
        escritor.startSession(atSourceTime: .zero)
        guard lector.startReading() else {
            escritor.cancelWriting()
            throw ErrorDeConformadoVFR.exportacion(
                lector.error?.localizedDescription ?? "no se pudo iniciar el lector"
            )
        }

        let entradasTerminadas = DispatchGroup()
        entradasTerminadas.enter()
        var siguienteDeVideo: CMSampleBuffer? = salidaDeVideo.copyNextSampleBuffer()
        var ultimoBuffer: CVPixelBuffer?
        var indiceDeFrame: Int64 = 0
        entradaDeVideo.requestMediaDataWhenReady(on: DispatchQueue(label: "editorcito.vfr.video")) {
            while entradaDeVideo.isReadyForMoreMediaData {
                let tiempoDeSalida = timebase.tiempo(indiceDeFrame)
                guard tiempoDeSalida.seconds < duracionDeVideo.seconds else {
                    entradaDeVideo.markAsFinished()
                    entradasTerminadas.leave()
                    return
                }

                // El lector entrega una muestra por cada frame existente, pero
                // no crea una muestra nueva durante un hueco. Se conserva el
                // último buffer presentado y se escribe en cada frame objetivo;
                // así un drop de origen se convierte en un frame repetido, no
                // en un salto del reloj.
                while let muestra = siguienteDeVideo {
                    let pts = CMSampleBufferGetPresentationTimeStamp(muestra)
                    guard pts.isNumeric else {
                        siguienteDeVideo = salidaDeVideo.copyNextSampleBuffer()
                        continue
                    }
                    guard pts.seconds <= tiempoDeSalida.seconds + 1e-6 else { break }
                    if let buffer = CMSampleBufferGetImageBuffer(muestra) {
                        ultimoBuffer = buffer
                    }
                    siguienteDeVideo = salidaDeVideo.copyNextSampleBuffer()
                }

                guard let buffer = ultimoBuffer,
                      adaptador.append(buffer, withPresentationTime: tiempoDeSalida) else {
                    entradaDeVideo.markAsFinished()
                    escritor.cancelWriting()
                    entradasTerminadas.leave()
                    return
                }
                indiceDeFrame += 1
            }
        }

        if let salidaDeAudio, let entradaDeAudio {
            entradasTerminadas.enter()
            entradaDeAudio.requestMediaDataWhenReady(on: DispatchQueue(label: "editorcito.vfr.audio")) {
                while entradaDeAudio.isReadyForMoreMediaData {
                    guard let muestra = salidaDeAudio.copyNextSampleBuffer() else {
                        entradaDeAudio.markAsFinished()
                        entradasTerminadas.leave()
                        return
                    }
                    guard entradaDeAudio.append(muestra) else {
                        entradaDeAudio.markAsFinished()
                        escritor.cancelWriting()
                        entradasTerminadas.leave()
                        return
                    }
                }
            }
        }

        await withCheckedContinuation { (continuacion: CheckedContinuation<Void, Never>) in
            entradasTerminadas.notify(queue: .global(qos: .utility)) {
                escritor.finishWriting {
                    continuacion.resume()
                }
            }
        }
        guard escritor.status == .completed else {
            throw ErrorDeConformadoVFR.exportacion(
                escritor.error?.localizedDescription ?? "el escritor terminó sin completar"
            )
        }
        try instalar(temporal, en: url)
    }

    private static func carpeta() throws -> URL {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let url = base.appendingPathComponent("Editorcito/VFR", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            return url
        } catch {
            throw ErrorDeConformadoVFR.sinCache
        }
    }

    private static func urlDeCache(para medio: MedioResuelto, timebase: Timebase) throws -> URL {
        let valores = try? medio.url.resourceValues(forKeys: [
            .fileSizeKey,
            .contentModificationDateKey,
            .fileResourceIdentifierKey
        ])
        let identidad = [
            medio.url.standardizedFileURL.path,
            String(describing: valores?.fileResourceIdentifier),
            String(describing: valores?.fileSize),
            String(describing: valores?.contentModificationDate),
            "\(timebase.numerador)/\(timebase.denominador)/\(timebase.dropFrame ? 1 : 0)",
            "cfr-writer-v3"
        ].joined(separator: "|")
        let digest = SHA256.hash(data: Data(identidad.utf8))
            .map { String(format: "%02x", $0) }
            .joined()
        return try carpeta().appendingPathComponent("vfr-\(digest).mov")
    }

    private static func instalar(_ temporal: URL, en destino: URL) throws {
        let archivos = FileManager.default
        if archivos.fileExists(atPath: destino.path) {
            _ = try archivos.replaceItemAt(destino, withItemAt: temporal, backupItemName: nil, options: [])
        } else {
            try archivos.moveItem(at: temporal, to: destino)
        }
    }
}

extension MedioResuelto {
    func conMontaje(
        asset: AVURLAsset,
        video: AVAssetTrack,
        audio: AVAssetTrack?,
        duration: CMTime,
        timebase: Timebase
    ) -> MedioResuelto {
        MedioResuelto(
            id: id,
            url: url,
            asset: self.asset,
            pistaDeVideo: pistaDeVideo,
            pistaDeAudio: pistaDeAudio,
            duracion: duracion,
            tamanoNatural: tamanoNatural,
            transformacionPreferida: transformacionPreferida,
            fps: fps,
            esVFR: esVFR,
            assetDeMontaje: asset,
            pistaDeVideoDeMontaje: video,
            pistaDeAudioDeMontaje: audio,
            duracionDeMontaje: duration,
            timebaseDeMontaje: timebase
        )
    }
}
