import Foundation

// Prueba de conformidad del medidor contra las señales oficiales de
// EBU Tech 3341/3342, más una generación sintética de las mismas señales para
// cuando el set de la EBU no está a mano.
//
// El set oficial se descarga de tech.ebu.ch (test material, «ebu-loudness-test-
// set») y se pasa como argumento:  ./probar-sonoridad.sh /ruta/al/set
//
// Cómo se resolvió qué significa «−23 dBFS» en la señal de referencia: el
// propio archivo seq-3341-1-16bit.wav de la EBU tiene el pico de cada canal a
// −22,94 dBFS y el RMS a −25,97 dBFS. Es decir, «dBFS» es la cresta por canal,
// y el medidor tiene que leer −23,0 LUFS ± 0,1. Un tono cuya cresta esté a
// −23 dBFS y cuyo RMS quede 3,01 dB por debajo —la interpretación alternativa
// de «dBFS como RMS»— leería −26,0 y quedaría descartada por la propia EBU.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

func formato(_ v: Double) -> String { String(format: "%.2f", v) }

// MARK: - Señales sintéticas

/// Senoide estéreo de 1 kHz con la cresta de pico pedida, en dBFS.
///
/// El argumento es la cresta, no el nivel: así la prueba replica exactamente
/// cómo está generado el archivo de la EBU (cresta a −23 dBFS por canal).
func seno(crestaEndBFS: Double, duracion: Double, muestreo: Double, canales: Int = 2) -> [Float] {
    let marcos = Int(duracion * muestreo)
    let amplitud = pow(10, crestaEndBFS / 20)
    var salida = [Float](repeating: 0, count: marcos * canales)
    let paso = 2.0 * Double.pi * 1000 / muestreo
    for m in 0..<marcos {
        let v = Float(amplitud * sin(paso * Double(m)))
        for c in 0..<canales { salida[m * canales + c] = v }
    }
    return salida
}

func medir(_ muestras: [Float], muestreo: Double, canales: Int, pesos: [Double]? = nil) -> MedidaDeSonoridad {
    let medidor = MedidorDeSonoridad(frecuencia: muestreo, canales: canales, pesos: pesos)
    medidor.procesar(entrelazado: muestras)
    return medidor.finalizar()
}

// MARK: - Carga de WAV del set de la EBU

struct AudioDePrueba {
    let muestreo: Double
    let canales: Int
    let entrelazado: [Float]

    /// Pico y RMS por canal, para mostrar qué hay dentro del archivo oficial.
    func estadisticas() -> (pico: [Double], rms: [Double]) {
        let marcos = entrelazado.count / canales
        var pico = [Double](repeating: 0, count: canales)
        var suma = [Double](repeating: 0, count: canales)
        for m in 0..<marcos {
            for c in 0..<canales {
                let v = Double(entrelazado[m * canales + c])
                pico[c] = max(pico[c], abs(v))
                suma[c] += v * v
            }
        }
        let rms = suma.map { sqrt($0 / Double(marcos)) }
        return (pico, rms)
    }
}

/// Carga WAV PCM de 16 y 24 bits, mono a 6 canales, con o sin extensible.
/// Solo la parte de datos importa para el medidor: el layout de canales se
/// declara aparte en cada prueba (los pesos van explícitos cuando hay surround).
func cargarWav(_ ruta: String) -> AudioDePrueba? {
    guard let datos = FileManager.default.contents(atPath: ruta) else { return nil }
    guard datos.count > 44, Array(datos[0..<4]) == Array("RIFF".utf8) else { return nil }

    var canales = 0
    var muestreo = 0.0
    var bits = 0
    var pcm = Data()

    var offset = 12
    while offset + 8 <= datos.count {
        let id = String(data: datos[offset..<offset + 4], encoding: .ascii)
        let tamano = datos.withUnsafeBytes {
            $0.loadUnaligned(fromByteOffset: offset + 4, as: UInt32.self)
        }
        guard offset + 8 + Int(tamano) <= datos.count else { break }

        if id == "fmt " {
            let formato = datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 8, as: UInt16.self)
            }
            canales = Int(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 10, as: UInt16.self)
            })
            muestreo = Double(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 12, as: UInt32.self)
            })
            bits = Int(datos.withUnsafeBytes {
                $0.loadUnaligned(fromByteOffset: offset + 22, as: UInt16.self)
            })
            // El extensible (0xFFFE) sigue siendo PCM aquí: el subformato
            // indicaría lo contrario, y todo el set de la EBU es PCM.
            _ = formato
        } else if id == "data" {
            pcm = datos.subdata(in: (offset + 8)..<(offset + 8 + Int(tamano)))
        }
        offset += 8 + Int(tamano) + Int(tamano) % 2
    }

    guard canales > 0, muestreo > 0, (bits == 16 || bits == 24), !pcm.isEmpty else { return nil }
    let bytesPorMuestra = bits / 8
    let marcos = pcm.count / (canales * bytesPorMuestra)
    guard marcos > 0 else { return nil }

    var salida = [Float](repeating: 0, count: marcos * canales)
    pcm.withUnsafeBytes { bytes in
        let crudos = bytes.bindMemory(to: UInt8.self)
        for m in 0..<marcos {
            for c in 0..<canales {
                let base = (m * canales + c) * bytesPorMuestra
                var v: Float = 0
                if bits == 16 {
                    let crudo = Int16(bitPattern: UInt16(crudos[base]) | (UInt16(crudos[base + 1]) << 8))
                    v = Float(crudo) / 32_768
                } else {
                    let u = UInt32(crudos[base]) | (UInt32(crudos[base + 1]) << 8) | (UInt32(crudos[base + 2]) << 16)
                    let signo = Int32(bitPattern: (u & 0x80_0000) != 0 ? u | 0xFF00_0000 : u)
                    v = Float(signo) / 8_388_608
                }
                salida[m * canales + c] = v
            }
        }
    }
    return AudioDePrueba(muestreo: muestreo, canales: canales, entrelazado: salida)
}

// MARK: - Suite oficial

func probarIntegrada(_ archivo: String, esperado: Double, pesos: [Double]? = nil, en dir: String) {
    guard let audio = cargarWav(dir + "/" + archivo) else {
        print("  FALLO  no se pudo leer \(archivo)"); fallos += 1; return
    }
    let medida = medir(audio.entrelazado, muestreo: audio.muestreo, canales: audio.canales, pesos: pesos)
    comprobar(abs(medida.integrada - esperado) < 0.1,
        "\(archivo) → \(formato(medida.integrada)) LUFS (esperado \(formato(esperado)) ± 0,1)")
}

func probarLRA(_ archivo: String, esperado: Double, en dir: String) {
    guard let audio = cargarWav(dir + "/" + archivo) else {
        print("  FALLO  no se pudo leer \(archivo)"); fallos += 1; return
    }
    let medida = medir(audio.entrelazado, muestreo: audio.muestreo, canales: audio.canales)
    comprobar(abs(medida.rango - esperado) <= 1.0,
        "\(archivo) → LRA \(formato(medida.rango)) LU (esperado \(formato(esperado)) ± 1)")
}

func probarPicoReal(_ archivo: String, esperado: Double, en dir: String) {
    guard let audio = cargarWav(dir + "/" + archivo) else {
        print("  FALLO  no se pudo leer \(archivo)"); fallos += 1; return
    }
    let medida = medir(audio.entrelazado, muestreo: audio.muestreo, canales: audio.canales)
    // La tolerancia de la EBU para los picos reales no es simétrica: el
    // sobreimpulso se tolera peor que el defecto.
    comprobar(medida.picoReal >= esperado - 0.4 && medida.picoReal <= esperado + 0.2,
        "\(archivo) → pico \(formato(medida.picoReal)) dBTP (esperado \(formato(esperado)))")
}

// MARK: - Sintéticas

let fs = 48_000.0
let canales = 2

print("— señal de referencia: senoide estéreo de 1 kHz, cresta −23 dBFS —")

// Si «−23 dBFS» fuera el RMS, la cresta iría 3,01 dB más alta y esto leería
// −26,0. La EBU exige −23,0, y el archivo oficial demuestra la convención.
let referencia = seno(crestaEndBFS: -23, duracion: 5, muestreo: fs)
let lectura = medir(referencia, muestreo: fs, canales: canales).integrada
print("  cresta −23 dBFS → \(formato(lectura)) LUFS")
comprobar(abs(lectura - (-23.0)) < 0.1, "la senoide de referencia lee −23,0 LUFS ± 0,1")

let otraLectura = medir(seno(crestaEndBFS: -26.01, duracion: 5, muestreo: fs), muestreo: fs, canales: canales).integrada
print("  cresta −26,01 dBFS (el «−23 dBFS» de la interpretación RMS) → \(formato(otraLectura)) LUFS")
comprobar(abs(otraLectura - (-23.0)) > 0.5, "la interpretación de RMS quedaría descartada por la EBU")

print("— linealidad: 6,02 dB exactos arriba y abajo —")

// Se compara contra la lectura de la propia referencia, no contra −23,0 fijo:
// el filtro pesa 0,01 dB distinto del ideal y lo que debe ser exacto es la
// caída respecto al mismo medidor.
let baja = medir(seno(crestaEndBFS: -29.02, duracion: 5, muestreo: fs), muestreo: fs, canales: canales).integrada
comprobar(abs(baja - (lectura - 6.02)) < 0.05, "cresta −29,02 dBFS → \(formato(baja)) LUFS, 6,02 abajo de la referencia")
let alta = medir(seno(crestaEndBFS: -16.98, duracion: 5, muestreo: fs), muestreo: fs, canales: canales).integrada
comprobar(abs(alta - (lectura + 6.02)) < 0.05, "cresta −16,98 dBFS → \(formato(alta)) LUFS, 6,02 arriba de la referencia")

print("— pico real y recorrido de la señal de referencia —")

let medidaRef = medir(referencia, muestreo: fs, canales: canales)
print("  pico real \(formato(medidaRef.picoReal)) dBTP · LRA \(formato(medidaRef.rango)) LU")
comprobar(abs(medidaRef.picoReal - (-23.0)) < 0.1, "el pico real coincide con la cresta (−23,0 dBTP)")
comprobar(medidaRef.rango < 0.5, "un tono constante no tiene recorrido")

print("— las puertas: silencio y pausas no arrastran la integrada —")

// El tono va a 40 s para que los bloques de 400 ms que solapan la frontera con
// la cola pesen menos de 0,02 LU en la media — solapan la transición, miden
// energía real y la norma los cuenta, así que acortar el tono sería fabricar
// el fallo. Sin puerta, 20 s de piso a −70 hundirían la media hasta ~−34.
let tono = seno(crestaEndBFS: -23, duracion: 40, muestreo: fs)
let lecturaTono = medir(tono, muestreo: fs, canales: canales).integrada
print("  tono solo (40 s): \(formato(lecturaTono)) LUFS")

// Piso a −70 dBFS de cresta: mide justo en el umbral de la puerta absoluta
// (−70 LUFS), así que tiene que quedarse fuera.
let conPiso = tono + seno(crestaEndBFS: -70, duracion: 20, muestreo: fs)
let lecturaConPiso = medir(conPiso, muestreo: fs, canales: canales).integrada
print("  40 s a −23 + 20 s de piso a −70 → \(formato(lecturaConPiso)) LUFS")
comprobar(abs(lecturaConPiso - (-23.0)) < 0.1, "un piso a −70 dBFS no arrastra la integrada")

let conSilencio = tono + [Float](repeating: 0, count: Int(15 * fs) * 2)
let lecturaConSilencio = medir(conSilencio, muestreo: fs, canales: canales).integrada
print("  40 s a −23 + 15 s de ceros → \(formato(lecturaConSilencio)) LUFS")
comprobar(abs(lecturaConSilencio - (-23.0)) < 0.1, "el silencio digital tampoco arrastra")

// Un pasaje a −40 LUFS está 17 LU por debajo de la media: la puerta relativa
// (−10 LU) tiene que quitarlo, o la media caería a ~−24.
let conPausa = tono + seno(crestaEndBFS: -40, duracion: 10, muestreo: fs)
let lecturaConPausa = medir(conPausa, muestreo: fs, canales: canales).integrada
print("  40 s a −23 + 10 s a −40 → \(formato(lecturaConPausa)) LUFS")
comprobar(abs(lecturaConPausa - (-23.0)) < 0.1, "la puerta relativa quita el pasaje 17 LU por debajo")

print("— coeficientes deducidos a 44,1 kHz —")

let fs44 = 44_100.0
let lectura44 = medir(seno(crestaEndBFS: -23, duracion: 5, muestreo: fs44), muestreo: fs44, canales: canales).integrada
print("  cresta −23 dBFS a 44,1 kHz → \(formato(lectura44)) LUFS")
comprobar(abs(lectura44 - (-23.0)) < 0.1, "el pesado K deducido mide bien fuera de 48 kHz")

print("— plan de normalización —")

func medida(_ integrada: Double, pico: Double) -> MedidaDeSonoridad {
    MedidaDeSonoridad(integrada: integrada, rango: 5, picoReal: pico, duracion: 60)
}

// Sin techo de por medio, la ganancia es justo la distancia al objetivo.
let desahogada = ObjetivoDeSonoridad.youtube.plan(para: medida(-23, pico: -10))
comprobar(desahogada != nil && abs(desahogada!.ganancia - 9) < 0.001, "de −23 a −14 pide +9 dB")
comprobar(desahogada != nil && abs(desahogada!.sonoridadResultante - (-14)) < 0.001, "y llega al objetivo")
comprobar(desahogada != nil && abs(desahogada!.picoResultante - (-1)) < 0.001, "dejando el pico en el techo")
comprobar(desahogada?.limitadoPorPico == false, "sin techo de por medio no hay límite")

let yaEnObjetivo = ObjetivoDeSonoridad.broadcastR128.plan(para: medida(-23, pico: -10))
comprobar(yaEnObjetivo != nil && abs(yaEnObjetivo!.ganancia) < 0.001, "ya en el objetivo no toca nada")

// Pico justo en el techo: subir los 9 dB pasaría de −1 dBTP y no cabe ni un
// decibelio; el plan lo dice con el resumen de limitadoPorPico.
let apretada = ObjetivoDeSonoridad.youtube.plan(para: medida(-23, pico: -1))
comprobar(apretada != nil && abs(apretada!.ganancia) < 0.001, "con el pico en el techo no sube nada")
comprobar(apretada?.limitadoPorPico == true, "y lo marca como limitado por pico")
comprobar(apretada != nil && apretada!.resumen.contains("No se sube más"), "el resumen lo dice sin adornos")

// Un montaje silencioso no tiene plan: no hay nada que normalizar.
comprobar(
    ObjetivoDeSonoridad.youtube.plan(para: MedidaDeSonoridad(integrada: -90, rango: 0, picoReal: -Double.infinity, duracion: 10)) == nil,
    "un silencio no da plan"
)

// MARK: - Set oficial de la EBU

if CommandLine.arguments.count > 1 {
    let dir = CommandLine.arguments[1]
    print("")
    print("— set oficial de la EBU (\(dir)) —")

    if let uno = cargarWav(dir + "/seq-3341-1-16bit.wav") {
        let (pico, rms) = uno.estadisticas()
        let picoDB = 20 * log10(pico.max() ?? 1)
        let rmsDB = 20 * log10(rms.max() ?? 1)
        print("  seq-3341-1: pico por canal \(formato(picoDB)) dBFS · RMS por canal \(formato(rmsDB)) dBFS")
        comprobar(abs(picoDB - (-23.0)) < 0.3 && abs(rmsDB - (-26.0)) < 0.5,
            "el archivo oficial confirma: «dBFS» es la cresta por canal (−23) y no el RMS (−26)")
    }

    print("  — sonoridad integrada (± 0,1) —")
    probarIntegrada("seq-3341-1-16bit.wav", esperado: -23.0, en: dir)
    probarIntegrada("1kHz Sine -20 LUFS-16bit.wav", esperado: -20.0, en: dir)
    probarIntegrada("1kHz Sine -26 LUFS-16bit.wav", esperado: -26.0, en: dir)
    probarIntegrada("1kHz Sine -40 LUFS-16bit.wav", esperado: -40.0, en: dir)
    probarIntegrada("seq-3341-2-16bit.wav", esperado: -33.0, en: dir)
    probarIntegrada("seq-3341-3-16bit-v02.wav", esperado: -23.0, en: dir)
    probarIntegrada("seq-3341-4-16bit-v02.wav", esperado: -23.0, en: dir)
    probarIntegrada("seq-3341-5-16bit-v02.wav", esperado: -23.0, en: dir)
    // Disposiciones reales: 5.0 y 5.1 no ordenan los surround igual.
    probarIntegrada("seq-3341-6-5channels-16bit.wav", esperado: -23.0,
        pesos: [1, 1, 1, 1.41, 1.41], en: dir)
    probarIntegrada("seq-3341-6-6channels-WAVEEX-16bit.wav", esperado: -23.0,
        pesos: [1, 1, 1, 0, 1.41, 1.41], en: dir)
    probarIntegrada("seq-3341-7_seq-3342-5-24bit.wav", esperado: -23.0, en: dir)
    probarIntegrada("seq-3341-2011-8_seq-3342-6-24bit-v02.wav", esperado: -23.0, en: dir)

    print("  — recorrido LRA (EBU Tech 3342, ± 1) —")
    probarLRA("seq-3342-1-16bit.wav", esperado: 10.0, en: dir)
    probarLRA("seq-3342-2-16bit.wav", esperado: 5.0, en: dir)
    probarLRA("seq-3342-3-16bit.wav", esperado: 20.0, en: dir)
    probarLRA("seq-3342-4-16bit.wav", esperado: 15.0, en: dir)
    probarLRA("seq-3341-7_seq-3342-5-24bit.wav", esperado: 5.0, en: dir)
    probarLRA("seq-3341-2011-8_seq-3342-6-24bit-v02.wav", esperado: 15.0, en: dir)

    print("  — picos reales (señales 15–23 del set v05) —")
    // Las definiciones del set v05, leídas del contenido de los archivos: las
    // señales 15–18 son senoides de 12/8/6 kHz con cresta de muestra a −6 dBFS
    // y pico real a −6 dBTP; la 19 es una senoide de 12 kHz a 0 dBFS (cuatro
    // muestras por periodo) cuyo pico real rebasa los 0 dBFS hasta ~+3 dBTP;
    // y las 20–23 son senoides de 8 kHz a 0 dBFS con pico real en 0 dBTP.
    // (libebur128 documenta otra numeración; el contenido del zip manda.)
    for n in 15...18 { probarPicoReal("seq-3341-\(n)-24bit.wav.wav", esperado: -6.0, en: dir) }
    probarPicoReal("seq-3341-19-24bit.wav.wav", esperado: 3.0, en: dir)
    for n in 20...23 { probarPicoReal("seq-3341-\(n)-24bit.wav.wav", esperado: 0.0, en: dir) }
} else {
    print("")
    print("(sin set de la EBU: pasa la ruta al directorio para la conformidad completa)")
}

print("")
print(fallos == 0 ? "TODO CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
