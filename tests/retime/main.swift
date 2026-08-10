import AVFoundation
import CoreGraphics
import Foundation

// Verifica el retime avanzado sobre un archivo real: un clip a velocidad 2×
// debe mostrar el doble de avance de luminancia que un clip a 1×, y un tramo
// congelado debe mostrar exactamente el mismo gris en todos sus frames.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func formato(_ v: Double) -> String { String(format: "%.1f", v) }

let dir = CommandLine.arguments[1]
let base = Timebase.p25
let mediaID = UUID()
let medio = try await MedioResuelto.cargar(id: mediaID, url: URL(fileURLWithPath: dir + "/patron-retime.m4v"))

/// Nivel de gris (0…9) del centro del frame decodificado, con el mismo paso
/// de 25 frames por escalón del patrón. La cuantización protege la medida del
/// desplazamiento del codec.
func grisDelFrame(_ render: MontajeRenderizable, en segundo: Double) async throws -> Int {
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
    return min(9, Int(datos[centro]) / 28)
}

/// Cuántos escalones de material (de 25 frames) avanzó el render entre dos
/// tiempos. En el patrón, cada escalón son exactamente 25 frames del clip.
func avanceDeMaterial(_ render: MontajeRenderizable, desde: Double, hasta: Double) async throws -> Double {
    let a = try await grisDelFrame(render, en: desde)
    let b = try await grisDelFrame(render, en: hasta)
    return Double(b - a) * 50
}

func montar(_ modificar: (inout Clip) -> Void, duracion: Int64 = base.frames(segundos: 8)) -> MontajeRenderizable {
    var linea = LineaDeTiempo.nueva(timebase: base)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: mediaID, nombre: "Retime", inicio: 0, duracion: duracion, entradaEnOrigen: 0)
    modificar(&clip)
    linea.sobrescribir(clip, enPista: v1, en: 0)
    return ConstructorDeMontaje.construir(linea, medios: [mediaID: medio])
}

print("— velocidad constante 1× —")
let normal = montar { _ in }
let avanceNormal = try await avanceDeMaterial(normal, desde: 1.0, hasta: 3.0)
comprobar(abs(avanceNormal - 50) < 3, "a 1× dos segundos avanzan 50 frames de material (\(formato(avanceNormal)))")

print("— velocidad 2× —")
// A 2× el clip de 8 s consume 16 s de material (dentro de los 20 s del patrón).
let doble = montar({ $0.velocidad = 2 }, duracion: base.frames(segundos: 8))
let avanceDoble = try await avanceDeMaterial(doble, desde: 1.0, hasta: 3.0)
comprobar(abs(avanceDoble - 100) < 6, "a 2× dos segundos avanzan 100 frames de material (\(formato(avanceDoble)))")

print("— rampa 1× → 3× —")
// Rampa lineal en la primera mitad del clip (0→3× en 4 s): entre 1 y 3 s la
// velocidad media es 2×, así que el avance es 100 frames.
var rampa = montar {
    $0.rampasDeVelocidad = [
        RampaDeVelocidad(frame: 0, velocidad: 1),
        RampaDeVelocidad(frame: base.frames(segundos: 4), velocidad: 3),
        RampaDeVelocidad(frame: base.frames(segundos: 8), velocidad: 3),
    ]
}
let avanceRampa = try await avanceDeMaterial(rampa, desde: 1.0, hasta: 3.0)
comprobar(abs(avanceRampa - 100) < 8, "la rampa acelera de verdad (\(formato(avanceRampa)))")

print("— congelado —")
// Velocidad 0 desde el segundo 2 hasta el 4: los frames de ese tramo deben
// mostrar el mismo gris que el frame del segundo 2. La rampa de entrada (1→0
// entre 0 y 2 s) consume la mitad, así que el nivel congelado es el 0.
var congelado = montar {
    $0.rampasDeVelocidad = [
        RampaDeVelocidad(frame: 0, velocidad: 1),
        RampaDeVelocidad(frame: base.frames(segundos: 2), velocidad: 0),
        RampaDeVelocidad(frame: base.frames(segundos: 4), velocidad: 0),
        RampaDeVelocidad(frame: base.frames(segundos: 6), velocidad: 1),
    ]
}
do {
    let a = try await grisDelFrame(congelado, en: 2.0)
    let b = try await grisDelFrame(congelado, en: 3.0)
    let c = try await grisDelFrame(congelado, en: 3.9)
    comprobar(a == b && a == c,
              "el congelado mantiene el frame (\(a), \(b), \(c))")
    // Después del tramo congelado el material avanza: a los 7 s la velocidad
    // ya es 1 y el nivel ha subido al menos un escalón.
    let d = try await grisDelFrame(congelado, en: 7.0)
    comprobar(d > a, "y al salir del congelado el material sigue (\(a) → \(d))")
} catch {
    print("  FALLO  decodificar el congelado: \(error)"); fallos += 1
}

print("")
print(fallos == 0 ? "RETIME CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
