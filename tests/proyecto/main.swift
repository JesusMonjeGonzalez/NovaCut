import Foundation

// Proyecto.swift usa el error de la aplicación; el arnés compila el modelo sin
// App.swift para mantener esta prueba headless.
enum EditorError: Error {
    case invalidProject
}

var fallos = 0

func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion {
        print("  ok  \(mensaje)")
    } else {
        print("  FALLO  \(mensaje)")
        fallos += 1
    }
}

let archivos = FileManager.default
let carpeta = archivos.temporaryDirectory
    .appendingPathComponent("editorcito-proyecto-\(UUID().uuidString)", isDirectory: true)
let carpetaMedia = carpeta.appendingPathComponent("Media", isDirectory: true)
try archivos.createDirectory(at: carpetaMedia, withIntermediateDirectories: true)
defer { try? archivos.removeItem(at: carpeta) }

let medioURL = carpetaMedia.appendingPathComponent("entrevista.mov")
let vecinoURL = carpeta.appendingPathComponent("vecino.mov")
let rutaExterna = URL(fileURLWithPath: "/Volumes/OtroDisco/externo.mov")
comprobar(archivos.createFile(atPath: medioURL.path, contents: Data()), "el medio de prueba existe")
comprobar(archivos.createFile(atPath: vecinoURL.path, contents: Data()), "el vecino de prueba existe")

print("- timebase y rutas relativas -")
comprobar(ProyectoEditorcito.timebaseMasCercana(a: 23.976) == .ntsc24, "23.976 se ancla a 24000/1001")
comprobar(ProyectoEditorcito.timebaseMasCercana(a: 0) == .p25, "un fps invalido usa 25p")
comprobar(
    ProyectoEditorcito.rutaRelativa(de: medioURL, respectoA: carpeta) == "Media/entrevista.mov",
    "una ruta dentro del proyecto se guarda relativa"
)
comprobar(
    ProyectoEditorcito.rutaRelativa(de: rutaExterna, respectoA: carpeta) == nil,
    "una ruta sin raiz comun no se fuerza como relativa"
)

let guardadoBase = ProyectoEditorcito.MedioGuardado(
    id: UUID(), ruta: medioURL.path, rutaRelativa: "Media/entrevista.mov",
    duracion: 10, ancho: 1920, alto: 1080, bytes: 1, fps: 25, vfr: false,
    nombre: medioURL.lastPathComponent, bin: "Todos", subclip: nil
)
let guardadoAbsoluto = ProyectoEditorcito.MedioGuardado(
    id: UUID(), ruta: medioURL.path, rutaRelativa: "Media/que-no-existe.mov",
    duracion: 10, ancho: 1920, alto: 1080, bytes: 1, fps: 25, vfr: false,
    nombre: medioURL.lastPathComponent, bin: "Todos", subclip: nil
)
let guardadoVecino = ProyectoEditorcito.MedioGuardado(
    id: UUID(), ruta: "/Volumes/OtroDisco/vecino.mov", rutaRelativa: nil,
    duracion: 10, ancho: 1920, alto: 1080, bytes: 1, fps: 25, vfr: false,
    nombre: vecinoURL.lastPathComponent, bin: "Todos", subclip: nil
)
let guardadoPerdido = ProyectoEditorcito.MedioGuardado(
    id: UUID(), ruta: "/ruta/que/no/existe.mov", rutaRelativa: nil,
    duracion: 10, ancho: 1920, alto: 1080, bytes: 1, fps: 25, vfr: false,
    nombre: "perdido.mov", bin: "Todos", subclip: nil
)
let proyectoDeRutas = ProyectoEditorcito(
    version: 2, nombre: "Rutas", medios: [guardadoBase, guardadoAbsoluto, guardadoVecino, guardadoPerdido],
    montaje: LineaDeTiempo.nueva(timebase: .p25)
)
comprobar(
    proyectoDeRutas.localizar(guardadoBase, carpetaDelProyecto: carpeta) == medioURL,
    "la ruta relativa resuelve primero el medio"
)
comprobar(
    proyectoDeRutas.localizar(guardadoAbsoluto, carpetaDelProyecto: carpeta) == medioURL,
    "la ruta absoluta sirve como fallback de una relativa rota"
)
comprobar(
    proyectoDeRutas.localizar(guardadoVecino, carpetaDelProyecto: carpeta) == vecinoURL,
    "el nombre del archivo permite localizar un vecino"
)
comprobar(
    proyectoDeRutas.localizar(guardadoPerdido, carpetaDelProyecto: carpeta) == nil,
    "un medio ausente queda sin resolver"
)

print("- round-trip del proyecto v2 -")
var montaje = LineaDeTiempo.nueva(timebase: .ntsc24)
let pista = montaje.pistas.first { $0.tipo == .video }!.id
let clip = Clip(
    mediaID: guardadoBase.id, nombre: "Entrevista", inicio: 12,
    duracion: 48, entradaEnOrigen: 24
)
montaje.sobrescribir(clip, enPista: pista, en: 12)
let proyecto = ProyectoEditorcito(
    version: 2, nombre: "Round trip", medios: [guardadoBase], montaje: montaje
)
let datos = try JSONEncoder().encode(proyecto)
let leido = try ProyectoEditorcito.leer(datos)
comprobar(leido.version == 2, "el proyecto v2 conserva su version")
comprobar(leido.nombre == "Round trip", "el nombre sobrevive al JSON")
comprobar(leido.montaje == montaje, "el montaje multipista sobrevive al JSON")
comprobar(leido.medios.first?.rutaRelativa == "Media/entrevista.mov", "la ruta relativa sobrevive al JSON")

print("- migracion v1 -")
let mediaID = UUID()
let clipID = UUID()
let v1 = """
{
  "version": 1,
  "media": [{
    "id": "\(mediaID.uuidString)",
    "path": "/Volumes/Legacy/entrevista.mov",
    "duration": 12.0,
    "width": 1280.0,
    "height": 720.0,
    "fileSize": 42,
    "frameRate": 23.976
  }],
  "clips": [{
    "id": "\(clipID.uuidString)",
    "mediaID": "\(mediaID.uuidString)",
    "sourceIn": 2.0,
    "sourceOut": 6.0
  }]
}
""".data(using: .utf8)!
let migrado = try ProyectoEditorcito.leer(v1)
let clipMigrado = migrado.montaje.todosLosClips.first?.clip
comprobar(migrado.version == 2, "la version 1 migra a version 2")
comprobar(migrado.medios.first?.rutaRelativa == nil, "la migracion conserva la ruta absoluta como fallback")
comprobar(migrado.montaje.timebase == .ntsc24, "la migracion ancla el fps antiguo al timebase racional")
comprobar(clipMigrado?.id == clipID, "la migracion conserva el id del clip")
comprobar(clipMigrado?.mediaID == mediaID, "la migracion conserva el enlace al medio")
comprobar(clipMigrado?.entradaEnOrigen == migrado.montaje.timebase.frames(segundos: 2), "la entrada se convierte a frames")
comprobar(clipMigrado?.duracion == migrado.montaje.timebase.frames(segundos: 4), "la duracion se convierte a frames")

do {
    _ = try ProyectoEditorcito.leer(Data("proyecto invalido".utf8))
    comprobar(false, "un proyecto invalido se rechaza")
} catch {
    if case EditorError.invalidProject = error {
        comprobar(true, "un proyecto invalido se rechaza")
    } else {
        comprobar(false, "un proyecto invalido se rechaza con el error correcto")
    }
}

if fallos == 0 {
    print("PROYECTO CORRECTO")
} else {
    print("PROYECTO ROTO - \(fallos) fallos")
    exit(1)
}
