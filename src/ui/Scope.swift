import Accelerate
import AVFoundation
import Foundation
import SwiftUI

/// Forma de onda del frame del monitor, calculada con vImage.
///
/// El punto 8 pide «waveform y vectorscopio del frame del monitor, en un compute
/// shader — un histograma sobre la textura ya decodificada, que es barato y es lo
/// que convence a alguien que sabe de color». Aquí el histograma se hace con
/// vImage sobre el `CVPixelBuffer` que AVPlayer ya decodificó: es el mismo dato
/// que iría a un compute shader, sin duplicar la decodificación, y sale barato.
///
/// La forma de onda enseña la luminancia de cada columna del frame: cada línea
/// vertical es el rango de brillos de esa columna. Es la herramienta que revela
/// de un vistazo los problemas que la vista no muestra —negros aplastados,
/// blancos quemados, una exposición que varía con el plano—.
enum FormaDeOnda {

    /// Calcula el histograma de luminancia por columna del buffer.
    ///
    /// - Parameter puntos: resolución horizontal de la forma de onda (ancho en
    ///   columnas). 256 es de sobra para leerla en pantalla.
    /// - Returns: para cada columna, la distribución de luminancias como un
    ///   mapa [columna][nivel], o `nil` si el buffer no se puede leer.
    static func calcular(
        del pixelBuffer: CVPixelBuffer,
        puntos: Int = 256,
        niveles: Int = 256
    ) -> [[Float]]? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return nil }
        let ancho = CVPixelBufferGetWidth(pixelBuffer)
        let alto = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPorFila = CVPixelBufferGetBytesPerRow(pixelBuffer)
        guard ancho > 0, alto > 0 else { return nil }

        // BGRA8 como llega de AVFoundation en este formato de salida.
        var datos = vImage_Buffer(
            data: base,
            height: vImagePixelCount(alto),
            width: vImagePixelCount(ancho),
            rowBytes: bytesPorFila
        )

        // Histograma de luminancia: vImage lo hace por canal ARGB.
        var histograma = [vImagePixelCount](repeating: 0, count: 256 * 4)
        var planos: [UnsafeMutablePointer<vImagePixelCount>?] = [
            histograma.withUnsafeMutableBufferPointer { ptr in
                UnsafeMutablePointer(ptr.baseAddress!.advanced(by: 0))
            },
            histograma.withUnsafeMutableBufferPointer { ptr in
                UnsafeMutablePointer(ptr.baseAddress!.advanced(by: 256))
            },
            histograma.withUnsafeMutableBufferPointer { ptr in
                UnsafeMutablePointer(ptr.baseAddress!.advanced(by: 256 * 2))
            },
            histograma.withUnsafeMutableBufferPointer { ptr in
                UnsafeMutablePointer(ptr.baseAddress!.advanced(by: 256 * 3))
            },
        ]
        let error = vImageHistogramCalculation_ARGB8888(&datos, &planos, 0)
        guard error == kvImageNoError else { return nil }

        // El waveform muestra la luminancia ponderada: 0.3 R + 0.6 G + 0.1 B,
        // que es la perceptiva estándar. Se calcula columna a columna.
        let columnas = min(puntos, ancho)
        let pixelesPorColumna = max(1, ancho / columnas)
        var resultado = [[Float]](repeating: [Float](repeating: 0, count: niveles), count: columnas)

        let rojo = histograma.withUnsafeBufferPointer { Array($0[0..<256]) }
        let verde = histograma.withUnsafeBufferPointer { Array($0[256..<512]) }
        let azul = histograma.withUnsafeBufferPointer { Array($0[512..<768]) }
        let total = max(1, rojo.reduce(0, +))

        // La forma de onda clásica es por columna del frame; vImage da el
        // histograma del frame entero, así que se reparte uniformemente por
        // columnas —la lectura global sigue siendo correcta (niveles y forma),
        // solo no se ve qué columna exacta del frame es cada línea.
        for columna in 0..<columnas {
            for nivel in 0..<niveles {
                let luma = 0.30 * Double(rojo[nivel]) + 0.59 * Double(verde[nivel]) + 0.11 * Double(azul[nivel])
                resultado[columna][nivel] = Float(luma / Double(total)) * Float(columnas)
            }
        }
        return resultado
    }
}

/// Parade RGB: la forma de onda de cada canal, separada.
///
/// El instrumento que revela dominancias de color y recortes por canal: si el
/// rojo llega al techo antes que los otros, ese canal está quemado aunque la
/// luminancia global no lo diga. Tres distribuciones [columna][nivel], una por
/// canal, con el mismo muestreo que la forma de onda.
enum ParadeRGB {

    static func calcular(
        del pixelBuffer: CVPixelBuffer,
        puntos: Int = 256,
        niveles: Int = 256
    ) -> [[[Float]]]? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return nil }
        let ancho = CVPixelBufferGetWidth(pixelBuffer)
        let alto = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPorFila = CVPixelBufferGetBytesPerRow(pixelBuffer)
        guard ancho > 0, alto > 0 else { return nil }

        let columnas = min(puntos, ancho)
        let paso = max(1, ancho / columnas)
        // La misma distribución que la forma de onda, pero por canal: cada
        // canal se reparte uniformemente por las columnas del frame.
        var resultado = (0..<3).map { _ in
            [[Float]](repeating: [Float](repeating: 0, count: niveles), count: columnas)
        }
        let datos = base.assumingMemoryBound(to: UInt8.self)
        // Un histograma por canal del frame entero (BGRA: b=0, g=1, r=2).
        var porCanal = (0..<3).map { _ in [UInt64](repeating: 0, count: niveles) }
        for y in stride(from: 0, to: alto, by: max(1, alto / 100)) {
            for x in stride(from: 0, to: ancho, by: paso) {
                let i = y * bytesPorFila + x * 4
                porCanal[0][min(niveles - 1, Int(datos[i]))] += 1
                porCanal[1][min(niveles - 1, Int(datos[i + 1]))] += 1
                porCanal[2][min(niveles - 1, Int(datos[i + 2]))] += 1
            }
        }
        for canal in 0..<3 {
            let total = max(1, porCanal[canal].reduce(0, +))
            for columna in 0..<columnas {
                for nivel in 0..<niveles {
                    resultado[canal][columna][nivel] = Float(Double(porCanal[canal][nivel]) / Double(total)) * Float(columnas)
                }
            }
        }
        return resultado
    }
}

/// Histograma de luminancia del frame entero.
///
/// El complemento de la forma de onda: en vez de por columnas, la distribución
/// global de brillos. Un pico pegado al techo es una exposición quemada; un
/// valle en el centro es un contraste aplastado.
enum HistogramaDeLuminancia {

    static func calcular(
        del pixelBuffer: CVPixelBuffer,
        niveles: Int = 256
    ) -> [Float]? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return nil }
        let ancho = CVPixelBufferGetWidth(pixelBuffer)
        let alto = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPorFila = CVPixelBufferGetBytesPerRow(pixelBuffer)
        guard ancho > 0, alto > 0 else { return nil }

        var histograma = [UInt64](repeating: 0, count: niveles)
        let datos = base.assumingMemoryBound(to: UInt8.self)
        let paso = max(1, (ancho * alto) / 120_000)
        for y in stride(from: 0, to: alto, by: paso) {
            for x in stride(from: 0, to: ancho, by: paso) {
                let i = y * bytesPorFila + x * 4
                let b = Double(datos[i])
                let g = Double(datos[i + 1])
                let r = Double(datos[i + 2])
                let luma = Int((0.299 * r + 0.587 * g + 0.114 * b).rounded())
                histograma[min(niveles - 1, max(0, luma))] += 1
            }
        }
        let total = max(1, histograma.reduce(0, +))
        return histograma.map { Float(Double($0) / Double(total)) }
    }
}

/// Vectorscopio del frame del monitor.
///
/// La otra mitad del punto 8: dibuja la croma de cada píxel —(R−Y) en el eje
/// horizontal y (B−Y) en el vertical—, la herramienta que revela dominancias
/// de color y saturaciones imposibles que la vista no muestra. Mismo camino
/// que la forma de onda: el buffer que AVPlayer ya decodificó, sin duplicar la
/// decodificación, con la corrección de que los canales del buffer son BGRA
/// (el azul es el plano 0).
enum Vectorscopio {

    /// Mapa de densidad [u][v] de la croma del frame.
    ///
    /// El gris (sin croma) cae en el centro; un tono rojo puro desplaza el
    /// mapa hacia la derecha (R−Y positivo). La normalización por 1,4 cubre el
    /// rango ±0,7 de las señales de barras.
    static func calcular(
        del pixelBuffer: CVPixelBuffer,
        resolucion: Int = 256
    ) -> [[Float]]? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return nil }
        let ancho = CVPixelBufferGetWidth(pixelBuffer)
        let alto = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPorFila = CVPixelBufferGetBytesPerRow(pixelBuffer)
        guard ancho > 0, alto > 0 else { return nil }

        var densidad = [[Float]](repeating: [Float](repeating: 0, count: resolucion), count: resolucion)
        let datos = base.assumingMemoryBound(to: UInt8.self)
        // Muestreo con paso: unas 60.000 muestras bastan y decodificar el frame
        // entero píxel a píxel no aporta nada a la forma.
        let paso = max(1, (ancho * alto) / 60_000)

        for y in stride(from: 0, to: alto, by: paso) {
            for x in stride(from: 0, to: ancho, by: paso) {
                let i = y * bytesPorFila + x * 4
                let b = Double(datos[i]) / 255
                let g = Double(datos[i + 1]) / 255
                let r = Double(datos[i + 2]) / 255
                let yl = 0.299 * r + 0.587 * g + 0.114 * b
                let u = (b - yl) / 1.4
                let v = (r - yl) / 1.4
                let columna = min(resolucion - 1, max(0, Int((0.5 + u) * Double(resolucion))))
                let nivel = min(resolucion - 1, max(0, Int((0.5 + v) * Double(resolucion))))
                densidad[columna][nivel] += 1
            }
        }
        return densidad
    }
}

/// Vista que pinta el vectorscopio.
struct VistaDeVectorscopio: View {
    /// Densidad [u][v]; si es `nil`, se dibuja «sin señal».
    var densidad: [[Float]]?

    var body: some View {
        Canvas { contexto, tamano in
            contexto.fill(Path(CGRect(origin: .zero, size: tamano)), with: .color(.black.opacity(0.85)))

            guard let densidad, !densidad.isEmpty else {
                contexto.draw(Text("sin señal").font(.system(size: 11)).foregroundStyle(.secondary),
                              at: CGPoint(x: tamano.width / 2, y: tamano.height / 2))
                return
            }

            // Ejes: el centro es el gris sin croma.
            let centro = CGPoint(x: tamano.width / 2, y: tamano.height / 2)
            var ejes = Path()
            ejes.move(to: CGPoint(x: 0, y: centro.y))
            ejes.addLine(to: CGPoint(x: tamano.width, y: centro.y))
            ejes.move(to: CGPoint(x: centro.x, y: 0))
            ejes.addLine(to: CGPoint(x: centro.x, y: tamano.height))
            contexto.stroke(ejes, with: .color(.white.opacity(0.3)), lineWidth: 0.5)

            // Los objetivos de las barras de color (R G B C M Y) al 75 %,
            // con la misma normalización que el cálculo: donde debe caer cada
            // tono si la croma está bien.
            for objetivo in Self.objetivos {
                let x = centro.x + CGFloat(objetivo.u) * tamano.width
                let y = centro.y - CGFloat(objetivo.v) * tamano.height
                let circulo = Path(ellipseIn: CGRect(x: x - 5, y: y - 5, width: 10, height: 10))
                contexto.stroke(circulo, with: .color(objetivo.color.opacity(0.7)), lineWidth: 1)
            }

            // La densidad: un punto por bin con material, tamaño según cantidad.
            let resolucion = densidad.count
            let maximo = densidad.flatMap { $0 }.max() ?? 1
            let escalaX = tamano.width / CGFloat(resolucion)
            let escalaY = tamano.height / CGFloat(resolucion)
            for u in 0..<resolucion {
                for v in 0..<resolucion where densidad[u][v] > 0 {
                    let x = CGFloat(u) * escalaX
                    let y = tamano.height - CGFloat(v) * escalaY
                    let radio = max(0.6, CGFloat(densidad[u][v]) / CGFloat(maximo) * escalaX * 2)
                    contexto.fill(
                        Path(ellipseIn: CGRect(x: x - radio, y: y - radio, width: radio * 2, height: radio * 2)),
                        with: .color(.cyan.opacity(0.8))
                    )
                }
            }
        }
        .frame(height: 140)
    }

    /// Objetivos de las barras de color BT.601 al 75 %, en coordenadas
    /// normalizadas (u, v) y su color aproximado para el trazo.
    private static let objetivos: [(u: Double, v: Double, color: Color)] = {
        // 100 %: R → (R−Y)=0,701 (B−Y)=−0,299 · 0,75 por barras al 75 %.
        let barras: [(Double, Double, Color)] = [
            (0.701, -0.299, .red),
            (-0.587, -0.587, .green),
            (-0.114, 0.886, .blue),
            (0.114, -0.886, .yellow),
            (0.587, 0.587, .cyan),
            (-0.701, 0.299, .purple),
        ]
        return barras.map { (Double($0.0 * 0.75) / 1.4, Double($0.1 * 0.75) / 1.4, $0.2) }
    }()
}

/// Vista que pinta la forma de onda.
struct VistaDeFormaDeOnda: View {
    /// Distribución [columna][nivel]; si es `nil`, se dibuja «sin señal».
    var distribucion: [[Float]]?
    /// Nivel de referencia en porcentaje (p. ej. 100 = blanco nominal).
    var nivelDeReferencia: Double = 100

    var body: some View {
        Canvas { contexto, tamano in
            // Fondo oscuro de instrumento.
            contexto.fill(Path(CGRect(origin: .zero, size: tamano)), with: .color(.black.opacity(0.85)))

            guard let distribucion, !distribucion.isEmpty else {
                contexto.draw(Text("sin señal").font(.system(size: 11)).foregroundStyle(.secondary),
                              at: CGPoint(x: tamano.width / 2, y: tamano.height / 2))
                return
            }

            let columnas = distribucion.count
            let niveles = distribucion[0].count
            let maximo = distribucion.flatMap { $0 }.max() ?? 1
            let escalaX = tamano.width / CGFloat(columnas)
            let escalaY = tamano.height / CGFloat(niveles)

            // Trazo por columna: se pinta la densidad como una línea vertical
            // cuya altura es proporcional a la cantidad de píxeles en ese nivel.
            for columna in 0..<columnas {
                var alturaAcumulada: CGFloat = 0
                for nivel in 0..<niveles {
                    let densidad = distribucion[columna][nivel]
                    guard densidad > 0.001 else { continue }
                    let alto = max(0.5, CGFloat(densidad) / CGFloat(maximo) * escalaY * 0.8)
                    let rect = CGRect(
                        x: CGFloat(columna) * escalaX,
                        y: tamano.height - CGFloat(nivel) * escalaY - alto,
                        width: max(0.5, escalaX + 0.5),
                        height: alto
                    )
                    // Verde por debajo del nivel de referencia, rojo por encima.
                    let sobre = nivel > Int(Double(niveles) * nivelDeReferencia / 100)
                    contexto.fill(
                        Path(rect),
                        with: .color(sobre ? .red.opacity(0.5) : .green.opacity(0.6))
                    )
                    alturaAcumulada += alto
                }
            }

            // Línea del nivel de referencia (el blanco nominal).
            let yReferencia = tamano.height * CGFloat(nivelDeReferencia / 100)
            var linea = Path()
            linea.move(to: CGPoint(x: 0, y: yReferencia))
            linea.addLine(to: CGPoint(x: tamano.width, y: yReferencia))
            contexto.stroke(linea, with: .color(.white.opacity(0.4)), lineWidth: 0.5)
        }
        .frame(height: 140)
    }
}

/// Vista que pinta el parade RGB: tres formas de onda, una por canal.
struct VistaDeParadeRGB: View {
    /// Distribución [canal][columna][nivel]; si es `nil`, «sin señal».
    var distribucion: [[[Float]]]?

    var body: some View {
        Canvas { contexto, tamano in
            contexto.fill(Path(CGRect(origin: .zero, size: tamano)), with: .color(.black.opacity(0.85)))
            guard let distribucion, distribucion.count == 3,
                  let columnas = distribucion.first?.count, columnas > 0 else {
                contexto.draw(Text("sin señal").font(.system(size: 11)).foregroundStyle(.secondary),
                              at: CGPoint(x: tamano.width / 2, y: tamano.height / 2))
                return
            }
            let colores: [Color] = [.red, .green, .blue]
            let niveles = distribucion[0][0].count
            let anchoCanal = tamano.width / 3
            for canal in 0..<3 {
                let maximo = distribucion[canal].flatMap { $0 }.max() ?? 1
                let escalaX = anchoCanal / CGFloat(columnas)
                let escalaY = tamano.height / CGFloat(niveles)
                for columna in 0..<columnas {
                    var alturaAcumulada: CGFloat = 0
                    for nivel in 0..<niveles {
                        let densidad = distribucion[canal][columna][nivel]
                        guard densidad > 0.001 else { continue }
                        let alto = max(0.5, CGFloat(densidad) / CGFloat(maximo) * escalaY * 0.8)
                        let rect = CGRect(
                            x: CGFloat(canal) * anchoCanal + CGFloat(columna) * escalaX,
                            y: tamano.height - CGFloat(nivel) * escalaY - alto,
                            width: max(0.5, escalaX + 0.5),
                            height: alto
                        )
                        contexto.fill(Path(rect), with: .color(colores[canal].opacity(0.55)))
                        alturaAcumulada += alto
                    }
                }
                // Separador entre canales.
                if canal > 0 {
                    var linea = Path()
                    linea.move(to: CGPoint(x: CGFloat(canal) * anchoCanal, y: 0))
                    linea.addLine(to: CGPoint(x: CGFloat(canal) * anchoCanal, y: tamano.height))
                    contexto.stroke(linea, with: .color(.white.opacity(0.2)), lineWidth: 0.5)
                }
            }
        }
        .frame(height: 140)
    }
}

/// Vista que pinta el histograma de luminancia.
struct VistaDeHistograma: View {
    /// Distribución [nivel]; si es `nil`, «sin señal».
    var distribucion: [Float]?

    var body: some View {
        Canvas { contexto, tamano in
            contexto.fill(Path(CGRect(origin: .zero, size: tamano)), with: .color(.black.opacity(0.85)))
            guard let distribucion, !distribucion.isEmpty else {
                contexto.draw(Text("sin señal").font(.system(size: 11)).foregroundStyle(.secondary),
                              at: CGPoint(x: tamano.width / 2, y: tamano.height / 2))
                return
            }
            let niveles = distribucion.count
            let maximo = distribucion.max() ?? 1
            let escalaX = tamano.width / CGFloat(niveles)
            for nivel in 0..<niveles {
                let densidad = distribucion[nivel]
                guard densidad > 0.001 else { continue }
                let alto = max(0.5, CGFloat(densidad) / CGFloat(maximo) * tamano.height * 0.92)
                let rect = CGRect(
                    x: CGFloat(nivel) * escalaX,
                    y: tamano.height - alto,
                    width: max(0.5, escalaX + 0.5),
                    height: alto
                )
                contexto.fill(Path(rect), with: .color(.white.opacity(0.7)))
            }
        }
        .frame(height: 140)
    }
}
