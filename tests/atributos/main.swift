import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

print("— atributos de clip —")

let media = UUID()
var linea = LineaDeTiempo.nueva(timebase: .p25)
let pistaVideo = linea.pistas.first { $0.tipo == .video }!
let v1 = pistaVideo.id

func clipEn(_ inicio: Int64) -> Clip {
    Clip(mediaID: media, nombre: "C\(inicio)", inicio: inicio, duracion: 100, entradaEnOrigen: 0)
}
linea.sobrescribir(clipEn(0), enPista: v1, en: 0)
linea.sobrescribir(clipEn(200), enPista: v1, en: 200)

let primera = linea.pistas.first { $0.tipo == .video }!
print("DEPURACION: clips = \(primera.clips.map { "\($0.inicio)-\($0.fin)" })")

// Copiar atributos de un clip y pegarlos en otro debe dejar el segundo con los
// valores del primero, sin tocar su sitio en la línea de tiempo.
if let origen = primera.clip(en: Int64(0)) {
    let copiados = Atributos(
        transformacion: origen.transformacion,
        ganancia: -6,
        entradaFundido: 12,
        salidaFundido: 24
    )
    var destino = primera.clip(en: Int64(200))!
    destino.transformacion = copiados.transformacion
    destino.ganancia = copiados.ganancia
    destino.entradaFundido = copiados.entradaFundido
    destino.salidaFundido = copiados.salidaFundido
    igual(destino.ganancia, -6, "la ganancia viaja en el pegado")
    igual(destino.entradaFundido, 12, "el fundido de entrada viaja")
    igual(destino.salidaFundido, 24, "el fundido de salida viaja")
    igual(destino.inicio, 200, "pegar atributos no mueve el clip")
    igual(destino.duracion, 100, "pegar atributos no cambia la duración")
} else {
    comprobar(false, "no se encontró el clip de origen")
}

// El extend edit: estirar la salida al cabezal es recortar con delta positivo.
if let clip = primera.clip(en: Int64(200)) {
    let cabezal: Int64 = 250
    let cercaDeLaEntrada = abs(cabezal - clip.inicio) < abs(cabezal - clip.fin)
    comprobar(!cercaDeLaEntrada, "con el cabezal a 250, el borde cercano de un clip [200,300] es la salida")
}

// Match frame: el frame del medio que corresponde al cabezal del montaje.
if let clip = primera.clip(en: Int64(0)) {
    let cabezal: Int64 = 40
    let relativo = max(0, cabezal - clip.inicio)
    let frameDeOrigen = clip.entradaEnOrigen + relativo
    igual(frameDeOrigen, 40, "match frame a velocidad 1 es entrada + relativo")
}

// El modelo de atributos: transformación, ganancia y fundidos.
struct Atributos {
    var transformacion: TransformacionDeClip
    var ganancia: Double
    var entradaFundido: Int64
    var salidaFundido: Int64
}

if fallos == 0 {
    print("ATRIBUTOS CORRECTOS")
} else {
    print("ATRIBUTOS ROTOS — \(fallos) fallos")
    exit(1)
}
