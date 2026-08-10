import AVFoundation
import Foundation

/// La ley de balance del paneo, compartida por el tap de paneo y la cadena de
/// mezcla: −1 es todo a la izquierda, +1 todo a la derecha, y el centro a
/// media amplitud por canal —no a plena, que doblaría el nivel—.
enum LeyDeBalance {
    static func ganancias(paneo: Double) -> (izquierda: Float, derecha: Float) {
        let p = Float(min(max(paneo, -1), 1))
        return (0.5 - p * 0.5, 0.5 + p * 0.5)
    }
}

/// El procesamiento de mezcla de una pista, modelado y codificado con el proyecto.
///
/// Es la respuesta a Fairlight y al Essential Sound de Premiere para este
/// nicho: lo básico, verificado. El orden de la cadena es fijo y se declara:
/// EQ → compresor → limiter → paneo.
struct BandaDeEQ: Codable, Hashable, Sendable {
    enum TipoDeBanda: String, Codable, Hashable, Sendable {
        case bajo
        case pico
        case alto
    }

    var frecuencia: Double = 1000
    var gananciaDB: Double = 0
    var calidad: Double = 1.0
    var tipo: TipoDeBanda = .pico
}

struct CompresorDePista: Codable, Hashable, Sendable {
    /// Por encima de este nivel (dBFS) se empieza a reducir.
    var umbralDB: Double = -18
    /// Cuánto se comprime: 4 significa que 4 dB por encima del umbral
    /// devuelven 1.
    var ratio: Double = 3
    var ataqueEnSegundos: Double = 0.005
    var solturaEnSegundos: Double = 0.15
}

struct LimitadorDePista: Codable, Hashable, Sendable {
    /// El techo de pico: nada de aquí sale por encima.
    var techoDB: Double = -3
    var ataqueEnSegundos: Double = 0.001
    var solturaEnSegundos: Double = 0.05
}

/// Puerta de ruido: por debajo del umbral la señal se considera ruido de fondo
/// y se cierra hasta la profundidad elegida. La limpieza básica de las
/// grabaciones de voz hecha como efecto de inserción, sin análisis espectral.
struct PuertaDeRuidoDePista: Codable, Hashable, Sendable {
    /// Por debajo de este nivel (dBFS) la puerta se cierra.
    var umbralDB: Double = -45
    /// Velocidad de cierre (señal que cae por debajo del umbral).
    var ataqueEnSegundos: Double = 0.002
    /// Velocidad de apertura (señal que vuelve a pasar).
    var solturaEnSegundos: Double = 0.08
    /// Cuánto se atenúa por debajo del umbral: 0,01 es −40 dB de cierre.
    var profundidad: Double = 0.01

    var umbralLineal: Double { pow(10.0, umbralDB / 20.0) }
}

/// Una banda del compresor multibanda. La cadena divide el espectro en graves,
/// medios y agudos con cruces Linkwitz-Riley fijos (250 Hz y 4 kHz), y cada
/// banda comprime con su propio umbral y ratio —la base del «de-esser» y del
/// control de retumbe sin tocar la voz.
struct BandaDeMultibanda: Codable, Hashable, Sendable {
    var umbralDB: Double = -18
    var ratio: Double = 3
    /// Con la banda desactivada su ganancia es 1: pasa sin tocar.
    var activa: Bool = true
}

struct CompresorMultibandaDePista: Codable, Hashable, Sendable {
    var graves: BandaDeMultibanda = BandaDeMultibanda()
    var medios: BandaDeMultibanda = BandaDeMultibanda()
    var agudos: BandaDeMultibanda = BandaDeMultibanda()
    var ataqueEnSegundos: Double = 0.005
    var solturaEnSegundos: Double = 0.15

    /// Los cruces se declaran fijos: 250 Hz separa graves de medios y 4 kHz
    /// medios de agudos, los valores de la escuela Fairlight/Multiband.
    static let cruceGraveMedio = 250.0
    static let cruceMedioAgudo = 4_000.0
}

/// Retardo de inserción: eco simple con realimentación y mezcla. El tiempo se
/// recorta a 2 s —más allá es retardo de ping-pong, que necesita otra pista.
struct RetardoDePista: Codable, Hashable, Sendable {
    var tiempoEnSegundos: Double = 0.3
    /// Cuánto del eco vuelve a entrar: 0 es un solo eco, cerca de 1 repite.
    var realimentacion: Double = 0.35
    /// 0…1: proporción de señal procesada mezclada con la seca.
    var mezcla: Double = 0.3

    static let tiempoMaximo = 2.0
}

/// Reverberación por el algoritmo de Schroeder (peines + paso-todo), el diseño
/// clásico de las salas sintéticas. `tamano` alarga la cola: a 1 es una sala
/// grande, a 0,1 una cabina.
struct ReverbDePista: Codable, Hashable, Sendable {
    var tamano: Double = 0.5
    /// 0…1: proporción de señal reverberada.
    var mezcla: Double = 0.3

    /// El tiempo de caída a −60 dB según el tamaño, en segundos.
    var tiempoDeCaida: Double { 0.4 + max(0.05, min(tamano, 1)) * 2.0 }
}

/// La cadena DSP de una pista, procesada muestra a muestra.
///
/// El mismo código corre en la reproducción y en la exportación porque el tap
/// viaja en el `mezclaDeAudio`, y el medidor mide lo que sale de aquí —la
/// regla de la casa: reproducir = exportar = medir. El compresor y el limiter
/// son enlazados (un detector para todos los canales), que es lo que mantiene
/// la imagen estéreo cuando un lado dispara.
///
/// El orden de la cadena es fijo y se declara: **puerta de ruido → EQ →
/// multibanda → compresor → limiter → reverb → retardo → paneo**. Los
/// efectos van detrás del limiter a propósito: su mezcla no se vuelve a
/// comprimir, y el paneo cierra la cadena.
final class CadenaDeMezcla: @unchecked Sendable {
    let canales: Int
    private let frecuencia: Double
    private let bandas: [BandaDeEQ]
    private let compresor: CompresorDePista?
    private let limitador: LimitadorDePista?
    private let paneo: Double?
    private let puerta: PuertaDeRuidoDePista?
    private let multibanda: CompresorMultibandaDePista?
    private let reverb: ReverbDePista?
    private let retardo: RetardoDePista?

    // Por canal, un biquad por banda (el diseño RBJ del Audio EQ Cookbook).
    private var filtros: [[(filtro: Biquad, estado: Biquad.Estado)]]
    // Detector del compresor (enlazado) y ganancia suavizada del limiter.
    private var envolventeDelCompresor = 0.0
    private var gananciaDelLimiter = 1.0
    // Ganancia suavizada de la puerta de ruido.
    private var gananciaDeLaPuerta = 1.0
    // Envolvente de pico de la entrada para la decisión de la puerta: una
    // senoide cruza el umbral dos veces por periodo, y decidir con la muestra
    // instantánea abriría y cerraría la puerta a cada cruce (el «chatter»).
    // La envolvente suaviza esos valles y la decisión solo salta de verdad.
    private var envolventeDeLaPuerta = 0.0

    // Multibanda: las cuatro secciones de segundo orden por canal (dos cruces
    // Linkwitz-Riley × dos secciones) y las envolventes enlazadas por banda.
    private var seccionesDeMultibanda: [[Biquad]] = []
    private var estadosDeMultibanda: [[Biquad.Estado]] = []
    private var envolventeDeLosGraves = 0.0
    private var envolventeDeLosMedios = 0.0
    private var envolventeDeLosAgudos = 0.0
    // Scratch de una trama: las muestras de cada banda, por canal. Se reservan
    // una vez en el constructor para no asignar memoria dentro del bucle de
    // audio (un `[Double](repeating:)` por trama haría parpadear el buffer).
    private var bandaGraves: [Double] = []
    private var bandaMedios: [Double] = []
    private var bandaAgudos: [Double] = []
    /// Scratch para adaptar buffers no intercalados al procesador común. Se
    /// reserva fuera del bucle de muestras y evita leer un canal inexistente.
    private var bufferIntercalado: [Float] = []

    // Reverb de Schroeder: cuatro peines en paralelo y dos paso-todo en serie,
    // con su historia por canal. Cada peine tiene su propia historia (cuatro
    // anillos por canal): compartir un anillo mezclaría los estados de los
    // peines, que es lo que hace que el eco se oiga antes de su tiempo.
    // Los retardos son los clásicos de la literatura escalados a la frecuencia
    // real de muestreo.
    private let peinesDeReverb: [Int]
    private let pasoTodoDeReverb: [Int]
    private var historiaDePeines: [[Float]] = []
    private var indicesDePeines: [Int] = []
    private var historiaDePasoTodo: [[Float]] = []
    private var indicesDePasoTodo: [Int] = []
    private let realimentacionDePeines: [Double]

    // Retardo: un anillo por canal, con su índice de escritura.
    private var anilloDeRetardo: [[Float]] = []
    private var indicesDeRetardo: [Int] = []

    private let alfaAtaqueCompresor: Double
    private let alfaSolturaCompresor: Double
    private let alfaAtaqueLimiter: Double
    private let alfaSolturaLimiter: Double
    private let alfaAtaquePuerta: Double
    private let alfaSolturaPuerta: Double
    private let alfaAtaqueMultibanda: Double
    private let alfaSolturaMultibanda: Double
    private let alfaDeEnvolventeDeLaPuerta: Double

    init(
        frecuencia: Double,
        canales: Int,
        bandas: [BandaDeEQ],
        compresor: CompresorDePista?,
        limitador: LimitadorDePista?,
        paneo: Double?,
        claveDeMedidor: String? = nil,
        puerta: PuertaDeRuidoDePista? = nil,
        multibanda: CompresorMultibandaDePista? = nil,
        reverb: ReverbDePista? = nil,
        retardo: RetardoDePista? = nil
    ) {
        self.frecuencia = frecuencia
        self.canales = max(canales, 1)
        self.bandas = bandas
        self.compresor = compresor
        self.limitador = limitador
        self.paneo = paneo
        self.puerta = puerta
        self.multibanda = multibanda
        self.reverb = reverb
        self.retardo = retardo
        self.claveDeMedidor = claveDeMedidor
        self.filtros = (0..<max(canales, 1)).map { _ in
            bandas.map { (Self.filtroDe(banda: $0, frecuencia: frecuencia), Biquad.Estado()) }
        }
        let fs = max(frecuencia, 1)
        self.alfaAtaqueCompresor = exp(-1.0 / (fs * max(compresor?.ataqueEnSegundos ?? 0.005, 0.0005)))
        self.alfaSolturaCompresor = exp(-1.0 / (fs * max(compresor?.solturaEnSegundos ?? 0.15, 0.0005)))
        self.alfaAtaqueLimiter = exp(-1.0 / (fs * max(limitador?.ataqueEnSegundos ?? 0.001, 0.0005)))
        self.alfaSolturaLimiter = exp(-1.0 / (fs * max(limitador?.solturaEnSegundos ?? 0.05, 0.0005)))
        self.alfaAtaquePuerta = exp(-1.0 / (fs * max(puerta?.ataqueEnSegundos ?? 0.002, 0.0005)))
        self.alfaSolturaPuerta = exp(-1.0 / (fs * max(puerta?.solturaEnSegundos ?? 0.08, 0.0005)))
        self.alfaAtaqueMultibanda = exp(-1.0 / (fs * max(multibanda?.ataqueEnSegundos ?? 0.005, 0.0005)))
        self.alfaSolturaMultibanda = exp(-1.0 / (fs * max(multibanda?.solturaEnSegundos ?? 0.15, 0.0005)))
        // La envolvente de la puerta cae con 20 ms: bastante para no seguir los
        // valles de una senoide, rápido para soltar la puerta cuando se calla.
        self.alfaDeEnvolventeDeLaPuerta = exp(-1.0 / (fs * 0.02))

        // Multibanda: dos cruces Linkwitz-Riley (dos secciones de Butterworth
        // encadenadas por cruce) por canal. La banda media es la diferencia de
        // los dos pasos bajos y la aguda lo que sobra de la entrada: así la
        // reconstrucción es exacta por construcción.
        let n = max(canales, 1)
        let seccion = Self.seccionDeButterworth(
            en: CompresorMultibandaDePista.cruceGraveMedio, frecuencia: frecuencia
        )
        let seccionAgudos = Self.seccionDeButterworth(
            en: CompresorMultibandaDePista.cruceMedioAgudo, frecuencia: frecuencia
        )
        self.seccionesDeMultibanda = (0..<n).map { _ in
            [seccion, seccion, seccionAgudos, seccionAgudos]
        }
        self.estadosDeMultibanda = (0..<n).map { _ in
            [Biquad.Estado(), Biquad.Estado(), Biquad.Estado(), Biquad.Estado()]
        }
        self.bandaGraves = [Double](repeating: 0, count: n)
        self.bandaMedios = [Double](repeating: 0, count: n)
        self.bandaAgudos = [Double](repeating: 0, count: n)

        // Reverb de Schroeder: peines y paso-todo de la literatura, escalados a
        // la frecuencia real. La realimentación del peine sale del tiempo de
        // caída elegido: g = 10^(−3D/(t60·fs)).
        let escala = max(frecuencia, 1) / 44_100.0
        let peines = [1116, 1188, 1277, 1356].map { max(1, Int(Double($0) * escala)) }
        self.peinesDeReverb = peines
        self.pasoTodoDeReverb = [556, 441].map { max(1, Int(Double($0) * escala)) }
        let t60 = reverb?.tiempoDeCaida ?? 0.9
        self.realimentacionDePeines = peines.map { pow(10.0, -3.0 * Double($0) / (t60 * fs)) }
        // Un anillo por peine y por canal, del tamaño del peine más largo.
        let tamanoDePeine = peines.max() ?? 1
        self.historiaDePeines = (0..<(n * 4)).map { _ in [Float](repeating: 0, count: tamanoDePeine) }
        self.indicesDePeines = [Int](repeating: 0, count: n * 4)
        // Cada paso-todo tiene su propio anillo doble (historia de entrada y de
        // salida) del tamaño exacto de su retardo: el anillo de tamaño D da la
        // lectura de hace D muestras en el propio índice de escritura.
        self.historiaDePasoTodo = (0..<(n * 2)).map { base in
            let retardo = self.pasoTodoDeReverb[base % 2]
            return [Float](repeating: 0, count: retardo * 2)
        }
        self.indicesDePasoTodo = [Int](repeating: 0, count: n * 2)

        // Retardo: un anillo por canal con el tamaño del tiempo elegido. El
        // paréntesis es obligatorio: `??` liga más flojo que `*`, y sin él el
        // anillo de 0,3 s se convertiría a 0 muestras y el eco llegaría en un
        // suspiro (cazado en el arnés).
        let retardoEnMuestras = max(16, min(
            Int((retardo?.tiempoEnSegundos ?? 0.3) * fs),
            Int(RetardoDePista.tiempoMaximo * fs)
        ))
        self.anilloDeRetardo = (0..<n).map { _ in [Float](repeating: 0, count: retardoEnMuestras) }
        self.indicesDeRetardo = [Int](repeating: 0, count: n)
    }

    /// La sección de Butterworth de segundo orden (Q = √2/2) de un cruce
    /// Linkwitz-Riley: dos de estas encadenadas dan 24 dB/octava y la suma
    /// plana con su complementaria.
    static func seccionDeButterworth(en f0: Double, frecuencia: Double) -> Biquad {
        let w0 = 2.0 * Double.pi * min(max(f0, 20), frecuencia / 2.5) / frecuencia
        let alfa = sin(w0) / sqrt(2.0)
        let coseno = cos(w0)
        let a0 = 1.0 + alfa
        return Biquad(
            b0: (1.0 - coseno) / 2.0 / a0,
            b1: (1.0 - coseno) / a0,
            b2: (1.0 - coseno) / 2.0 / a0,
            a1: -2.0 * coseno / a0,
            a2: (1.0 - alfa) / a0
        )
    }

    /// El biquad paramétrico de una banda, por el diseño RBJ.
    static func filtroDe(banda: BandaDeEQ, frecuencia: Double) -> Biquad {
        let f0 = min(max(banda.frecuencia, 20), frecuencia / 2.5)
        let g = banda.gananciaDB
        let q = max(banda.calidad, 0.1)
        let A = pow(10.0, g / 40.0)
        let w0 = 2.0 * Double.pi * f0 / frecuencia
        let alfa = sin(w0) / (2.0 * q)
        let coseno = -2.0 * cos(w0)

        switch banda.tipo {
        case .pico:
            let a0 = 1.0 + alfa / A
            return Biquad(
                b0: (1.0 + alfa * A) / a0,
                b1: coseno / a0,
                b2: (1.0 - alfa * A) / a0,
                a1: coseno / a0,
                a2: (1.0 - alfa / A) / a0
            )
        case .bajo:
            let raiz = 2.0 * sqrt(A) * alfa
            let a0 = (A + 1.0) + (A - 1.0) * cos(w0) + raiz
            return Biquad(
                b0: (A * ((A + 1.0) - (A - 1.0) * cos(w0)) + raiz) / a0,
                b1: (2.0 * A * ((A - 1.0) - (A + 1.0) * cos(w0))) / a0,
                b2: (A * ((A + 1.0) - (A - 1.0) * cos(w0)) - raiz) / a0,
                a1: (-2.0 * ((A - 1.0) + (A + 1.0) * cos(w0))) / a0,
                a2: ((A + 1.0) + (A - 1.0) * cos(w0) - raiz) / a0
            )
        case .alto:
            let raiz = 2.0 * sqrt(A) * alfa
            let a0 = (A + 1.0) - (A - 1.0) * cos(w0) + raiz
            return Biquad(
                b0: (A * ((A + 1.0) + (A - 1.0) * cos(w0)) + raiz) / a0,
                b1: (-2.0 * A * ((A - 1.0) + (A + 1.0) * cos(w0))) / a0,
                b2: (A * ((A + 1.0) + (A - 1.0) * cos(w0)) - raiz) / a0,
                a1: (2.0 * ((A - 1.0) - (A + 1.0) * cos(w0))) / a0,
                a2: ((A + 1.0) - (A - 1.0) * cos(w0) - raiz) / a0
            )
        }
    }

    /// Acepta tanto audio intercalado como planar. AVFoundation puede entregar
    /// ambos formatos según el medio y el dispositivo; el DSP interno trabaja
    /// en una representación única para conservar sus detectores enlazados.
    func procesar(bufferList: UnsafeMutablePointer<AudioBufferList>, frames: Int) {
        let lista = UnsafeMutableAudioBufferListPointer(bufferList)
        guard frames > 0, !lista.isEmpty else { return }

        let esIntercalado = lista.count == 1
        let numeroDeCanales = esIntercalado
            ? Int(lista[0].mNumberChannels)
            : lista.count
        guard numeroDeCanales == canales, numeroDeCanales > 0 else { return }
        let bytesNecesarios = UInt32(frames * MemoryLayout<Float>.size)

        if esIntercalado {
            guard let datos = lista[0].mData,
                  lista[0].mDataByteSize >= bytesNecesarios else { return }
            procesar(entrelazado: datos.assumingMemoryBound(to: Float.self), frames: frames)
            return
        }

        guard lista.allSatisfy({ $0.mData != nil && $0.mDataByteSize >= bytesNecesarios }) else { return }
        let total = frames * numeroDeCanales
        if bufferIntercalado.count < total {
            bufferIntercalado = [Float](repeating: 0, count: total)
        }
        for canal in 0..<numeroDeCanales {
            guard let datos = lista[canal].mData?.assumingMemoryBound(to: Float.self) else { return }
            for frame in 0..<frames {
                bufferIntercalado[frame * numeroDeCanales + canal] = datos[frame]
            }
        }
        bufferIntercalado.withUnsafeMutableBufferPointer { buffer in
            guard let base = buffer.baseAddress else { return }
            procesar(entrelazado: base, frames: frames)
        }
        for canal in 0..<numeroDeCanales {
            guard let datos = lista[canal].mData?.assumingMemoryBound(to: Float.self) else { return }
            for frame in 0..<frames {
                datos[frame] = bufferIntercalado[frame * numeroDeCanales + canal]
            }
        }
    }

    /// Procesa un buffer intercalado de `frames` muestras por canal.
    func procesar(entrelazado: UnsafeMutablePointer<Float>, frames: Int) {
        guard frames > 0 else { return }
        let canales = self.canales

        for f in 0..<frames {
            // 1. Puerta de ruido: la decisión se toma sobre la envolvente de
            // pico de la entrada y cierra por debajo del umbral con
            // ataque/soltura suavizados.
            if let puerta {
                var picoDelTramo = 0.0
                for c in 0..<canales {
                    picoDelTramo = max(picoDelTramo, abs(Double(entrelazado[f * canales + c])))
                }
                envolventeDeLaPuerta = max(picoDelTramo, alfaDeEnvolventeDeLaPuerta * envolventeDeLaPuerta)
                let objetivo = envolventeDeLaPuerta > puerta.umbralLineal ? 1.0 : puerta.profundidad
                gananciaDeLaPuerta = objetivo < gananciaDeLaPuerta
                    ? alfaAtaquePuerta * gananciaDeLaPuerta + (1 - alfaAtaquePuerta) * objetivo
                    : alfaSolturaPuerta * gananciaDeLaPuerta + (1 - alfaSolturaPuerta) * objetivo
                for c in 0..<canales {
                    entrelazado[f * canales + c] *= Float(gananciaDeLaPuerta)
                }
            }

            // 2. EQ, por canal.
            var pico = 0.0
            for c in 0..<canales {
                let i = f * canales + c
                var x = Double(entrelazado[i])
                for canal in 0..<filtros[c].count {
                    x = filtros[c][canal].filtro.procesar(x, &filtros[c][canal].estado)
                }
                entrelazado[i] = Float(x)
                pico = max(pico, abs(x))
            }

            // 3. Compresor multibanda: cada banda comprime con su detector
            // enlazado y la suma se reconstruye exacta por construcción.
            if let multibanda {
                for c in 0..<canales {
                    let i = f * canales + c
                    let x = Double(entrelazado[i])
                    var pasoGrave = seccionesDeMultibanda[c][0].procesar(x, &estadosDeMultibanda[c][0])
                    pasoGrave = seccionesDeMultibanda[c][1].procesar(pasoGrave, &estadosDeMultibanda[c][1])
                    var pasoAgudo = seccionesDeMultibanda[c][2].procesar(x, &estadosDeMultibanda[c][2])
                    pasoAgudo = seccionesDeMultibanda[c][3].procesar(pasoAgudo, &estadosDeMultibanda[c][3])
                    bandaGraves[c] = pasoGrave
                    bandaMedios[c] = pasoAgudo - pasoGrave
                    bandaAgudos[c] = x - pasoAgudo
                }
                var picoDeGraves = 0.0, picoDeMedios = 0.0, picoDeAgudos = 0.0
                for c in 0..<canales {
                    picoDeGraves = max(picoDeGraves, abs(bandaGraves[c]))
                    picoDeMedios = max(picoDeMedios, abs(bandaMedios[c]))
                    picoDeAgudos = max(picoDeAgudos, abs(bandaAgudos[c]))
                }
                let g1 = Self.gananciaDeBanda(
                    multibanda.graves, env: &envolventeDeLosGraves, pico: picoDeGraves,
                    alfaAtaque: alfaAtaqueMultibanda, alfaSoltura: alfaSolturaMultibanda
                )
                let g2 = Self.gananciaDeBanda(
                    multibanda.medios, env: &envolventeDeLosMedios, pico: picoDeMedios,
                    alfaAtaque: alfaAtaqueMultibanda, alfaSoltura: alfaSolturaMultibanda
                )
                let g3 = Self.gananciaDeBanda(
                    multibanda.agudos, env: &envolventeDeLosAgudos, pico: picoDeAgudos,
                    alfaAtaque: alfaAtaqueMultibanda, alfaSoltura: alfaSolturaMultibanda
                )
                depurarBandaGraves = bandaGraves[0]
                depurarBandaMedios = bandaMedios[0]
                depurarBandaAgudos = bandaAgudos[0]
                depurarGananciaGraves = g1
                depurarGananciaMedios = g2
                depurarGananciaAgudos = g3
                for c in 0..<canales {
                    entrelazado[f * canales + c] = Float(
                        bandaGraves[c] * g1 + bandaMedios[c] * g2 + bandaAgudos[c] * g3
                    )
                }
            }

            // 4. Compresor: detector enlazado con ataque/soltura exponencial.
            if let compresor {
                let nivel = pico > envolventeDelCompresor
                    ? alfaAtaqueCompresor * envolventeDelCompresor + (1 - alfaAtaqueCompresor) * pico
                    : alfaSolturaCompresor * envolventeDelCompresor + (1 - alfaSolturaCompresor) * pico
                envolventeDelCompresor = nivel
                let enDB = 20.0 * log10(max(nivel, 1e-12))
                let sobre = max(0, enDB - compresor.umbralDB)
                let ganancia = pow(10.0, -(sobre * (1.0 - 1.0 / max(compresor.ratio, 1))) / 20.0)
                for c in 0..<canales {
                    entrelazado[f * canales + c] *= Float(ganancia)
                }
            }

            // 5. Limiter: techo duro con ganancia suavizada.
            if let limitador {
                var picoDelTramo = 0.0
                for c in 0..<canales {
                    picoDelTramo = max(picoDelTramo, abs(Double(entrelazado[f * canales + c])))
                }
                let techo = pow(10.0, limitador.techoDB / 20.0)
                let objetivo = picoDelTramo > techo ? techo / max(picoDelTramo, 1e-12) : 1.0
                gananciaDelLimiter = objetivo < gananciaDelLimiter
                    ? alfaAtaqueLimiter * gananciaDelLimiter + (1 - alfaAtaqueLimiter) * objetivo
                    : alfaSolturaLimiter * gananciaDelLimiter + (1 - alfaSolturaLimiter) * objetivo
                for c in 0..<canales {
                    entrelazado[f * canales + c] *= Float(gananciaDelLimiter)
                }
            }

            // 6. Reverb de Schroeder, por canal: cuatro peines en paralelo y
            // dos paso-todo en serie.
            if let reverb {
                for c in 0..<canales {
                    let i = f * canales + c
                    let x = Double(entrelazado[i])
                    var honda = 0.0
                    for k in 0..<4 {
                        let base = c * 4 + k
                        let retardo = peinesDeReverb[k]
                        let tamano = historiaDePeines[base].count
                        let escritura = indicesDePeines[base]
                        let lectura = historiaDePeines[base][(escritura + tamano - retardo) % tamano]
                        historiaDePeines[base][escritura] = Float(
                            x + realimentacionDePeines[k] * Double(lectura)
                        )
                        indicesDePeines[base] = (escritura + 1) % tamano
                        honda += Double(lectura)
                    }
                    honda *= 0.25
                    // Dos paso-todo en serie: cada uno con su anillo doble de
                    // tamaño exacto a su retardo, historia de entrada y salida.
                    for k in 0..<2 {
                        let base = c * 2 + k
                        let retardo = pasoTodoDeReverb[k]
                        let escritura = indicesDePasoTodo[base]
                        let entradaVieja = historiaDePasoTodo[base][escritura]
                        let salidaVieja = historiaDePasoTodo[base][escritura + retardo]
                        let y = Double(entradaVieja) - 0.5 * honda + 0.5 * Double(salidaVieja)
                        historiaDePasoTodo[base][escritura] = Float(honda)
                        historiaDePasoTodo[base][escritura + retardo] = Float(y)
                        indicesDePasoTodo[base] = (escritura + 1) % retardo
                        honda = y
                    }
                    entrelazado[i] = Float(x * (1.0 - reverb.mezcla) + honda * reverb.mezcla)
                }
            }

            // 7. Retardo: anillo por canal con realimentación y mezcla. La
            // lectura es el índice de escritura actual: ahí vive la muestra de
            // hace exactamente `tiempo` (el anillo tiene ese tamaño).
            if let retardo {
                for c in 0..<canales {
                    let i = f * canales + c
                    let x = Double(entrelazado[i])
                    let lectura = Double(anilloDeRetardo[c][indicesDeRetardo[c]])
                    anilloDeRetardo[c][indicesDeRetardo[c]] = Float(x + retardo.realimentacion * lectura)
                    indicesDeRetardo[c] = (indicesDeRetardo[c] + 1) % anilloDeRetardo[c].count
                    entrelazado[i] = Float(x * (1.0 - retardo.mezcla) + lectura * retardo.mezcla)
                }
            }
        }

        // 8. Paneo al final de la cadena: la ley de balance de la casa.
        if let paneo, canales >= 2 {
            let g = LeyDeBalance.ganancias(paneo: paneo)
            for f in 0..<frames {
                entrelazado[f * canales] *= g.izquierda
                entrelazado[f * canales + 1] *= g.derecha
            }
        }

        // Medidor en vivo: el pico del tramo alimenta el registro compartido,
        // que la interfaz lee con su propio temporizador.
        if let claveDeMedidor {
            var pico = 0.0
            for f in 0..<frames {
                for c in 0..<canales {
                    pico = max(pico, abs(Double(entrelazado[f * canales + c])))
                }
            }
            MedidorEnVivo.compartido.escribir(clave: claveDeMedidor, pico: pico)
        }
    }

    /// La ganancia de una banda del multibanda, con su detector enlazado: la
    /// misma ley que el compresor de pista, por banda y sobre el pico de la
    /// banda ya separada por los filtros.
    private static func gananciaDeBanda(
        _ banda: BandaDeMultibanda,
        env: inout Double,
        pico: Double,
        alfaAtaque: Double,
        alfaSoltura: Double
    ) -> Double {
        guard banda.activa, banda.ratio > 1 else { return 1.0 }
        let nivel = pico > env
            ? alfaAtaque * env + (1 - alfaAtaque) * pico
            : alfaSoltura * env + (1 - alfaSoltura) * pico
        env = nivel
        let enDB = 20.0 * log10(max(nivel, 1e-12))
        let sobre = max(0, enDB - banda.umbralDB)
        return pow(10.0, -(sobre * (1.0 - 1.0 / banda.ratio)) / 20.0)
    }

    /// Clave del medidor en vivo, si esta cadena lo alimenta.
    private let claveDeMedidor: String?

    /// Solo para el arnés: las envolventes de las bandas tras procesar.
    func depurarEnvolventes() -> (Double, Double, Double) {
        (envolventeDeLosGraves, envolventeDeLosMedios, envolventeDeLosAgudos)
    }

    /// Solo para el arnés: la última trama de bandas y sus ganancias.
    var depurarBandaGraves: Double = 0
    var depurarBandaMedios: Double = 0
    var depurarBandaAgudos: Double = 0
    var depurarGananciaGraves: Double = 1
    var depurarGananciaMedios: Double = 1
    var depurarGananciaAgudos: Double = 1
}
/// Niveles en vivo de las pistas, escritos por los taps de mezcla y leídos por
/// la interfaz. Es un registro con cerrojo porque el tap corre en el hilo de
/// audio y la UI en el principal.
final class MedidorEnVivo: @unchecked Sendable {
    static let compartido = MedidorEnVivo()
    private let cerrojo = NSLock()
    private var picos: [String: Double] = [:]

    func escribir(clave: String, pico: Double) {
        cerrojo.lock()
        picos[clave] = pico
        cerrojo.unlock()
    }

    /// El pico de la pista en unidades lineales (0…1), o `nil` si aún no hay.
    func picoDe(_ clave: String) -> Double? {
        cerrojo.lock()
        defer { cerrojo.unlock() }
        return picos[clave]
    }
}

/// El tap que cuelga la cadena de mezcla de una pista en el `audioMix`.
///
/// El mismo patrón que `TapDePaneo`: el puntero del tap es la clave del
/// registro (el callback de proceso no recibe estado propio), y la
/// finalización lo libera. Como el tap viaja dentro de `mezclaDeAudio`, la
/// reproducción y la exportación aplican exactamente el mismo código.
final class TapDeMezcla {

    private let cadena: CadenaDeMezcla

    init(cadena: CadenaDeMezcla) {
        self.cadena = cadena
    }

    private static var registro: [MTAudioProcessingTap: TapDeMezcla] = [:]
    private static let cerrojo = NSLock()

    var tap: MTAudioProcessingTap {
        var callbacks = MTAudioProcessingTapCallbacks(
            version: kMTAudioProcessingTapCallbacksVersion_0,
            clientInfo: nil,
            init: nil,
            finalize: { tap in
                TapDeMezcla.cerrojo.lock()
                TapDeMezcla.registro[tap] = nil
                TapDeMezcla.cerrojo.unlock()
            },
            prepare: nil,
            unprepare: nil,
            process: { tap, numberOfFrames, flags, bufferListInOut, _, _ in
                if flags & UInt32(kMTAudioProcessingTapFlag_EndOfStream) != 0 { return }
                TapDeMezcla.cerrojo.lock()
                guard let mezcla = TapDeMezcla.registro[tap] else {
                    TapDeMezcla.cerrojo.unlock()
                    return
                }
                TapDeMezcla.cerrojo.unlock()

                mezcla.cadena.procesar(bufferList: bufferListInOut, frames: Int(numberOfFrames))
            }
        )
        var tapOut: MTAudioProcessingTap?
        let status = MTAudioProcessingTapCreate(
            kCFAllocatorDefault,
            &callbacks,
            kMTAudioProcessingTapCreationFlag_PostEffects,
            &tapOut
        )
        guard status == noErr, let tap = tapOut else {
            // Sin tap no hay procesamiento, pero el audio suena: se degrada a
            // un tap de identidad, como el paneo.
            return Self.tapDeGradacion
        }
        TapDeMezcla.cerrojo.lock()
        TapDeMezcla.registro[tap] = self
        TapDeMezcla.cerrojo.unlock()
        return tap
    }

    private static var tapDeGradacion: MTAudioProcessingTap {
        var callbacks = MTAudioProcessingTapCallbacks(
            version: kMTAudioProcessingTapCallbacksVersion_0,
            clientInfo: nil,
            init: nil,
            finalize: nil,
            prepare: nil,
            unprepare: nil,
            process: { _, _, _, _, _, _ in }
        )
        var tapOut: MTAudioProcessingTap?
        let status = MTAudioProcessingTapCreate(
            kCFAllocatorDefault,
            &callbacks,
            kMTAudioProcessingTapCreationFlag_PostEffects,
            &tapOut
        )
        guard status == noErr, let tap = tapOut else {
            fatalError("editorcito: no se pudo crear el tap de mezcla")
        }
        return tap
    }
}
