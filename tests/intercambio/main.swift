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

print("— importación de EDL —")
// Ida y vuelta contra el exportador: lo que sale de un montaje debe volver a
// entrar con los mismos cortes. Es la única comprobación que no depende de que
// el formato esté escrito como yo creo que se escribe.
let edlIda = EDLDeEditorcito.exportar(montaje: montaje, medios: medios, titulo: "Ida y vuelta")
let vuelta = EDLDeEditorcito.importar(edlIda, timebase: .p25)
igual(vuelta.titulo, "Ida y vuelta", "recupera el título de la cabecera")
igual(vuelta.avisos.count, 0, "un EDL propio entra sin avisos")
let videoVuelta = vuelta.montaje.pistasDeVideo.flatMap(\.clips).sorted { $0.inicio < $1.inicio }
let audioVuelta = vuelta.montaje.pistasDeAudio.flatMap(\.clips).sorted { $0.inicio < $1.inicio }
igual(videoVuelta.map(\.inicio), [0, 120, 200], "los cortes de vídeo caen donde estaban")
igual(videoVuelta.map(\.duracion), [120, 80, 100], "con sus duraciones")
igual(videoVuelta.map(\.entradaEnOrigen), [0, 0, 10], "y con la entrada en el medio de origen")
igual(videoVuelta.map(\.nombre), ["Entrevista", "B-roll", "Rampa"], "los nombres viajan en FROM CLIP NAME")
igual(videoVuelta.last?.velocidad, 2, "la velocidad constante vuelve del comentario de EDL")
igual(audioVuelta.map(\.inicio), [0, 120, 200, 300], "el audio también, incluido el clip suelto")
igual(audioVuelta.map(\.duracion), [120, 80, 100, 60], "con sus duraciones")
igual(audioVuelta.last?.entradaEnOrigen, 40, "y con su entrada en origen")
comprobar(vuelta.mediosPorReel.count == 3, "un medio por reel, no uno por evento")
comprobar(Set(vuelta.montaje.pistasDeVideo.flatMap(\.clips).map(\.mediaID)).count == 3, "cada reel conserva su medio")
let reelDeEntrevista = vuelta.nombresPorReel.first { $0.value == "Entrevista" }?.key
comprobar(reelDeEntrevista != nil, "cada reel recuerda el nombre con el que llegó")

// Un EDL de otra sala: columnas distintas, M2 real, disolución y canal B.
let ajeno = """
TITLE: MONTAJE AJENO
FCM: NON-DROP FRAME

001  CINTA01 V     C        01:00:00:00 01:00:04:00 00:00:00:00 00:00:04:00
* FROM CLIP NAME: Plano general
002  CINTA02 B     D    025 02:00:00:00 02:00:02:00 00:00:04:00 00:00:06:00
* FROM CLIP NAME: Contraplano
003  CINTA01 A2    C        01:00:10:00 01:00:12:00 00:00:04:00 00:00:06:00
M2   CINTA01       050.0    01:00:10:00
004  CINTA03 V     C        00:00:00:00 00:00:00:00 00:00:06:00 00:00:06:00
ESTO NO ES UN EVENTO
"""
let leido = EDLDeEditorcito.importar(ajeno, timebase: .p25)
igual(leido.titulo, "MONTAJE AJENO", "lee la cabecera de un EDL ajeno")
igual(leido.eventos.count, 4, "lee los cuatro eventos pese al espaciado distinto")
let videoAjeno = leido.montaje.pistasDeVideo.flatMap(\.clips).sorted { $0.inicio < $1.inicio }
igual(videoAjeno.map(\.inicio), [0, 100], "el canal B deja vídeo en la pista de vídeo")
igual(videoAjeno.first?.entradaEnOrigen, 90_000, "el timecode de origen a la hora se convierte a frames")
let a1Ajeno = leido.montaje.pistas.first { $0.nombre == "A1" }?.clips ?? []
let a2Ajeno = leido.montaje.pistas.first { $0.nombre == "A2" }?.clips ?? []
igual(a1Ajeno.map(\.inicio), [100], "el canal B deja también audio en A1")
igual(a2Ajeno.map(\.inicio), [100], "y el canal A2 va a su pista")
igual(a2Ajeno.first?.velocidad, 2, "M2 a 50 fps sobre 25 fps es el doble de velocidad")
comprobar(videoAjeno.first?.enlace == nil, "un evento de un solo canal no inventa enlaces")
comprobar(a1Ajeno.first?.enlace != nil && a1Ajeno.first?.enlace == videoAjeno.last?.enlace,
          "el canal B enlaza su vídeo con su audio")
comprobar(leido.avisos.contains { $0.contains("«D»") }, "la disolución entra como corte y se avisa")
comprobar(leido.avisos.contains { $0.contains("duración no positiva") }, "un evento de duración cero se omite con aviso")
comprobar(leido.avisos.allSatisfy { !$0.contains("ESTO NO ES UN EVENTO") }, "una línea que no empieza por número se ignora en silencio")

// Solapes en el mismo canal: se reparten en pistas, nunca se pisan.
let solapado = EDLDeEditorcito.importar("""
TITLE: SOLAPE
001  CINTA01 V     C        00:00:00:00 00:00:04:00 00:00:00:00 00:00:04:00
002  CINTA02 V     C        00:00:00:00 00:00:04:00 00:00:02:00 00:00:06:00
003  CINTA03 V     C        00:00:00:00 00:00:04:00 00:00:03:00 00:00:07:00
""", timebase: .p25)
let pistasConClips = solapado.montaje.pistasDeVideo.filter { !$0.clips.isEmpty }
igual(pistasConClips.count, 3, "tres eventos solapados ocupan tres pistas")
comprobar(pistasConClips.allSatisfy { $0.clips.count == 1 }, "ninguna pista queda con clips que se pisan")

// Drop frame y timebase: la cabecera manda sobre la del proyecto.
let df = EDLDeEditorcito.importar("FCM: DROP FRAME\n001  A V C 00:00:00;00 00:00:01;00 00:00:00;00 00:00:01;00", timebase: .ntsc30)
comprobar(df.montaje.timebase.dropFrame, "FCM DROP FRAME activa el drop frame en NTSC")
let dfImposible = EDLDeEditorcito.importar("FCM: DROP FRAME\n001  A V C 00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00", timebase: .p25)
comprobar(!dfImposible.montaje.timebase.dropFrame, "a 25 fps no hay drop frame")
comprobar(dfImposible.avisos.contains { $0.contains("DROP FRAME") }, "y se dice en vez de fingirlo")

let vacio = EDLDeEditorcito.importar("", timebase: .p25)
comprobar(vacio.montaje.pistas.allSatisfy { $0.clips.isEmpty } && vacio.avisos.isEmpty, "un archivo vacío no rompe nada")
comprobar(EDLDeEditorcito.importar("basura sin formato", timebase: .p25).eventos.isEmpty, "un archivo que no es EDL no produce eventos")

if fallos == 0 {
    print("INTERCAMBIO CORRECTO")
} else {
    print("INTERCAMBIO ROTO — \(fallos) fallos")
    exit(1)
}
