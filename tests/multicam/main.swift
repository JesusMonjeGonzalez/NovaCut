import AVFoundation
import CoreGraphics
import Foundation

// Prueba la multicámara sobre archivos reales: tres «cámaras» generadas con
// pads de silencio distintos (crearCamaras.swift). El grupo guarda los
// desfases, el clip multicámara corta de ángulo en ángulo, y se comprueba que
// el frame decodificado en cada tramo es el color de la cámara activa y que el
// audio del tramo mide el tono de esa cámara.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func formato(_ v: Double) -> String { String(format: "%.2f", v) }

let dir = CommandLine.arguments[1]
let base = Timebase.p25
let grupoID = UUID()
let idA = UUID()
let idB = UUID()
let idC = UUID()

let camaras = [
    (id: idA, archivo: "camaraA.m4v"),
    (id: idB, archivo: "camaraB.m4v"),
    (id: idC, archivo: "camaraC.m4v"),
]

var medios: [UUID: MedioResuelto] = [:]
for camara in camaras {
    let medio = try await MedioResuelto.cargar(id: camara.id, url: URL(fileURLWithPath: dir + "/" + camara.archivo))
    medios[camara.id] = medio
    print("Cámara cargada: \(camara.archivo) · \(medio.duracion.seconds) s · audio \(medio.tieneAudio ? "sí" : "no")")
}

// Los pads eran 0,5 / 1,5 / 1,0 s y la referencia es el arranque más tardío
// (1,5 s): A empieza 1,0 s antes que la referencia, B es la referencia y C
// 0,5 s antes. Los tonos de las tres empiezan exactamente en t=1,5 s de grupo.
func grupo(conDesfases: Bool) -> GrupoMulticam {
    var grupo = GrupoMulticam(id: grupoID, nombre: "Entrevista", mediaIDs: [idA, idB, idC])
    if conDesfases {
        grupo.desfases = [
            idA: base.frames(segundos: 1.0),
            idB: 0,
            idC: base.frames(segundos: 0.5),
        ]
    }
    return grupo
}

func montar(conDesfases: Bool, cortes: [CorteDeAngulo]) -> (MontajeRenderizable, LineaDeTiempo) {
    var linea = LineaDeTiempo.nueva(timebase: base)
    linea.gruposMulticam = [grupo(conDesfases: conDesfases)]
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id

    let inicio = base.frames(segundos: 1.5)
    var video = Clip(
        mediaID: idA, nombre: "Entrevista", inicio: inicio,
        duracion: base.frames(segundos: 3.0), entradaEnOrigen: 0
    )
    video.multicam = MulticamDeClip(grupoID: grupoID, inicial: idA, cortes: cortes)
    linea.sobrescribir(video, enPista: v1, en: inicio)

    // El audio enlazado sigue los mismos cortes: imagen y sonido cambian juntos.
    var audio = Clip(
        mediaID: idA, nombre: "Entrevista", inicio: inicio,
        duracion: base.frames(segundos: 3.0), entradaEnOrigen: 0
    )
    audio.multicam = video.multicam
    linea.sobrescribir(audio, enPista: a1, en: inicio)

    let render = ConstructorDeMontaje.construir(linea, medios: medios)
    return (render, linea)
}

func colorDelFrame(_ render: MontajeRenderizable, en segundo: Double) async throws -> (r: Double, g: Double, b: Double) {
    let generador = AVAssetImageGenerator(asset: render.composicion)
    generador.videoComposition = render.composicionDeVideo
    generador.appliesPreferredTrackTransform = false
    generador.requestedTimeToleranceBefore = .zero
    generador.requestedTimeToleranceAfter = .zero
    let (imagen, _) = try await generador.image(at: CMTime(seconds: segundo, preferredTimescale: 600))
    let ancho = imagen.width
    let alto = imagen.height
    var datos = [UInt8](repeating: 0, count: ancho * alto * 4)
    let contexto = CGContext(
        data: &datos, width: ancho, height: alto, bitsPerComponent: 8,
        bytesPerRow: ancho * 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    contexto.draw(imagen, in: CGRect(x: 0, y: 0, width: ancho, height: alto))
    let centro = (ancho / 2 + alto / 2 * ancho) * 4
    return (Double(datos[centro]) / 255, Double(datos[centro + 1]) / 255, Double(datos[centro + 2]) / 255)
}

func medirTramo(_ render: MontajeRenderizable, desde: Double, hasta: Double) throws -> MedidaDeSonoridad {
    let rango = CMTimeRange(
        start: CMTime(seconds: desde, preferredTimescale: 600),
        duration: CMTime(seconds: hasta - desde, preferredTimescale: 600)
    )
    return try SonoridadMedia.medir(render, timeRange: rango)
}

print("— montaje con cortes de ángulo y desfases correctos —")
let cortes = [
    CorteDeAngulo(frame: base.frames(segundos: 1.0), mediaID: idB),
    CorteDeAngulo(frame: base.frames(segundos: 2.0), mediaID: idC),
]
let (render, linea) = montar(conDesfases: true, cortes: cortes)
print("  avisos: \(render.avisos.isEmpty ? "ninguno" : render.avisos.map(\.mensaje).joined(separator: " | "))")
comprobar(render.avisos.isEmpty, "con los desfases correctos no hay avisos")

do {
    let (r, g, b) = try await colorDelFrame(render, en: 2.0)
    comprobar(r > 0.5 && g < 0.3 && b < 0.3, "en t=2,0 manda la cámara A (roja): \(formato(r)),\(formato(g)),\(formato(b))")
} catch {
    print("  FALLO  decodificar frame en t=2,0: \(error)"); fallos += 1
}
do {
    let (r, g, b) = try await colorDelFrame(render, en: 3.0)
    comprobar(g > 0.5 && r < 0.3 && b < 0.3, "en t=3,0 manda la cámara B (verde): \(formato(r)),\(formato(g)),\(formato(b))")
} catch {
    print("  FALLO  decodificar frame en t=3,0: \(error)"); fallos += 1
}
do {
    let (r, g, b) = try await colorDelFrame(render, en: 4.0)
    comprobar(b > 0.5 && r < 0.3 && g < 0.3, "en t=4,0 manda la cámara C (azul): \(formato(r)),\(formato(g)),\(formato(b))")
} catch {
    print("  FALLO  decodificar frame en t=4,0: \(error)"); fallos += 1
}

let tramoA = try medirTramo(render, desde: 1.7, hasta: 2.3)
comprobar(abs(tramoA.integrada - (-20.0)) < 0.5, "el audio del tramo A mide −20 LUFS (\(formato(tramoA.integrada)))")
let tramoB = try medirTramo(render, desde: 2.7, hasta: 3.3)
comprobar(abs(tramoB.integrada - (-26.0)) < 0.5, "el audio del tramo B mide −26 LUFS (\(formato(tramoB.integrada)))")
let tramoC = try medirTramo(render, desde: 3.7, hasta: 4.3)
comprobar(abs(tramoC.integrada - (-32.0)) < 0.5, "el audio del tramo C mide −32 LUFS (\(formato(tramoC.integrada)))")

print("— los desfases importan: sin ellos la sincronización se rompe —")

// El clip arranca en t=0,6, antes de que la cámara A tenga material (su tono
// empieza en t=1,5 de grupo con el desfase correcto de 1,0 s). Con los
// desfases bien puestos, el constructor avisa y el tramo queda mudo; sin
// ellos, el material de A suena desde t=0,5 y el tramo mide el tono un
// segundo antes de donde debería.
func montarTemprano(conDesfases: Bool) -> (MontajeRenderizable, LineaDeTiempo) {
    var linea = LineaDeTiempo.nueva(timebase: base)
    linea.gruposMulticam = [grupo(conDesfases: conDesfases)]
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id
    let inicio = base.frames(segundos: 0.6)
    var video = Clip(
        mediaID: idA, nombre: "Entrevista", inicio: inicio,
        duracion: base.frames(segundos: 2.4), entradaEnOrigen: 0
    )
    video.multicam = MulticamDeClip(grupoID: grupoID, inicial: idA)
    linea.sobrescribir(video, enPista: v1, en: inicio)
    var audio = Clip(
        mediaID: idA, nombre: "Entrevista", inicio: inicio,
        duracion: base.frames(segundos: 2.4), entradaEnOrigen: 0
    )
    audio.multicam = video.multicam
    linea.sobrescribir(audio, enPista: a1, en: inicio)
    let render = ConstructorDeMontaje.construir(linea, medios: medios)
    return (render, linea)
}

let (temprano, _) = montarTemprano(conDesfases: true)
comprobar(temprano.avisos.contains { $0.mensaje.contains("antes del arranque") },
    "con desfases correctos, pedir material antes del arranque avisa")
do {
    _ = try medirTramo(temprano, desde: 0.7, hasta: 1.3)
    print("  FALLO  el tramo previo al arranque debía estar mudo"); fallos += 1
} catch {
    print("  ok  el tramo previo al arranque está mudo")
}
let (tempranoSin, _) = montarTemprano(conDesfases: false)
do {
    let medida = try medirTramo(tempranoSin, desde: 0.7, hasta: 1.3)
    comprobar(abs(medida.integrada - (-20.0)) < 0.5,
        "sin desfases el material suena 1 s antes: la sincronización se rompe (\(formato(medida.integrada)) LUFS)")
} catch {
    print("  FALLO  sin desfases debía oírse el tono"); fallos += 1
}

print("")
print(fallos == 0 ? "MULTICAM CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
