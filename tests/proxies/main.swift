import Foundation

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
    .appendingPathComponent("editorcito-proxies-\(UUID().uuidString)", isDirectory: true)
try archivos.createDirectory(at: carpeta, withIntermediateDirectories: true)
defer { try? archivos.removeItem(at: carpeta) }

let conservar = UUID()
let eliminar = UUID()
let noEsProxy = carpeta.appendingPathComponent("no-borrar.txt")
let conservarURL = carpeta.appendingPathComponent("\(conservar.uuidString).mp4")
let eliminarURL = carpeta.appendingPathComponent("\(eliminar.uuidString).mp4")
try Data(repeating: 1, count: 11).write(to: conservarURL)
try Data(repeating: 2, count: 37).write(to: eliminarURL)
try Data("no es un proxy".utf8).write(to: noEsProxy)

let resultado = ProxyService.limpiar(conservando: [conservar], en: carpeta)
comprobar(resultado.archivosEliminados == 1, "limpia solo el proxy que no pertenece al proyecto")
comprobar(resultado.bytesLiberados == 37, "informa los bytes liberados")
comprobar(archivos.fileExists(atPath: conservarURL.path), "conserva el proxy del proyecto actual")
comprobar(!archivos.fileExists(atPath: eliminarURL.path), "el proxy huérfano desaparece")
comprobar(archivos.fileExists(atPath: noEsProxy.path), "no borra archivos ajenos a la caché")

// ── Presupuesto de disco ──
// Cada proxy pesa 1000 bytes y se le fija una fecha de acceso distinta para
// poder afirmar el orden de desalojo sin depender del reloj del sistema.
let cache = archivos.temporaryDirectory
    .appendingPathComponent("editorcito-limite-\(UUID().uuidString)", isDirectory: true)
try archivos.createDirectory(at: cache, withIntermediateDirectories: true)
defer { try? archivos.removeItem(at: cache) }

let ids = (0..<4).map { _ in UUID() }
var urls: [UUID: URL] = [:]
func poblarCache() throws {
    for archivo in (try? archivos.contentsOfDirectory(at: cache, includingPropertiesForKeys: nil)) ?? [] {
        try archivos.removeItem(at: archivo)
    }
    for (posicion, id) in ids.enumerated() {
        var url = cache.appendingPathComponent("\(id.uuidString).mp4")
        try Data(repeating: 7, count: 1000).write(to: url)
        var valores = URLResourceValues()
        // ids[0] es el más antiguo; ids[3] el más reciente.
        valores.contentAccessDate = Date(timeIntervalSince1970: 1_000 + Double(posicion) * 60)
        try url.setResourceValues(valores)
        urls[id] = url
    }
    try Data("ajeno".utf8).write(to: cache.appendingPathComponent("apuntes.txt"))
}
func sigueEnDisco(_ id: UUID) -> Bool { archivos.fileExists(atPath: urls[id]!.path) }

try poblarCache()
let uso = ProxyService.uso(conservando: [ids[3]], en: cache)
comprobar(uso.archivos == 4 && uso.bytesTotales == 4000, "el inventario ignora archivos ajenos a la caché")
comprobar(uso.bytesEnUso == 1000 && uso.bytesDesalojables == 3000, "separa lo que el proyecto abierto necesita")

let recorte = ProxyService.aplicarLimite(bytes: 2500, conservando: [], en: cache)
comprobar(recorte.archivosEliminados == 2 && recorte.bytesLiberados == 2000, "desaloja lo justo para entrar en el límite")
comprobar(recorte.bytesRestantes == 2000 && !recorte.excedeElLimite, "informa lo que queda ocupado")
comprobar(!sigueEnDisco(ids[0]) && !sigueEnDisco(ids[1]), "desaloja por acceso más antiguo primero")
comprobar(sigueEnDisco(ids[2]) && sigueEnDisco(ids[3]), "se detiene en cuanto la caché cabe, sin vaciarla")
comprobar(archivos.fileExists(atPath: cache.appendingPathComponent("apuntes.txt").path), "el presupuesto no borra archivos ajenos")

try poblarCache()
let protegidos = ProxyService.aplicarLimite(bytes: 1500, conservando: [ids[0], ids[1]], en: cache)
comprobar(sigueEnDisco(ids[0]) && sigueEnDisco(ids[1]), "los proxies del proyecto abierto nunca se desalojan")
comprobar(protegidos.archivosEliminados == 2 && protegidos.bytesRestantes == 2000, "desaloja todo lo desalojable antes de rendirse")
comprobar(protegidos.excedeElLimite, "avisa cuando el proyecto abierto no cabe en el límite")

try poblarCache()
let sinLimite = ProxyService.aplicarLimite(bytes: 0, conservando: [], en: cache)
comprobar(sinLimite.archivosEliminados == 0 && !sinLimite.excedeElLimite, "un límite de cero desactiva el presupuesto")
comprobar(ProxyService.aplicarLimite(bytes: -1, conservando: [], en: cache).archivosEliminados == 0, "un límite negativo tampoco borra")
comprobar(ProxyService.aplicarLimite(bytes: 4000, conservando: [], en: cache).archivosEliminados == 0, "no desaloja si ya cabe justo en el límite")
comprobar(ids.allSatisfy(sigueEnDisco), "ninguna pasada inocua deja la caché tocada")

// ── Identidad del proxy ──
// El nombre lleva la huella del medio: si el archivo cambia en disco, el proxy
// anterior deja de valer en vez de seguir enseñando lo que ya no existe.
let medio = cache.appendingPathComponent("toma.mov")
try Data(repeating: 3, count: 4096).write(to: medio)
var atributos = URLResourceValues()
atributos.contentModificationDate = Date(timeIntervalSince1970: 5_000)
var medioMutable = medio
try medioMutable.setResourceValues(atributos)

let medioID = UUID()
let huellaInicial = ProxyService.huella(de: medio)
comprobar(huellaInicial != nil, "calcula la huella de un medio existente")
comprobar(ProxyService.huella(de: medio) == huellaInicial, "la huella es estable si el archivo no cambia")
let nombreInicial = ProxyService.nombre(id: medioID, huella: huellaInicial)
comprobar(nombreInicial.hasPrefix(medioID.uuidString + "~") && nombreInicial.hasSuffix(".mp4"), "el nombre enlaza medio y huella")

try Data(repeating: 4, count: 8192).write(to: medioMutable)
atributos.contentModificationDate = Date(timeIntervalSince1970: 9_000)
try medioMutable.setResourceValues(atributos)
let huellaNueva = ProxyService.huella(de: medio)
comprobar(huellaNueva != huellaInicial, "sustituir el medio cambia la huella")
comprobar(ProxyService.nombre(id: medioID, huella: huellaNueva) != nombreInicial, "y por tanto el proxy que le toca")
comprobar(ProxyService.huella(de: cache.appendingPathComponent("no-existe.mov")) == nil, "un medio ausente no tiene huella")
comprobar(ProxyService.nombre(id: medioID, huella: nil) == "\(medioID.uuidString).mp4", "sin huella se conserva el nombre anterior")

let viejo = cache.appendingPathComponent(nombreInicial)
let vigente = cache.appendingPathComponent(ProxyService.nombre(id: medioID, huella: huellaNueva))
let ajeno = cache.appendingPathComponent(ProxyService.nombre(id: UUID(), huella: huellaInicial))
for url in [viejo, vigente, ajeno] { try Data(repeating: 5, count: 100).write(to: url) }
comprobar(ProxyService.idDeProxy(viejo) == medioID, "lee el medio del nombre con huella")
comprobar(ProxyService.idDeProxy(cache.appendingPathComponent("\(medioID.uuidString).mp4")) == medioID, "lee también el nombre anterior")
comprobar(ProxyService.idDeProxy(cache.appendingPathComponent("apuntes.txt")) == nil, "un archivo ajeno no tiene medio")
comprobar(ProxyService.retirarVersionesAnteriores(id: medioID, excepto: vigente, en: cache) == 1, "retira solo la versión caducada")
comprobar(!archivos.fileExists(atPath: viejo.path), "la versión caducada desaparece")
comprobar(archivos.fileExists(atPath: vigente.path), "la versión vigente se conserva")
comprobar(archivos.fileExists(atPath: ajeno.path), "no toca proxies de otros medios")
comprobar(ProxyService.uso(conservando: [medioID], en: cache).bytesEnUso == 100, "el inventario reconoce el nombre con huella")

for url in [vigente, ajeno, medio] { try? archivos.removeItem(at: url) }

let vacia = cache.appendingPathComponent("sin-crear", isDirectory: true)
comprobar(ProxyService.uso(conservando: [], en: vacia).archivos == 0, "una caché inexistente se informa vacía")
comprobar(ProxyService.aplicarLimite(bytes: 10, conservando: [], en: vacia).bytesRestantes == 0, "una caché inexistente no rompe el recorte")

let marcado = urls[ids[0]]!
ProxyService.marcarUso(marcado)
let refrescado = try marcado.resourceValues(forKeys: [.contentAccessDateKey]).contentAccessDate ?? .distantPast
comprobar(refrescado.timeIntervalSince1970 > 1_500, "reutilizar un proxy refresca su marca de acceso")
let trasReutilizar = ProxyService.aplicarLimite(bytes: 3500, conservando: [], en: cache)
comprobar(trasReutilizar.archivosEliminados == 1 && !sigueEnDisco(ids[1]), "tras reutilizarlo deja de ser el primero en caer")

comprobar(ProxyService.enGigabytes(2_500_000_000) == "2.50 GB", "formatea gigabytes")
comprobar(ProxyService.enGigabytes(37_000_000) == "37 MB", "por debajo de 0,1 GB informa en megabytes")

if fallos == 0 {
    print("PROXIES CORRECTO")
} else {
    print("PROXIES ROTO - \(fallos) fallos")
    exit(1)
}
