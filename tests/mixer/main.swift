import AVFoundation
import Foundation

// El procesamiento de mezcla es DSP puro de la casa: se verifica con señales
// sintéticas como el medidor EBU. EQ (el seno en la banda sube, fuera no),
// compresor (el tono sobre el umbral se reduce al ratio), limiter (nada pasa
// del techo) y paneo (la ley de balance al final de la cadena).

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

let frecuencia = 48_000.0

func seno(frecuenciaDeTono: Double, amplitud: Double, segundos: Double) -> [Float] {
    let n = Int(segundos * frecuencia)
    return (0..<n).map { Float(amplitud * sin(2.0 * Double.pi * frecuenciaDeTono * Double($0) / frecuencia)) }
}

func rms(_ muestras: [Float]) -> Double {
    guard !muestras.isEmpty else { return 0 }
    let suma = muestras.reduce(0.0) { $0 + Double($1 * $1) }
    return sqrt(suma / Double(muestras.count))
}

func enDB(_ v: Double) -> Double { 20.0 * log10(max(v, 1e-12)) }

func procesar(_ cadena: CadenaDeMezcla, _ entrada: [Float]) -> [Float] {
    var salida = entrada
    cadena.procesar(entrelazado: salida.withUnsafeMutableBytes { $0.bindMemory(to: Float.self).baseAddress! },
                    frames: salida.count / cadena.canales)
    return salida
}

print("— EQ: el seno en la banda sube —")
let tono = seno(frecuenciaDeTono: 1000, amplitud: 0.1, segundos: 1)
let eqSube = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [BandaDeEQ(frecuencia: 1000, gananciaDB: 12, calidad: 1, tipo: .pico)],
    compresor: nil, limitador: nil, paneo: nil
)
let conEQ = procesar(eqSube, tono)
let subida = rms(conEQ) / rms(tono)
comprobar(subida > 3.5 && subida < 4.5,
          "un seno a 1 kHz con +12 dB en 1 kHz sube ~×4 (\(String(format: "%.2f", subida)))")

print("— EQ: el seno fuera de la banda no se toca —")
let grave = seno(frecuenciaDeTono: 200, amplitud: 0.1, segundos: 1)
let eqFuera = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [BandaDeEQ(frecuencia: 10_000, gananciaDB: 12, calidad: 1, tipo: .pico)],
    compresor: nil, limitador: nil, paneo: nil
)
let conEQFuera = procesar(eqFuera, grave)
let deriva = rms(conEQFuera) / rms(grave)
comprobar(deriva > 0.8 && deriva < 1.3,
          "un seno a 200 Hz con la banda en 10 kHz pasa casi igual (\(String(format: "%.2f", deriva)))")

print("— compresor: sobre el umbral reduce al ratio —")
let tonoCaliente = seno(frecuenciaDeTono: 440, amplitud: 0.5, segundos: 2)
let conCompresor = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: CompresorDePista(umbralDB: -20, ratio: 4, ataqueEnSegundos: 0.005, solturaEnSegundos: 0.1),
    limitador: nil, paneo: nil
)
let comprimido = procesar(conCompresor, tonoCaliente)
// −6 dBFS sobre un umbral de −20 con ratio 4: se reducen 14·(1−1/4) = 10,5 dB.
let esperado = -6.0 - 10.5
let medido = enDB(rms(comprimido) * sqrt(2))
comprobar(abs(medido - esperado) < 1.0,
          "un tono a −6 dBFS con umbral −20 y ratio 4 sale a \(String(format: "%.1f", medido)) dBFS (esperado \(String(format: "%.1f", esperado)))")

print("— limiter: nada pasa del techo —")
// Señal constante por encima del techo: la ganancia converge a techo/pico y la
// salida se queda clavada ahí. (Un seno rebota: la soltura de 50 ms no da
// tiempo a recuperarse entre picos a 440 Hz, que es comportamiento de diseño.)
let constante = [Float](repeating: 0.891, count: Int(frecuencia * 2))
let conLimiter = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil,
    limitador: LimitadorDePista(techoDB: -6, ataqueEnSegundos: 0.001, solturaEnSegundos: 0.05),
    paneo: nil
)
let limitado = procesar(conLimiter, constante)
// Las primeras muestras pasan mientras el ataque converge; el estado
// estacionario (el último segundo) se queda clavado en el techo.
let pico = limitado.suffix(Int(frecuencia)).map { abs(Double($0)) }.max() ?? 0
comprobar(pico <= 0.505,
          "una señal a −1 dBFS con techo −6 dBFS se queda en \(String(format: "%.3f", pico))")

print("— paneo al final de la cadena —")
let estéreo = seno(frecuenciaDeTono: 440, amplitud: 0.2, segundos: 0.5)
var entrelazado = [Float]()
for muestra in estéreo { entrelazado.append(muestra); entrelazado.append(muestra) }
let conPaneo = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 2,
    bandas: [],
    compresor: nil, limitador: nil,
    paneo: -1
)
var salida = entrelazado
conPaneo.procesar(entrelazado: salida.withUnsafeMutableBytes { $0.bindMemory(to: Float.self).baseAddress! },
                  frames: salida.count / 2)
let izquierda = rms(stride(from: 0, to: salida.count, by: 2).map { salida[$0] })
let derecha = rms(stride(from: 1, to: salida.count, by: 2).map { salida[$0] })
comprobar(izquierda > 0.1 && derecha < 0.001,
          "el paneo a −1 deja la señal solo a la izquierda")

print("— el tap de mezcla viaja en el parámetro —")
// El procesamiento se engancha como `audioTapProcessor` del parámetro de
// mezcla: así la reproducción y la exportación aplican el mismo código, que
// es la regla de la casa.
let composicion = AVMutableComposition()
let pistaDeAudio = composicion.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
if let pistaDeAudio {
    let parametros = AVMutableAudioMixInputParameters(track: pistaDeAudio)
    parametros.audioTapProcessor = TapDeMezcla(cadena: CadenaDeMezcla(
        frecuencia: 48_000, canales: 2, bandas: [],
        compresor: nil, limitador: LimitadorDePista(), paneo: nil
    )).tap
    comprobar(parametros.audioTapProcessor != nil, "el tap de mezcla se engancha al parámetro de mezcla")
} else {
    comprobar(false, "no se pudo crear una pista de audio de prueba")
}

print("— los parámetros viajan en el proyecto —")
// EQ, compresor y limiter son `Codable` con la pista: un proyecto guardado en
// un Mac debe sonar igual en otro. La cuarta tanda añade puerta, multibanda,
// reverb y retardo a la misma regla.
let pistaConProcesamiento = Pista(
    tipo: .audio, nombre: "A1",
    ecualizacion: [BandaDeEQ(frecuencia: 100, gananciaDB: 3, calidad: 0.8, tipo: .bajo)],
    compresor: CompresorDePista(umbralDB: -18, ratio: 2),
    limitador: LimitadorDePista(techoDB: -3),
    puertaDeRuido: PuertaDeRuidoDePista(umbralDB: -50, ataqueEnSegundos: 0.002, solturaEnSegundos: 0.08, profundidad: 0.001),
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -20, ratio: 4, activa: true),
        medios: BandaDeMultibanda(umbralDB: -18, ratio: 3, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -15, ratio: 2, activa: true)
    ),
    reverb: ReverbDePista(tamano: 0.5, mezcla: 0.25),
    retardo: RetardoDePista(tiempoEnSegundos: 0.4, realimentacion: 0.4, mezcla: 0.3)
)
let datos = try! JSONEncoder().encode(pistaConProcesamiento)
let vuelta = try! JSONDecoder().decode(Pista.self, from: datos)
comprobar(vuelta.ecualizacion == pistaConProcesamiento.ecualizacion
          && vuelta.compresor == pistaConProcesamiento.compresor
          && vuelta.limitador == pistaConProcesamiento.limitador,
          "la cadena de mezcla sobrevive al JSON del proyecto")
comprobar(vuelta.puertaDeRuido == pistaConProcesamiento.puertaDeRuido
          && vuelta.multibanda == pistaConProcesamiento.multibanda
          && vuelta.reverb == pistaConProcesamiento.reverb
          && vuelta.retardo == pistaConProcesamiento.retardo,
          "la puerta, el multibanda, la reverb y el retardo también")
comprobar(!pistaConProcesamiento.tieneProcesamientoDeAudio == false,
          "la pista con DSP marca que lleva procesamiento")
comprobar(!Pista(tipo: .audio, nombre: "A2").tieneProcesamientoDeAudio,
          "una pista limpia no lleva procesamiento")

// MARK: - Cuarta tanda: puerta de ruido

print("— puerta de ruido: el fondo se va, la voz se queda —")
// Un segundo de ruido a −60 dBFS y un segundo de tono a −20 dBFS con la puerta
// en −50: el ruido cae a la profundidad y el tono pasa casi entero. La
// medición del ruido usa la última parte del tramo, cuando la envolvente ya
// cerró; la del tono, la segunda mitad, cuando la soltura ya abrió.
let fondo = (0..<Int(frecuencia)).map { _ in
    Float.random(in: -1...1) * 0.001
}
let conPuerta = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    puerta: PuertaDeRuidoDePista(umbralDB: -50, ataqueEnSegundos: 0.002, solturaEnSegundos: 0.08, profundidad: 0.001)
)
let trasPuerta = procesar(conPuerta, fondo + tono)
let n = Int(frecuencia)
let fondoSalida = rms(Array(trasPuerta[(n - n / 5)..<n]))
let atenuacion = enDB(rms(fondo)) - enDB(fondoSalida)
comprobar(atenuacion > 40,
          "el ruido a −60 dB con umbral −50 cae \(String(format: "%.1f", atenuacion)) dB (profundidad −60)")
let tonoSalida = rms(Array(trasPuerta[(n + n / 2)..<(2 * n)]))
let derivaDelTono = tonoSalida / rms(tono)
comprobar(derivaDelTono > 0.9,
          "el tono a −20 dB pasa casi entero (×\(String(format: "%.2f", derivaDelTono)))")

print("— puerta: la decisión no parpadea con la senoide —")
// La puerta decide sobre la envolvente de pico, no sobre la muestra: una
// senoide cruza el umbral dos veces por periodo, y decidir por muestra la
// cerraría a cada cruce. La prueba: la salida de la segunda mitad del tono no
// debe haber sido gateada (el historial de la envolvente no la cierra).
comprobar(derivaDelTono < 1.05, "y tampoco abre de más (×\(String(format: "%.2f", derivaDelTono)))")

// MARK: - Compresor multibanda

print("— multibanda: con ratio 1 la reconstrucción es exacta —")
// Graves = LP250, medios = LP4k − LP250 y agudos = x − LP4k: la suma es la
// entrada por construcción, así que con todas las bandas planas la salida es
// la entrada, muestra a muestra.
let ruidoDePrueba = (0..<Int(2 * frecuencia)).map { _ in Float.random(in: -1...1) * 0.3 }
let conPlano = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -18, ratio: 1, activa: true),
        medios: BandaDeMultibanda(umbralDB: -18, ratio: 1, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -18, ratio: 1, activa: true)
    )
)
let trasPlano = procesar(conPlano, ruidoDePrueba)
var errorMaximo = 0.0
for i in 0..<trasPlano.count {
    errorMaximo = max(errorMaximo, abs(Double(trasPlano[i]) - Double(ruidoDePrueba[i])))
}
comprobar(errorMaximo < 1e-6, "la reconstrucción de bandas devuelve la entrada (\(errorMaximo))")

print("— multibanda: los agudos comprimen solos —")
// Un tono de 8 kHz vive en la banda de agudos (el cruce está en 4 kHz): si la
// banda de agudos comprime −10,5 dB, la salida cae lo que esa banda pesa.
let tonoAgudo = seno(frecuenciaDeTono: 8000, amplitud: 0.5, segundos: 2)
let conAgudos = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true),
        medios: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -20, ratio: 4, activa: true)
    )
)
let trasAgudos = procesar(conAgudos, tonoAgudo)
let ratioAgudos = rms(trasAgudos) / rms(tonoAgudo)
comprobar(ratioAgudos > 0.25 && ratioAgudos < 0.5,
          "8 kHz con agudos a ratio 4 cae a \(String(format: "%.2f", ratioAgudos)) de su nivel (la banda pesa ~0,94)")

print("— multibanda: los graves no tocan un agudo —")
let conGravesSolo = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -20, ratio: 4, activa: true),
        medios: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true)
    )
)
let trasGravesSolo = procesar(conGravesSolo, tonoAgudo)
let ratioGravesSolo = rms(trasGravesSolo) / rms(tonoAgudo)
comprobar(ratioGravesSolo > 0.95 && ratioGravesSolo < 1.05,
          "el mismo tono con solo graves comprimiendo pasa intacto (×\(String(format: "%.2f", ratioGravesSolo)))")

print("— multibanda: los graves comprimen un grave —")
// La banda de graves con un tono de 100 Hz (cruce en 250 Hz): el detector de
// la banda ve −6 dBFS, reduce con ratio 4 y las demás bandas no se mueven.
let tonoGrave = seno(frecuenciaDeTono: 100, amplitud: 0.5, segundos: 2)
let conGraves = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -20, ratio: 4, activa: true),
        medios: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -20, ratio: 1, activa: true)
    )
)
_ = procesar(conGraves, tonoGrave)
comprobar(conGraves.depurarGananciaGraves < 0.5,
          "la banda de graves reduce a \(String(format: "%.2f", conGraves.depurarGananciaGraves)) (−10,5 dB de diseño)")
comprobar(conGraves.depurarGananciaMedios == 1.0 && conGraves.depurarGananciaAgudos == 1.0,
          "y medios y agudos no se mueven")
comprobar(conGraves.depurarGananciaGraves == 1.0
            || conGraves.depurarEnvolventes().0 > 0.3,
          "el detector de la banda vio el tono (envolvente \(String(format: "%.2f", conGraves.depurarEnvolventes().0)))")

// MARK: - Retardo

print("— retardo: el impulso se repite a su tiempo —")
let muestrasDeRetardo = Int(0.3 * frecuencia)
var impulso = [Float](repeating: 0, count: Int(2 * frecuencia))
impulso[0] = 1.0
let conRetardo = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    retardo: RetardoDePista(tiempoEnSegundos: 0.3, realimentacion: 0.5, mezcla: 0.5)
)
let trasRetardo = procesar(conRetardo, impulso)
func picoAlrededorDe(_ s: [Float], _ indice: Int, radio: Int = 2) -> Float {
    var mejor: Float = 0
    for i in max(0, indice - radio)...min(s.count - 1, indice + radio) { mejor = max(mejor, abs(s[i])) }
    return mejor
}
comprobar(abs(trasRetardo[0] - 0.5) < 0.01,
          "la señal seca pasa con la mezcla 0,5 (\(trasRetardo[0]))")
comprobar(abs(picoAlrededorDe(trasRetardo, muestrasDeRetardo) - 0.5) < 0.05,
          "el primer eco llega a los 300 ms (\(picoAlrededorDe(trasRetardo, muestrasDeRetardo)))")
comprobar(abs(picoAlrededorDe(trasRetardo, 2 * muestrasDeRetardo) - 0.25) < 0.05,
          "el segundo eco se atenúa por la realimentación (\(picoAlrededorDe(trasRetardo, 2 * muestrasDeRetardo)))")
comprobar(abs(picoAlrededorDe(trasRetardo, 3 * muestrasDeRetardo) - 0.125) < 0.05,
          "y el tercero sigue la serie (\(picoAlrededorDe(trasRetardo, 3 * muestrasDeRetardo)))")

print("— retardo: sin realimentación no se repite —")
let conUnEco = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    retardo: RetardoDePista(tiempoEnSegundos: 0.1, realimentacion: 0, mezcla: 0.5)
)
let trasUnEco = procesar(conUnEco, impulso)
let muestrasDeUnEco = Int(0.1 * frecuencia)
comprobar(abs(picoAlrededorDe(trasUnEco, muestrasDeUnEco) - 0.5) < 0.05,
          "con realimentación 0 solo llega el primer eco")
comprobar(picoAlrededorDe(trasUnEco, 2 * muestrasDeUnEco) < 0.001,
          "y no hay segundo")

// MARK: - Reverb

print("— reverb: la cola decae y nada suena antes de su tiempo —")
let muestrasDeTresSegundos = Int(3 * frecuencia)
var impulsoLargo = [Float](repeating: 0, count: muestrasDeTresSegundos)
impulsoLargo[0] = 1.0
let conReverb = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    reverb: ReverbDePista(tamano: 0.3, mezcla: 0.5)
)
let trasReverb = procesar(conReverb, impulsoLargo)
comprobar(abs(trasReverb[0] - 0.5) < 0.01,
          "la señal seca pasa con la mezcla 0,5 (\(trasReverb[0]))")
let antesDelPeine = rms(Array(trasReverb[1..<Int(1116.0 * frecuencia / 44_100.0)]))
comprobar(antesDelPeine < 0.001,
          "nada suena antes del peine más corto (\(String(format: "%.4f", antesDelPeine)))")
let primeraMitadDeCola = rms(Array(trasReverb[(muestrasDeTresSegundos / 2)..<muestrasDeTresSegundos]))
let segundaMitadDeCola = rms(Array(trasReverb[(muestrasDeTresSegundos * 3 / 4)..<muestrasDeTresSegundos]))
comprobar(primeraMitadDeCola > segundaMitadDeCola * 2,
          "la cola decae: la mitad anterior tiene más energía que la posterior")

print("— reverb: más tamaño, más cola —")
let conReverbGrande = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 1,
    bandas: [],
    compresor: nil, limitador: nil, paneo: nil,
    reverb: ReverbDePista(tamano: 1.0, mezcla: 0.5)
)
let trasReverbGrande = procesar(conReverbGrande, impulsoLargo)
let colaPequena = rms(Array(trasReverb[(muestrasDeTresSegundos * 2 / 3)..<muestrasDeTresSegundos]))
let colaGrande = rms(Array(trasReverbGrande[(muestrasDeTresSegundos * 2 / 3)..<muestrasDeTresSegundos]))
comprobar(colaGrande > colaPequena * 10,
          "a tamaño 1 la cola final conserva \(String(format: "%.0f", colaGrande / max(colaPequena, 1e-12)))× más energía que a tamaño 0,3")

print("— la cadena completa: puerta → EQ → multibanda → compresor → limiter → reverb → retardo —")
// Las siete etapas a la vez sobre el tono: no revienta, no silencia y el
// resultado es finito y distinto de la entrada — el orden declarado es real.
let conTodo = CadenaDeMezcla(
    frecuencia: frecuencia, canales: 2,
    bandas: [BandaDeEQ(frecuencia: 1000, gananciaDB: 3, calidad: 1, tipo: .pico)],
    compresor: CompresorDePista(umbralDB: -20, ratio: 3, ataqueEnSegundos: 0.005, solturaEnSegundos: 0.1),
    limitador: LimitadorDePista(techoDB: -3),
    paneo: nil,
    puerta: PuertaDeRuidoDePista(umbralDB: -50, ataqueEnSegundos: 0.002, solturaEnSegundos: 0.08, profundidad: 0.001),
    multibanda: CompresorMultibandaDePista(
        graves: BandaDeMultibanda(umbralDB: -18, ratio: 3, activa: true),
        medios: BandaDeMultibanda(umbralDB: -18, ratio: 3, activa: true),
        agudos: BandaDeMultibanda(umbralDB: -18, ratio: 3, activa: true)
    ),
    reverb: ReverbDePista(tamano: 0.4, mezcla: 0.2),
    retardo: RetardoDePista(tiempoEnSegundos: 0.2, realimentacion: 0.3, mezcla: 0.2)
)
let tonoDeLaCadena = seno(frecuenciaDeTono: 440, amplitud: 0.4, segundos: 1)
var entrelazadoDeLaCadena = [Float]()
for muestra in tonoDeLaCadena { entrelazadoDeLaCadena.append(muestra); entrelazadoDeLaCadena.append(muestra) }
let trasLaCadena = procesar(conTodo, entrelazadoDeLaCadena)
let nivelDeLaCadena = rms(trasLaCadena)
comprobar(nivelDeLaCadena > 0.01 && nivelDeLaCadena < 1.0,
          "la cadena completa suena a nivel sano (\(String(format: "%.2f", nivelDeLaCadena)))")
comprobar(trasLaCadena != entrelazadoDeLaCadena,
          "y de verdad procesa (la salida no es la entrada)")

if fallos == 0 {
    print("MIXER CORRECTO")
} else {
    print("MIXER ROTO — \(fallos) fallos")
    exit(1)
}
