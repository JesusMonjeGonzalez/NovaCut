import AVFoundation
import Foundation

let ruta = CommandLine.arguments[1]
let url = URL(fileURLWithPath: ruta)
let id = UUID()

let medio = try await MedioResuelto.cargar(id: id, url: url)
print("Medio: \(medio.tamanoNatural) natural, \(medio.tamanoVisible) visible, \(medio.fps) fps")
print("Duración: \(medio.duracion.seconds) s · vídeo=\(medio.tieneVideo) audio=\(medio.tieneAudio)")

let base = ProyectoEditorcitoTimebase(medio.fps)
print("Base de tiempo elegida: \(base.nombre) (\(base.numerador)/\(base.denominador))")

let preparados = await ConformadorVFR.preparar(medios: [id: medio], para: base)
guard preparados.fallos.isEmpty, let medioParaMontaje = preparados.medios[id] else {
    print("FALLO al conformar: \(preparados.fallos.joined(separator: " | "))")
    exit(1)
}
if preparados.conformados > 0 {
    print("VFR conformado: \(preparados.conformados) intermediario(s)")
}

var linea = LineaDeTiempo.nueva(timebase: base)
let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
let v2 = linea.pistas.first { $0.nombre == "V2" }!.id
let a1 = linea.pistas.first { $0.nombre == "A1" }!.id

// Tres clips en V1, uno superpuesto en V2 con opacidad, y audio suelto en A1.
// Los tramos se calculan sobre la duración real para que el arnés funcione con
// corpus cortos igual que con grabaciones largas.
let framesDeFuente = max(1, base.frames(segundos: medio.duracion.seconds))
let duracionDeClip = max(1, min(100, framesDeFuente / 4))
let duracionDeTransicion = max(1, min(12, duracionDeClip / 4))
for i in 0..<3 {
    let inicio = Int64(i) * duracionDeClip
    var c = Clip(mediaID: id, nombre: "corte\(i)", inicio: inicio,
                 duracion: duracionDeClip, entradaEnOrigen: inicio)
    c.entradaFundido = duracionDeTransicion
    c.salidaFundido = duracionDeTransicion
    linea.sobrescribir(c, enPista: v1, en: inicio)
}
if let indice = linea.indiceDePista(v1) {
    let transicion = Transicion(tipo: .disolucion, duracion: duracionDeTransicion)
    linea.pistas[indice].clips[0].transicionSalida = transicion
    linea.pistas[indice].clips[1].transicionEntrada = transicion
}
let inicioEncima = duracionDeClip + duracionDeClip / 2
let duracionEncima = max(1, duracionDeClip / 2)
var encima = Clip(mediaID: id, nombre: "encima", inicio: inicioEncima,
                  duracion: duracionEncima, entradaEnOrigen: duracionDeClip)
encima.transformacion.opacidad = 55
encima.transformacion.escala = 60
encima.transformacion.posicionX = 300
linea.sobrescribir(encima, enPista: v2, en: inicioEncima)

var pista = Clip(mediaID: id, nombre: "música", inicio: 0,
                 duracion: duracionDeClip * 3, entradaEnOrigen: 0)
pista.ganancia = -9
pista.entradaFundido = min(25, duracionDeClip)
linea.sobrescribir(pista, enPista: a1, en: 0)
linea.subtitulos = [Subtitulo(inicio: 0, fin: duracionDeClip * 3, texto: "Prueba de subtítulo")]

let render = ConstructorDeMontaje.construir(linea, medios: [id: medioParaMontaje])
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
let instante = base.tiempo(inicioEncima)
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
