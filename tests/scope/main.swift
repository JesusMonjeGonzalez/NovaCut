import CoreVideo
import Foundation

// El vectorscopio es lógica pura: un buffer BGRA sintético de color sólido
// debe concentrar la densidad de croma en el bin que predicen las ecuaciones
// (R−Y, B−Y) — y el gris, sin croma, en el centro.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

func bufferDeColor(_ b: UInt8, _ g: UInt8, _ r: UInt8) -> CVPixelBuffer {
    var pixelBuffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32BGRA,
                        [kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180] as CFDictionary,
                        &pixelBuffer)
    let pb = pixelBuffer!
    CVPixelBufferLockBaseAddress(pb, [])
    let base = CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)
    for i in stride(from: 0, to: 320 * 180 * 4, by: 4) {
        base[i] = b; base[i + 1] = g; base[i + 2] = r; base[i + 3] = 255
    }
    CVPixelBufferUnlockBaseAddress(pb, [])
    return pb
}

/// El bin de máxima densidad y su proporción del total.
func picoDe(_ densidad: [[Float]]) -> (u: Int, v: Int, proporcion: Double) {
    var mejor = (0, 0)
    var valor = Float(0)
    for u in densidad.indices {
        for v in densidad[u].indices where densidad[u][v] > valor {
            valor = densidad[u][v]
            mejor = (u, v)
        }
    }
    let total = densidad.flatMap { $0 }.reduce(0, +)
    return (mejor.0, mejor.1, total > 0 ? Double(valor) / Double(total) : 0)
}

func cerca(_ a: Int, _ b: Int, tolerancia: Int = 4) -> Bool { abs(a - b) <= tolerancia }

print("— gris: la croma cae al centro —")
let gris = Vectorscopio.calcular(del: bufferDeColor(128, 128, 128))!
let picoGris = picoDe(gris)
comprobar(cerca(picoGris.u, 128) && cerca(picoGris.v, 128),
          "el gris cae en el centro (u=\(picoGris.u), v=\(picoGris.v))")

print("— rojo puro: R−Y positivo —")
// r=1, g=0, b=0 → Y=0,299 → u=(0−0,299)/1,4=−0,214 → columna 73 · v=(1−0,299)/1,4=0,501 → nivel 255.
let rojo = Vectorscopio.calcular(del: bufferDeColor(0, 0, 255))!
let picoRojo = picoDe(rojo)
comprobar(cerca(picoRojo.u, 73) && cerca(picoRojo.v, 255),
          "el rojo puro cae donde predicen las ecuaciones (u=\(picoRojo.u), v=\(picoRojo.v))")

print("— azul puro: B−Y positivo —")
// r=0, g=0, b=1 → Y=0,114 → u=(1−0,114)/1,4=0,633 → columna 255 · v=(0−0,114)/1,4=−0,081 → nivel 107.
let azul = Vectorscopio.calcular(del: bufferDeColor(255, 0, 0))!
let picoAzul = picoDe(azul)
comprobar(cerca(picoAzul.u, 255) && cerca(picoAzul.v, 107),
          "el azul puro cae donde predicen las ecuaciones (u=\(picoAzul.u), v=\(picoAzul.v))")

print("— un solo color concentra la densidad —")
comprobar(picoGris.proporcion > 0.9, "un frame sólido concentra la densidad en un bin (proporción \(String(format: "%.2f", picoGris.proporcion)))")

print("— parade RGB: cada canal en su distribución —")
// Rojo puro: el pico del canal rojo cae en el nivel más alto (255) y los
// picos de verde y azul en el más bajo (0), porque ese canal no tiene señal.
func picoDeCanal(_ canal: [[Float]]) -> Int {
    var pico = 0
    var mejor: Float = 0
    for nivel in 0..<256 where canal[0][nivel] > mejor {
        mejor = canal[0][nivel]
        pico = nivel
    }
    return pico
}
let paradeRojo = ParadeRGB.calcular(del: bufferDeColor(0, 0, 255))!
let picoParadeRojo = picoDeCanal(paradeRojo[2])
let picoParadeVerde = picoDeCanal(paradeRojo[1])
let picoParadeAzul = picoDeCanal(paradeRojo[0])
comprobar(cerca(picoParadeRojo, 255, tolerancia: 3), "el rojo puro quema el canal rojo del parade (nivel \(picoParadeRojo))")
comprobar(cerca(picoParadeVerde, 0, tolerancia: 3), "el verde no tiene señal (nivel \(picoParadeVerde))")
comprobar(cerca(picoParadeAzul, 0, tolerancia: 3), "el azul tampoco (nivel \(picoParadeAzul))")

print("— histograma de luminancia —")
// Gris 50 % (128): la luminancia debe concentrarse en el nivel 128 con tolerancia.
let histograma = HistogramaDeLuminancia.calcular(del: bufferDeColor(128, 128, 128))!
var picoLuma = 0
var mejorLuma: Float = 0
for nivel in 0..<256 where histograma[nivel] > mejorLuma {
    mejorLuma = histograma[nivel]
    picoLuma = nivel
}
comprobar(cerca(picoLuma, 128, tolerancia: 5), "el gris 50 % cae en su nivel de luminancia (\(picoLuma))")
let sumaHistograma = histograma.reduce(0, +)
comprobar(abs(sumaHistograma - 1) < 0.01, "el histograma está normalizado (suma \(String(format: "%.3f", sumaHistograma)))")

if fallos == 0 {
    print("SCOPE CORRECTO")
} else {
    print("SCOPE ROTO — \(fallos) fallos")
    exit(1)
}
