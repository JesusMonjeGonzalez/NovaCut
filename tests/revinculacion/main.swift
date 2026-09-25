import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

let archivos = FileManager.default
let raiz = archivos.temporaryDirectory
    .appendingPathComponent("editorcito-relink-\(UUID().uuidString)", isDirectory: true)
defer { try? archivos.removeItem(at: raiz) }

func escribir(_ ruta: String, bytes: Int) throws -> URL {
    let url = raiz.appendingPathComponent(ruta)
    try archivos.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(repeating: 9, count: bytes).write(to: url)
    return url
}

// Un rodaje movido de sitio: el archivo aparece en un subdirectorio, con otro
// nombre de carpeta, y hay una copia de menor calidad con el mismo nombre.
let bueno = try escribir("rodaje/dia 2/A001.mov", bytes: 5000)
_ = try escribir("rodaje/comprimidos/A001.mov", bytes: 120)
let audio = try escribir("rodaje/audio/ENTREVISTA.WAV", bytes: 800)
_ = try escribir("rodaje/dia 2/A002.mov", bytes: 4000)

let idVideo = UUID()
let idAudio = UUID()
let idPerdido = UUID()

let resultado = Revinculacion.buscar(
    [
        MedioPendiente(id: idVideo, nombre: "A001.mov", bytes: 5000),
        MedioPendiente(id: idAudio, nombre: "entrevista.wav", bytes: 0),
        MedioPendiente(id: idPerdido, nombre: "B009.mov", bytes: 100),
    ],
    en: raiz
)
// El recorrido devuelve la ruta real (/private/var…), así que se comparan
// rutas resueltas y no cadenas.
func mismoArchivo(_ izquierda: URL?, _ derecha: URL) -> Bool {
    izquierda?.resolvingSymlinksInPath().standardizedFileURL == derecha.resolvingSymlinksInPath().standardizedFileURL
}
comprobar(mismoArchivo(resultado.encontrados[idVideo], bueno), "entre dos archivos con el mismo nombre gana el que coincide en tamaño")
comprobar(mismoArchivo(resultado.encontrados[idAudio], audio), "la búsqueda no distingue mayúsculas")
comprobar(resultado.encontrados[idPerdido] == nil, "no inventa una coincidencia para lo que no está")
comprobar(resultado.sinEncontrar == ["B009.mov"], "informa lo que falta con el nombre pedido")
comprobar(resultado.archivosExaminados == 4 && !resultado.truncado, "recorre la carpeta una sola vez")

// Sin tamaño de referencia, o con uno que no cuadra, decide la profundidad y
// luego el orden alfabético: dos ejecuciones deben dar lo mismo.
let ambiguo = UUID()
let primera = Revinculacion.buscar([MedioPendiente(id: ambiguo, nombre: "A001.mov", bytes: 0)], en: raiz)
let segunda = Revinculacion.buscar([MedioPendiente(id: ambiguo, nombre: "A001.mov", bytes: 77)], en: raiz)
comprobar(primera.encontrados[ambiguo] == segunda.encontrados[ambiguo], "sin tamaño útil el desempate es estable")
comprobar(primera.encontrados[ambiguo]?.lastPathComponent == "A001.mov", "y sigue siendo el archivo pedido")

// Nada que buscar y carpeta inexistente: ni recorrido ni fallo.
let vacio = Revinculacion.buscar([], en: raiz)
comprobar(vacio.encontrados.isEmpty && vacio.archivosExaminados == 0, "sin medios pendientes no se recorre nada")
let ausente = Revinculacion.buscar(
    [MedioPendiente(id: UUID(), nombre: "A001.mov", bytes: 0)],
    en: raiz.appendingPathComponent("no-existe")
)
comprobar(ausente.encontrados.isEmpty && ausente.sinEncontrar == ["A001.mov"], "una carpeta inexistente no encuentra nada y lo dice")

// Tope de recorrido: la carpeta enorme se corta y se avisa, en vez de dejar la
// aplicación colgada recorriendo un disco entero.
let cortado = Revinculacion.buscar(
    [MedioPendiente(id: UUID(), nombre: "A002.mov", bytes: 4000)],
    en: raiz,
    maximoDeArchivos: 1
)
comprobar(cortado.truncado, "avisa cuando la búsqueda se detiene en el tope")
comprobar(cortado.archivosExaminados <= 1, "y no examina más archivos de los permitidos")

let resumen = Revinculacion.resumen(resultado)
comprobar(resumen.contains("2 medios revinculados"), "el resumen cuenta lo revinculado")
comprobar(resumen.contains("1 sin localizar: B009.mov"), "y nombra lo que falta")
comprobar(!Revinculacion.resumen(cortado).isEmpty && Revinculacion.resumen(cortado).contains("demasiado grande"),
          "el resumen avisa de la búsqueda truncada")

if fallos == 0 {
    print("REVINCULACION CORRECTA")
} else {
    print("REVINCULACION ROTA - \(fallos) fallos")
    exit(1)
}
