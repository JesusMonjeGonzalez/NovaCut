import AVFoundation
import Foundation

/// El archivo `.editorcito`.
///
/// La versión 2 guarda el montaje entero —pistas, marcadores, base de tiempo y
/// todas las propiedades de cada clip— en lugar de una lista plana de recortes.
/// La versión 1 se sigue leyendo y se convierte al abrirla, porque perder los
/// montajes que ya existían para estrenar un formato nuevo es la clase de
/// decisión que hace que nadie vuelva a confiar en guardar.
struct ProyectoEditorcito: Codable {

    struct MedioGuardado: Codable {
        let id: UUID
        /// Ruta absoluta. Sigue estando por compatibilidad y como último recurso.
        let ruta: String
        /// Ruta relativa a la carpeta del proyecto, que es la que sobrevive a mover
        /// la carpeta entera a otro disco o a otro ordenador.
        let rutaRelativa: String?
        let duracion: Double
        let ancho: Double
        let alto: Double
        let bytes: Int64?
        let fps: Double?
        let vfr: Bool?
        /// Nombre del archivo, para poder buscarlo al revincular.
        let nombre: String
        let bin: String?
        /// Si es un subclip, su recorte sobre el medio base.
        let subclip: SubclipOrigen?
    }

    let version: Int
    let nombre: String?
    let medios: [MedioGuardado]
    let montaje: LineaDeTiempo

    // MARK: Versión 1

    private struct ProyectoV1: Codable {
        struct SavedMedia: Codable {
            let id: UUID
            let path: String
            let duration: Double
            let width: Double
            let height: Double
            let fileSize: Int64?
            let frameRate: Double?
        }
        struct SavedClip: Codable {
            let id: UUID
            let mediaID: UUID
            let sourceIn: Double
            let sourceOut: Double
        }
        let version: Int
        let media: [SavedMedia]
        let clips: [SavedClip]
    }

    /// Lee un archivo de cualquier versión conocida.
    static func leer(_ datos: Data) throws -> ProyectoEditorcito {
        if let v2 = try? JSONDecoder().decode(ProyectoEditorcito.self, from: datos), v2.version >= 2 {
            return v2
        }
        guard let v1 = try? JSONDecoder().decode(ProyectoV1.self, from: datos), v1.version == 1 else {
            throw EditorError.invalidProject
        }
        return migrar(v1)
    }

    /// Convierte un montaje secuencial de una pista al modelo multipista.
    ///
    /// La cadencia se toma del primer medio con imagen, y las duraciones se
    /// redondean a frames enteros: un proyecto viejo guardaba segundos en coma
    /// flotante y no hay forma de recuperar más precisión de la que tenía.
    private static func migrar(_ v1: ProyectoV1) -> ProyectoEditorcito {
        let fps = v1.media.compactMap(\.frameRate).first { $0 > 0 } ?? 25
        var linea = LineaDeTiempo.nueva(timebase: timebaseMasCercana(a: fps))
        let pista = linea.pistas.first { $0.tipo == .video }!.id

        var cursor: Int64 = 0
        for guardado in v1.clips {
            let duracion = max(1, linea.timebase.frames(segundos: guardado.sourceOut - guardado.sourceIn))
            let nombre = v1.media.first { $0.id == guardado.mediaID }
                .map { URL(fileURLWithPath: $0.path).deletingPathExtension().lastPathComponent } ?? ""
            let clip = Clip(
                id: guardado.id,
                mediaID: guardado.mediaID,
                nombre: nombre,
                inicio: cursor,
                duracion: duracion,
                entradaEnOrigen: linea.timebase.frames(segundos: guardado.sourceIn)
            )
            linea.sobrescribir(clip, enPista: pista, en: cursor)
            cursor += duracion
        }

        return ProyectoEditorcito(
            version: 2,
            nombre: nil,
            medios: v1.media.map {
                MedioGuardado(
                    id: $0.id, ruta: $0.path, rutaRelativa: nil,
                    duracion: $0.duration, ancho: $0.width, alto: $0.height,
                    bytes: $0.fileSize, fps: $0.frameRate, vfr: nil,
                    nombre: URL(fileURLWithPath: $0.path).lastPathComponent,
                    bin: nil, subclip: nil
                )
            },
            montaje: linea
        )
    }

    /// La cadencia estándar más parecida a la que trae el material.
    ///
    /// Un archivo declara `23.976023`, y quedarse con ese decimal en el proyecto es
    /// justo lo que produce la deriva que el modelo intenta evitar. Se ancla a la
    /// fracción exacta más cercana.
    static func timebaseMasCercana(a fps: Double) -> Timebase {
        guard fps > 0 else { return .p25 }
        return Timebase.habituales.min { abs($0.fps - fps) < abs($1.fps - fps) } ?? .p25
    }

    // MARK: Revinculación

    /// Busca el archivo de un medio aunque el proyecto haya cambiado de sitio.
    ///
    /// Se prueba por este orden: la ruta relativa a la carpeta del proyecto, la
    /// ruta absoluta original, y el mismo nombre de archivo dentro de la carpeta
    /// del proyecto. Con eso, mover el proyecto entero —el caso normal— deja de
    /// romper nada, que es la queja número uno con los archivos de montaje.
    func localizar(_ medio: MedioGuardado, carpetaDelProyecto: URL?) -> URL? {
        let gestor = FileManager.default

        if let relativa = medio.rutaRelativa, let carpeta = carpetaDelProyecto {
            let candidato = carpeta.appendingPathComponent(relativa).standardizedFileURL
            if gestor.fileExists(atPath: candidato.path) { return candidato }
        }
        if gestor.fileExists(atPath: medio.ruta) { return URL(fileURLWithPath: medio.ruta) }
        if let carpeta = carpetaDelProyecto {
            let vecino = carpeta.appendingPathComponent(medio.nombre)
            if gestor.fileExists(atPath: vecino.path) { return vecino }
        }
        return nil
    }

    static func rutaRelativa(de archivo: URL, respectoA carpeta: URL?) -> String? {
        guard let carpeta else { return nil }
        let base = carpeta.standardizedFileURL.pathComponents
        let destino = archivo.standardizedFileURL.pathComponents
        var comunes = 0
        while comunes < base.count, comunes < destino.count, base[comunes] == destino[comunes] { comunes += 1 }
        // Sin nada en común no hay ruta relativa útil: están en discos distintos.
        guard comunes > 1 else { return nil }
        let subir = Array(repeating: "..", count: base.count - comunes)
        return (subir + destino[comunes...]).joined(separator: "/")
    }
}
