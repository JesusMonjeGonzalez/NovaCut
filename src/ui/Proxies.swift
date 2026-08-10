import AVFoundation
import Foundation

enum ProxyService {
    private static var carpeta: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Editorcito/Proxies", isDirectory: true)
    }

    static func crear(id: UUID, asset: AVAsset) async throws -> URL {
        let destino = carpeta.appendingPathComponent("\(id.uuidString).mp4")
        if FileManager.default.fileExists(atPath: destino.path) { return destino }
        try FileManager.default.createDirectory(at: carpeta, withIntermediateDirectories: true)

        let temporal = EscrituraAtomica.temporal(para: destino)
        try? FileManager.default.removeItem(at: temporal)
        defer { try? FileManager.default.removeItem(at: temporal) }

        guard let exportador = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetMediumQuality) else {
            throw ProxyError.noDisponible
        }
        exportador.outputURL = temporal
        exportador.outputFileType = .mp4
        exportador.shouldOptimizeForNetworkUse = false
        await exportador.export()
        guard exportador.status == .completed else {
            throw exportador.error ?? ProxyError.fallo
        }
        try EscrituraAtomica.instalar(temporal, en: destino)
        return destino
    }
}

enum ProxyError: LocalizedError {
    case noDisponible
    case fallo

    var errorDescription: String? {
        switch self {
        case .noDisponible: "macOS no ofrece un exportador compatible para este proxy."
        case .fallo: "No se pudo generar el proxy."
        }
    }
}
