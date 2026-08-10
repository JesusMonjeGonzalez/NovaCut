import Foundation

/// Sonoridad según ITU-R BS.1770-4 y EBU R128.
///
/// Un pico de muestra no dice nada sobre lo alto que suena un montaje: una voz
/// comprimida y un tema de piano pueden dar el mismo pico y llevarse diez decibelios
/// de diferencia al oído. Todas las plataformas normalizan por sonoridad al recibir
/// el archivo, así que exportar sin medirla significa que alguien —YouTube, Spotify,
/// la emisora— va a mover el nivel del máster sin preguntar y sin contarlo.
///
/// Se mide con el pesado K y la puerta del estándar, no con un RMS a ojo:
///
/// - **Pesado K**: un realce de graves-agudos de +4 dB por encima de 1,7 kHz y un
///   paso alto a 38 Hz. Aproxima cómo pesa el oído cada banda.
/// - **Bloques de 400 ms con 75 % de solape**, que es lo que hace que el número no
///   dependa de dónde se corte el archivo.
/// - **Doble puerta**: se descarta lo que baja de −70 LUFS (silencio) y después lo
///   que queda 10 LU por debajo de la media de lo demás (pausas). Sin la puerta, un
///   minuto de silencio al final baja la medida y el máster sale alto.
///
/// El fichero no importa AVFoundation a propósito: es aritmética sobre muestras y
/// así se puede verificar con `swiftc` contra las señales de prueba de la EBU sin
/// arrastrar un decodificador. La lectura del medio vive en `SonoridadMedia.swift`.

// MARK: - Biquad

/// Sección de segundo orden en forma directa I. Se guarda el estado por canal
/// aparte porque el mismo filtro se aplica a todos los canales en paralelo.
struct Biquad: Sendable {
    let b0: Double, b1: Double, b2: Double, a1: Double, a2: Double

    struct Estado: Sendable {
        var x1 = 0.0, x2 = 0.0, y1 = 0.0, y2 = 0.0
    }

    @inline(__always)
    func procesar(_ x: Double, _ e: inout Estado) -> Double {
        let y = b0 * x + b1 * e.x1 + b2 * e.x2 - a1 * e.y1 - a2 * e.y2
        e.x2 = e.x1; e.x1 = x
        e.y2 = e.y1; e.y1 = y
        return y
    }
}

/// Las dos secciones del pesado K, deducidas para la frecuencia de muestreo real.
///
/// El estándar publica los coeficientes solo para 48 kHz. Copiarlos y usarlos con
/// 44,1 kHz desplaza las dos esquinas del filtro y falsea la medida, así que aquí se
/// diseñan con la transformada bilineal a partir de las especificaciones de la
/// norma; a 48 kHz salen exactamente los coeficientes publicados.
enum PesadoK {

    static func secciones(frecuencia: Double) -> (realce: Biquad, pasoAlto: Biquad) {
        // Sección 1: realce en estantería alta, +3,999844 dB a partir de 1681,97 Hz.
        let f0 = 1681.974450955533
        let g = 3.999843853973347
        let q0 = 0.7071752369554196

        let k0 = tan(Double.pi * f0 / frecuencia)
        let vh = pow(10.0, g / 20.0)
        let vb = pow(vh, 0.4996667741545416)
        let a0 = 1.0 + k0 / q0 + k0 * k0

        let realce = Biquad(
            b0: (vh + vb * k0 / q0 + k0 * k0) / a0,
            b1: 2.0 * (k0 * k0 - vh) / a0,
            b2: (vh - vb * k0 / q0 + k0 * k0) / a0,
            a1: 2.0 * (k0 * k0 - 1.0) / a0,
            a2: (1.0 - k0 / q0 + k0 * k0) / a0
        )

        // Sección 2: paso alto RLB a 38,135 Hz. Quita el retumbe que el oído no
        // percibe como volumen pero que sí infla cualquier medida de energía.
        let f1 = 38.13547087602444
        let q1 = 0.5003270373238773
        let k1 = tan(Double.pi * f1 / frecuencia)
        let a1 = 1.0 + k1 / q1 + k1 * k1

        let pasoAlto = Biquad(
            b0: 1.0, b1: -2.0, b2: 1.0,
            a1: 2.0 * (k1 * k1 - 1.0) / a1,
            a2: (1.0 - k1 / q1 + k1 * k1) / a1
        )

        return (realce, pasoAlto)
    }
}

// MARK: - Pico real

/// Pico entre muestras, por sobremuestreo ×4.
///
/// El pico de muestra miente: una senoide justo por debajo de 0 dBFS cuyas muestras
/// caen a los lados de la cresta marca −0,5 y al reconstruirla en el conversor pasa
/// de 0 y satura. El estándar pide medir sobre la señal reconstruida, y ×4 es el
/// mínimo que fija BS.1770-4.
///
/// El interpolador se diseña al arrancar —seno cardinal por ventana de Blackman,
/// doce coeficientes por fase— en lugar de arrastrar una tabla copiada. Queda dentro
/// de una décima de decibelio del pico verdadero, que es la tolerancia de la norma.
struct InterpoladorDePico: Sendable {
    static let fases = 4
    static let coeficientesPorFase = 12

    let fases: [[Double]]

    init() {
        let total = Self.fases * Self.coeficientesPorFase
        let centro = Double(total - 1) / 2.0
        var h = [Double](repeating: 0, count: total)

        for n in 0..<total {
            let t = (Double(n) - centro) / Double(Self.fases)
            let sinc = abs(t) < 1e-9 ? 1.0 : sin(Double.pi * t) / (Double.pi * t)
            // Blackman: los lóbulos laterales de una ventana rectangular meterían
            // rizado en la banda y el pico saldría inventado.
            let w = 0.42
                - 0.5 * cos(2.0 * Double.pi * Double(n) / Double(total - 1))
                + 0.08 * cos(4.0 * Double.pi * Double(n) / Double(total - 1))
            h[n] = sinc * w
        }

        // Cada fase se normaliza a ganancia unidad en continua: si no, el
        // sobremuestreo introduce un rizado de amplitud que se leería como pico.
        var salida: [[Double]] = []
        for fase in 0..<Self.fases {
            var coef: [Double] = []
            var n = fase
            while n < total { coef.append(h[n]); n += Self.fases }
            let suma = coef.reduce(0, +)
            if abs(suma) > 1e-12 { coef = coef.map { $0 / suma } }
            salida.append(coef)
        }
        self.fases = salida
    }

    /// Pico absoluto de un canal ya desentrelazado, incluyendo las muestras crudas.
    func pico(_ x: [Double]) -> Double {
        var maximo = 0.0
        for v in x { maximo = max(maximo, abs(v)) }

        let taps = Self.coeficientesPorFase
        guard x.count >= taps else { return maximo }

        for fase in fases {
            for i in 0...(x.count - taps) {
                var acc = 0.0
                for k in 0..<taps { acc += fase[k] * x[i + k] }
                maximo = max(maximo, abs(acc))
            }
        }
        return maximo
    }
}

// MARK: - Medidor

/// Resultado de medir un montaje entero.
struct MedidaDeSonoridad: Sendable, Equatable {
    /// Sonoridad integrada con doble puerta, en LUFS.
    let integrada: Double
    /// Recorrido de sonoridad (percentil 95 − percentil 10 de lo momentáneo), en LU.
    let rango: Double
    /// Pico real entre muestras, en dBTP.
    let picoReal: Double
    /// Duración medida, en segundos.
    let duracion: Double

    /// `true` cuando no hubo ni un bloque por encima de la puerta absoluta.
    var esSilencio: Bool { integrada <= -70.0 }
}

/// Medidor incremental: se le van dando trozos de audio entrelazado y al final
/// devuelve la medida. Trabaja en fragmentos de 100 ms, que es el paso del solape
/// del 75 % de los bloques de 400 ms y también el de la ventana corta de 3 s.
final class MedidorDeSonoridad {

    private let frecuencia: Double
    private let canales: Int
    private let pesos: [Double]

    private let realce: Biquad
    private let pasoAlto: Biquad
    private var estadoRealce: [Biquad.Estado]
    private var estadoPasoAlto: [Biquad.Estado]

    private let muestrasPorFragmento: Int
    private var acumulado: [Double]
    private var contadas = 0

    /// Suma de cuadrados por canal en cada fragmento de 100 ms.
    private var fragmentos: [[Double]] = []
    private var totalDeMuestras = 0

    private let interpolador = InterpoladorDePico()
    private var picoLineal = 0.0
    /// Cola por canal para que el pico no se pierda en la costura entre trozos.
    private var colaDePico: [[Double]]

    /// `pesos` permite fijar la disposición real de los canales.
    ///
    /// Los surround van +1,5 dB en BS.1770-4, pero en qué índice están no se
    /// deduce del número de canales: un WAV 5.1 ordena L R C LFE Ls Rs y una
    /// pista de cine 5.0 ordena L R C Ls Rs. Aplicar el peso del surround al
    /// índice fijo de la otra convención descuenta 1,5 dB de un canal entero.
    /// Quien lee el medio (AVFoundation) conoce la disposición y la pasa aquí;
    /// sin ella, mono y estéreo van a uno y cinco o más canales asumen el orden
    /// 5.0 que el propio estándar usa en sus señales de prueba.
    init(frecuencia: Double, canales: Int, pesos: [Double]? = nil) {
        precondition(frecuencia > 0 && canales > 0, "medidor sin señal que medir")
        self.frecuencia = frecuencia
        self.canales = canales

        if let pesos {
            precondition(pesos.count == canales, "pesos y canales no coinciden")
            self.pesos = pesos
        } else {
            var g = [Double](repeating: 1.0, count: canales)
            if canales >= 5 { g[3] = 1.41; g[4] = 1.41 }
            self.pesos = g
        }

        let k = PesadoK.secciones(frecuencia: frecuencia)
        self.realce = k.realce
        self.pasoAlto = k.pasoAlto
        self.estadoRealce = Array(repeating: Biquad.Estado(), count: canales)
        self.estadoPasoAlto = Array(repeating: Biquad.Estado(), count: canales)

        self.muestrasPorFragmento = max(Int((frecuencia * 0.1).rounded()), 1)
        self.acumulado = [Double](repeating: 0, count: canales)
        self.colaDePico = Array(repeating: [], count: canales)
    }

    /// Añade audio entrelazado (canal 0, canal 1, …, canal 0, …).
    func procesar(entrelazado: [Float]) {
        guard !entrelazado.isEmpty else { return }
        let marcos = entrelazado.count / canales

        for c in 0..<canales { colaDePico[c].reserveCapacity(colaDePico[c].count + marcos) }

        for m in 0..<marcos {
            for c in 0..<canales {
                let x = Double(entrelazado[m * canales + c])
                colaDePico[c].append(x)

                let y = pasoAlto.procesar(
                    realce.procesar(x, &estadoRealce[c]),
                    &estadoPasoAlto[c]
                )
                acumulado[c] += y * y
            }

            contadas += 1
            totalDeMuestras += 1
            if contadas == muestrasPorFragmento {
                fragmentos.append(acumulado)
                acumulado = [Double](repeating: 0, count: canales)
                contadas = 0
            }
        }

        vaciarColaDePico(dejandoSolape: true)
    }

    /// Cierra la medida. A partir de aquí el medidor no admite más audio.
    func finalizar() -> MedidaDeSonoridad {
        // El último fragmento incompleto se descarta: el estándar mide bloques
        // enteros, y escalar un fragmento corto lo sobrepondera.
        vaciarColaDePico(dejandoSolape: false)

        let momentaneos = bloques(fragmentosPorBloque: 4)
        let cortos = bloques(fragmentosPorBloque: 30)

        return MedidaDeSonoridad(
            integrada: integrar(momentaneos),
            rango: recorrido(cortos),
            picoReal: picoLineal > 0 ? 20.0 * log10(picoLineal) : -Double.infinity,
            duracion: Double(totalDeMuestras) / frecuencia
        )
    }

    // MARK: bloques

    /// Sonoridad de cada ventana solapada, en LUFS, junto a su energía pesada.
    /// Sonoridad momentánea a lo largo del material: un valor cada 100 ms.
    ///
    /// Es la misma curva con la que se calcula la integrada, así que detectar silencios no
    /// introduce una segunda forma de medir que pudiera discrepar de la primera.
    func curvaMomentanea() -> [Double] {
        bloques(fragmentosPorBloque: 4).map(\.nivel)
    }

    /// Separación entre valores de `curvaMomentanea`, en segundos.
    static let pasoDeLaCurva = 0.1

    private func bloques(fragmentosPorBloque n: Int) -> [(nivel: Double, energia: Double)] {
        guard fragmentos.count >= n else { return [] }
        var salida: [(Double, Double)] = []
        salida.reserveCapacity(fragmentos.count - n + 1)

        let divisor = Double(n * muestrasPorFragmento)
        for inicio in 0...(fragmentos.count - n) {
            var energia = 0.0
            for c in 0..<canales {
                var suma = 0.0
                for j in inicio..<(inicio + n) { suma += fragmentos[j][c] }
                energia += pesos[c] * (suma / divisor)
            }
            let nivel = energia > 0 ? -0.691 + 10.0 * log10(energia) : -Double.infinity
            salida.append((nivel, energia))
        }
        return salida
    }

    /// Integrada con las dos puertas del estándar.
    private func integrar(_ bloques: [(nivel: Double, energia: Double)]) -> Double {
        // Puerta absoluta: −70 LUFS deja fuera el silencio de verdad.
        let sobreAbsoluta = bloques.filter { $0.nivel > -70.0 }
        guard !sobreAbsoluta.isEmpty else { return -Double.infinity }

        let mediaAbsoluta = sobreAbsoluta.reduce(0.0) { $0 + $1.energia } / Double(sobreAbsoluta.count)
        guard mediaAbsoluta > 0 else { return -Double.infinity }

        // Puerta relativa: 10 LU por debajo de esa media. Es la que impide que las
        // pausas de un diálogo tiren la medida hacia abajo.
        let umbral = -0.691 + 10.0 * log10(mediaAbsoluta) - 10.0
        let sobreRelativa = sobreAbsoluta.filter { $0.nivel > umbral }
        guard !sobreRelativa.isEmpty else { return -Double.infinity }

        let media = sobreRelativa.reduce(0.0) { $0 + $1.energia } / Double(sobreRelativa.count)
        return media > 0 ? -0.691 + 10.0 * log10(media) : -Double.infinity
    }

    /// Recorrido de sonoridad (EBU Tech 3342) sobre las ventanas de 3 s.
    private func recorrido(_ bloques: [(nivel: Double, energia: Double)]) -> Double {
        let sobreAbsoluta = bloques.filter { $0.nivel > -70.0 }
        guard sobreAbsoluta.count >= 2 else { return 0 }

        let media = sobreAbsoluta.reduce(0.0) { $0 + $1.energia } / Double(sobreAbsoluta.count)
        guard media > 0 else { return 0 }

        // Aquí la puerta relativa es de 20 LU, no de 10: el recorrido quiere
        // conservar los pasajes suaves que sí forman parte de la obra.
        let umbral = -0.691 + 10.0 * log10(media) - 20.0
        let niveles = sobreAbsoluta.map(\.nivel).filter { $0 > umbral }.sorted()
        guard niveles.count >= 2 else { return 0 }

        return percentil(niveles, 0.95) - percentil(niveles, 0.10)
    }

    private func percentil(_ ordenados: [Double], _ p: Double) -> Double {
        let posicion = p * Double(ordenados.count - 1)
        let bajo = Int(posicion.rounded(.down))
        let alto = min(bajo + 1, ordenados.count - 1)
        let t = posicion - Double(bajo)
        return ordenados[bajo] * (1 - t) + ordenados[alto] * t
    }

    // MARK: pico

    /// Sobremuestrea lo que hay en cola. Se dejan atrás los coeficientes justos para
    /// que el trozo siguiente pueda solapar y ningún pico caiga en la juntura.
    private func vaciarColaDePico(dejandoSolape: Bool) {
        let solape = dejandoSolape ? InterpoladorDePico.coeficientesPorFase - 1 : 0
        for c in 0..<canales {
            let x = colaDePico[c]
            guard x.count > solape else { continue }
            picoLineal = max(picoLineal, interpolador.pico(x))
            colaDePico[c] = solape > 0 ? Array(x.suffix(solape)) : []
        }
    }
}

// MARK: - Objetivos de entrega

/// A dónde va el máster. Cada destino normaliza a un número distinto, y entregar
/// por encima solo consigue que lo bajen ellos; entregar por debajo, que lo suban y
/// con él el ruido de fondo.
enum ObjetivoDeSonoridad: String, Codable, CaseIterable, Sendable, Identifiable {
    case ninguno
    case youtube
    case appleMusicPodcast
    case broadcastR128

    var id: String { rawValue }

    var nombre: String {
        switch self {
        case .ninguno: return "Sin normalizar"
        case .youtube: return "YouTube / Spotify (−14 LUFS)"
        case .appleMusicPodcast: return "Apple / pódcast (−16 LUFS)"
        case .broadcastR128: return "Difusión EBU R128 (−23 LUFS)"
        }
    }

    /// Sonoridad de destino en LUFS.
    var objetivo: Double? {
        switch self {
        case .ninguno: return nil
        case .youtube: return -14
        case .appleMusicPodcast: return -16
        case .broadcastR128: return -23
        }
    }

    /// Techo de pico real. −1 dBTP es lo que piden todas las especificaciones de
    /// entrega, porque los códecs con pérdida mueven el pico al recodificar.
    var techoDePico: Double {
        switch self {
        case .broadcastR128: return -1
        default: return -1
        }
    }
}

/// Qué ganancia hay que aplicar al máster y qué se sacrifica al aplicarla.
struct PlanDeNormalizacion: Sendable, Equatable {
    /// Ganancia a aplicar, en dB.
    let ganancia: Double
    /// Sonoridad que quedará tras aplicarla, en LUFS.
    let sonoridadResultante: Double
    /// Pico real que quedará, en dBTP.
    let picoResultante: Double
    /// `true` si el techo de pico impidió llegar al objetivo.
    let limitadoPorPico: Bool

    var resumen: String {
        // Cuando el techo de pico mandó hay que decirlo aunque la ganancia
        // haya sido cero: «ya está en el objetivo» sería mentira si el pico no
        // dejó subir nada y el montaje se queda lejos de donde se pedía.
        if limitadoPorPico {
            let base = String(
                format: "%@%.1f dB → %.1f LUFS, pico %.1f dBTP",
                ganancia > 0 ? "+" : "", ganancia, sonoridadResultante, picoResultante
            )
            return base + ". No se sube más para no pasar el techo de pico: "
                + "para llegar al objetivo haría falta comprimir, y comprimir sin decirlo "
                + "cambiaría el sonido del montaje."
        }
        if ganancia == 0 { return "Ya está en el objetivo; no se toca el nivel." }
        let signo = ganancia > 0 ? "+" : ""
        return String(
            format: "%@%.1f dB → %.1f LUFS, pico %.1f dBTP",
            signo, ganancia, sonoridadResultante, picoResultante
        )
    }
}

extension ObjetivoDeSonoridad {

    /// Calcula la ganancia de máster para una medida dada.
    ///
    /// Nunca comprime ni limita por su cuenta: si el objetivo no cabe bajo el techo
    /// de pico, sube lo que puede y lo dice. Un limitador silencioso metido en la
    /// exportación es la clase de cosa que arruina una mezcla sin dejar rastro.
    func plan(para medida: MedidaDeSonoridad) -> PlanDeNormalizacion? {
        guard let objetivo, medida.integrada.isFinite, !medida.esSilencio else { return nil }

        let deseada = objetivo - medida.integrada
        let margenDePico = medida.picoReal.isFinite ? techoDePico - medida.picoReal : deseada
        let ganancia = min(deseada, margenDePico)

        return PlanDeNormalizacion(
            ganancia: ganancia,
            sonoridadResultante: medida.integrada + ganancia,
            picoResultante: medida.picoReal.isFinite ? medida.picoReal + ganancia : -Double.infinity,
            limitadoPorPico: ganancia < deseada - 0.05
        )
    }
}
