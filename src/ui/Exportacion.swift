import AVFoundation
import CoreGraphics
import Foundation

enum PresetExportacion: String, CaseIterable, Identifiable, Sendable {
    case mp4
    case hevc
    case vertical
    case prores
    case audio
    case master

    var id: String { rawValue }

    var nombre: String {
        switch self {
        case .mp4: "MP4 · H.264 1080p"
        case .hevc: "MP4 · H.265/HEVC 1080p"
        case .vertical: "MP4 vertical · 1080×1920"
        case .prores: "ProRes 422 · máxima calidad"
        case .audio: "Audio · M4A"
        case .master: "Master · máxima calidad"
        }
    }

    var extensionDeArchivo: String {
        switch self {
        case .audio: "m4a"
        case .prores, .master: "mov"
        default: "mp4"
        }
    }

    var tipoDeArchivo: AVFileType {
        switch self {
        case .audio: .m4a
        case .prores, .master: .mov
        default: .mp4
        }
    }

    var presetAV: String {
        switch self {
        case .audio: AVAssetExportPresetAppleM4A
        case .hevc: AVAssetExportPresetHEVCHighestQuality
        case .prores: AVAssetExportPresetAppleProRes422LPCM
        case .master: AVAssetExportPresetHighestQuality
        case .mp4, .vertical: AVAssetExportPresetHighestQuality
        }
    }

    var tamano: CGSize? {
        switch self {
        case .vertical: CGSize(width: 1080, height: 1920)
        default: nil
        }
    }

    var esSoloAudio: Bool { self == .audio }
}

struct TrabajoDeExportacion: Identifiable {
    let id = UUID()
    let preset: PresetExportacion
    let url: URL
    /// Instantáneas del documento y sus medios al crear el trabajo. Una edición
    /// posterior no debe cambiar silenciosamente lo que ya estaba en la cola.
    let montaje: LineaDeTiempo
    let medios: [UUID: MedioResuelto]
    /// Ganancia de máster en dB que la normalización decidió, si la hay.
    let ganancia: Double?
}

/// Escribe primero fuera del destino y solo sustituye el archivo anterior cuando
/// el exportador terminó. Un fallo o una cancelación no puede destruir un master
/// que ya existía.
enum EscrituraAtomica {
    static func temporal(para destino: URL) -> URL {
        destino.deletingLastPathComponent()
            .appendingPathComponent(".\(destino.lastPathComponent).\(UUID().uuidString).part")
    }

    static func instalar(_ temporal: URL, en destino: URL) throws {
        let archivos = FileManager.default
        if archivos.fileExists(atPath: destino.path) {
            _ = try archivos.replaceItemAt(
                destino,
                withItemAt: temporal,
                backupItemName: nil,
                options: []
            )
        } else {
            try archivos.moveItem(at: temporal, to: destino)
        }
    }
}
