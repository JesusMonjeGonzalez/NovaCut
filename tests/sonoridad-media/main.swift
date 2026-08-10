import AVFoundation
import Foundation

// Verifica el camino completo de medición sobre un archivo real: el audio del
// montaje se lee con AVAssetReader sobre la composición y su audioMix, y la
// medida tiene que reproducir la del archivo original cuando el clip va a 0 dB,
// y bajar exactamente 6,02 dB cuando la mezcla baja 6,02. Sin esto, el medidor
// de Sonoridad.swift podría ser perfecto y la app medir otra cosa.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func formato(_ v: Double) -> String { String(format: "%.2f", v) }

/// Lee la pista de audio del archivo tal cual, sin composición ni mezcla.
func medirFuente(_ medio: MedioResuelto, segundos: Double) throws -> MedidaDeSonoridad {
    guard let pista = medio.pistaDeAudio else { throw ErrorDeSonoridadMedia.sinAudio }
    let lector = try AVAssetReader(asset: medio.asset)
    let ajustes: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVLinearPCMIsFloatKey: true,
        AVLinearPCMBitDepthKey: 32,
        AVLinearPCMIsNonInterleaved: false,
    ]
    let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: ajustes)
    guard lector.canAdd(salida) else { throw ErrorDeSonoridadMedia.sinAudio }
    lector.add(salida)
    guard lector.startReading() else { throw ErrorDeSonoridadMedia.lecturaFallida("lector") }

    let hasta = CMTime(seconds: segundos, preferredTimescale: 600)
    var medidor: MedidorDeSonoridad?
    while let buffer = salida.copyNextSampleBuffer() {
        if CMTimeCompare(buffer.presentationTimeStamp, hasta) > 0 { break }
        guard let descripcion = CMSampleBufferGetFormatDescription(buffer),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(descripcion),
              let bloque = CMSampleBufferGetDataBuffer(buffer) else { continue }
        let marcos = CMSampleBufferGetNumSamples(buffer)
        let canales = Int(asbd.pointee.mChannelsPerFrame)
        let frecuencia = asbd.pointee.mSampleRate
        guard marcos > 0, canales > 0, frecuencia > 0 else { continue }
        if medidor == nil { medidor = MedidorDeSonoridad(frecuencia: frecuencia, canales: canales) }
        let bytes = CMBlockBufferGetDataLength(bloque)
        guard bytes >= marcos * canales * MemoryLayout<Float>.size else { continue }
        var muestras = [Float](repeating: 0, count: marcos * canales)
        let estado = muestras.withUnsafeMutableBytes {
            CMBlockBufferCopyDataBytes(bloque, atOffset: 0, dataLength: bytes, destination: $0.baseAddress!)
        }
        guard estado == kCMBlockBufferNoErr else { continue }
        medidor?.procesar(entrelazado: muestras)
    }
    lector.cancelReading()
    guard let medidor else { throw ErrorDeSonoridadMedia.sinAudio }
    return medidor.finalizar()
}

/// Monta el primer tramo del archivo en una línea de tiempo con la ganancia
/// dada y mide la composición con su mezcla: el camino real de la app.
func montarYMedir(_ medio: MedioResuelto, ganancia: Double, segundos: Double) throws -> MedidaDeSonoridad {
    let base = Timebase.habituales.min { abs($0.fps - medio.fps) < abs($1.fps - medio.fps) } ?? .p25
    var linea = LineaDeTiempo.nueva(timebase: base)
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id

    var clip = Clip(
        mediaID: medio.id,
        nombre: "prueba",
        inicio: 0,
        duracion: base.frames(segundos: segundos),
        entradaEnOrigen: 0
    )
    clip.ganancia = ganancia
    linea.sobrescribir(clip, enPista: a1, en: 0)

    let render = ConstructorDeMontaje.construir(linea, medios: [medio.id: medio])
    return try SonoridadMedia.medir(render)
}

/// Igual que `montarYMedir` pero con el clip empezando en el segundo pedido.
func montarYMedirConDesplazamiento(_ medio: MedioResuelto, segundos: Double, en segundo: Double) throws -> MedidaDeSonoridad {
    let base = Timebase.habituales.min { abs($0.fps - medio.fps) < abs($1.fps - medio.fps) } ?? .p25
    var linea = LineaDeTiempo.nueva(timebase: base)
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id
    let clip = Clip(
        mediaID: medio.id, nombre: "prueba",
        inicio: base.frames(segundos: segundo),
        duracion: base.frames(segundos: segundos),
        entradaEnOrigen: 0
    )
    linea.sobrescribir(clip, enPista: a1, en: clip.inicio)
    let render = ConstructorDeMontaje.construir(linea, medios: [medio.id: medio])
    return try SonoridadMedia.medir(render)
}

/// Montaje con el clip desplazado y fundidos: el silencio previo no debe sonar
/// y los fundidos deben aplicarse; lo que se compara es que el lector y la
/// exportación midan lo mismo sobre este montaje, que es lo que necesita la
/// normalización al exportar.
func montajeComplejo(_ medio: MedioResuelto, segundos: Double, conDucking: Bool) throws -> (AVMutableComposition, AVMutableAudioMix?) {
    let base = Timebase.habituales.min { abs($0.fps - medio.fps) < abs($1.fps - medio.fps) } ?? .p25
    var linea = LineaDeTiempo.nueva(timebase: base)
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id
    let a2 = linea.pistas.first { $0.nombre == "A2" }!.id

    // Voz: un clip desplazado cinco segundos, con fundidos de entrada y salida.
    var voz = Clip(
        mediaID: medio.id, nombre: "voz",
        inicio: base.frames(segundos: 5),
        duracion: base.frames(segundos: segundos - 6),
        entradaEnOrigen: 0
    )
    voz.entradaFundido = base.frames(segundos: 0.5)
    voz.salidaFundido = base.frames(segundos: 0.5)
    linea.sobrescribir(voz, enPista: a1, en: voz.inicio)

    // Música de fondo a −9 dB desde el principio, con ducking sobre la voz.
    var musica = Clip(
        mediaID: medio.id, nombre: "música",
        inicio: 0,
        duracion: base.frames(segundos: segundos),
        entradaEnOrigen: 0
    )
    musica.ganancia = -9
    linea.sobrescribir(musica, enPista: a2, en: 0)
    if conDucking, let indice = linea.indiceDePista(a2) {
        linea.pistas[indice].ducking = true
    }

    let render = ConstructorDeMontaje.construir(linea, medios: [medio.id: medio])
    return (render.composicion, render.mezclaDeAudio)
}

/// Mide un archivo ya exportado leyendo su pista de audio directamente.
func medirArchivo(_ url: URL) async throws -> MedidaDeSonoridad {
    let asset = AVURLAsset(url: url)
    guard let pista = (try? await asset.loadTracks(withMediaType: .audio))?.first else {
        throw ErrorDeSonoridadMedia.sinAudio
    }
    let lector = try AVAssetReader(asset: asset)
    let ajustes: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVLinearPCMIsFloatKey: true,
        AVLinearPCMBitDepthKey: 32,
        AVLinearPCMIsNonInterleaved: false,
    ]
    let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: ajustes)
    guard lector.canAdd(salida) else { throw ErrorDeSonoridadMedia.sinAudio }
    lector.add(salida)
    guard lector.startReading() else { throw ErrorDeSonoridadMedia.lecturaFallida("lector") }

    var medidor: MedidorDeSonoridad?
    while let buffer = salida.copyNextSampleBuffer() {
        guard let descripcion = CMSampleBufferGetFormatDescription(buffer),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(descripcion),
              let bloque = CMSampleBufferGetDataBuffer(buffer) else { continue }
        let marcos = CMSampleBufferGetNumSamples(buffer)
        let canales = Int(asbd.pointee.mChannelsPerFrame)
        let frecuencia = asbd.pointee.mSampleRate
        guard marcos > 0, canales > 0, frecuencia > 0 else { continue }
        if medidor == nil { medidor = MedidorDeSonoridad(frecuencia: frecuencia, canales: canales) }
        let bytes = CMBlockBufferGetDataLength(bloque)
        guard bytes >= marcos * canales * MemoryLayout<Float>.size else { continue }
        var muestras = [Float](repeating: 0, count: marcos * canales)
        let estado = muestras.withUnsafeMutableBytes {
            CMBlockBufferCopyDataBytes(bloque, atOffset: 0, dataLength: bytes, destination: $0.baseAddress!)
        }
        guard estado == kCMBlockBufferNoErr else { continue }
        medidor?.procesar(entrelazado: muestras)
    }
    lector.cancelReading()
    guard let medidor else { throw ErrorDeSonoridadMedia.sinAudio }
    return medidor.finalizar()
}

let ruta = CommandLine.arguments[1]
let medio = try await MedioResuelto.cargar(id: UUID(), url: URL(fileURLWithPath: ruta))
guard medio.tieneAudio else {
    print("FALLO  el archivo no tiene audio")
    exit(1)
}
print("Medio: \(medio.pistaDeAudio != nil ? "con audio" : "sin audio") · \(medio.fps) fps · \(medio.duracion.seconds) s")

// Un minuto basta para la prueba; lo que se compara es el mismo tramo por los
// dos caminos, no la duración completa.
let segundos = min(60, medio.duracion.seconds)
print("Tramo medido: \(formato(segundos)) s")
print("")

print("— lectura directa del archivo —")
let fuente = try medirFuente(medio, segundos: segundos)
print("  integrada \(formato(fuente.integrada)) LUFS · LRA \(formato(fuente.rango)) LU · pico real \(formato(fuente.picoReal)) dBTP")

print("— composición con mezcla, clip a 0 dB —")
let plano = try montarYMedir(medio, ganancia: 0, segundos: segundos)
print("  integrada \(formato(plano.integrada)) LUFS · LRA \(formato(plano.rango)) LU · pico real \(formato(plano.picoReal)) dBTP")
comprobar(abs(plano.integrada - fuente.integrada) < 0.3,
    "el montaje a 0 dB mide como la fuente (± 0,3)")
comprobar(abs(plano.picoReal - fuente.picoReal) < 0.3,
    "y su pico real también (± 0,3)")

print("— composición con mezcla, clip a −6,02 dB —")
let atenuado = try montarYMedir(medio, ganancia: -6.02, segundos: segundos)
print("  integrada \(formato(atenuado.integrada)) LUFS · pico real \(formato(atenuado.picoReal)) dBTP")
comprobar(abs(atenuado.integrada - (fuente.integrada - 6.02)) < 0.3,
    "bajar 6,02 dB en la mezcla baja la medida 6,02 (± 0,3)")
comprobar(abs(atenuado.picoReal - (fuente.picoReal - 6.02)) < 0.3,
    "y el pico real baja los mismos 6,02 (± 0,3)")

print("— silencio previo a un clip desplazado —")
// Si el renderizador arrancase la envolvente a pleno nivel antes del clip, la
// medida del montaje con el clip en 5 s subiría varios LUFS por el silencio
// sonando. La integrada tiene que quedar donde la del clip desde cero.
let desplazado = try montarYMedirConDesplazamiento(medio, segundos: 10, en: 5)
print("  clip de 10 s a partir de 5 s → \(formato(desplazado.integrada)) LUFS")
comprobar(abs(desplazado.integrada - fuente.integrada) < 0.3,
    "el silencio previo no suena (± 0,3)")

print("— el lector predice la exportación —")
// La normalización al exportar decide con el lector; si el lector y la
// exportación no midieran lo mismo, la decisión sería sobre otra señal. Se
// exporta el montaje con voz desplazada, fundidos y ducking, y se compara.
let (composicion, mezcla) = try montajeComplejo(medio, segundos: segundos, conDucking: true)
let medidaComposicion = try SonoridadMedia.medir(MontajeRenderizable(
    composicion: composicion, composicionDeVideo: nil, mezclaDeAudio: mezcla,
    tamano: .zero, avisos: []
))
let exportado = FileManager.default.temporaryDirectory
    .appendingPathComponent("editorcito-sonoridad-\(UUID().uuidString).m4a")
defer { try? FileManager.default.removeItem(at: exportado) }
guard let sesion = AVAssetExportSession(asset: composicion, presetName: AVAssetExportPresetAppleM4A) else {
    print("  FALLO  sin sesión de exportación"); fallos += 1
    exit(1)
}
sesion.outputURL = exportado
sesion.outputFileType = .m4a
sesion.audioMix = mezcla
await sesion.export()
guard sesion.status == .completed else {
    print("  FALLO  exportación: \(String(describing: sesion.error))"); fallos += 1
    exit(1)
}
let medidaExportada = try await medirArchivo(exportado)
print("  lector: \(formato(medidaComposicion.integrada)) LUFS · exportado: \(formato(medidaExportada.integrada)) LUFS (AAC)")
comprobar(abs(medidaExportada.integrada - medidaComposicion.integrada) < 0.5,
    "lo que mide el lector es lo que entrega la exportación (± 0,5)")

print("— el ducking se oye —")
let (sinDucking, mezclaSinDucking) = try montajeComplejo(medio, segundos: segundos, conDucking: false)
let medidaSinDucking = try SonoridadMedia.medir(MontajeRenderizable(
    composicion: sinDucking, composicionDeVideo: nil, mezclaDeAudio: mezclaSinDucking,
    tamano: .zero, avisos: []
))
print("  sin ducking: \(formato(medidaSinDucking.integrada)) LUFS · con ducking: \(formato(medidaComposicion.integrada)) LUFS")
comprobar(medidaComposicion.integrada < medidaSinDucking.integrada - 0.5,
    "el ducking baja la medida de la música de fondo")

print("— la ganancia de máster se aplica por pista —")
// La normalización al exportar dobla la ganancia en el volumen de las pistas
// de audio de una copia del montaje; tiene que medir lo mismo que si se la
// diera al clip.
func montajeConGananciaPorPista(_ medio: MedioResuelto, gananciaDePista: Double, segundos: Double) -> (AVMutableComposition, AVMutableAudioMix?) {
    let base = Timebase.habituales.min { abs($0.fps - medio.fps) < abs($1.fps - medio.fps) } ?? .p25
    var linea = LineaDeTiempo.nueva(timebase: base)
    let a1 = linea.pistas.first { $0.nombre == "A1" }!.id
    let clip = Clip(
        mediaID: medio.id, nombre: "prueba", inicio: 0,
        duracion: base.frames(segundos: segundos), entradaEnOrigen: 0
    )
    linea.sobrescribir(clip, enPista: a1, en: 0)
    if let indice = linea.indiceDePista(a1) {
        linea.pistas[indice].volumen = gananciaDePista
    }
    let render = ConstructorDeMontaje.construir(linea, medios: [medio.id: medio])
    return (render.composicion, render.mezclaDeAudio)
}
func montarConVolumen(_ medio: MedioResuelto, gananciaDePista: Double, segundos: Double) throws -> MedidaDeSonoridad {
    let (composicion, mezcla) = montajeConGananciaPorPista(medio, gananciaDePista: gananciaDePista, segundos: segundos)
    return try SonoridadMedia.medir(MontajeRenderizable(
        composicion: composicion, composicionDeVideo: nil, mezclaDeAudio: mezcla,
        tamano: .zero, avisos: []
    ))
}
let porPista = try montarConVolumen(medio, gananciaDePista: 3, segundos: segundos)
let porClip = try montarYMedir(medio, ganancia: 3, segundos: segundos)
print("  volumen de pista: \(formato(porPista.integrada)) LUFS · ganancia de clip: \(formato(porClip.integrada)) LUFS")
comprobar(abs(porPista.integrada - porClip.integrada) < 0.05,
    "volumen de pista y ganancia de clip miden igual")
comprobar(abs(porPista.integrada - (fuente.integrada + 3)) < 0.3,
    "y suben los +3 dB pedidos")

print("— ciclo completo de normalización —")
// El camino que usa la app al exportar: medir la composición, calcular el plan
// honesto, doblar la ganancia en las pistas de una copia del montaje y
// exportar; el archivo final tiene que acabar midiendo el objetivo.
let objetivo = ObjetivoDeSonoridad.youtube
let medidaInicial = try montarYMedir(medio, ganancia: 0, segundos: segundos)
guard let plan = objetivo.plan(para: medidaInicial) else {
    print("  FALLO  no hay plan para esta medida"); fallos += 1
    exit(1)
}
print("  medida \(formato(medidaInicial.integrada)) LUFS · plan: \(plan.resumen)")
let (composicionNormalizada, mezclaNormalizada) = montajeConGananciaPorPista(medio, gananciaDePista: plan.ganancia, segundos: segundos)
let exportadoNormalizado = FileManager.default.temporaryDirectory
    .appendingPathComponent("editorcito-normalizado-\(UUID().uuidString).m4a")
defer { try? FileManager.default.removeItem(at: exportadoNormalizado) }
guard let sesion2 = AVAssetExportSession(asset: composicionNormalizada, presetName: AVAssetExportPresetAppleM4A) else {
    print("  FALLO  sin sesión de exportación"); fallos += 1
    exit(1)
}
sesion2.outputURL = exportadoNormalizado
sesion2.outputFileType = .m4a
sesion2.audioMix = mezclaNormalizada
await sesion2.export()
guard sesion2.status == .completed else {
    print("  FALLO  exportación: \(String(describing: sesion2.error))"); fallos += 1
    exit(1)
}
let medidaFinal = try await medirArchivo(exportadoNormalizado)
print("  exportado normalizado: \(formato(medidaFinal.integrada)) LUFS (objetivo \(formato(objetivo.objetivo ?? 0)))")
comprobar(abs(medidaFinal.integrada - (objetivo.objetivo ?? 0)) < 0.5,
    "el exportado llega al objetivo de sonoridad (± 0,5)")
comprobar(medidaFinal.picoReal <= objetivo.techoDePico + 0.1,
    "y no pasa el techo de pico")

print("")
print(fallos == 0 ? "PIPELINE DE SONORIDAD CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
