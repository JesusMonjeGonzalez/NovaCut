import AppKit
import AudioToolbox
import AVFoundation
import CoreGraphics
import CoreImage
import CoreImage.CIFilterBuiltins
import Foundation
import QuartzCore

// Constantes de parámetros de Core Image que el módulo no reexporta.
private let kCIInputNeutral = "inputNeutral"
private let kCIInputTargetNeutral = "inputTargetNeutral"

/// Un medio con todo lo que hace falta saber de él ya resuelto.
///
/// Se prepara una sola vez al importar porque en AVFoundation moderno leer una
/// pista es asíncrono, y montar la composición tiene que ser síncrono para poder
/// hacerse en cada cambio del montaje sin parpadeos ni condiciones de carrera.
struct MedioResuelto {
    let id: UUID
    let url: URL
    /// El asset y sus pistas se guardan resueltos. Rehacer un `AVURLAsset` por cada
    /// clip y en cada reconstrucción del montaje es lo que hace que un timeline de
    /// doscientos cortes tarde segundos en responder a un simple arrastre.
    let asset: AVURLAsset
    let pistaDeVideo: AVAssetTrack?
    let pistaDeAudio: AVAssetTrack?
    let duracion: CMTime
    let tamanoNatural: CGSize
    /// Matriz que la cámara dejó grabada para orientar la imagen. Ignorarla es la
    /// causa número uno de vídeos de móvil que salen tumbados en la exportación.
    let transformacionPreferida: CGAffineTransform
    let fps: Double
    let esVFR: Bool

    init(
        id: UUID,
        url: URL,
        asset: AVURLAsset,
        pistaDeVideo: AVAssetTrack?,
        pistaDeAudio: AVAssetTrack?,
        duracion: CMTime,
        tamanoNatural: CGSize,
        transformacionPreferida: CGAffineTransform,
        fps: Double,
        esVFR: Bool = false
    ) {
        self.id = id
        self.url = url
        self.asset = asset
        self.pistaDeVideo = pistaDeVideo
        self.pistaDeAudio = pistaDeAudio
        self.duracion = duracion
        self.tamanoNatural = tamanoNatural
        self.transformacionPreferida = transformacionPreferida
        self.fps = fps
        self.esVFR = esVFR
    }

    var tieneVideo: Bool { pistaDeVideo != nil }
    var tieneAudio: Bool { pistaDeAudio != nil }

    /// Tamaño ya orientado, que es el que ve el espectador.
    var tamanoVisible: CGSize {
        let t = tamanoNatural.applying(transformacionPreferida)
        return CGSize(width: abs(t.width), height: abs(t.height))
    }

    static func cargar(id: UUID, url: URL) async throws -> MedioResuelto {
        let asset = AVURLAsset(url: url, options: [AVURLAssetPreferPreciseDurationAndTimingKey: true])
        let duracion = try await asset.load(.duration)
        let video = try await asset.loadTracks(withMediaType: .video).first
        let audio = try await asset.loadTracks(withMediaType: .audio).first

        var tamano = CGSize.zero
        var transformacion = CGAffineTransform.identity
        var fps = 0.0
        var esVFR = false
        if let video {
            tamano = try await video.load(.naturalSize)
            transformacion = try await video.load(.preferredTransform)
            fps = Double(try await video.load(.nominalFrameRate))
            esVFR = await Self.esCadenciaVFR(pista: video)
        }
        return MedioResuelto(
            id: id, url: url, asset: asset,
            pistaDeVideo: video, pistaDeAudio: audio,
            duracion: duracion, tamanoNatural: tamano,
            transformacionPreferida: transformacion, fps: fps, esVFR: esVFR
        )
    }

    /// Detección VFR por reloj de PTS, no por metadatos. El truco clásico de
    /// comparar la duración mínima de frame contra la nominal no ve las
    /// grabaciones con caídas de frames (Screen Recording de macOS graba a 60
    /// y suelta fotogramas bajo carga: la duración mínima sigue siendo 1/60 y
    /// nadie nota nada). Aquí se leen los tiempos de presentación de una
    /// ventana inicial —solo demultiplexa, no decodifica— y se cuenta cuántos
    /// saltos superan 1,5× la cadencia mediana: más de un 2 % es material que
    /// un conformado ingenuo desfasaría.
    static func esCadenciaVFR(pista: AVAssetTrack) async -> Bool {
        let minimo = (try? await pista.load(.minFrameDuration)) ?? .invalid
        if !minimo.isNumeric || minimo.value <= 0 { return true }
        guard let asset = pista.asset, let lector = try? AVAssetReader(asset: asset) else { return false }
        let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: nil)
        lector.add(salida)
        guard lector.startReading() else { return false }

        var pts = [Double]()
        let tope = 400
        while lector.status == .reading, let sb = salida.copyNextSampleBuffer(), pts.count < tope {
            let t = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sb))
            if t.isFinite { pts.append(t) }
        }
        lector.cancelReading()
        guard pts.count > 30 else { return false }

        pts.sort()
        var unicos = [Double]()
        for t in pts where unicos.last.map({ t - $0 > 1e-6 }) ?? true { unicos.append(t) }
        guard unicos.count > 10 else { return false }

        var deltas = [Double]()
        for i in 1..<unicos.count { deltas.append(unicos[i] - unicos[i - 1]) }
        let ordenados = deltas.sorted()
        let mediana = ordenados[ordenados.count / 2]
        guard mediana > 0 else { return false }
        var huecos = 0
        for d in deltas where d > mediana * 1.5 { huecos += 1 }
        return Double(huecos) / Double(deltas.count) > 0.02
    }

    /// Captura una imagen pequeña para identificar el medio sin decodificarlo en
    /// cada repintado de la biblioteca o del timeline.
    static func miniatura(_ medio: MedioResuelto) async -> CGImage? {
        guard medio.tieneVideo else { return nil }
        let generador = AVAssetImageGenerator(asset: medio.asset)
        generador.appliesPreferredTrackTransform = true
        generador.maximumSize = CGSize(width: 320, height: 180)
        let segundo = max(0, min(medio.duracion.seconds / 2, medio.duracion.seconds - 0.001))
        guard segundo.isFinite else { return nil }
        return try? await generador.image(
            at: CMTime(seconds: segundo, preferredTimescale: 600)
        ).image
    }

    /// Reduce el audio a picos PCM mono para dibujar una forma de onda ligera.
    /// Lee a 8 kHz y por bloques, por lo que no carga el archivo entero en memoria.
    static func formaDeOnda(url: URL, puntos: Int = 160) async -> [Float]? {
        guard puntos > 0 else { return nil }
        let asset = AVURLAsset(url: url)
        guard let pista = (try? await asset.loadTracks(withMediaType: .audio))?.first,
              let lector = try? AVAssetReader(asset: asset) else { return nil }

        let frecuencia = 8_000.0
        guard let duracion = (try? await asset.load(.duration))?.seconds else { return nil }
        guard duracion.isFinite, duracion > 0 else { return nil }

        let ajustes: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsNonInterleaved: false,
            AVNumberOfChannelsKey: 1,
            AVSampleRateKey: frecuencia,
        ]
        let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: ajustes)
        guard lector.canAdd(salida) else { return nil }
        lector.add(salida)
        guard lector.startReading() else { return nil }

        let muestrasPorPunto = max(1, Int((duracion * frecuencia / Double(puntos)).rounded(.up)))
        var picos = [Float](repeating: 0, count: puntos)
        var indiceDeMuestra = 0

        while let buffer = salida.copyNextSampleBuffer(),
              let bloque = CMSampleBufferGetDataBuffer(buffer) {
            let bytes = CMBlockBufferGetDataLength(bloque)
            guard bytes >= 2 else { continue }
            var datos = [UInt8](repeating: 0, count: bytes)
            let estado = datos.withUnsafeMutableBytes { destino in
                CMBlockBufferCopyDataBytes(
                    bloque,
                    atOffset: 0,
                    dataLength: bytes,
                    destination: destino.baseAddress!
                )
            }
            guard estado == kCMBlockBufferNoErr else { continue }

            for offset in stride(from: 0, to: bytes - 1, by: 2) {
                let bits = UInt16(datos[offset]) | (UInt16(datos[offset + 1]) << 8)
                let muestra = abs(Float(Int16(bitPattern: bits))) / 32_768
                let punto = min(puntos - 1, indiceDeMuestra / muestrasPorPunto)
                picos[punto] = max(picos[punto], muestra)
                indiceDeMuestra += 1
            }
        }

        return picos.contains(where: { $0 > 0 }) ? picos : nil
    }
}

private enum CompositorError: LocalizedError {
    case sinInstruccion
    case sinFrame
    case sinBuffer

    var errorDescription: String? {
        switch self {
        case .sinInstruccion: "El compositor recibió una instrucción que no puede leer."
        case .sinFrame: "El compositor no recibió el frame de origen."
        case .sinBuffer: "El compositor no pudo obtener un buffer de destino."
        }
    }
}

/// Captura un frame del montaje tal y como se ve en el monitor.
///
/// El reproductor instala siempre un `AVPlayerItem` sobre la composición del
/// render, así que el `asset` del item nunca es un `AVURLAsset`: la única vía
/// de obtener el frame real —con su color, encuadre y capas aplicados— es
/// regenerarlo desde la propia composición con su `videoComposition`, que
/// activa el compositor custom igual que en la reproducción. El generador no
/// renderiza el `animationTool` de los subtítulos; el resto del montaje sí.
enum CapturadorDeFrames {
    static func capturarFrame(
        de composicion: AVComposition,
        videoComposition: AVMutableVideoComposition?,
        en segundo: Double,
        maximo: CGSize? = nil
    ) async -> CGImage? {
        guard segundo.isFinite, segundo >= 0 else { return nil }
        let generador = AVAssetImageGenerator(asset: composicion)
        generador.videoComposition = videoComposition
        generador.appliesPreferredTrackTransform = true
        if let maximo { generador.maximumSize = maximo }
        // Tolerancia cero: el frame que se pide es el que se ve en el cabezal.
        // La tolerancia por defecto (~1 s) devolvería otro fotograma y la forma
        // de onda mentiría sobre lo que está en pantalla.
        generador.requestedTimeToleranceBefore = .zero
        generador.requestedTimeToleranceAfter = .zero
        return try? await generador.image(
            at: CMTime(seconds: segundo, preferredTimescale: 600)
        ).image
    }
}

/// La instrucción de composición con el color y la LUT que hay que aplicar al tramo.
///
/// `AVMutableVideoCompositionInstruction` no lleva ni color ni LUT; el compositor
/// custom necesita saber qué corrección aplica cada tramo, y esa información viaja
/// en esta subclase. Sin color ni LUT, se comporta exactamente como la instrucción
/// normal.
final class InstruccionConColor: AVMutableVideoCompositionInstruction {
    /// Corrección del clip que manda en el tramo, o `nil` para neutro.
    var colorDeClip: ColorDeClip?
    /// Ruta al archivo `.cube` del clip que manda en el tramo, o `nil`.
    var lutDeClip: String?
    /// Efectos por capa, indexados por el trackID de su pista de composición:
    /// máscara, modo de fusión, viñeta y desenfoque de cada clip del tramo.
    var efectosPorCapa: [Int32: EfectosDeCapa] = [:]
}

/// Efectos de una capa dentro de un tramo de instrucción.
struct EfectosDeCapa {
    var modoDeFusion: ModoDeFusion = .normal
    var mascara: MascaraDeClip? = nil
    var croma: ChromaKeyDeClip? = nil
    var vignette: Double = 0
    var radioDeVignette: Double = 0.75
    var desenfoque: Double = 0

    var tieneEfectos: Bool {
        modoDeFusion != .normal || mascara?.activa == true || croma?.esNeutro == false
            || vignette > 0.001 || desenfoque > 0.001
    }
}

/// Componedor de capas y corrección de color por clip con `CIFilter`.
///
/// Es la única vía que aplica el color **igual en reproducción y exportación**:
/// el `customVideoCompositorClass` lo usa AVPlayer en directo y AVAssetExportSession
/// al exportar, con el mismo código. Sin esto, el color del monitor y el del
/// archivo divergirían —el fallo de los fundidos otra vez—.
///
/// Con un compositor custom activo, AVFoundation **no** compone las capas: le
/// entrega los frames de origen en bruto y el compositor debe hacerlo todo. El
/// compositor anterior tomaba solo la primera pista —cualquier clip con color
/// en el montaje hacía desaparecer las capas inferiores de todos los segmentos
/// (verificado con un experimento de dos vídeos superpuestos)—. Este compone de
/// abajo arriba con la transformación, el recorte y la opacidad de cada
/// `layerInstruction`, evaluadas en el tiempo de la petición, y aplica la
/// cadena de color sobre el compuesto.
final class CompositorDeColor: NSObject, AVVideoCompositing {

    // El contexto trabaja sin gestión de color: AVFoundation entrega BGRA en
    // bruto y el compositor nativo compone en bruto. Si Core Image convierte al
    // espacio de trabajo, los colores saturados salen con un tinte (verificado
    // comparando contra el compositor nativo) y el monitor mentiría.
    private let contexto = CIContext(options: [
        .cacheIntermediates: false,
        .workingColorSpace: NSNull(),
        .outputColorSpace: NSNull(),
    ])

    // MARK: AVVideoCompositing

    var sourcePixelBufferAttributes: [String: Any]? {
        [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
    }

    var requiredPixelBufferAttributesForRenderContext: [String: Any] {
        [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
    }

    func renderContextChanged(_ newRenderContext: AVVideoCompositionRenderContext) {}

    func startRequest(_ request: AVAsynchronousVideoCompositionRequest) {
        guard let instruccion = request.videoCompositionInstruction as? AVMutableVideoCompositionInstruction else {
            // Una instrucción que no podemos leer: se entrega el primer frame o
            // se falla con error — nunca en silencio.
            if let trackID = request.sourceTrackIDs.first?.int32Value,
               let frame = request.sourceFrame(byTrackID: trackID) {
                request.finish(withComposedVideoFrame: frame)
            } else {
                request.finish(with: CompositorError.sinInstruccion)
            }
            return
        }
        // El color viaja en la subclase de la casa; una instrucción plana se
        // compone igual, solo sin cadena de color.
        let color = (instruccion as? InstruccionConColor)?.colorDeClip
        let lut = (instruccion as? InstruccionConColor)?.lutDeClip
        let efectosPorCapa = (instruccion as? InstruccionConColor)?.efectosPorCapa ?? [:]

        // El contrato de `AVVideoCompositing` exige terminar con `finish(...)` en
        // **cada** petición: un solo `return` sin finish deja la exportación
        // esperando un frame para siempre, sin error.
        let tiempo = request.compositionTime
        let inicioDeLaInstruccion = instruccion.timeRange.start

        // Atajo barato: una sola capa sin efectos es el propio frame de origen,
        // y el render con Core Image no aporta nada.
        if instruccion.layerInstructions.count == 1,
           let unica = instruccion.layerInstructions.first,
           unica.trackID == request.sourceTrackIDs.first?.int32Value,
           Self.transformacionDe(unica, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion).isIdentity,
           Self.opacidadDe(unica, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion) == 1,
           Self.recorteDe(unica, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion).isNull,
           color?.esNeutro != false,
           lut == nil,
           !(efectosPorCapa[unica.trackID]?.tieneEfectos ?? false) {
            if let frame = request.sourceFrame(byTrackID: unica.trackID) {
                request.finish(withComposedVideoFrame: frame)
                return
            }
        }

        // Fondo de la instrucción, como hace el compositor nativo.
        let fondo: CIImage = {
            let cg = instruccion.backgroundColor ?? CGColor(red: 0, green: 0, blue: 0, alpha: 1)
            return CIImage(color: CIColor(cgColor: cg))
                .cropped(to: CGRect(origin: .zero, size: request.renderContext.size))
        }()

        // Las capas van de arriba abajo en la lista de la instrucción; se
        // componen de abajo arriba.
        var compuesta = fondo
        let tamanoDeRender = request.renderContext.size
        for capa in instruccion.layerInstructions.reversed() {
            guard request.sourceTrackIDs.contains(where: { $0.int32Value == capa.trackID }),
                  let frame = request.sourceFrame(byTrackID: capa.trackID) else { continue }
            var imagen = CIImage(cvPixelBuffer: frame)

            let recorte = Self.recorteDe(capa, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion)
            if !recorte.isNull, !recorte.isEmpty {
                imagen = imagen.cropped(to: recorte)
            }
            let transformacion = Self.transformacionDe(capa, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion)
            if !transformacion.isIdentity {
                imagen = imagen.transformed(by: transformacion)
            }

            // Efectos de la capa: desenfoque, viñeta y máscara se aplican antes
            // de mezclar; el modo de fusión decide cómo se compone con las de
            // debajo en vez del «encima» de siempre.
            let efectos = efectosPorCapa[capa.trackID] ?? EfectosDeCapa()
            if efectos.desenfoque > 0.001 {
                let radio = efectos.desenfoque * min(tamanoDeRender.width, tamanoDeRender.height)
                imagen = imagen.applyingFilter("CIGaussianBlur", parameters: [kCIInputRadiusKey: radio])
            }
            if efectos.vignette > 0.001 {
                let radio = efectos.radioDeVignette * min(tamanoDeRender.width, tamanoDeRender.height) / 2
                imagen = imagen.applyingFilter("CIVignette", parameters: [
                    kCIInputRadiusKey: max(0.5, radio),
                    kCIInputIntensityKey: efectos.vignette,
                ])
            }
            if let mascara = efectos.mascara, mascara.activa {
                imagen = Self.aplicar(mascara, a: imagen, tamano: tamanoDeRender)
            }
            if let croma = efectos.croma, !croma.esNeutro {
                imagen = Self.aplicar(croma, a: imagen)
            }

            let opacidad = Self.opacidadDe(capa, en: tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion)
            if opacidad < 1 {
                // El compositor nativo mezcla el frame opaco con el peso de su
                // opacidad: out = frame·α + fondo·(1−α). Los frames llegan
                // opacos (alfa 255, verificado), así que basta escalar el alfa
                // y componer: `composited(over:)` hace justo esa matemática.
                // El round-trip de premultiplicación doblaba el fundido.
                imagen = imagen.applyingFilter("CIColorMatrix", parameters: [
                    "inputAVector": CIVector(x: 0, y: 0, z: 0, w: CGFloat(opacidad)),
                ])
            }

            if let filtro = efectos.modoDeFusion.filtroCI {
                // El modo de fusión mezcla con lo ya compuesto en vez de
                // ponerse encima. La opacidad ya viaja en el alfa del frame.
                compuesta = imagen.applyingFilter(filtro, parameters: [
                    kCIInputBackgroundImageKey: compuesta,
                ])
            } else {
                compuesta = imagen.composited(over: compuesta)
            }
        }

        if let color, !color.esNeutro {
            compuesta = Self.aplicar(color, a: compuesta)
        }
        if let rutaDeLut = lut {
            compuesta = Self.aplicar(lut: rutaDeLut, a: compuesta)
        }

        guard let buffer = request.renderContext.newPixelBuffer() else {
            request.finish(with: CompositorError.sinBuffer)
            return
        }
        contexto.render(compuesta, to: buffer)
        request.finish(withComposedVideoFrame: buffer)
    }

    // MARK: Evaluación de rampas

    /// Interpola la rampa de una propiedad en el tiempo pedido.
    ///
    /// El compositor nativo interpola linealmente entre los extremos de la rampa
    /// que contiene al instante; aquí se replica esa matemática. Dos detalles:
    /// una rampa de duración cero (`setTransform(at:)` del constructor, que fija
    /// un valor constante) se devuelve tal cual, y si el getter no la encuentra
    /// en el tiempo pedido se consulta el arranque de la instrucción —que es
    /// donde el constructor las fija.
    private static func valorDeRampa<T>(
        _ tiempo: CMTime,
        inicioDeLaInstruccion: CMTime,
        _ consultar: (CMTime, UnsafeMutablePointer<T>?, UnsafeMutablePointer<T>?, UnsafeMutablePointer<CMTimeRange>?) -> Bool,
        porDefecto: T,
        interpolar: (T, T, Double) -> T
    ) -> T {
        func en(_ t: CMTime) -> T? {
            var inicio = porDefecto
            var fin = porDefecto
            var rango = CMTimeRange.zero
            guard consultar(t, &inicio, &fin, &rango) else { return nil }
            if rango.duration.seconds <= 0 { return inicio }
            let s = min(max((t.seconds - rango.start.seconds) / rango.duration.seconds, 0), 1)
            return interpolar(inicio, fin, s)
        }
        return en(tiempo) ?? en(inicioDeLaInstruccion) ?? porDefecto
    }

    private static func transformacionDe(_ capa: AVVideoCompositionLayerInstruction, en tiempo: CMTime, inicioDeLaInstruccion: CMTime) -> CGAffineTransform {
        valorDeRampa(tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion, capa.getTransformRamp, porDefecto: .identity) { a, b, t in
            CGAffineTransform(
                a: a.a + (b.a - a.a) * t,
                b: a.b + (b.b - a.b) * t,
                c: a.c + (b.c - a.c) * t,
                d: a.d + (b.d - a.d) * t,
                tx: a.tx + (b.tx - a.tx) * t,
                ty: a.ty + (b.ty - a.ty) * t
            )
        }
    }

    private static func opacidadDe(_ capa: AVVideoCompositionLayerInstruction, en tiempo: CMTime, inicioDeLaInstruccion: CMTime) -> Float {
        valorDeRampa(tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion, capa.getOpacityRamp, porDefecto: 1) { a, b, t in
            a + (b - a) * Float(t)
        }
    }

    private static func recorteDe(_ capa: AVVideoCompositionLayerInstruction, en tiempo: CMTime, inicioDeLaInstruccion: CMTime) -> CGRect {
        valorDeRampa(tiempo, inicioDeLaInstruccion: inicioDeLaInstruccion, capa.getCropRectangleRamp, porDefecto: .null) { a, b, t in
            CGRect(
                x: a.minX + (b.minX - a.minX) * t,
                y: a.minY + (b.minY - a.minY) * t,
                width: a.width + (b.width - a.width) * t,
                height: a.height + (b.height - a.height) * t
            )
        }
    }

    /// La corrección de color encadenada, en orden fijo.
    ///
    /// El orden importa y se declara: exposición → temperatura → contraste y
    /// saturación → altas y sombras. Cambiar el orden cambia el resultado, y un
    /// editor que lo aplique en otro orden daría un color distinto al del
    /// monitor —la regla de que reproducción y exportación tienen que coincidir—.
    static func aplicar(_ color: ColorDeClip, a imagen: CIImage) -> CIImage {
        // Exposición en EV (no lineal): ±3 EV con el deslizador a ±100.
        let controles = imagen.applyingFilter("CIColorControls", parameters: [
            kCIInputBrightnessKey: Float(color.exposicion / 100 * 3) * 0.5,
            kCIInputContrastKey: Float(color.contraste / 50 + 1),
            kCIInputSaturationKey: Float(color.saturacion / 100 + 1),
        ])
        // Temperatura en kelvin alrededor del blanco de 6500 K.
        let conTemperatura = controles.applyingFilter("CITemperatureAndTint", parameters: [
            kCIInputNeutral: CIVector(x: 6500 + color.temperatura * 30, y: 0),
            kCIInputTargetNeutral: CIVector(x: 6500, y: 0),
        ])
        // Altas y sombras: una curva de 5 puntos interpolada a una tabla RGB.
        // `CIColorCurves` espera la tabla en `inputCurvesData`, no en puntos.
        let alto = Float(color.altas / 100)
        let bajo = Float(color.sombras / 100)
        let puntos: [(Float, Float)] = [
            (0, 0 + bajo * 0.35),
            (0.25, 0.25 + bajo * 0.3),
            (0.5, 0.5),
            (0.75, 0.75 + alto * 0.3),
            (1, 1 + alto * 0.35),
        ]
        let tabla = curvaDeTabla(puntos)
        let conCurvas = conTemperatura.applyingFilter("CIColorCurves", parameters: [
            "inputCurvesData": tabla as NSData,
            "inputColorSpace": CGColorSpace(name: CGColorSpace.sRGB) as Any,
        ])

        // Curvas RGB propias del clip, si las hay. La luminancia se aplica a
        // los tres canales y luego cada canal su curva —el orden del nodo de
        // curvas de Resolve—. La tabla ya viene con ese orden de `CurvasDeClip`.
        let conCurvasPropias: CIImage
        if let curvas = color.curvas, !curvas.esIdentidad {
            conCurvasPropias = conCurvas.applyingFilter("CIColorCurves", parameters: [
                "inputCurvesData": curvas.tabla as NSData,
                "inputColorSpace": CGColorSpace(name: CGColorSpace.sRGB) as Any,
            ])
        } else {
            conCurvasPropias = conCurvas
        }

        // Ruedas de color: sombras, medios y altas por canal. Cada rueda se
        // convierte en una curva de tres puntos sobre el canal; las tres curvas
        // (R, G, B) se combinan en la tabla RGB y se aplican como las curvas.
        guard let ruedas = color.ruedas, !ruedas.esNeutro else { return conCurvasPropias }
        let tablaDeRuedas = tablaDeRuedas(ruedas)
        return conCurvasPropias.applyingFilter("CIColorCurves", parameters: [
            "inputCurvesData": tablaDeRuedas as NSData,
            "inputColorSpace": CGColorSpace(name: CGColorSpace.sRGB) as Any,
        ])
    }

    /// La tabla RGB de las ruedas de color: cada canal con su curva de tres
    /// puntos (sombras, medios, altas) desplazada sobre la diagonal.
    static func tablaDeRuedas(_ ruedas: RuedasDeColor) -> Data {
        let curvas: [[(Double, Double)]] = [
            RuedasDeColor.curvaDe(ruedas.sombrasRojo, ruedas.mediosRojo, ruedas.altasRojo),
            RuedasDeColor.curvaDe(ruedas.sombrasVerde, ruedas.mediosVerde, ruedas.altasVerde),
            RuedasDeColor.curvaDe(ruedas.sombrasAzul, ruedas.mediosAzul, ruedas.altasAzul),
        ]
        var datos = Data(capacity: 256 * 3 * MemoryLayout<Float>.size)
        for i in 0..<256 {
            let x = Double(i) / 255
            for canal in 0..<3 {
                // La y de la curva es el desplazamiento: punto + desplazamiento.
                let y = min(max(x + ColorDeClip.interpolar(curvas[canal], en: x), 0), 1)
                let f = Float(y)
                datos.append(contentsOf: withUnsafeBytes(of: f) { Array($0) })
            }
        }
        return datos
    }

    /// Aplica una LUT `.cube` con `CIColorCube`.
    ///
    /// Va **después** de la cadena primaria —el orden es parte del resultado y
    /// se declara: corrección primaria primero, LUT encima—. Las LUTs se
    /// cachean por ruta; si el archivo no se puede leer o parsear, la imagen
    /// sale sin tocar (el constructor ya avisó de que la LUT no se encontró).
    static func aplicar(lut ruta: String, a imagen: CIImage) -> CIImage {
        guard let cubo = ParseadorDeCubes.cargar(ruta: ruta) else { return imagen }
        return imagen.applyingFilter("CIColorCube", parameters: [
            "inputCubeDimension": cubo.tamano,
            "inputCubeData": cubo.datos as NSData,
        ])
    }

    /// Recorta una imagen a la forma de la máscara, con la pluma suavizando el
    /// borde.
    ///
    /// La máscara se dibuja como un gradiente en el lienzo: blanco donde se ve,
    /// negro donde no, y una transición suave de ancho `pluma` en el borde.
    /// `CIBlendWithMask` pinta `inputImage` donde la máscara es blanca y el
    /// `backgroundImage` donde es negra: con el fondo transparente, el frame
    /// queda opaco dentro de la forma y transparente fuera, y la opacidad del
    /// clip sigue funcionando encima. Invertida, la máscara se invierte y se
    /// ve todo menos la forma.
    static func aplicar(_ mascara: MascaraDeClip, a imagen: CIImage, tamano: CGSize) -> CIImage {
        let ancho = tamano.width * mascara.tamanoX
        let alto = tamano.height * mascara.tamanoY
        let centro = CGPoint(x: tamano.width * mascara.posicionX,
                             y: tamano.height * (1 - mascara.posicionY))
        let rect = CGRect(x: centro.x - ancho / 2, y: centro.y - alto / 2,
                          width: ancho, height: alto)
        let pluma = mascara.pluma * min(ancho, alto) / 2

        let forma: CIImage
        switch mascara.forma {
        case .rectangulo:
            // Esquinas redondeadas con el radio de la pluma: a pluma cero es un
            // rectángulo de esquinas vivas, y al crecer la pluma las esquinas
            // se redondean y el borde se suaviza con el desenfoque.
            forma = CIImage(color: .black)
                .cropped(to: rect)
                .applyingFilter("CIRoundedRectangleGenerator", parameters: [
                    "inputExtent": CIVector(cgRect: rect),
                    "inputRadius": max(0, pluma),
                ])
                .applyingFilter("CIGaussianBlur", parameters: [kCIInputRadiusKey: pluma])
        case .elipse:
            forma = CIImage(color: .black)
                .cropped(to: rect)
                .applyingFilter("CIRadialGradient", parameters: [
                    "inputCenter": CIVector(x: centro.x, y: centro.y),
                    "inputRadius0": max(0, min(ancho, alto) / 2 - pluma),
                    "inputRadius1": min(ancho, alto) / 2,
                    "inputColor0": CIColor(red: 1, green: 1, blue: 1, alpha: 1),
                    "inputColor1": CIColor(red: 0, green: 0, blue: 0, alpha: 0),
                ])
        }

        let transparente = CIImage(color: .clear).cropped(to: CGRect(origin: .zero, size: tamano))
        if mascara.invertida {
            // La máscara invertida se construye con luminancia invertida:
            // `CIBlendWithMask` mezcla por la luminancia de la máscara, así que
            // la forma se compone sobre un fondo negro opaco (blanco dentro,
            // negro fuera) y luego se invierte el color: negro dentro, blanco
            // fuera —se ve todo menos la forma, con la pluma suavizando el
            // borde en el camino.
            let fondoNegro = CIImage(color: .black).cropped(to: CGRect(origin: .zero, size: tamano))
            let sobreNegro = forma.composited(over: fondoNegro)
            let invertida = sobreNegro.applyingFilter("CIColorInvert", parameters: [:])
            return imagen.applyingFilter("CIBlendWithMask", parameters: [
                kCIInputBackgroundImageKey: transparente,
                kCIInputMaskImageKey: invertida,
            ])
        }
        return imagen.applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: transparente,
            kCIInputMaskImageKey: forma,
        ])
    }

    /// Hace transparente el color de clave (pantalla verde o azul) de la capa.
    ///
    /// La señal de clave es la dominancia del canal de la pantalla sobre los
    /// otros dos: para una clave verde, `G − (R+B)/2`. Es la fórmula clásica
    /// del chroma key porque distingue la pantalla del gris (el gris da cero)
    /// y de los colores de piel (el rojo no domina el verde). Esa señal pasa
    /// por una rampa de tolerancia que decide el alfa, y el derrame del color
    /// de clave en los bordes del sujeto se suprime restando la clave
    /// proporcionalmente a lo transparente que quedó. El resultado es una
    /// imagen con alfa real: el sujeto se puede poner sobre cualquier fondo.
    static func aplicar(_ croma: ChromaKeyDeClip, a imagen: CIImage) -> CIImage {
        // Señal de clave: exceso del canal dominante de la pantalla sobre la
        // media de los otros dos. CIColorDistance no existe en macOS, y la
        // proyección simple sobre el color de clave confundía el gris con la
        // pantalla (el gris se iba con ella); la dominancia no.
        let verdeEsClave = croma.verde >= croma.rojo && croma.verde >= croma.azul
        let azulEsClave = !verdeEsClave && croma.azul >= croma.rojo
        let vector: CIVector
        if verdeEsClave {
            // salida = −0,5·R + 1·G − 0,5·B  (en el canal R, que es el que
            // luego se lleva al alfa).
            vector = CIVector(x: -0.5, y: 1, z: -0.5, w: 0)
        } else if azulEsClave {
            vector = CIVector(x: -0.5, y: -0.5, z: 1, w: 0)
        } else {
            vector = CIVector(x: 1, y: -0.5, z: -0.5, w: 0)
        }
        let senal = imagen.applyingFilter("CIColorMatrix", parameters: [
            "inputRVector": vector,
            "inputGVector": vector,
            "inputBVector": vector,
            "inputAVector": CIVector(x: 0, y: 0, z: 0, w: 1),
            "inputBiasVector": CIVector(x: 0, y: 0, z: 0, w: 0),
        ])

        // La tolerancia y el suavizado se convierten en una rampa de alfa:
        // señal alta (pantalla) → transparente; señal baja (sujeto) → opaco;
        // el suavizado es la transición entre los dos.
        let tolerancia = max(0.001, min(croma.tolerancia, 0.99))
        let inicio = tolerancia
        let fin = min(tolerancia + croma.suavizado * 0.4, 1.0)
        let conRampa = senal.applyingFilter("CIColorCurves", parameters: [
            "inputCurvesData": curvaDeTabla([
                (0, 1),
                (Float(inicio), 1),
                (Float(fin), 0),
                (1, 0),
            ]) as NSData,
            "inputColorSpace": CGColorSpace(name: CGColorSpace.sRGB) as Any,
        ])

        // El alfa sale de la luminancia de la rampa (se ve la imagen donde la
        // rampa es blanca).
        let transparente = CIImage(color: .clear).cropped(to: imagen.extent)
        let conAlfa = imagen.applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: transparente,
            kCIInputMaskImageKey: conRampa,
        ])

        // Suprimir derrame: restar el color de clave atenuado a los píxeles
        // que quedaron medio transparentes (el borde del sujeto). Se resta
        // con una matriz de color y se enmascara con el alfa invertido: solo
        // toca donde la pantalla aún se cuela.
        let derrame = croma.suprimirDerrame
        guard derrame > 0.001 else { return conAlfa }
        let conResta = conAlfa.applyingFilter("CIColorMatrix", parameters: [
            "inputRVector": CIVector(x: 1, y: 0, z: 0, w: 0),
            "inputGVector": CIVector(x: 0, y: 1, z: 0, w: 0),
            "inputBVector": CIVector(x: 0, y: 0, z: 1, w: 0),
            "inputAVector": CIVector(x: 0, y: 0, z: 0, w: 1),
            "inputBiasVector": CIVector(x: -CGFloat(croma.rojo * derrame), y: -CGFloat(croma.verde * derrame), z: -CGFloat(croma.azul * derrame), w: 0),
        ])
        let alfaInvertido = conRampa.applyingFilter("CIColorInvert", parameters: [:])
        return conResta.applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: conAlfa,
            kCIInputMaskImageKey: alfaInvertido,
        ])
    }

    /// Interpola los puntos de control de la curva a la tabla RGB de 256 entradas
    /// que espera `CIColorCurves` (256 × 3 floats, por canal).
    static func curvaDeTabla(_ puntos: [(Float, Float)]) -> Data {
        var datos = Data(capacity: 256 * 3 * MemoryLayout<Float>.size)
        for i in 0..<256 {
            let x = Float(i) / 255
            let y: Float
            if x <= puntos[0].0 {
                y = puntos[0].1
            } else if x >= puntos[puntos.count - 1].0 {
                y = puntos[puntos.count - 1].1
            } else {
                var indice = 1
                while indice < puntos.count - 1 && puntos[indice].0 < x { indice += 1 }
                let a = puntos[indice - 1]
                let b = puntos[indice]
                let t = (x - a.0) / max(b.0 - a.0, 1e-6)
                y = a.1 + (b.1 - a.1) * t
            }
            let v = min(max(y, 0), 1)
            datos.append(contentsOf: withUnsafeBytes(of: v) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: v) { Array($0) })
            datos.append(contentsOf: withUnsafeBytes(of: v) { Array($0) })
        }
        return datos
    }
}

/// Un aviso del constructor del montaje, con su gravedad.
///
/// `critico` significa que el resultado que se renderiza difiere del montaje que
/// pide el usuario (un clip que no entra, un retime sin soporte); los de ajuste
/// solo recortan metraje al final de un archivo. La exportación debe preguntar
/// cuando hay críticos: exportar un montaje con un medio offline produce huecos
/// que nadie anunció.
struct AvisoDeMontaje: Equatable {
    let mensaje: String
    let critico: Bool

    init(_ mensaje: String, critico: Bool) {
        self.mensaje = mensaje
        self.critico = critico
    }
}

/// Todo lo que hay que entregarle al reproductor o al exportador.
struct MontajeRenderizable {
    let composicion: AVMutableComposition
    let composicionDeVideo: AVMutableVideoComposition?
    let mezclaDeAudio: AVMutableAudioMix?
    let tamano: CGSize
    let avisos: [AvisoDeMontaje]
    /// Firma estructural del montaje con el que se construyó: si la siguiente
    /// reconstrucción trae la misma firma, las pistas de la composición son
    /// reutilizables y solo se rehacen las instrucciones y la mezcla.
    let firma: String
    /// Pistas de vídeo ya compuestas: sus clips renderizados (con la pista de
    /// composición de cada uno) se reutilizan en la reconstrucción incremental.
    let pistasCompuestas: [(pista: Pista, clips: [ClipRenderizado])]
    /// Pistas de audio de la composición, en el orden en que se crearon (una
    /// por clip de audio). Se reutilizan igual que las de vídeo.
    let tracksDeAudio: [AVMutableCompositionTrack]

    init(
        composicion: AVMutableComposition,
        composicionDeVideo: AVMutableVideoComposition?,
        mezclaDeAudio: AVMutableAudioMix?,
        tamano: CGSize,
        avisos: [AvisoDeMontaje],
        firma: String,
        pistasCompuestas: [(pista: Pista, clips: [ClipRenderizado])],
        tracksDeAudio: [AVMutableCompositionTrack]
    ) {
        self.composicion = composicion
        self.composicionDeVideo = composicionDeVideo
        self.mezclaDeAudio = mezclaDeAudio
        self.tamano = tamano
        self.avisos = avisos
        self.firma = firma
        self.pistasCompuestas = pistasCompuestas
        self.tracksDeAudio = tracksDeAudio
    }

    var estaVacio: Bool { composicion.tracks.isEmpty }
}

/// Paneo de una pista, aplicado con un tap de audio.
///
/// `AVMutableAudioMixInputParameters` solo sabe de volumen y rampas: no tiene
/// paneo. La vía correcta es un `MTAudioProcessingTap` enganchado al parámetro de
/// la pista, que multiplica cada canal por la ganancia de su lado. Como el tap
/// viaja dentro de `mezclaDeAudio`, **el mismo código suena en la reproducción y
/// se aplica igual en la exportación** —la regla que ya costó un fallo de
/// fundidos—, y el paneo deja de ser una función muerta.
///
/// Es una ley de balance, no de potencia constante: al extremo, el canal contrario
/// se anula del todo, que es lo que se espera al girar una perilla.
final class TapDePaneo {

    /// La ganancia de cada canal sale del paneo: −1 es todo a la izquierda y +1
    /// todo a la derecha, con interpolación lineal entre medias.
    static func ganancias(paneo: Double) -> (izquierda: Float, derecha: Float) {
        LeyDeBalance.ganancias(paneo: paneo)
    }

    private let izquierda: Float
    private let derecha: Float

    init(paneo: Double) {
        let g = Self.ganancias(paneo: paneo)
        izquierda = g.izquierda
        derecha = g.derecha
    }

    /// El registro de taps vivos. El callback de proceso no recibe estado propio
    /// en este SDK, así que el puntero del tap es la clave con la que se recupera
    /// el objeto —y la finalización lo libera y lo borra.
    private static var registro: [MTAudioProcessingTap: TapDePaneo] = [:]
    private static let cerrojo = NSLock()

    var tap: MTAudioProcessingTap {
        var callbacks = MTAudioProcessingTapCallbacks(
            version: kMTAudioProcessingTapCallbacksVersion_0,
            clientInfo: nil,
            init: nil,
            // El registro se limpia cuando AVFoundation libera el tap —cada
            // `rebuildPreview` reemplaza el item y con él el `audioMix`—, así que
            // no puede crecer sin límite.
            finalize: { tap in
                TapDePaneo.cerrojo.lock()
                TapDePaneo.registro[tap] = nil
                TapDePaneo.cerrojo.unlock()
            },
            prepare: nil,
            unprepare: nil,
            process: { tap, numberOfFrames, flags, bufferListInOut, _, _ in
                // La última llamada trae el flag de fin de flujo y no hay muestras
                // que tocar; además, después viene la liberación.
                if flags & UInt32(kMTAudioProcessingTapFlag_EndOfStream) != 0 { return }
                let bufferList = bufferListInOut.pointee

                TapDePaneo.cerrojo.lock()
                guard let paneo = TapDePaneo.registro[tap] else {
                    TapDePaneo.cerrojo.unlock()
                    return
                }
                TapDePaneo.cerrojo.unlock()

                let abuf = bufferList.mBuffers
                guard let datos = abuf.mData else { return }
                let canales = Int(abuf.mNumberChannels)
                guard canales >= 2 else { return }
                let frames = Int(numberOfFrames)
                let floats = datos.assumingMemoryBound(to: Float.self)
                for frame in 0..<frames {
                    let base = frame * canales
                    floats[base] *= paneo.izquierda
                    floats[base + 1] *= paneo.derecha
                }
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
            // Sin tap no hay paneo, pero el audio suena: el fallo no puede dejar
            // la pista muda, así que se degrada a centro.
            return TapDePaneo(paneo: 0).tapDeGradacion
        }
        TapDePaneo.cerrojo.lock()
        TapDePaneo.registro[tap] = self
        TapDePaneo.cerrojo.unlock()
        return tap
    }

    /// Un tap de identidad para cuando la creación falla: el audio sigue sonando
    /// al centro, que es el comportamiento de una pista sin paneo.
    private var tapDeGradacion: MTAudioProcessingTap {
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
            fatalError("editorcito: no se pudo crear el tap de paneo")
        }
        return tap
    }
}

struct ClipRenderizado {
    let original: Clip
    let render: Clip
    let pista: AVMutableCompositionTrack
}

/// Convierte la línea de tiempo en algo que AVFoundation sabe reproducir.
///
/// Cada pista del montaje es una pista de la composición, y el orden manda: una
/// pista de vídeo superior tapa a la de debajo, igual que en cualquier NLE. Los
/// fundidos, la opacidad, el encuadre y la ganancia se resuelven con rampas de la
/// propia composición, así que no hay que renderizar nada para verlos.
enum ConstructorDeMontaje {

    static func construir(
        _ linea: LineaDeTiempo,
        medios: [UUID: MedioResuelto],
        tamanoDeSalida: CGSize? = nil,
        reutilizando previo: MontajeRenderizable? = nil
    ) -> MontajeRenderizable {
        let firma = linea.firmaDeComposicion
        // La reconstrucción incremental reutiliza las pistas cuando la
        // estructura no cambió: solo se rehacen las instrucciones y la mezcla,
        // que es donde viven los atributos (color, ganancia, keyframes…).
        let incremental = previo.flatMap { $0.firma == firma ? $0 : nil }
        let composicion = incremental?.composicion ?? AVMutableComposition()
        var avisos: [AvisoDeMontaje] = []
        let timebase = linea.timebase

        let tamano = tamanoDeSalida ?? tamanoSugerido(linea, medios: medios)

        // Un timeline de frames enteros no puede inventar los PTS variables de un
        // móvil. Se permite previsualizar y exportar bajo decisión explícita, pero
        // el riesgo debe aparecer antes de que el usuario confíe en la sincronía.
        var vfrAdvertidos: Set<UUID> = []
        for (_, clip) in linea.todosLosClips where !clip.esAjuste && !clip.esTitulo {
            guard let medio = medios[clip.mediaID], medio.esVFR,
                  vfrAdvertidos.insert(clip.mediaID).inserted else { continue }
            avisos.append(AvisoDeMontaje(
                "«\(clip.nombre)» tiene cadencia variable: la sincronía exacta requiere conformar sus PTS antes de entregar",
                critico: true
            ))
        }

        // El "solo" de una pista silencia a las demás de su tipo. Es la convención
        // de todas las mesas de mezclas y de todos los editores.
        let haySoloDeAudio = linea.pistas.contains { $0.tipo == .audio && $0.solo }
        let haySoloDeVideo = linea.pistas.contains { $0.tipo == .video && $0.solo }

        // Las pistas de vídeo se recorren de abajo arriba para que el índice de capa
        // coincida con la altura visual. El filtro de visible/solo se aplica aquí
        // para que las instrucciones (que deciden el color del tramo) vean las
        // mismas pistas que la inserción.
        let pistasDeVideo = linea.pistas
            .filter { $0.tipo == .video }
            .reversed()
            .filter { $0.visible && (!haySoloDeVideo || $0.solo) }
        let pistasDeAudio = linea.pistas.filter { $0.tipo == .audio }
        // El diálogo que dispara el ducking: por defecto la primera pista de
        // audio (la convención de voz), pero cada pista puede elegir su propia
        // fuente de sidechain con `fuenteDeDucking`.
        func rangosDe(_ pista: Pista) -> [(inicio: Int64, fin: Int64)] {
            let fuente: Pista?
            if let id = pista.fuenteDeDucking, let encontrada = pistasDeAudio.first(where: { $0.id == id }) {
                fuente = encontrada
            } else {
                fuente = pistasDeAudio.first
            }
            return fuente?.clips.filter { $0.habilitado }.map { ($0.inicio, $0.fin) } ?? []
        }

        var pistasDeVideoCompuestas: [(pista: Pista, clips: [ClipRenderizado])] = []
        var parametrosDeAudio: [AVMutableAudioMixInputParameters] = []

        // MARK: Vídeo

        if let incremental {
            // Las pistas de la composición se reutilizan, pero los clips
            // renderizados se rehacen con el montaje actual: las instrucciones
            // leen de ellos los atributos (color, keyframes, fundidos), que es
            // justo lo que puede haber cambiado. La firma garantiza el mismo
            // orden de clips, así que cada uno recupera su pista.
            for (indicePista, entrada) in incremental.pistasCompuestas.enumerated() {
                // La pista fresca del montaje actual (la guardada en el render
                // anterior llevaría los clips viejos).
                guard let pista = pistasDeVideo.first(where: { $0.id == entrada.pista.id }) else { continue }
                let visible = pista.visible && (!haySoloDeVideo || pista.solo)
                let clips = pista.clips.filter { $0.habilitado && visible }.sorted { $0.inicio < $1.inicio }
                var renderizados: [ClipRenderizado] = []
                var cursorDePista = 0
                for (indice, clip) in clips.enumerated() {
                    if clip.esAjuste || clip.esTitulo { continue }
                    guard cursorDePista < entrada.clips.count else { break }
                    let render = clipParaRenderizar(clip, en: clips, indice: indice)
                    renderizados.append(ClipRenderizado(
                        original: clip, render: render,
                        pista: entrada.clips[cursorDePista].pista
                    ))
                    cursorDePista += 1
                }
                if !renderizados.isEmpty {
                    if pistasDeVideoCompuestas.indices.contains(indicePista) {
                        pistasDeVideoCompuestas[indicePista] = (pista, renderizados)
                    } else {
                        pistasDeVideoCompuestas.append((pista, renderizados))
                    }
                }
            }
        } else {
        for pista in pistasDeVideo {
            let visible = pista.visible && (!haySoloDeVideo || pista.solo)
            let clips = pista.clips.filter { $0.habilitado && visible }.sorted { $0.inicio < $1.inicio }
            guard !clips.isEmpty else { continue }

            var renderizados: [ClipRenderizado] = []
            for (indice, clip) in clips.enumerated() {
                // Una pista de ajuste no genera material: solo manda su color
                // (y su LUT) sobre lo que hay debajo, en las instrucciones.
                if clip.esAjuste { continue }
                // Un título tampoco: se quema encima del vídeo con la misma
                // herramienta de post-proceso que los subtítulos.
                if clip.esTitulo { continue }
                guard let medio = medios[clip.mediaID] else {
                    avisos.append(AvisoDeMontaje("«\(clip.nombre)» no encuentra su archivo", critico: true))
                    continue
                }
                guard medio.tieneVideo,
                      let destino = composicion.addMutableTrack(
                        withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid
                      ) else { continue }
                let render = clipParaRenderizar(clip, en: clips, indice: indice)
                let ok: Bool
                if let multicam = clip.multicam {
                    ok = insertarMulticam(render, multicam: multicam, de: linea, medios: medios,
                                          tipo: .video, en: destino, timebase: timebase, avisos: &avisos)
                } else {
                    ok = insertar(render, de: medio, tipo: .video, en: destino, timebase: timebase, avisos: &avisos)
                }
                if !ok {
                    composicion.removeTrack(destino)
                    continue
                }
                renderizados.append(ClipRenderizado(original: clip, render: render, pista: destino))
            }
            if !renderizados.isEmpty { pistasDeVideoCompuestas.append((pista, renderizados)) }
        }
        }

        // Una LUT que no existe se avisa, no se traga: el compositor aplicará la
        // imagen sin ella y el usuario tiene que saberlo antes de exportar.
        for pista in pistasDeVideo {
            for clip in pista.clips where clip.habilitado {
                if let lut = clip.lutDeColor, !lut.isEmpty,
                   !FileManager.default.fileExists(atPath: lut) {
                    avisos.append(AvisoDeMontaje(
                        "«\(clip.nombre)» no encuentra su LUT: \(lut)", critico: false
                    ))
                }
            }
        }

        // MARK: Audio

        var tracksDeAudioCreadas: [AVMutableCompositionTrack] = []
        // Cursor en la lista plana de pistas de audio del render anterior: los
        // clips se recorren en el mismo orden (la firma lo garantiza), así que
        // la pista que le toca a cada clip es la del cursor.
        var cursorDeAudio = 0
        for pista in pistasDeAudio {
            let suena = !pista.silenciada && (!haySoloDeAudio || pista.solo)
            let clips = pista.clips.filter { $0.habilitado }.sorted { $0.inicio < $1.inicio }
            guard !clips.isEmpty else { continue }

            for (indice, clip) in clips.enumerated() {
                guard let medio = medios[clip.mediaID], medio.tieneAudio else { continue }
                let destino: AVMutableCompositionTrack
                if let incremental, cursorDeAudio < incremental.tracksDeAudio.count {
                    destino = incremental.tracksDeAudio[cursorDeAudio]
                } else {
                    guard let creada = composicion.addMutableTrack(
                        withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid
                    ) else { continue }
                    destino = creada
                    tracksDeAudioCreadas.append(destino)
                }
                cursorDeAudio += 1
                let render = clipParaRenderizar(clip, en: clips, indice: indice)
                if incremental == nil {
                    let ok: Bool
                    if let multicam = clip.multicam {
                        ok = insertarMulticam(render, multicam: multicam, de: linea, medios: medios,
                                              tipo: .audio, en: destino, timebase: timebase, avisos: &avisos)
                    } else {
                        ok = insertar(render, de: medio, tipo: .audio, en: destino, timebase: timebase, avisos: &avisos)
                    }
                    guard ok else {
                        composicion.removeTrack(destino)
                        continue
                    }
                }

                let parametros = AVMutableAudioMixInputParameters(track: destino)
                if !suena {
                    parametros.setVolume(0, at: .zero)
                } else {
                    let nivelDePista = Float(pow(10.0, pista.volumenDB / 20.0))
                    aplicarVolumen(
                        [render],
                        a: parametros,
                        timebase: timebase,
                        nivelDePista: nivelDePista,
                        ducking: pista.duckingActivo ? rangosDe(pista) : []
                    )
                    // El procesamiento de la pista viaja en el mismo parámetro
                    // de mezcla que el volumen, así que la reproducción y la
                    // exportación aplican el mismo código (la regla de la casa).
                    // Puerta → EQ → multibanda → compresor → limiter → reverb →
                    // retardo → paneo en una sola cadena; sin procesamiento, el
                    // tap de paneo de siempre.
                    let cadena = pista.tieneProcesamientoDeAudio
                    let paneoActivo = pista.paneo.map { abs($0) > 0.01 } ?? false
                    if cadena {
                        parametros.audioTapProcessor = TapDeMezcla(cadena: CadenaDeMezcla(
                            frecuencia: Self.frecuenciaDeMuestreo(de: destino),
                            canales: Self.numeroDeCanales(de: destino),
                            bandas: pista.ecualizacion ?? [],
                            compresor: pista.compresor,
                            limitador: pista.limitador,
                            paneo: pista.paneo,
                            claveDeMedidor: pista.id.uuidString,
                            puerta: pista.puertaDeRuido,
                            multibanda: pista.multibanda,
                            reverb: pista.reverb,
                            retardo: pista.retardo
                        )).tap
                    } else if paneoActivo, let paneo = pista.paneo {
                        parametros.audioTapProcessor = TapDePaneo(paneo: paneo).tap
                    }
                    // Sin procesamiento ni paneo no hay tap: un tap de identidad
                    // añadido a todas las pistas silenciaba la exportación
                    // (verificado sobre archivos reales). El medidor en vivo de
                    // esas pistas queda vacío, que es honesto.
                }
                parametrosDeAudio.append(parametros)
            }
        }

        // MARK: Instrucciones de vídeo

        var composicionDeVideo: AVMutableVideoComposition?
        let hayAjustes = pistasDeVideo.contains { pista in pista.clips.contains(where: \.esAjuste) }
        let hayTitulos = pistasDeVideo.contains { pista in pista.clips.contains(where: \.esTitulo) }
        if !pistasDeVideoCompuestas.isEmpty || hayAjustes || hayTitulos {
            composicionDeVideo = construirInstrucciones(
                pistasDeVideoCompuestas, pistasDeVideo: pistasDeVideo,
                linea: linea, medios: medios, tamano: tamano,
                subtitulos: linea.subtitulos ?? [],
                estilos: linea.estilosDeSubtitulo,
                palabrasDelMontaje: linea.palabrasDelMontaje()
            )
        }

        var mezcla: AVMutableAudioMix?
        if !parametrosDeAudio.isEmpty {
            let m = AVMutableAudioMix()
            m.inputParameters = parametrosDeAudio
            mezcla = m
        }

        return MontajeRenderizable(
            composicion: composicion,
            composicionDeVideo: composicionDeVideo,
            mezclaDeAudio: mezcla,
            tamano: tamano,
            avisos: avisos,
            firma: firma,
            pistasCompuestas: pistasDeVideoCompuestas,
            tracksDeAudio: incremental?.tracksDeAudio ?? tracksDeAudioCreadas
        )
    }

    /// La cadencia de muestreo real de la pista de composición: los coeficientes
    /// del EQ dependen de ella, y usar 48 k cuando el material es de 44,1 k
    /// desplazaría las frecuencias.
    private static func frecuenciaDeMuestreo(de pista: AVAssetTrack) -> Double {
        if !pista.formatDescriptions.isEmpty,
           let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(pista.formatDescriptions[0] as! CMAudioFormatDescription) {
            return asbd.pointee.mSampleRate > 0 ? asbd.pointee.mSampleRate : 48_000
        }
        return 48_000
    }

    private static func numeroDeCanales(de pista: AVAssetTrack) -> Int {
        for descripcion in pista.formatDescriptions {
            let audio = descripcion as! CMAudioFormatDescription
            guard let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(audio) else { continue }
            return max(1, Int(asbd.pointee.mChannelsPerFrame))
        }
        return 2
    }

    // MARK: - Inserción

    private static func clipParaRenderizar(_ clip: Clip, en clips: [Clip], indice: Int) -> Clip {
        var render = clip

        if indice > 0 {
            let anterior = clips[indice - 1]
            if anterior.fin == clip.inicio,
               let salida = anterior.transicionSalida,
               let entrada = clip.transicionEntrada {
                render.entradaFundido = max(render.entradaFundido, min(salida.duracion, entrada.duracion))
            }
        }

        if indice + 1 < clips.count {
            let siguiente = clips[indice + 1]
            if clip.fin == siguiente.inicio,
               let salida = clip.transicionSalida,
               let entrada = siguiente.transicionEntrada {
                let duracion = min(salida.duracion, entrada.duracion)
                render.duracion += duracion
                render.salidaFundido = max(render.salidaFundido, duracion)
            }
        }
        return render
    }

    /// Inserta los tramos de ángulo de un clip multicámara en su pista.
    ///
    /// Cada corte del clip es un tramo de una fuente distinta, y todos entran
    /// en la misma pista de composición: para AVFoundation una pista puede
    /// tener varios rangos de orígenes distintos. El audio enlazado hace el
    /// mismo recorrido, así que el corte de ángulo cambia imagen y sonido a la
    /// vez. El retime se deja fuera a propósito: estirar cada tramo con
    /// `scaleTimeRange` sobre un montón de orígenes es pedirle al motor una
    /// pieza que todavía no se ha validado.
    private static func insertarMulticam(
        _ clip: Clip,
        multicam: MulticamDeClip,
        de linea: LineaDeTiempo,
        medios: [UUID: MedioResuelto],
        tipo: AVMediaType,
        en destino: AVMutableCompositionTrack,
        timebase: Timebase,
        avisos: inout [AvisoDeMontaje]
    ) -> Bool {
        guard clip.velocidad == 1, clip.rampasDeVelocidad == nil else {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» va a velocidad distinta de 1: el retime multicámara todavía no está soportado", critico: true))
            return false
        }
        guard let grupo = linea.gruposMulticam?.first(where: { $0.id == multicam.grupoID }) else {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» no encuentra su grupo multicámara", critico: true))
            return false
        }
        let segmentos = multicam.segmentos(duracion: clip.duracion)
        guard !segmentos.isEmpty else {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» no tiene ángulos que mostrar", critico: true))
            return false
        }

        var alguno = false
        for segmento in segmentos {
            guard let medio = medios[segmento.mediaID] else {
                avisos.append(AvisoDeMontaje("«\(clip.nombre)» no encuentra el ángulo \(segmento.mediaID)", critico: true))
                continue
            }
            let desfase = grupo.desfases[segmento.mediaID] ?? 0
            if insertarSegmento(segmento, del: clip, de: medio, desfase: desfase,
                                tipo: tipo, en: destino, timebase: timebase, avisos: &avisos) {
                alguno = true
            }
        }
        return alguno
    }

    /// Un tramo de ángulo dentro de la pista de composición.
    ///
    /// El clip multicámara vive en el tiempo del grupo: su ventana es el
    /// tramo [inicio, fin) de la línea de tiempo, y cada ángulo muestra en el
    /// instante de grupo `t` su material en `t − desfase` (el ángulo que
    /// arranca `desfase` frames después tiene su comienzo ahí). Por eso
    /// `entradaEnOrigen` no participa: recortar el clip mueve su ventana, no
    /// el origen de cada cámara.
    private static func insertarSegmento(
        _ segmento: (desde: Int64, hasta: Int64, mediaID: UUID),
        del clip: Clip,
        de medio: MedioResuelto,
        desfase: Int64,
        tipo: AVMediaType,
        en destino: AVMutableCompositionTrack,
        timebase: Timebase,
        avisos: inout [AvisoDeMontaje]
    ) -> Bool {
        guard let origen = (tipo == .video ? medio.pistaDeVideo : medio.pistaDeAudio) else { return false }

        let grupoT = clip.inicio + segmento.desde
        let entradaOrigen = grupoT - desfase
        guard entradaOrigen >= 0 else {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» pedía material antes del arranque del ángulo", critico: true))
            return false
        }
        let entrada = timebase.tiempo(entradaOrigen)
        var duracionDeOrigen = timebase.tiempo(segmento.hasta - segmento.desde)
        let disponible = CMTimeSubtract(medio.duracion, entrada)
        if CMTimeCompare(duracionDeOrigen, disponible) > 0 {
            duracionDeOrigen = disponible
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» pedía más metraje del que tiene el ángulo", critico: false))
        }
        guard CMTimeCompare(duracionDeOrigen, .zero) > 0 else { return false }

        let rangoDeOrigen = CMTimeRange(start: entrada, duration: duracionDeOrigen)
        do {
            try destino.insertTimeRange(rangoDeOrigen, of: origen, at: timebase.tiempo(grupoT))
        } catch {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» no se pudo montar: \(error.localizedDescription)", critico: true))
            return false
        }
        return true
    }

    private static func insertar(
        _ clip: Clip,
        de medio: MedioResuelto,
        tipo: AVMediaType,
        en destino: AVMutableCompositionTrack,
        timebase: Timebase,
        avisos: inout [AvisoDeMontaje]
    ) -> Bool {
        guard let origen = (tipo == .video ? medio.pistaDeVideo : medio.pistaDeAudio) else { return false }

        if clip.velocidad < 0 {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» va marcha atrás y eso todavía necesita renderizado previo", critico: true))
            return false
        }

        // Con rampas de velocidad el clip se parte en tramos, cada uno con su
        // trozo de origen y su duración en el montaje; sin ellas es un solo
        // tramo a velocidad constante.
        let piezas = clip.piezasDeVelocidad()
        var alguno = false
        var posicionDeOrigen = clip.entradaEnOrigen
        for pieza in piezas {
            let ok = insertarPieza(
                pieza, del: clip, de: medio, origen: origen,
                posicionDeOrigen: &posicionDeOrigen, tipo: tipo,
                en: destino, timebase: timebase, avisos: &avisos
            )
            alguno = alguno || ok
        }
        return alguno
    }

    /// Inserta un tramo de un clip en la composición y lo estira (o congela).
    private static func insertarPieza(
        _ pieza: (desde: Int64, hasta: Int64, consumo: Int64),
        del clip: Clip,
        de medio: MedioResuelto,
        origen: AVAssetTrack,
        posicionDeOrigen: inout Int64,
        tipo: AVMediaType,
        en destino: AVMutableCompositionTrack,
        timebase: Timebase,
        avisos: inout [AvisoDeMontaje]
    ) -> Bool {
        // Un congelado (velocidad 0) no consume material: se inserta un solo
        // frame de origen y se estira a la duración del tramo.
        let framesDeOrigen: Int64 = pieza.consumo == 0 ? 1 : pieza.consumo
        let entrada = timebase.tiempo(posicionDeOrigen)
        var duracionDeOrigen = timebase.tiempo(framesDeOrigen)
        let disponible = CMTimeSubtract(medio.duracion, entrada)
        if CMTimeCompare(duracionDeOrigen, disponible) > 0 {
            duracionDeOrigen = disponible
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» pedía más metraje del que tiene el archivo", critico: false))
        }
        posicionDeOrigen += framesDeOrigen
        guard CMTimeCompare(duracionDeOrigen, .zero) > 0 else { return false }

        let rangoDeOrigen = CMTimeRange(start: entrada, duration: duracionDeOrigen)
        let inicio = timebase.tiempo(clip.inicio + pieza.desde)
        let duracionDelTramo = timebase.tiempo(pieza.hasta - pieza.desde)

        do {
            try destino.insertTimeRange(rangoDeOrigen, of: origen, at: inicio)
        } catch {
            avisos.append(AvisoDeMontaje("«\(clip.nombre)» no se pudo montar: \(error.localizedDescription)", critico: true))
            return false
        }

        // El tramo se estira a su duración en el montaje, que es como
        // AVFoundation hace el retime. Un congelado estira un frame a todo el
        // tramo: es exactamente lo que hace el freeze frame de cualquier NLE.
        if CMTimeCompare(duracionDelTramo, duracionDeOrigen) != 0 && duracionDelTramo.seconds > 0 {
            let rangoInsertado = CMTimeRange(start: inicio, duration: duracionDeOrigen)
            destino.scaleTimeRange(rangoInsertado, toDuration: duracionDelTramo)
        }
        return true
    }

    // MARK: - Audio

    /// Ganancia, fundidos y ducking como rampas explícitas y contiguas.
    ///
    /// AVFoundation interpola linealmente entre los puntos de control de
    /// `setVolume`: un «sostener» construido con dos puntos —inicio con el
    /// nivel, fin con silencio— se convierte en una bajada en toda la duración
    /// del clip, tanto en la lectura de mezcla como en la exportación. La
    /// única forma de que el renderizador haga lo que pide el montaje es
    /// expresar hasta el tramo constante como una rampa v→v con su propio
    /// `timeRange`, y sin solapes: la API rechaza rampas que se pisen.
    private static func aplicarVolumen(
        _ clips: [Clip],
        a parametros: AVMutableAudioMixInputParameters,
        timebase: Timebase,
        nivelDePista: Float,
        ducking: [(inicio: Int64, fin: Int64)] = []
    ) {
        var anteriorFin: Int64 = 0
        for clip in clips {
            let inicio = timebase.tiempo(clip.inicio)
            let fin = timebase.tiempo(clip.fin)
            let nivelInicial = nivelDePista * Float(pow(10.0, clip.gananciaEn(frame: 0) / 20.0))
            let nivelFinal = nivelDePista * Float(pow(10.0, clip.gananciaEn(frame: clip.duracion) / 20.0))

            let entrada = min(clip.entradaFundido, clip.duracion)
            let salida = min(clip.salidaFundido, clip.duracion - entrada)
            // Sin fundido de salida, el último frame deja de sonar: la rampa
            // de un frame evita que el renderizador sostenga el nivel tras el
            // clip, que es lo que pasaba con un punto de silencio al final.
            let centroInicio = clip.inicio + entrada
            let centroFin = clip.fin - salida - (salida > 0 ? 0 : 1)

            // Silencio antes del clip: la cola del anterior ya acabó en cero,
            // y esta rampa evita que el renderizador arranque a otro nivel.
            if clip.inicio > anteriorFin {
                rampa(parametros, de: 0, a: 0, desde: timebase.tiempo(anteriorFin), hasta: inicio)
            }

            if entrada > 0 {
                rampa(parametros, de: 0, a: nivelInicial, desde: inicio, hasta: timebase.tiempo(centroInicio))
            }

            // Tramo central: envolvente del clip entre los dos fundidos, partida
            // en piezas por los keyframes de ganancia y por los tramos de ducking.
            let piezasDeGanancia = piezasDeGanancia(
                desde: centroInicio, hasta: centroFin,
                nivelDePista: nivelDePista, clip: clip
            )
            let piezas = piezasCentrales(
                piezasDeGanancia,
                nivelInicial: nivelInicial,
                ducking: ducking, timebase: timebase
            )
            for pieza in piezas {
                rampa(
                    parametros,
                    de: pieza.vInicio, a: pieza.vFin,
                    desde: timebase.tiempo(pieza.desde), hasta: timebase.tiempo(pieza.hasta)
                )
            }

            rampa(parametros, de: nivelFinal, a: 0, desde: timebase.tiempo(centroFin), hasta: fin)
            anteriorFin = clip.fin
        }
    }

    /// El tramo central del clip como rampas entre keyframes de ganancia.
    ///
    /// Cada keyframe dentro del tramo es un borde de rampa, y cada segmento va
    /// del valor de `gananciaEn` de su inicio al de su fin —la misma
    /// interpolación lineal que usa el modelo, así la envolvente que suena es
    /// exactamente la que se dibuja. Sin keyframes, el resultado es la única
    /// rampa lineal del tramo, como antes.
    static func piezasDeGanancia(
        desde: Int64, hasta: Int64,
        nivelDePista: Float,
        clip: Clip
    ) -> [(desde: Int64, hasta: Int64, vInicio: Float, vFin: Float)] {
        guard hasta > desde else { return [] }

        var bordes: [Int64] = [desde, hasta]
        for keyframe in clip.keyframes ?? [] {
            let frame = clip.inicio + keyframe.frame
            if frame > desde && frame < hasta { bordes.append(frame) }
        }
        bordes.sort()

        var piezas: [(desde: Int64, hasta: Int64, vInicio: Float, vFin: Float)] = []
        for i in 0..<(bordes.count - 1) {
            let a = bordes[i]
            let b = bordes[i + 1]
            guard b > a else { continue }
            piezas.append((
                a, b,
                nivelDePista * Float(pow(10.0, clip.gananciaEn(frame: a - clip.inicio) / 20.0)),
                nivelDePista * Float(pow(10.0, clip.gananciaEn(frame: b - clip.inicio) / 20.0))
            ))
        }
        return piezas
    }

    /// El tramo central del clip, con los hundimientos del ducking recortados.
    ///
    /// Cada rango de diálogo convierte una parte del tramo en tres piezas
    /// contiguas: bajada al nivel atenuado, estancia y subida de vuelta. El
    /// nivel del hundimiento se toma del arranque del clip, como hacía el
    /// mezclador anterior; si el tramo central ya es una rampa por keyframes,
    /// el ducking se dibuja sobre ella por piezas.
    private static func piezasCentrales(
        _ piezasDeGanancia: [(desde: Int64, hasta: Int64, vInicio: Float, vFin: Float)],
        nivelInicial: Float,
        ducking: [(inicio: Int64, fin: Int64)],
        timebase: Timebase
    ) -> [(desde: Int64, hasta: Int64, vInicio: Float, vFin: Float)] {
        guard let primero = piezasDeGanancia.first, let ultimo = piezasDeGanancia.last else { return [] }
        // El tramo central del clip, que es la ventana de los hundimientos.
        let desde = primero.desde
        let hasta = ultimo.hasta
        var piezas = piezasDeGanancia

        for rango in ducking where rango.fin > desde && rango.inicio < hasta {
            let comienzo = max(rango.inicio, desde)
            let finDucking = min(rango.fin, hasta)
            guard finDucking > comienzo else { continue }
            let transicion = min(Int64(timebase.fps * 0.12), (finDucking - comienzo) / 2)
            guard transicion > 0 else { continue }

            let atenuado = nivelInicial * 0.25
            var nuevas: [(desde: Int64, hasta: Int64, vInicio: Float, vFin: Float)] = []
            for pieza in piezas {
                if pieza.hasta <= comienzo || pieza.desde >= finDucking {
                    nuevas.append(pieza)
                    continue
                }
                if pieza.desde < comienzo {
                    nuevas.append((pieza.desde, comienzo, pieza.vInicio, pieza.vInicio))
                }
                nuevas.append((comienzo, comienzo + transicion, nivelInicial, atenuado))
                nuevas.append((comienzo + transicion, finDucking - transicion, atenuado, atenuado))
                nuevas.append((finDucking - transicion, finDucking, atenuado, nivelInicial))
                if pieza.hasta > finDucking {
                    nuevas.append((finDucking, pieza.hasta, pieza.vFin, pieza.vFin))
                }
            }
            piezas = nuevas.filter { $0.hasta > $0.desde }
        }
        return piezas
    }

    private static func rampa(
        _ parametros: AVMutableAudioMixInputParameters,
        de vInicio: Float, a vFin: Float,
        desde: CMTime, hasta: CMTime
    ) {
        let duracion = CMTimeSubtract(hasta, desde)
        guard duracion.seconds > 0 else { return }
        parametros.setVolumeRamp(
            fromStartVolume: vInicio, toEndVolume: vFin,
            timeRange: CMTimeRange(start: desde, duration: duracion)
        )
    }

    // MARK: - Instrucciones de vídeo

    private static func construirInstrucciones(
        _ pistas: [(pista: Pista, clips: [ClipRenderizado])],
        pistasDeVideo: [Pista],
        linea: LineaDeTiempo,
        medios: [UUID: MedioResuelto],
        tamano: CGSize,
        subtitulos: [Subtitulo],
        estilos: [EstiloDeSubtitulo],
        palabrasDelMontaje: [PalabraDelMontaje]
    ) -> AVMutableVideoComposition {
        let timebase = linea.timebase

        // Los cortes de todas las pistas definen los tramos en los que la escena no
        // cambia. Dentro de cada tramo la lista de capas es fija, que es justo lo
        // que exige una instrucción de composición.
        var fronteras: Set<Int64> = [0]
        for (_, clips) in pistas {
            for clipRenderizado in clips where clipRenderizado.render.habilitado {
                let clip = clipRenderizado.render
                fronteras.insert(clip.inicio)
                fronteras.insert(clip.fin)
                for keyframe in clipRenderizado.original.keyframes ?? [] {
                    let frame = clipRenderizado.original.inicio + keyframe.frame
                    if frame > clip.inicio && frame < clip.fin { fronteras.insert(frame) }
                }
                // Cada corte de ángulo cambia el origen en ese tramo, y un
                // tramo no puede llevar dos fuentes en la misma instrucción.
                for corte in clipRenderizado.original.multicam?.cortes ?? [] {
                    let frame = clipRenderizado.original.inicio + corte.frame
                    if frame > clip.inicio && frame < clip.fin { fronteras.insert(frame) }
                }
            }
        }
        let cortes = fronteras.sorted()
        var instrucciones: [AVMutableVideoCompositionInstruction] = []

        for i in 0..<max(0, cortes.count - 1) {
            let desde = cortes[i]
            let hasta = cortes[i + 1]
            guard hasta > desde else { continue }

            var capas: [AVMutableVideoCompositionLayerInstruction] = []
            var efectosPorCapa: [Int32: EfectosDeCapa] = [:]
            // El color y la LUT del tramo son los del clip visible de la pista
            // superior, incluida una pista de ajuste que no genera renderizado.
            // Las pistas originales van de la inferior a la superior (el mismo
            // orden que la inserción), así que se recorren al revés: la primera
            // con un clip en el tramo es la que manda. El código anterior cogía
            // la inferior, contradiciendo su propio comentario —otro bug de dos
            // pistas con color—.
            var colorDelTramo: ColorDeClip?
            var lutDelTramo: String?
            for pista in pistasDeVideo.reversed() {
                guard let clip = pista.clips.first(where: {
                    $0.habilitado && $0.inicio <= desde && $0.fin >= hasta
                }) else { continue }
                colorDelTramo = clip.color
                lutDelTramo = clip.lutDeColor
                break
            }
            for (_, clips) in pistas {
                guard let renderizado = clips.first(where: {
                    $0.render.habilitado && $0.render.inicio <= desde && $0.render.fin >= hasta
                }) else { continue }
                let clip = renderizado.original
                // En un clip multicámara la fuente del tramo es el ángulo
                // activo ahí; el clip en sí no dice qué medio es.
                let medioID = clip.multicam?.medioActivo(en: max(0, desde - clip.inicio)) ?? clip.mediaID
                guard let medio = medios[medioID] else { continue }
                let transformacion = clip.transformacionEn(frame: max(0, desde - clip.inicio))

                let capa = AVMutableVideoCompositionLayerInstruction(assetTrack: renderizado.pista)
                capa.setTransform(
                    matrizDe(clip, transformacion: transformacion, medio: medio, tamanoDeSalida: tamano),
                    at: timebase.tiempo(desde)
                )
                aplicarOpacidad(renderizado.render, transformacion: transformacion, a: capa, tramo: (desde, hasta), timebase: timebase)
                aplicarRecorte(transformacion: transformacion, a: capa, medio: medio, tamano: tamano, en: timebase.tiempo(desde))
                capas.append(capa)
                // Los efectos de la capa viajan en la instrucción, indexados por
                // el trackID de su pista: el compositor los aplica por capa.
                efectosPorCapa[renderizado.pista.trackID] = EfectosDeCapa(
                    modoDeFusion: clip.modoDeFusion,
                    mascara: clip.mascara,
                    croma: clip.croma,
                    vignette: clip.color.vignette,
                    radioDeVignette: clip.color.radioDeVignette,
                    desenfoque: clip.color.desenfoque
                )
            }

            let instruccion = InstruccionConColor()
            instruccion.timeRange = CMTimeRange(
                start: timebase.tiempo(desde),
                duration: timebase.tiempo(hasta - desde)
            )
            // Las capas van de arriba abajo en la lista, así que se invierte: la
            // pista superior del montaje tiene que quedar la primera.
            instruccion.layerInstructions = capas.reversed()
            instruccion.backgroundColor = CGColor(red: 0, green: 0, blue: 0, alpha: 1)
            // El color y la LUT del clip visible de la pista superior, que es lo
            // que se ve.
            instruccion.colorDeClip = colorDelTramo
            instruccion.lutDeClip = lutDelTramo
            instruccion.efectosPorCapa = efectosPorCapa
            instrucciones.append(instruccion)
        }

        let composicion = AVMutableVideoComposition()
        composicion.renderSize = tamano
        composicion.frameDuration = timebase.tiempo(1)
        if instrucciones.isEmpty {
            // Solo títulos (o subtítulos) y ninguna capa de vídeo: AVFoundation
            // exige al menos una instrucción para pintar el fondo.
            let sola = InstruccionConColor()
            sola.timeRange = CMTimeRange(
                start: .zero,
                duration: CMTime(seconds: max(0.01, linea.duracion > 0 ? timebase.segundos(linea.duracion) : 1), preferredTimescale: 600)
            )
            sola.layerInstructions = []
            sola.backgroundColor = CGColor(red: 0, green: 0, blue: 0, alpha: 1)
            instrucciones.append(sola)
        }
        composicion.instructions = instrucciones
        // El compositor custom se activa con color, LUT o efectos de capa
        // (máscara, modo de fusión, viñeta, desenfoque): sin nada de eso,
        // AVFoundation usa su mezcla nativa y no hay coste.
        if instrucciones.contains(where: {
            let conColor = $0 as? InstruccionConColor
            let conEfectos = conColor?.efectosPorCapa.values.contains(where: \.tieneEfectos) ?? false
            return conColor?.colorDeClip?.esNeutro == false
                || conColor?.colorDeClip?.tieneAjustes == true
                || conColor?.lutDeClip != nil
                || conEfectos
        }) {
            composicion.customVideoCompositorClass = CompositorDeColor.self
        }
        if !subtitulos.isEmpty || pistasDeVideo.contains(where: { $0.clips.contains { $0.esTitulo } }) {
            let titulos = pistasDeVideo
                .reversed()
                .flatMap { pista in pista.clips.filter { $0.esTitulo } }
            composicion.animationTool = herramientaDeSubtitulos(
                subtitulos, estilos: estilos, palabrasDelMontaje: palabrasDelMontaje,
                titulos: titulos, tamano: tamano, timebase: timebase
            )
        }
        return composicion
    }

    /// Quema los subtítulos con su estilo y, si lo pide, resalta la palabra que suena.
    ///
    /// El resalte palabra a palabra sale del mismo dato que la edición por texto:
    /// `palabrasDelMontaje` ya coloca cada palabra del transcript en los frames del
    /// montaje donde se oye, con velocidad y recorte resueltos. Aquí se dibuja una
    /// capa por palabra y se anima su color: la que está sonando se pinta con el
    /// color de resalte y las demás con el del texto. Sin transcript —o con el modo
    /// en «ninguno»— se quema el subtítulo entero de una vez, como siempre.
    private static func herramientaDeSubtitulos(
        _ subtitulos: [Subtitulo],
        estilos: [EstiloDeSubtitulo],
        palabrasDelMontaje: [PalabraDelMontaje],
        titulos: [Clip] = [],
        tamano: CGSize,
        timebase: Timebase
    ) -> AVVideoCompositionCoreAnimationTool {
        let contenedor = CALayer()
        contenedor.frame = CGRect(origin: .zero, size: tamano)
        let video = CALayer()
        video.frame = contenedor.bounds
        contenedor.addSublayer(video)

        for titulo in titulos {
            quemaTitulo(titulo, en: contenedor, tamano: tamano, timebase: timebase)
        }

        func estiloDe(_ subtitulo: Subtitulo) -> EstiloDeSubtitulo {
            estilos.first { $0.nombre == subtitulo.estilo } ?? .porDefecto
        }

        for subtitulo in subtitulos {
            let estilo = estiloDe(subtitulo)
            let inicio = timebase.segundos(subtitulo.inicio)
            let duracion = max(0.01, timebase.segundos(subtitulo.fin - subtitulo.inicio))
            let fundido = min(0.15, duracion / 2)
            let opacidad = CAKeyframeAnimation(keyPath: "opacity")
            opacidad.values = [0, 1, 1, 0]
            opacidad.keyTimes = [0, fundido / duracion, 1 - fundido / duracion, 1].map(NSNumber.init(value:))
            opacidad.beginTime = inicio
            opacidad.duration = duracion
            opacidad.isRemovedOnCompletion = false
            opacidad.fillMode = .both

            let palabras = palabrasDelMontaje.filter { $0.desde >= subtitulo.inicio && $0.desde < subtitulo.fin }

            let resaltar = estilo.modoDeResalte != .ninguno && !palabras.isEmpty
            if resaltar {
                quemaPorPalabras(
                    subtitulo, palabras: palabras, estilo: estilo,
                    tamano: tamano, timebase: timebase,
                    inicio: inicio, duracion: duracion, fundido: fundido,
                    en: contenedor
                )
            } else {
                let capa = capaDeTexto(subtitulo.texto, estilo: estilo, tamano: tamano)
                capa.opacity = 0
                capa.add(opacidad, forKey: "editorcito.subtitle.\(subtitulo.id.uuidString)")
                contenedor.addSublayer(capa)
            }
        }
        return AVVideoCompositionCoreAnimationTool(postProcessingAsVideoLayer: video, in: contenedor)
    }

    /// Quema un título sobre el vídeo en el tramo del clip.
    ///
    /// El título es una capa de texto con fundido de entrada y salida,
    /// posicionada en el lienzo. Comparte el mecanismo de los subtítulos
    /// (CoreAnimation post-proceso), así que reproduce y exporta igual.
    private static func quemaTitulo(
        _ clip: Clip,
        en contenedor: CALayer,
        tamano: CGSize,
        timebase: Timebase
    ) {
        guard let titulo = clip.titulo else { return }
        let inicio = timebase.segundos(clip.inicio)
        let duracion = max(0.01, timebase.segundos(clip.duracion))
        let fundido = min(timebase.segundos(titulo.fundido), duracion / 2)

        let capa: CALayer
        switch titulo.forma {
        case .texto:
            guard !titulo.texto.isEmpty else { return }
            let fuente = NSFont(name: titulo.fuente, size: titulo.tamano) ?? NSFont.boldSystemFont(ofSize: titulo.tamano)
            let textLayer = CATextLayer()
            textLayer.string = titulo.texto
            textLayer.font = fuente
            textLayer.fontSize = titulo.tamano
            textLayer.foregroundColor = colorDe(titulo.color)
            textLayer.alignmentMode = .center
            textLayer.contentsScale = 2

            let medida = NSAttributedString(string: titulo.texto, attributes: [.font: fuente])
                .boundingRect(with: CGSize(width: tamano.width * 0.9, height: titulo.tamano * 4),
                              options: [.usesLineFragmentOrigin], context: nil)
            let ancho = min(tamano.width * 0.9, max(medida.width + titulo.tamano, 60))
            let alto = medida.height + titulo.tamano * 0.4
            let centro = CGPoint(x: tamano.width * titulo.posicionX,
                                 y: tamano.height * (1 - titulo.posicionY))
            textLayer.frame = CGRect(x: centro.x - ancho / 2, y: centro.y - alto / 2,
                                     width: ancho, height: alto)
            // El contorno se pinta con un borde del color de la fuente.
            if titulo.contorno > 0 {
                textLayer.borderColor = textLayer.foregroundColor
                textLayer.borderWidth = titulo.contorno
            }
            capa = textLayer

        case .rectangulo, .elipse, .linea:
            let ancho = tamano.width * titulo.ancho
            let alto = titulo.forma == .linea
                ? max(4, titulo.contorno > 0 ? titulo.contorno : 6)
                : tamano.height * titulo.alto
            let centro = CGPoint(x: tamano.width * titulo.posicionX,
                                 y: tamano.height * (1 - titulo.posicionY))
            let rect = CGRect(x: centro.x - ancho / 2, y: centro.y - alto / 2,
                              width: ancho, height: alto)
            let forma = CAShapeLayer()
            let camino = CGMutablePath()
            switch titulo.forma {
            case .rectangulo:
                let radio = min(8, alto / 2)
                camino.addRoundedRect(in: rect, cornerWidth: radio, cornerHeight: radio)
            case .elipse:
                camino.addEllipse(in: rect)
            default:
                camino.move(to: CGPoint(x: rect.minX, y: centro.y))
                camino.addLine(to: CGPoint(x: rect.maxX, y: centro.y))
            }
            forma.path = camino
            if titulo.contorno > 0 {
                forma.fillColor = CGColor.clear
                forma.strokeColor = colorDe(titulo.color)
                forma.lineWidth = titulo.contorno
            } else {
                forma.fillColor = colorDe(titulo.color)
            }
            capa = forma

        case .imagen:
            guard let ruta = titulo.rutaDeImagen,
                  let imagen = NSImage(contentsOfFile: ruta) else { return }
            let capaDeImagen = CALayer()
            capaDeImagen.contents = imagen.cgImage(forProposedRect: nil, context: nil, hints: nil)
            let ancho = tamano.width * titulo.ancho
            // Mantiene la relación de aspecto de la imagen: un logotipo no se
            // deforma por el tamaño declarado del título.
            let proporcion = imagen.size.height > 0 ? imagen.size.width / imagen.size.height : 1
            let alto = min(tamano.height * titulo.alto, ancho / max(proporcion, 0.01))
            let anchoFinal = alto * proporcion
            let centro = CGPoint(x: tamano.width * titulo.posicionX,
                                 y: tamano.height * (1 - titulo.posicionY))
            capaDeImagen.frame = CGRect(x: centro.x - anchoFinal / 2, y: centro.y - alto / 2,
                                        width: anchoFinal, height: alto)
            capaDeImagen.contentsGravity = .resizeAspect
            capa = capaDeImagen
        }

        capa.opacity = 0
        let animacion = CAKeyframeAnimation(keyPath: "opacity")
        animacion.values = [0, 1, 1, 0]
        animacion.keyTimes = [0, fundido / duracion, 1 - fundido / duracion, 1].map(NSNumber.init(value:))
        animacion.beginTime = inicio
        animacion.duration = duracion
        animacion.isRemovedOnCompletion = false
        animacion.fillMode = .both
        capa.add(animacion, forKey: "editorcito.title.\(clip.id.uuidString)")
        contenedor.addSublayer(capa)
    }

    /// Dibuja un subtítulo palabra a palabra, con la activa resaltada.
    private static func quemaPorPalabras(
        _ subtitulo: Subtitulo,
        palabras: [PalabraDelMontaje],
        estilo: EstiloDeSubtitulo,
        tamano: CGSize,
        timebase: Timebase,
        inicio: Double,
        duracion: Double,
        fundido: Double,
        en contenedor: CALayer
    ) {
        // Una capa por palabra, colocada según la fuente y el tamaño. Todas heredan
        // la misma posición base; el texto es el que las hace distintas.
        let fuente = NSFont(name: estilo.fuente, size: estilo.cuerpo) ?? NSFont.boldSystemFont(ofSize: estilo.cuerpo)
        let colorBase = colorDe(estilo.color)
        let colorActivo = colorDe(estilo.colorDeResalte)
        let margen = tamano.width * estilo.margen
        let ancho = tamano.width - margen * 2
        let altura = estilo.cuerpo * 1.6
        let y = tamano.height * (0.08 + 0.68 * estilo.posicion)

        for palabra in palabras {
            let capa = CATextLayer()
            capa.string = palabra.texto
            capa.font = fuente
            capa.fontSize = estilo.cuerpo
            capa.foregroundColor = colorBase
            capa.alignmentMode = .center
            capa.contentsScale = 2
            // El resalte por palabra no lleva fondo: el fondo del estilo se pinta en
            // una capa aparte debajo, para no dibujar un rectángulo por palabra.
            capa.frame = CGRect(x: margen, y: y, width: ancho, height: altura)
            capa.position = CGPoint(x: tamano.width / 2, y: y + altura / 2)
            capa.bounds = CGRect(x: 0, y: 0, width: ancho, height: altura)
            capa.opacity = 0

            let desde = timebase.segundos(palabra.desde)
            let hasta = timebase.segundos(palabra.hasta)
            let colorAnimacion = CAKeyframeAnimation(keyPath: "foregroundColor")
            // La palabra se pinta activa mientras suena; fuera, del color del texto.
            colorAnimacion.values = [
                colorBase, colorActivo, colorActivo, colorBase
            ]
            colorAnimacion.keyTimes = [
                0,
                max(0, (desde - inicio) / duracion),
                min(1, (hasta - inicio) / duracion),
                1
            ].map(NSNumber.init(value:))
            colorAnimacion.beginTime = inicio
            colorAnimacion.duration = duracion
            colorAnimacion.isRemovedOnCompletion = false
            colorAnimacion.fillMode = .both
            capa.add(colorAnimacion, forKey: "editorcito.highlight.\(palabra.id.uuidString)")

            let opacidad = CAKeyframeAnimation(keyPath: "opacity")
            opacidad.values = [0, 1, 1, 0]
            opacidad.keyTimes = [0, fundido / duracion, 1 - fundido / duracion, 1].map(NSNumber.init(value:))
            opacidad.beginTime = inicio
            opacidad.duration = duracion
            opacidad.isRemovedOnCompletion = false
            opacidad.fillMode = .both
            capa.add(opacidad, forKey: "editorcito.subtitle.\(subtitulo.id.uuidString)")
            contenedor.addSublayer(capa)
        }

        // El fondo del estilo, en una sola capa debajo de las palabras.
        if estilo.opacidadDeFondo > 0 {
            let fondo = CALayer()
            fondo.backgroundColor = colorDe(estilo.fondo).copy(alpha: estilo.opacidadDeFondo)
            fondo.frame = CGRect(x: margen, y: y, width: ancho, height: altura)
            fondo.cornerRadius = 6
            fondo.opacity = 0
            let opacidad = CAKeyframeAnimation(keyPath: "opacity")
            opacidad.values = [0, 1, 1, 0]
            opacidad.keyTimes = [0, fundido / duracion, 1 - fundido / duracion, 1].map(NSNumber.init(value:))
            opacidad.beginTime = inicio
            opacidad.duration = duracion
            opacidad.isRemovedOnCompletion = false
            opacidad.fillMode = .both
            fondo.add(opacidad, forKey: "editorcito.background.\(subtitulo.id.uuidString)")
            contenedor.addSublayer(fondo)
        }
    }

    /// La capa de texto de un subtítulo con el estilo aplicado.
    private static func capaDeTexto(
        _ texto: String,
        estilo: EstiloDeSubtitulo,
        tamano: CGSize
    ) -> CATextLayer {
        let capa = CATextLayer()
        let margen = tamano.width * estilo.margen
        capa.frame = CGRect(x: margen, y: tamano.height * (0.08 + 0.68 * estilo.posicion),
                            width: tamano.width - margen * 2, height: estilo.cuerpo * 1.6)
        capa.string = texto
        capa.fontSize = estilo.cuerpo
        capa.font = NSFont(name: estilo.fuente, size: estilo.cuerpo) ?? NSFont.boldSystemFont(ofSize: estilo.cuerpo)
        capa.alignmentMode = .center
        capa.foregroundColor = colorDe(estilo.color)
        if estilo.opacidadDeFondo > 0 {
            capa.backgroundColor = colorDe(estilo.fondo).copy(alpha: estilo.opacidadDeFondo)
        }
        capa.cornerRadius = 6
        capa.contentsScale = 2
        return capa
    }

    private static func colorDe(_ c: ColorDeSubtitulo) -> CGColor {
        CGColor(red: c.rojo, green: c.verde, blue: c.azul, alpha: 1)
    }

    private static func aplicarOpacidad(
        _ clip: Clip,
        transformacion: TransformacionDeClip,
        a capa: AVMutableVideoCompositionLayerInstruction,
        tramo: (Int64, Int64),
        timebase: Timebase
    ) {
        let nivel = Float(max(0, min(100, transformacion.opacidad)) / 100)
        let (desde, hasta) = tramo

        let finDeEntrada = clip.inicio + clip.entradaFundido
        let inicioDeSalida = clip.fin - clip.salidaFundido

        if clip.entradaFundido > 0 && desde < finDeEntrada {
            let corte = min(hasta, finDeEntrada)
            capa.setOpacityRamp(
                fromStartOpacity: Float(desde - clip.inicio) / Float(clip.entradaFundido) * nivel,
                toEndOpacity: Float(corte - clip.inicio) / Float(clip.entradaFundido) * nivel,
                timeRange: CMTimeRange(start: timebase.tiempo(desde), duration: timebase.tiempo(corte - desde))
            )
        } else if clip.salidaFundido > 0 && hasta > inicioDeSalida {
            let arranque = max(desde, inicioDeSalida)
            capa.setOpacityRamp(
                fromStartOpacity: Float(clip.fin - arranque) / Float(clip.salidaFundido) * nivel,
                toEndOpacity: Float(clip.fin - hasta) / Float(clip.salidaFundido) * nivel,
                timeRange: CMTimeRange(start: timebase.tiempo(arranque), duration: timebase.tiempo(hasta - arranque))
            )
        } else {
            capa.setOpacity(nivel, at: timebase.tiempo(desde))
        }
    }

    private static func aplicarRecorte(
        transformacion: TransformacionDeClip,
        a capa: AVMutableVideoCompositionLayerInstruction,
        medio: MedioResuelto,
        tamano: CGSize,
        en tiempo: CMTime
    ) {
        let t = transformacion
        guard t.recorteIzquierda > 0 || t.recorteDerecha > 0 || t.recorteArriba > 0 || t.recorteAbajo > 0
        else { return }
        // El recorte se expresa en porcentaje sobre el lienzo de salida, que es lo
        // que el usuario ve; traducirlo a píxeles del original sería impredecible
        // en cuanto se mezclan resoluciones.
        let x = tamano.width * t.recorteIzquierda / 100
        let y = tamano.height * t.recorteAbajo / 100
        let ancho = tamano.width * (100 - t.recorteIzquierda - t.recorteDerecha) / 100
        let alto = tamano.height * (100 - t.recorteArriba - t.recorteAbajo) / 100
        guard ancho > 0, alto > 0 else { return }
        capa.setCropRectangle(CGRect(x: x, y: y, width: ancho, height: alto), at: tiempo)
    }

    /// Matriz final del clip: orientación de cámara, encaje en el lienzo y encuadre.
    ///
    /// El orden importa y es el que arregla el caso que más se rompe en la práctica,
    /// un vertical de móvil junto a un horizontal de cámara: primero se endereza la
    /// imagen con lo que dijo la cámara, después se encaja en el lienzo del
    /// proyecto, y solo al final se aplica lo que haya pedido quien monta.
    private static func matrizDe(
        _ clip: Clip,
        transformacion: TransformacionDeClip,
        medio: MedioResuelto,
        tamanoDeSalida: CGSize
    ) -> CGAffineTransform {
        let orientada = medio.tamanoVisible
        guard orientada.width > 0, orientada.height > 0 else { return .identity }

        var matriz = medio.transformacionPreferida
        // `preferredTransform` puede dejar la imagen en coordenadas negativas; se
        // recoloca en el origen antes de escalar.
        let caja = CGRect(origin: .zero, size: medio.tamanoNatural).applying(medio.transformacionPreferida)
        matriz = matriz.concatenating(CGAffineTransform(translationX: -caja.minX, y: -caja.minY))

        // Encaje "contener": la imagen entra entera y sobra fondo, en vez de
        // recortar sin avisar por los lados.
        let factor = min(tamanoDeSalida.width / orientada.width, tamanoDeSalida.height / orientada.height)
        let escalada = CGSize(width: orientada.width * factor, height: orientada.height * factor)
        matriz = matriz
            .concatenating(CGAffineTransform(scaleX: factor, y: factor))
            .concatenating(CGAffineTransform(
                translationX: (tamanoDeSalida.width - escalada.width) / 2,
                y: (tamanoDeSalida.height - escalada.height) / 2
            ))

        let t = transformacion
        if t.esIdentidad { return matriz }

        // Escala y giro alrededor del centro del lienzo, no de la esquina, que es
        // lo que espera cualquiera que haya usado un editor.
        let centroX = tamanoDeSalida.width / 2
        let centroY = tamanoDeSalida.height / 2
        let usuario = CGAffineTransform(translationX: -centroX, y: -centroY)
            .concatenating(CGAffineTransform(scaleX: t.escala / 100, y: t.escala / 100))
            .concatenating(CGAffineTransform(rotationAngle: -t.rotacion * .pi / 180))
            .concatenating(CGAffineTransform(translationX: centroX + t.posicionX, y: centroY - t.posicionY))

        return matriz.concatenating(usuario)
    }

    // MARK: - Tamaño

    /// Resolución del proyecto: la del primer clip de vídeo del montaje.
    ///
    /// Es lo que hacen Premiere y Resolve al crear una secuencia arrastrando un
    /// clip, y evita la pregunta de configuración que nadie sabe contestar antes de
    /// haber visto el material.
    static func tamanoSugerido(_ linea: LineaDeTiempo, medios: [UUID: MedioResuelto]) -> CGSize {
        for pista in linea.pistas where pista.tipo == .video {
            for clip in pista.clips.sorted(by: { $0.inicio < $1.inicio }) {
                if let medio = medios[clip.mediaID], medio.tieneVideo {
                    let t = medio.tamanoVisible
                    if t.width > 0 && t.height > 0 { return t }
                }
            }
        }
        return CGSize(width: 1920, height: 1080)
    }
}
