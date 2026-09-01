import AVFoundation
import CryptoKit
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
               medio.assetDeMontaje != nil,
               medio.pistaDeVideoDeMontaje != nil {
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
           medio.assetDeMontaje != nil,
           medio.pistaDeVideoDeMontaje != nil {
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

        // Una composición de una sola capa con frameDuration fijo obliga al
        // exportador a elegir una muestra para cada instante CFR. La matriz se
        // deja neutra: el montaje aplica después la orientación original una sola
        // vez, igual para el asset original y para el intermediario.
        let videoComposition = AVMutableVideoComposition()
        videoComposition.renderSize = tamano
        videoComposition.frameDuration = timebase.tiempo(1)
        let instruccion = AVMutableVideoCompositionInstruction()
        instruccion.timeRange = CMTimeRange(start: .zero, duration: duration)
        let capa = AVMutableVideoCompositionLayerInstruction(assetTrack: video)
        capa.setTransform(.identity, at: .zero)
        instruccion.layerInstructions = [capa]
        videoComposition.instructions = [instruccion]

        guard let exportador = AVAssetExportSession(
            asset: medio.asset,
            presetName: AVAssetExportPresetHighestQuality
        ) else {
            throw ErrorDeConformadoVFR.exportacion("el preset de máxima calidad no está disponible")
        }

        let temporal = url.deletingLastPathComponent()
            .appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString).part")
        try? FileManager.default.removeItem(at: temporal)
        defer { try? FileManager.default.removeItem(at: temporal) }

        exportador.outputURL = temporal
        exportador.outputFileType = .mov
        exportador.videoComposition = videoComposition
        exportador.shouldOptimizeForNetworkUse = false
        await exportador.export()
        guard exportador.status == .completed else {
            throw ErrorDeConformadoVFR.exportacion(
                exportador.error?.localizedDescription ?? "el exportador terminó sin completar"
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
            "\(timebase.numerador)/\(timebase.denominador)/\(timebase.dropFrame ? 1 : 0)"
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
