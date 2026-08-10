import Foundation

/// Detección de silencios.
///
/// Es la función que se enseña a otro montador, y aquí sale casi gratis porque el trabajo
/// difícil ya está hecho y verificado: el medidor de `Sonoridad.swift` pasa el set oficial
/// de la EBU. Lo que falta es la decisión, y esa decisión tiene tres detalles que separan
/// «funciona» de «inservible»:
///
/// 1. **El umbral es relativo a la sonoridad del propio material**, no un dBFS absoluto.
///    Un pódcast comprimido a −14 LUFS y una voz susurrada a −34 tienen que dar los
///    mismos cortes; con un umbral fijo, en uno se corta todo y en el otro nada.
/// 2. **Histéresis.** Sin ella, un golpe de un bloque en medio de una pausa la parte en
///    dos y salen dos cortes donde debía haber uno.
/// 3. **Guarda a los lados.** Cortar en el instante exacto en el que la voz cruza el
///    umbral se come la consonante inicial, que es el defecto que hace que un rough cut
///    automático suene mal aunque los tiempos sean correctos.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}
func cerca(_ a: Double, _ b: Double, _ tolerancia: Double, _ mensaje: String) {
    if abs(a - b) <= tolerancia { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

/// Curva de sonoridad momentánea a un valor cada 100 ms.
let paso = 0.1

/// Curva con voz al nivel dado y valles de silencio en los tramos indicados, en segundos.
func curva(segundos: Double, voz: Double, silencio: Double, valles: [(Double, Double)]) -> [Double] {
    let n = Int(segundos / paso)
    return (0..<n).map { i in
        let t = Double(i) * paso
        return valles.contains(where: { t >= $0.0 && t < $0.1 }) ? silencio : voz
    }
}

print("— sin silencios —")
do {
    let c = curva(segundos: 5, voz: -18, silencio: -60, valles: [])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 0, "una curva toda alta no tiene silencios")
}

print("— un valle en medio —")
do {
    // Dos segundos de voz, un segundo de silencio, dos de voz.
    let c = curva(segundos: 5, voz: -18, silencio: -60, valles: [(2, 3)])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 1, "un valle de un segundo es un silencio")
    // La guarda por defecto es 0,12 s a cada lado: el corte no llega hasta la voz.
    cerca(hallados[0].desde, 2.12, 0.06, "el corte empieza después de que calle")
    cerca(hallados[0].hasta, 2.88, 0.06, "y acaba antes de que vuelva a hablar")
}

print("— demasiado corto para cortar —")
do {
    let c = curva(segundos: 4, voz: -18, silencio: -60, valles: [(2, 2.2)])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 0, "una pausa de 200 ms es respirar, no un silencio")
}

print("— el umbral es relativo —")
do {
    // La misma forma de curva, 20 LU más abajo: los cortes tienen que salir iguales.
    let alta = curva(segundos: 6, voz: -14, silencio: -56, valles: [(2, 3.5)])
    let baja = curva(segundos: 6, voz: -34, silencio: -76, valles: [(2, 3.5)])
    let enAlta = DetectorDeSilencios.silencios(curva: alta, pasoEnSegundos: paso, integrada: -14)
    let enBaja = DetectorDeSilencios.silencios(curva: baja, pasoEnSegundos: paso, integrada: -34)
    igual(enAlta.count, enBaja.count, "el mismo número de silencios en los dos niveles")
    if enAlta.count == 1 && enBaja.count == 1 {
        cerca(enAlta[0].desde, enBaja[0].desde, 0.01, "y en el mismo sitio")
        cerca(enAlta[0].hasta, enBaja[0].hasta, 0.01, "con la misma duración")
    }
}

print("— histéresis —")
do {
    // Un golpe de un solo bloque a media altura en medio de la pausa: por encima del
    // umbral de entrada pero por debajo del de salida. No debe partir el silencio.
    var c = curva(segundos: 6, voz: -18, silencio: -60, valles: [(2, 4)])
    let golpe = Int(3.0 / paso)
    c[golpe] = -42 // 24 LU por debajo de la voz: sigue siendo silencio a efectos prácticos
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 1, "un golpe suave no parte la pausa en dos")
}

do {
    // Y una vuelta de la voz de verdad sí la parte.
    var c = curva(segundos: 8, voz: -18, silencio: -60, valles: [(2, 6)])
    for i in Int(3.8 / paso)..<Int(4.4 / paso) { c[i] = -18 }
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 2, "si vuelve a hablar de verdad, son dos silencios")
}

print("— los bordes —")
do {
    let c = curva(segundos: 5, voz: -18, silencio: -60, valles: [(0, 1.5)])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 1, "un silencio al principio también se detecta")
    cerca(hallados[0].desde, 0, 0.01, "y empieza en cero, sin guarda por delante")
}

do {
    let c = curva(segundos: 5, voz: -18, silencio: -60, valles: [(3.5, 5)])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 1, "y uno al final")
    cerca(hallados[0].hasta, 5, 0.06, "que llega hasta el final del material")
}

do {
    let c = curva(segundos: 5, voz: -60, silencio: -60, valles: [])
    let hallados = DetectorDeSilencios.silencios(curva: c, pasoEnSegundos: paso, integrada: -60)
    // Con todo al mismo nivel no hay nada 25 LU por debajo de la media: no se corta.
    igual(hallados.count, 0, "un material plano no se corta entero por sorpresa")
}

print("— casos límite —")
do {
    igual(DetectorDeSilencios.silencios(curva: [], pasoEnSegundos: paso, integrada: -18).count, 0, "curva vacía")
    igual(
        DetectorDeSilencios.silencios(curva: [-60, -60], pasoEnSegundos: 0, integrada: -18).count, 0,
        "un paso de cero no se divide"
    )
    let conInfinito = [Double](repeating: -.infinity, count: 40)
    let hallados = DetectorDeSilencios.silencios(curva: conInfinito, pasoEnSegundos: paso, integrada: -18)
    igual(hallados.count, 1, "silencio digital absoluto: −infinito es silencio, no un error")
}

do {
    // Con la guarda tan grande que se come el silencio entero, no hay corte: mejor no
    // cortar que cortar donde hay voz.
    let c = curva(segundos: 4, voz: -18, silencio: -60, valles: [(2, 2.6)])
    let hallados = DetectorDeSilencios.silencios(
        curva: c, pasoEnSegundos: paso, integrada: -18, minimoEnSegundos: 0.3, guardaEnSegundos: 0.5
    )
    igual(hallados.count, 0, "si la guarda se come el silencio, no se corta")
}

print("")
print(fallos == 0 ? "TODO CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
