import AVFoundation
import Foundation

struct ResultadoDeLimpiezaDeProxies {
    let archivosEliminados: Int
    let bytesLiberados: Int64
}

/// Foto del disco ocupado por la caché, separando lo que el proyecto abierto
/// necesita de lo que se puede desalojar sin volver a codificar nada.
struct UsoDeCacheDeProxies {
    let archivos: Int
    let bytesTotales: Int64
    let bytesEnUso: Int64

    var bytesDesalojables: Int64 { bytesTotales - bytesEnUso }
}

struct ResultadoDeDesalojoDeProxies {
    let archivosEliminados: Int
    let bytesLiberados: Int64
    let bytesRestantes: Int64
    /// El proyecto abierto ocupa por sí solo más que el límite. No se borra nada
    /// suyo: se avisa, porque desalojarlo obligaría a regenerarlo al instante.
    let excedeElLimite: Bool
}

enum ProxyService {
    private static var carpeta: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Editorcito/Proxies", isDirectory: true)
    }

    /// Huella del medio: tamaño y fecha de modificación. Si el usuario sustituye
    /// el archivo por otra versión, la huella cambia y el proxy anterior deja de
    /// valer, en vez de seguir enseñando material que ya no existe.
    static func huella(de origen: URL) -> UInt64? {
        guard let valores = try? origen.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey]),
              let bytes = valores.fileSize else { return nil }
        let modificado = valores.contentModificationDate.map { Int64($0.timeIntervalSince1970.rounded()) } ?? 0
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        func mezclar(_ valor: UInt64) {
            withUnsafeBytes(of: valor.littleEndian) { crudos in
                for byte in crudos {
                    hash ^= UInt64(byte)
                    hash = hash &* 0x0000_0100_0000_01b3
                }
            }
        }
        mezclar(UInt64(bitPattern: Int64(bytes)))
        mezclar(UInt64(bitPattern: modificado))
        return hash
    }

    /// Nombre del proxy. La tilde separa el medio de su huella: el UUID ya usa
    /// guiones, así que reutilizarlos haría ambigua la lectura del identificador.
    static func nombre(id: UUID, huella: UInt64?) -> String {
        guard let huella else { return "\(id.uuidString).mp4" }
        return "\(id.uuidString)~\(String(format: "%016llx", huella)).mp4"
    }

    /// Identificador del medio al que pertenece un proxy, en el nombre nuevo y
    /// en el anterior sin huella.
    static func idDeProxy(_ url: URL) -> UUID? {
        guard url.pathExtension.lowercased() == "mp4" else { return nil }
        let nombre = url.deletingPathExtension().lastPathComponent
        return UUID(uuidString: String(nombre.prefix(while: { $0 != "~" })))
    }

    static func crear(id: UUID, origen: URL, asset: AVAsset) async throws -> URL {
        let esperado = nombre(id: id, huella: huella(de: origen))
        let destino = carpeta.appendingPathComponent(esperado)
        if FileManager.default.fileExists(atPath: destino.path) {
            marcarUso(destino)
            return destino
        }
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
        await withTaskCancellationHandler {
            await exportador.export()
        } onCancel: {
            exportador.cancelExport()
        }
        try Task.checkCancellation()
        guard exportador.status == .completed else {
            if exportador.status == .cancelled || Task.isCancelled {
                throw ProxyError.cancelled
            }
            throw exportador.error ?? ProxyError.fallo
        }
        try EscrituraAtomica.instalar(temporal, en: destino)
        retirarVersionesAnteriores(id: id, excepto: destino, en: carpeta)
        return destino
    }

    /// Borra los proxies del mismo medio con otra huella: describen una versión
    /// del archivo que ya no está en disco y nadie va a volver a pedirlos.
    @discardableResult
    static func retirarVersionesAnteriores(id: UUID, excepto vigente: URL, en carpeta: URL) -> Int {
        var retirados = 0
        for entrada in inventario(en: carpeta)
        where entrada.id == id && entrada.url.lastPathComponent != vigente.lastPathComponent {
            if (try? FileManager.default.removeItem(at: entrada.url)) != nil { retirados += 1 }
        }
        return retirados
    }

    static func limpiar(conservando ids: Set<UUID>) -> ResultadoDeLimpiezaDeProxies {
        limpiar(conservando: ids, en: carpeta)
    }

    static func uso(conservando ids: Set<UUID>) -> UsoDeCacheDeProxies {
        uso(conservando: ids, en: carpeta)
    }

    static func uso(conservando ids: Set<UUID>, en carpeta: URL) -> UsoDeCacheDeProxies {
        let entradas = inventario(en: carpeta)
        return UsoDeCacheDeProxies(
            archivos: entradas.count,
            bytesTotales: entradas.reduce(0) { $0 + $1.bytes },
            bytesEnUso: entradas.filter { ids.contains($0.id) }.reduce(0) { $0 + $1.bytes }
        )
    }

    @discardableResult
    static func aplicarLimite(bytes limite: Int64, conservando ids: Set<UUID>) -> ResultadoDeDesalojoDeProxies {
        aplicarLimite(bytes: limite, conservando: ids, en: carpeta)
    }

    /// Recorta la caché al presupuesto desalojando primero lo que hace más tiempo
    /// que no se abre. Un límite de cero o negativo desactiva el presupuesto.
    /// Los proxies del proyecto abierto nunca se desalojan.
    @discardableResult
    static func aplicarLimite(
        bytes limite: Int64,
        conservando ids: Set<UUID>,
        en carpeta: URL
    ) -> ResultadoDeDesalojoDeProxies {
        var entradas = inventario(en: carpeta)
        var restantes = entradas.reduce(0) { $0 + $1.bytes }
        guard limite > 0 else {
            return ResultadoDeDesalojoDeProxies(
                archivosEliminados: 0, bytesLiberados: 0, bytesRestantes: restantes, excedeElLimite: false
            )
        }
        // Más viejo primero; el nombre desempata para que dos accesos con la misma
        // marca de tiempo no produzcan desalojos distintos en cada pasada.
        entradas.sort { ($0.acceso, $0.url.lastPathComponent) < ($1.acceso, $1.url.lastPathComponent) }

        var eliminados = 0
        var liberados: Int64 = 0
        for entrada in entradas where restantes > limite {
            guard !ids.contains(entrada.id) else { continue }
            guard (try? FileManager.default.removeItem(at: entrada.url)) != nil else { continue }
            eliminados += 1
            liberados += entrada.bytes
            restantes -= entrada.bytes
        }
        return ResultadoDeDesalojoDeProxies(
            archivosEliminados: eliminados,
            bytesLiberados: liberados,
            bytesRestantes: restantes,
            excedeElLimite: restantes > limite
        )
    }

    /// Refresca la marca de acceso para que reutilizar un proxy lo aleje del
    /// desalojo. El sistema no siempre actualiza `atime` al abrir por AVAsset.
    static func marcarUso(_ url: URL) {
        var valores = URLResourceValues()
        valores.contentAccessDate = Date()
        var url = url
        try? url.setResourceValues(valores)
    }

    static func enGigabytes(_ bytes: Int64) -> String {
        let gb = Double(bytes) / 1_000_000_000
        return gb < 0.1
            ? String(format: "%.0f MB", Double(bytes) / 1_000_000)
            : String(format: "%.2f GB", gb)
    }

    private struct EntradaDeCache {
        let id: UUID
        let url: URL
        let bytes: Int64
        let acceso: Date
    }

    private static func inventario(en carpeta: URL) -> [EntradaDeCache] {
        let claves: [URLResourceKey] = [.fileSizeKey, .contentAccessDateKey, .contentModificationDateKey]
        guard let archivos = try? FileManager.default.contentsOfDirectory(
            at: carpeta, includingPropertiesForKeys: claves, options: [.skipsHiddenFiles]
        ) else { return [] }
        return archivos.compactMap { archivo in
            guard let id = idDeProxy(archivo),
                  let valores = try? archivo.resourceValues(forKeys: Set(claves)) else { return nil }
            return EntradaDeCache(
                id: id,
                url: archivo,
                bytes: Int64(valores.fileSize ?? 0),
                acceso: valores.contentAccessDate ?? valores.contentModificationDate ?? .distantPast
            )
        }
    }

    static func limpiar(conservando ids: Set<UUID>, en carpeta: URL) -> ResultadoDeLimpiezaDeProxies {
        guard let archivosEnCache = try? FileManager.default.contentsOfDirectory(
            at: carpeta,
            includingPropertiesForKeys: [.fileSizeKey],
            options: [.skipsHiddenFiles]
        ) else {
            return ResultadoDeLimpiezaDeProxies(archivosEliminados: 0, bytesLiberados: 0)
        }

        var eliminados = 0
        var bytesLiberados: Int64 = 0
        for archivo in archivosEnCache {
            guard let id = idDeProxy(archivo), !ids.contains(id) else { continue }
            let bytes = (try? archivo.resourceValues(forKeys: [.fileSizeKey]).fileSize).map(Int64.init) ?? 0
            guard (try? FileManager.default.removeItem(at: archivo)) != nil else { continue }
            eliminados += 1
            bytesLiberados += bytes
        }
        return ResultadoDeLimpiezaDeProxies(
            archivosEliminados: eliminados,
            bytesLiberados: bytesLiberados
        )
    }
}

enum ProxyError: LocalizedError {
    case noDisponible
    case cancelled
    case fallo

    var errorDescription: String? {
        switch self {
        case .noDisponible: "macOS no ofrece un exportador compatible para este proxy."
        case .cancelled: "La generación del proxy fue cancelada."
        case .fallo: "No se pudo generar el proxy."
        }
    }
}
