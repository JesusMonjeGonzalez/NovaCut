import CoreGraphics
import Foundation

/// Una LUT `.cube` ya convertida a los datos que espera `CIColorCube`.
struct CuboDeLUT {
    /// `LUT_3D_SIZE`: arista del cubo (33, 17, 65…).
    let tamano: Int
    /// N³ entradas RGBA en coma flotante, con el índice azul variando más rápido
    /// —el orden del estándar `.cube`, que es el que espera Core Image—.
    let datos: Data
}
/// Lee y convierte archivos `.cube` —el formato de LUTs de DaVinci, Premiere y
/// compañía— a los datos de `CIColorCube`.
///
/// El formato: `LUT_3D_SIZE N` seguido de N³ líneas «r g b», con comentarios `#`
/// y `DOMAIN_MIN`/`DOMAIN_MAX` opcionales. Es lógica pura a propósito: se
/// verifica sin abrir la aplicación ni decodificar nada.
enum ParseadorDeCubes {

    /// Las LUTs no cambian dentro de una sesión; se cachean por ruta.
    private static var cache: [String: CuboDeLUT] = [:]
    private static let cerrojo = NSLock()

    static func cargar(ruta: String) -> CuboDeLUT? {
        cerrojo.lock()
        if let cubo = cache[ruta] {
            cerrojo.unlock()
            return cubo
        }
        cerrojo.unlock()

        guard let contenido = try? String(contentsOfFile: ruta, encoding: .utf8),
              let cubo = parsear(contenido: contenido) else { return nil }
        cerrojo.lock()
        cache[ruta] = cubo
        cerrojo.unlock()
        return cubo
    }

    /// Lógica pura, testeable sin archivos.
    static func parsear(contenido: String) -> CuboDeLUT? {
        var tamano3D: Int?
        var tamano1D: Int?
        var minimo = (0.0, 0.0, 0.0)
        var maximo = (1.0, 1.0, 1.0)
        var valores: [Double] = []

        for linea in contenido.components(separatedBy: .newlines) {
            let limpia = linea.trimmingCharacters(in: .whitespaces)
            if limpia.isEmpty || limpia.hasPrefix("#") { continue }
            let partes = limpia.split(separator: " ", omittingEmptySubsequences: true)
            guard !partes.isEmpty else { continue }
            switch partes[0] {
            case "LUT_3D_SIZE" where partes.count == 2:
                tamano3D = Int(partes[1])
            case "LUT_1D_SIZE" where partes.count == 2:
                tamano1D = Int(partes[1])
            case "DOMAIN_MIN" where partes.count == 4:
                minimo = (Double(partes[1]) ?? 0, Double(partes[2]) ?? 0, Double(partes[3]) ?? 0)
            case "DOMAIN_MAX" where partes.count == 4:
                maximo = (Double(partes[1]) ?? 1, Double(partes[2]) ?? 1, Double(partes[3]) ?? 1)
            default:
                if partes.count >= 3,
                   let r = Double(partes[0]),
                   let g = Double(partes[1]),
                   let b = Double(partes[2]) {
                    valores.append(r)
                    valores.append(g)
                    valores.append(b)
                }
            }
        }

        if let n = tamano3D, n > 0 {
            let esperadas = n * n * n * 3
            guard valores.count >= esperadas else { return nil }
            return CuboDeLUT(
                tamano: n,
                datos: datosRGBA(Array(valores.prefix(esperadas)), tamano: n, minimo: minimo, maximo: maximo)
            )
        }
        if let n = tamano1D, n > 0, !valores.isEmpty {
            return cuboDeUnaDimension(Array(valores.prefix(n * 3)), tamano: n, minimo: minimo, maximo: maximo)
        }
        return nil
    }

    /// Una LUT 1D aplica una curva a cada canal por separado: se expande a un
    /// cubo donde la entrada (r, g, b) vale (fR[r], fG[g], fB[b]), interpolando
    /// linealmente entre puntos de la curva.
    private static func cuboDeUnaDimension(_ valores: [Double], tamano: Int, minimo: (Double, Double, Double), maximo: (Double, Double, Double)) -> CuboDeLUT? {
        guard valores.count >= tamano * 3 else { return nil }

        func curva(_ canal: Int) -> [(entrada: Double, salida: Double)] {
            (0..<tamano).map { i in
                let v = valores[i * 3 + canal]
                let salida: Double
                switch canal {
                case 0: salida = min(max(v, minimo.0), maximo.0)
                case 1: salida = min(max(v, minimo.1), maximo.1)
                default: salida = min(max(v, minimo.2), maximo.2)
                }
                return (Double(i) / Double(tamano - 1), salida)
            }
        }
        let rojos = curva(0)
        let verdes = curva(1)
        let azules = curva(2)

        func aplicar(_ curva: [(entrada: Double, salida: Double)], _ x: Double) -> Double {
            if x <= curva[0].entrada { return curva[0].salida }
            if x >= curva[curva.count - 1].entrada { return curva[curva.count - 1].salida }
            for i in 1..<curva.count where x <= curva[i].entrada {
                let a = curva[i - 1]
                let b = curva[i]
                let t = (x - a.entrada) / max(b.entrada - a.entrada, 1e-9)
                return a.salida + (b.salida - a.salida) * t
            }
            return curva[curva.count - 1].salida
        }

        var datos = Data(capacity: tamano * tamano * tamano * 4 * MemoryLayout<Float>.size)
        for indice in 0..<(tamano * tamano * tamano) {
            let b = Double(indice % tamano) / Double(tamano - 1)
            let g = Double((indice / tamano) % tamano) / Double(tamano - 1)
            let r = Double(indice / (tamano * tamano)) / Double(tamano - 1)
            let rg = Float(aplicar(rojos, r))
            let gg = Float(aplicar(verdes, g))
            let bg = Float(aplicar(azules, b))
            datos.append(contentsOf: withUnsafeBytes(of: rg) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: gg) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: bg) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: Float(1)) { Array($0) })
        }
        return CuboDeLUT(tamano: tamano, datos: datos)
    }

    /// Convierte las entradas (r g b) del archivo a RGBA en coma flotante, con
    /// el alfa a 1 y el dominio recortado a 0…1 (Core Image los exige).
    private static func datosRGBA(_ valores: [Double], tamano: Int, minimo: (Double, Double, Double), maximo: (Double, Double, Double)) -> Data {
        let escalaR = maximo.0 - minimo.0 > 0 ? 1.0 / (maximo.0 - minimo.0) : 1.0
        let escalaG = maximo.1 - minimo.1 > 0 ? 1.0 / (maximo.1 - minimo.1) : 1.0
        let escalaB = maximo.2 - minimo.2 > 0 ? 1.0 / (maximo.2 - minimo.2) : 1.0

        var datos = Data(capacity: valores.count / 3 * 4 * MemoryLayout<Float>.size)
        var i = 0
        while i + 2 < valores.count {
            let r = Float(min(max((valores[i] - minimo.0) * escalaR, 0), 1))
            let g = Float(min(max((valores[i + 1] - minimo.1) * escalaG, 0), 1))
            let b = Float(min(max((valores[i + 2] - minimo.2) * escalaB, 0), 1))
            datos.append(contentsOf: withUnsafeBytes(of: r) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: g) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: b) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: Float(1)) { Array($0) })
            i += 3
        }
        return datos
    }
}
