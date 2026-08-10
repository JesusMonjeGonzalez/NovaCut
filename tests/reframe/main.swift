import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

print("— reencuadre vertical —")

// El suavizado no cambia el número de muestras ni sus frames.
let muestras = [
    DetectorDeSujeto.Muestra(frame: 0, centro: (0.5, 0.5), tamano: 0.3),
    DetectorDeSujeto.Muestra(frame: 30, centro: (0.6, 0.7), tamano: 0.3),
    DetectorDeSujeto.Muestra(frame: 60, centro: (0.4, 0.3), tamano: 0.3),
]
let suavizadas = DetectorDeSujeto.suavizar(muestras, ventana: 3)
igual(suavizadas.count, 3, "el suavizado conserva el número de muestras")
igual(suavizadas[2].frame, 60, "el suavizado conserva los frames")

// La muestra del medio, con vecinas a los dos lados, se acerca a la media.
comprobar(
    abs(suavizadas[1].centro.x - (0.5 + 0.6 + 0.4) / 3) < 0.001 &&
    abs(suavizadas[1].centro.y - (0.5 + 0.7 + 0.3) / 3) < 0.001,
    "la muestra central se suaviza con sus vecinas"
)

// Un sujeto que ocupa el 30 % de la imagen se deja al 100 % de escala.
let claves = ReframeVertical.keyframes(de: muestras)
igual(claves.count, 3, "una muestra por keyframe")
comprobar(abs(claves[0].transformacion.escala - 100) < 0.01,
          "un sujeto del 30 % no necesita zoom")

// Un sujeto pequeño obliga a acercar: menos del 30 % sube la escala.
let pequeno = [DetectorDeSujeto.Muestra(frame: 0, centro: (0.5, 0.5), tamano: 0.1)]
let clavePequena = ReframeVertical.keyframes(de: pequeno)[0]
comprobar(clavePequena.transformacion.escala > 100,
          "un sujeto del 10 % se acerca")

// El centro del sujeto se traduce a posición: arriba en la imagen (y=1) es
// posición positiva en el lienzo (la convención de la casa).
let arriba = [DetectorDeSujeto.Muestra(frame: 0, centro: (0.5, 1.0), tamano: 0.3)]
let claveArriba = ReframeVertical.keyframes(de: arriba)[0]
comprobar(claveArriba.transformacion.posicionY > 0,
          "el sujeto arriba de la imagen sube la posición del clip")

// La conversión a keyframes de un clip conserva el giro y la opacidad del usuario.
var clip = Clip(mediaID: UUID(), nombre: "C", inicio: 0, duracion: 100, entradaEnOrigen: 0)
clip.transformacion.rotacion = 15
clip.transformacion.opacidad = 80
let aplicado = ReframeVertical.aplicar(a: clip, muestras: muestras, transformacionBase: clip.transformacion)
igual(aplicado.keyframes?.count, 3, "aplicar vuelca las muestras como keyframes")
if let primera = aplicado.keyframes?.first {
    igual(primera.transformacion.rotacion, 15, "el giro del usuario se conserva")
    igual(primera.transformacion.opacidad, 80, "la opacidad del usuario se conserva")
}

if fallos == 0 {
    print("REFRAME CORRECTO")
} else {
    print("REFRAME ROTO — \(fallos) fallos")
    exit(1)
}
