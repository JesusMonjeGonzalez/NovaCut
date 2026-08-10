import AVFoundation
import Foundation

let ruta = CommandLine.arguments[1]
let url = URL(fileURLWithPath: ruta)
let id = UUID()

let medio = try await MedioResuelto.cargar(id: id, url: url)
print("Medio: \(medio.tamanoNatural) natural, \(medio.tamanoVisible) visible, \(medio.fps) fps")
print("Duración: \(medio.duracion.seconds) s · vídeo=\(medio.tieneVideo) audio=\(medio.tieneAudio)")

var base = ProyectoEditorcitoTimebase(medio.fps)
print("Base de tiempo elegida: \(base.nombre) (\(base.numerador)/\(base.denominador))")

var linea = LineaDeTiempo.nueva(timebase: base)
let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
let v2 = linea.pistas.first { $0.nombre == "V2" }!.id
let a1 = linea.pistas.first { $0.nombre == "A1" }!.id

// Tres clips en V1, uno superpuesto en V2 con opacidad, y audio suelto en A1.
for i in 0..<3 {
    var c = Clip(mediaID: id, nombre: "corte\(i)", inicio: Int64(i) * 100,
                 duracion: 100, entradaEnOrigen: Int64(i) * 500)
    c.entradaFundido = 12
    c.salidaFundido = 12
    linea.sobrescribir(c, enPista: v1, en: Int64(i) * 100)
}
if let indice = linea.indiceDePista(v1) {
    let transicion = Transicion(tipo: .disolucion, duracion: 12)
    linea.pistas[indice].clips[0].transicionSalida = transicion
    linea.pistas[indice].clips[1].transicionEntrada = transicion
}
var encima = Clip(mediaID: id, nombre: "encima", inicio: 150, duracion: 80, entradaEnOrigen: 2000)
encima.transformacion.opacidad = 55
encima.transformacion.escala = 60
encima.transformacion.posicionX = 300
linea.sobrescribir(encima, enPista: v2, en: 150)

var pista = Clip(mediaID: id, nombre: "música", inicio: 0, duracion: 300, entradaEnOrigen: 5000)
pista.ganancia = -9
pista.entradaFundido = 25
linea.sobrescribir(pista, enPista: a1, en: 0)
linea.subtitulos = [Subtitulo(inicio: 0, fin: 300, texto: "Prueba de subtítulo")]

let render = ConstructorDeMontaje.construir(linea, medios: [id: medio])
print("")
print("Pistas de composición: \(render.composicion.tracks.count)")
print("  vídeo: \(render.composicion.tracks(withMediaType: .video).count)")
print("  audio: \(render.composicion.tracks(withMediaType: .audio).count)")
print("Duración compuesta: \(render.composicion.duration.seconds) s (montaje \(base.segundos(linea.duracion)) s)")
print("Tamaño de render: \(render.tamano)")
print("Instrucciones de vídeo: \(render.composicionDeVideo?.instructions.count ?? 0)")
print("Parámetros de audio: \(render.mezclaDeAudio?.inputParameters.count ?? 0)")
print("Frame duration: \(render.composicionDeVideo?.frameDuration.value ?? 0)/\(render.composicionDeVideo?.frameDuration.timescale ?? 0)")
print("Avisos: \(render.avisos.isEmpty ? "ninguno" : render.avisos.map(\.mensaje).joined(separator: " | "))")

// Capas por instrucción: donde se superponen las dos pistas debe haber dos.
if let vc = render.composicionDeVideo {
    let capas = vc.instructions.compactMap { ($0 as? AVMutableVideoCompositionInstruction)?.layerInstructions.count }
    print("Capas por tramo: \(capas)")
    print("Máximo de capas simultáneas: \(capas.max() ?? 0)")
}

// Se lee un frame de verdad por el camino compuesto: es la prueba de que esto
// no solo se construye, sino que decodifica.
let generador = AVAssetImageGenerator(asset: render.composicion)
generador.videoComposition = render.composicionDeVideo
generador.appliesPreferredTrackTransform = false
generador.requestedTimeToleranceBefore = .zero
generador.requestedTimeToleranceAfter = .zero
let instante = base.tiempo(170)
do {
    let (imagen, real) = try await generador.image(at: instante)
    print("Frame en \(base.timecode(170)) decodificado: \(imagen.width)×\(imagen.height) en t=\(real.seconds)")
} catch {
    print("FALLO al decodificar: \(error)")
    exit(1)
}
print("")
print("PIPELINE CORRECTO")

func ProyectoEditorcitoTimebase(_ fps: Double) -> Timebase {
    Timebase.habituales.min { abs($0.fps - fps) < abs($1.fps - fps) } ?? .p25
}
