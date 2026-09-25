import Foundation

/// Un medio que el proyecto no encuentra y hay que localizar.
struct MedioPendiente {
    let id: UUID
    let nombre: String
    /// Tamaño registrado al guardar el proyecto. Cero cuando no se conoce.
    let bytes: Int64

    init(id: UUID, nombre: String, bytes: Int64 = 0) {
        self.id = id
        self.nombre = nombre
        self.bytes = bytes
    }
}

struct ResultadoDeRevinculacion {
    let encontrados: [UUID: URL]
    /// Nombres que no aparecieron en la carpeta, en el orden en que se pidieron.
    let sinEncontrar: [String]
    let archivosExaminados: Int
    /// El recorrido se detuvo en el tope: la carpeta elegida es demasiado grande
    /// y puede que el archivo estuviera más allá.
    let truncado: Bool
}

/// Localiza en una carpeta los medios que el proyecto ha perdido.
///
/// Mover un rodaje entero de disco es la forma normal de romper un proyecto, y
/// revincular archivo por archivo no es una respuesta razonable cuando son
/// doscientos. Aquí se recorre la carpeta una sola vez y se resuelve todo junto.
enum Revinculacion {
    /// Coincidencia por nombre, sin distinguir mayúsculas porque el sistema de
    /// archivos de macOS tampoco las distingue por defecto. Entre varios
    /// candidatos con el mismo nombre gana el que además coincide en tamaño;
    /// después, el menos profundo; y en último término el orden alfabético, para
    /// que dos ejecuciones sobre la misma carpeta den siempre lo mismo.
    static func buscar(
        _ pendientes: [MedioPendiente],
        en raiz: URL,
        maximoDeArchivos: Int = 200_000
    ) -> ResultadoDeRevinculacion {
        guard !pendientes.isEmpty else {
            return ResultadoDeRevinculacion(encontrados: [:], sinEncontrar: [], archivosExaminados: 0, truncado: false)
        }
        var buscados: [String: [MedioPendiente]] = [:]
        for pendiente in pendientes {
            buscados[pendiente.nombre.lowercased(), default: []].append(pendiente)
        }

        var candidatos: [String: [(url: URL, bytes: Int64, profundidad: Int)]] = [:]
        var examinados = 0
        var truncado = false
        let claves: [URLResourceKey] = [.isRegularFileKey, .fileSizeKey, .nameKey]
        let recorrido = FileManager.default.enumerator(
            at: raiz,
            includingPropertiesForKeys: claves,
            options: [.skipsHiddenFiles, .skipsPackageDescendants]
        )
        while let archivo = recorrido?.nextObject() as? URL {
            if examinados >= maximoDeArchivos {
                truncado = true
                break
            }
            guard let valores = try? archivo.resourceValues(forKeys: Set(claves)),
                  valores.isRegularFile == true else { continue }
            examinados += 1
            let nombre = (valores.name ?? archivo.lastPathComponent).lowercased()
            guard buscados[nombre] != nil else { continue }
            candidatos[nombre, default: []].append(
                (archivo, Int64(valores.fileSize ?? 0), archivo.pathComponents.count)
            )
        }

        var encontrados: [UUID: URL] = [:]
        var sinEncontrar: [String] = []
        for pendiente in pendientes {
            let posibles = candidatos[pendiente.nombre.lowercased()] ?? []
            guard let elegido = posibles.min(by: { izquierda, derecha in
                let coincideIzquierda = pendiente.bytes > 0 && izquierda.bytes == pendiente.bytes
                let coincideDerecha = pendiente.bytes > 0 && derecha.bytes == pendiente.bytes
                if coincideIzquierda != coincideDerecha { return coincideIzquierda }
                if izquierda.profundidad != derecha.profundidad { return izquierda.profundidad < derecha.profundidad }
                return izquierda.url.path < derecha.url.path
            }) else {
                sinEncontrar.append(pendiente.nombre)
                continue
            }
            encontrados[pendiente.id] = elegido.url
        }
        return ResultadoDeRevinculacion(
            encontrados: encontrados,
            sinEncontrar: sinEncontrar,
            archivosExaminados: examinados,
            truncado: truncado
        )
    }

    /// Resumen para la barra de estado.
    static func resumen(_ resultado: ResultadoDeRevinculacion) -> String {
        let encontrados = resultado.encontrados.count
        var texto = "\(encontrados) medio\(encontrados == 1 ? "" : "s") revinculado\(encontrados == 1 ? "" : "s")"
        if !resultado.sinEncontrar.isEmpty {
            let muestra = resultado.sinEncontrar.prefix(3).joined(separator: ", ")
            texto += " · \(resultado.sinEncontrar.count) sin localizar: \(muestra)"
        }
        if resultado.truncado {
            texto += " · carpeta demasiado grande, la búsqueda se detuvo antes de terminar"
        }
        return texto
    }
}
