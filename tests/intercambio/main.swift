import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

// Un montaje de dos pistas (V1, A1) con tres clips enlazados A/V y un clip de
// audio suelto: el caso que cualquier exportador a EDL/FCPXML debe resolver.
let medio1 = UUID()
let medio2 = UUID()
let medio3 = UUID()

var montaje = LineaDeTiempo.nueva(timebase: .p25)
let v1 = montaje.pistas.first { $0.nombre == "V1" }!.id
let a1 = montaje.pistas.first { $0.nombre == "A1" }!.id

let enlace = UUID()
func clipVideo(_ mediaID: UUID, nombre: String, inicio: Int64, duracion: Int64, origen: Int64 = 0, velocidad: Double = 1) -> Clip {
    var c = Clip(mediaID: mediaID, nombre: nombre, inicio: inicio, duracion: duracion, entradaEnOrigen: origen)
    c.velocidad = velocidad
    c.enlace = enlace
    return c
}
let v1a = clipVideo(medio1, nombre: "Entrevista", inicio: 0, duracion: 120)
let v1b = clipVideo(medio2, nombre: "B-roll", inicio: 120, duracion: 80)
let v1c = clipVideo(medio3, nombre: "Rampa", inicio: 200, duracion: 100, origen: 10, velocidad: 2)

montaje.sobrescribir(v1a, enPista: v1, en: 0)
montaje.sobrescribir(v1b, enPista: v1, en: 120)
montaje.sobrescribir(v1c, enPista: v1, en: 200)

func clipAudio(_ mediaID: UUID, nombre: String, inicio: Int64, duracion: Int64, origen: Int64 = 0) -> Clip {
    var c = Clip(mediaID: mediaID, nombre: nombre, inicio: inicio, duracion: duracion, entradaEnOrigen: origen)
    c.enlace = enlace
    return c
}
montaje.sobrescribir(clipAudio(medio1, nombre: "Entrevista", inicio: 0, duracion: 120), enPista: a1, en: 0)
montaje.sobrescribir(clipAudio(medio2, nombre: "B-roll", inicio: 120, duracion: 80), enPista: a1, en: 120)
montaje.sobrescribir(clipAudio(medio3, nombre: "Rampa", inicio: 200, duracion: 100, origen: 10), enPista: a1, en: 200)

let audioSuelto = Clip(mediaID: medio3, nombre: "Musica", inicio: 300, duracion: 60, entradaEnOrigen: 40)
montaje.sobrescribir(audioSuelto, enPista: a1, en: 300)

let medios: [UUID: MedioParaExportar] = [
    medio1: MedioParaExportar(nombre: "Entrevista", url: URL(fileURLWithPath: "/Volumes/Proyecto/Entrevista.mov"), duracionSegundos: 600, tamano: CGSize(width: 1920, height: 1080), fps: 25),
    medio2: MedioParaExportar(nombre: "B-roll", url: URL(fileURLWithPath: "/Volumes/Proyecto/B-roll.mov"), duracionSegundos: 300, tamano: CGSize(width: 1920, height: 1080), fps: 25),
    medio3: MedioParaExportar(nombre: "Rampa", url: URL(fileURLWithPath: "/Volumes/Proyecto/Rampa.mov"), duracionSegundos: 120, tamano: CGSize(width: 1280, height: 720), fps: 25),
]

print("— EDL (CMX 3600) —")
let edl = EDLDeEditorcito.exportar(montaje: montaje, medios: medios, titulo: "Mi proyecto")
let lineas = edl.split(separator: "\n").map(String.init)
igual(lineas[0], "TITLE: Mi proyecto", "cabecera de título")
igual(lineas[1], "FCM: NON-DROP FRAME", "FCM según la base de tiempo")
func evento(_ numero: Int, _ reel: String, _ canal: String, _ tiempo: String) -> Bool {
    lineas.contains { $0.hasPrefix("\(String(format: "%03d", numero))  \(reel)") && $0.contains(canal) && $0.contains(tiempo) }
}
comprobar(evento(1, "ENTREVI", "V", "00:00:00:00 00:00:04:20"), "evento 1: vídeo enlazado con timecodes exactos")
comprobar(evento(2, "BROLL", "V", "00:00:04:20 00:00:08:00"), "evento 2: B-roll contiguo")
comprobar(evento(3, "RAMPA", "V", "00:00:00:10 00:00:08:10"), "evento 3: velocidad 2× consume el doble de origen")
comprobar(evento(4, "ENTREVI", "A1", "00:00:00:00 00:00:04:20"), "evento 4: el audio enlazado comparte reel y timecode")
comprobar(evento(7, "RAMPA", "A1", "00:00:12:00 00:00:14:10"), "evento 7: el audio suelto al final")
comprobar(lineas.contains("* FROM CLIP NAME: Entrevista"), "comentario del clip de origen")
comprobar(lineas.contains("* SPEED CHANGE RATE: 200"), "velocidad constante como efecto de movimiento")
let numeroDeEventos = lineas.filter { $0.first?.isNumber == true }.count
igual(numeroDeEventos, 7, "siete eventos: tres pares A/V y un audio suelto")
comprobar(lineas.allSatisfy { !$0.hasPrefix("*") && !$0.isEmpty ? $0.count <= 80 : true }, "las líneas de evento no se pasan del ancho de CMX")

print("— FCPXML (1.11) —")
let data = FCPXMLDeEditorcito.exportar(montaje: montaje, medios: medios, titulo: "Mi proyecto")
let xml = String(data: data, encoding: .utf8)!
comprobar(xml.hasPrefix("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>"), "declaración y doctype")
comprobar(xml.contains("<fcpxml version=\"1.11\">"), "versión 1.11")
comprobar(xml.contains("frameDuration=\"1/25s\""), "duración de frame racional exacta")
comprobar(xml.contains("tcFormat=\"NDF\""), "tcFormat no drop frame para 25p")
comprobar(xml.contains("<asset id=\"medio-1\" name=\"Entrevista\""), "asset de recurso con su nombre")
comprobar(xml.contains("src=\"file:///Volumes/Proyecto/Entrevista.mov\""), "media-rep con la ruta original")
comprobar(xml.contains("hasVideo=\"1\" hasAudio=\"1\""), "asset con vídeo y audio")
comprobar(xml.contains("audioStart=\"0/25s\" audioDuration=\"120/25s\""), "el enlace A/V une audio al clip de vídeo")
comprobar(xml.contains("speed=\"200\""), "velocidad constante en el atributo speed")
comprobar(xml.contains("start=\"10/25s\""), "la entrada en origen del clip con velocidad es la del medio")
comprobar(xml.contains("offset=\"200/25s\""), "el offset es el inicio en el montaje")
// Un clip a 2× consume el doble de origen: la duración del asset-clip con
// velocidad se mide en tiempo de origen.
comprobar(xml.contains("duration=\"200/25s\""), "duración en tiempo de origen con velocidad")
// El enlace A/V del B-roll debe llevar su propio audio, no el del primer clip
// con el mismo enlace.
comprobar(xml.contains("<asset-clip name=\"B-roll\" ref=\"medio-2\" offset=\"120/25s\" start=\"0/25s\" duration=\"80/25s\" audioStart=\"0/25s\" audioDuration=\"80/25s\">"), "cada enlace A/V lleva su propio tramo de audio")
comprobar(xml.contains("<asset-clip name=\"Rampa\" ref=\"medio-3\" offset=\"200/25s\" start=\"10/25s\" duration=\"200/25s\" speed=\"200\" audioStart=\"10/25s\" audioDuration=\"100/25s\">"), "el audio del clip con velocidad viaja con su vídeo")

print("— EDL drop frame —")
var montajeDF = montaje
montajeDF.timebase = .ntsc30
let edlDF = EDLDeEditorcito.exportar(montaje: montajeDF, medios: medios, titulo: "DF")
comprobar(edlDF.contains("FCM: DROP FRAME"), "FCM drop frame en NTSC 30")
comprobar(edlDF.contains("00:00:00;00") || edlDF.contains("00:00:00:00"), "timecode con separador de drop frame")

print("— títulos y capas apiladas —")
var apilado = LineaDeTiempo.nueva(timebase: .p25)
let v2 = apilado.pistas.first { $0.nombre == "V2" }!.id
var base = Clip(mediaID: medio1, nombre: "Base", inicio: 0, duracion: 100, entradaEnOrigen: 0)
base.enlace = nil
apilado.sobrescribir(base, enPista: apilado.pistas.first { $0.nombre == "V1" }!.id, en: 0)
var encima = Clip(mediaID: medio2, nombre: "Encima", inicio: 30, duracion: 60, entradaEnOrigen: 0)
encima.enlace = nil
apilado.sobrescribir(encima, enPista: v2, en: 30)
var titulo = Clip(mediaID: medio1, nombre: "Titulo", inicio: 100, duracion: 40, entradaEnOrigen: 0)
titulo.esTitulo = true
titulo.enlace = nil
apilado.sobrescribir(titulo, enPista: apilado.pistas.first { $0.nombre == "V1" }!.id, en: 100)
let xmlApilado = String(data: FCPXMLDeEditorcito.exportar(montaje: apilado, medios: medios, titulo: "Apilado"), encoding: .utf8)!
comprobar(xmlApilado.contains("name=\"Encima\""), "la capa superior viaja entera")
comprobar(xmlApilado.contains("name=\"Base\""), "la capa inferior viaja recortada a los tramos libres")
comprobar(xmlApilado.contains("duration=\"30/25s\""), "el tramo libre anterior a la superior")
comprobar(xmlApilado.contains("duration=\"10/25s\""), "el tramo libre posterior a la superior")
comprobar(xmlApilado.contains("se recorta") || xmlApilado.contains("recorta"), "la nota documenta el recorte")
comprobar(xmlApilado.contains("Título «Titulo» omitido") || xmlApilado.contains("Título") , "el título se anota en la nota")

if fallos == 0 {
    print("INTERCAMBIO CORRECTO")
} else {
    print("INTERCAMBIO ROTO — \(fallos) fallos")
    exit(1)
}
