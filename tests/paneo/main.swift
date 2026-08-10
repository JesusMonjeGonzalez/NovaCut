import AVFoundation
import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

print("— paneo —")

// La ley de balance: al extremo se anula el canal contrario, en el centro los dos
// a media amplitud (no a plena, que doblaría el nivel).
let izquierda = TapDePaneo.ganancias(paneo: -1)
comprobar(abs(izquierda.izquierda - 1) < 0.001 && abs(izquierda.derecha) < 0.001,
          "paneo a −1: todo a la izquierda")
let derecha = TapDePaneo.ganancias(paneo: 1)
comprobar(abs(derecha.izquierda) < 0.001 && abs(derecha.derecha - 1) < 0.001,
          "paneo a +1: todo a la derecha")
let centro = TapDePaneo.ganancias(paneo: 0)
comprobar(abs(centro.izquierda - 0.5) < 0.001 && abs(centro.derecha - 0.5) < 0.001,
          "paneo al centro: media amplitud en cada canal")
let medio = TapDePaneo.ganancias(paneo: 0.5)
comprobar(medio.izquierda < centro.izquierda && medio.derecha > centro.derecha,
          "paneo a la derecha reparte la ganancia en esa dirección")

// Un paneo fuera de rango se recorta, no se acepta tal cual: un valor de 3 en el
// proyecto no puede dejar la pista a la izquierda con ganancia negativa.
let disparatado = TapDePaneo.ganancias(paneo: 3)
comprobar(disparatado.derecha > 0 && disparatado.izquierda >= 0,
          "un paneo disparatado se recorta al extremo")

// El tap se crea y se puede enganchar a un parámetro de mezcla de una pista real:
// es la única forma de que el paneo llegue a la exportación.
let composicion = AVMutableComposition()
let pista = composicion.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
if let pista {
    let parametros = AVMutableAudioMixInputParameters(track: pista)
    parametros.audioTapProcessor = TapDePaneo(paneo: -0.4).tap
    comprobar(parametros.audioTapProcessor != nil, "el tap de paneo se engancha al parámetro de mezcla")
} else {
    comprobar(false, "no se pudo crear una pista de audio de prueba")
}

// La envolvente de ganancia: los keyframes intermedios tienen que partir el
// tramo central en rampas — sin esto, un keyframe a mitad del clip no suena.
var clip = Clip(mediaID: UUID(), nombre: "C", inicio: 100, duracion: 100, entradaEnOrigen: 0)
clip.keyframes = [
    ClipKeyframe(frame: 0, transformacion: .identidad, ganancia: 0),
    ClipKeyframe(frame: 50, transformacion: .identidad, ganancia: -6),
    ClipKeyframe(frame: 99, transformacion: .identidad, ganancia: 0),
]
let piezas = ConstructorDeMontaje.piezasDeGanancia(desde: 100, hasta: 200, nivelDePista: 1, clip: clip)
comprobar(piezas.count == 3, "un keyframe intermedio parte el tramo en tres rampas (tiene \(piezas.count))")
comprobar(piezas.count == 3 && abs(piezas[0].vInicio - 1.0) < 0.001,
          "la rampa empieza a 0 dB")
comprobar(piezas.count == 3 && abs(piezas[0].vFin - pow(10, -6.0 / 20)) < 0.001,
          "la rampa llega al valor del keyframe (−6 dB → \(String(format: "%.3f", pow(10, -6.0 / 20))))")
comprobar(piezas.count == 3 && abs(piezas[1].vInicio - pow(10, -6.0 / 20)) < 0.001 && abs(piezas[1].vFin - 1.0) < 0.001,
          "la segunda rampa vuelve de −6 dB a 0 dB")
comprobar(piezas.count == 3 && piezas[2].desde == 199 && piezas[2].hasta == 200,
          "el último tramo llega hasta el final del clip")

let sinClaves = ConstructorDeMontaje.piezasDeGanancia(desde: 0, hasta: 100, nivelDePista: 1, clip: Clip(mediaID: UUID(), nombre: "S", inicio: 0, duracion: 100, entradaEnOrigen: 0))
comprobar(sinClaves.count == 1, "sin keyframes, una sola rampa para todo el tramo")

if fallos == 0 {
    print("PANEO CORRECTO")
} else {
    print("PANEO ROTO — \(fallos) fallos")
    exit(1)
}
