import AVFoundation
import Foundation

/// Renderiza clips anidados a archivos en la caché de la aplicación.
///
/// AVFoundation no puede anidar composiciones: una composición dentro de otra
/// pierde sus instrucciones de vídeo (color, LUT, transformaciones) porque la
/// composición interior es un simple asset para la exterior. La alternativa
/// honesta es la que usan los editores que pre-renderizan: el nido se
/// convierte en un archivo de vídeo —con todas sus capas y su mezcla ya
/// resueltas— y el clip anidado es un clip normal cuyo medio es ese archivo.
/// Se guarda la línea de tiempo interior en el clip para poder re-renderizar
/// cuando el interior cambie y para poder desanidar.
enum ServicioDeNidos {

    /// Carpeta de la caché donde viven los nidos renderizados.
    static func carpetaDeNidos() throws -> URL {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let carpeta = base.appendingPathComponent("Editorcito/Nidos", isDirectory: true)
        try FileManager.default.createDirectory(at: carpeta, withIntermediateDirectories: true)
        return carpeta
    }

    /// Renderiza una línea de tiempo a un archivo de vídeo en la caché.
    ///
    /// Usa calidad master (ProRes en `mov` en este sistema): el nido es una
    /// pieza intermedia, y re-renderizar un nido de otro nido con pérdida a
    /// cada nivel degradaría la imagen dos veces.
    static func renderizar(
        _ linea: LineaDeTiempo,
        medios: [UUID: MedioResuelto],
        id: UUID
    ) async throws -> URL {
        let preparados = await ConformadorVFR.preparar(medios: medios, para: linea.timebase)
        let render = ConstructorDeMontaje.construir(linea, medios: preparados.medios)
        guard !render.estaVacio else { throw EditorError.exportFailed }
        let url = try carpetaDeNidos()
            .appendingPathComponent("nido-\(id.uuidString)")
            .appendingPathExtension("mov")

        guard let session = AVAssetExportSession(
            asset: render.composicion,
            presetName: AVAssetExportPresetHighestQuality
        ) else { throw EditorError.exportUnavailable }

        let temporal = EscrituraAtomica.temporal(para: url)
        try? FileManager.default.removeItem(at: temporal)
        defer { try? FileManager.default.removeItem(at: temporal) }
        session.outputURL = temporal
        session.outputFileType = .mov
        session.videoComposition = render.composicionDeVideo
        session.audioMix = render.mezclaDeAudio
        session.shouldOptimizeForNetworkUse = false
        await session.export()
        guard session.status == .completed else {
            throw EditorError.exportFailed
        }
        try EscrituraAtomica.instalar(temporal, en: url)
        return url
    }
}
