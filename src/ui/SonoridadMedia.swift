import AVFoundation
import CoreMedia
import Foundation

enum ErrorDeSonoridadMedia: Error {
    case sinAudio
    case lecturaFallida(String)
}

/// Lee el audio ya mezclado de un montaje y lo mide con `MedidorDeSonoridad`.
///
/// El medidor no toca AVFoundation a propósito (se verifica con `swiftc`
/// contra las señales de la EBU); aquí vive la otra mitad: pasar del montaje a
/// las muestras que oiría el espectador. El camino es `AVAssetReader` con
/// `AVAssetReaderAudioMixOutput`, que aplica durante la lectura las mismas
/// rampas de volumen, fundidos y ducking que `ConstructorDeMontaje` dejó en el
/// `audioMix` — medir aquí es medir lo que va a exportarse.
///
/// No se fuerza ni la frecuencia de muestreo ni el número de canales: el lector
/// entrega el formato nativo y el medidor deduce el pesado K para esa
/// frecuencia. La disposición de canales se queda en el defecto por conteo
/// (estéreo y mono a uno; cinco o más asumen el orden 5.0), que es lo único
/// que se puede saber sin pedir el layout a AVFoundation; la mezcla con EQ y
/// compañía tendrá que traer la disposición real consigo.
enum SonoridadMedia {

    /// `timeRange` limita la medición al tramo que va a exportarse: sin él, un
    /// montaje con rango de trabajo mediría el material fuera del rango y la
    /// normalización decidiría con una señal que no es la que se entrega.
    static func medir(_ render: MontajeRenderizable, timeRange: CMTimeRange? = nil) throws -> MedidaDeSonoridad {
        try medirConCurva(render, timeRange: timeRange).medida
    }

    /// La medida **y** la curva momentánea con la que se calculó.
    ///
    /// La detección de silencios necesita la curva, y sacarla del mismo medidor que la
    /// integrada evita tener dos formas de medir que pudieran no coincidir: el umbral de
    /// silencio es relativo a la integrada, así que las dos cifras tienen que venir de la
    /// misma pasada sobre las mismas muestras.
    static func medirConCurva(
        _ render: MontajeRenderizable,
        timeRange: CMTimeRange? = nil
    ) throws -> (medida: MedidaDeSonoridad, curva: [Double]) {
        try medirConCurva(
            composicion: render.composicion,
            mezcla: render.mezclaDeAudio,
            timeRange: timeRange
        )
    }

    /// La medición sobre los objetos que cruzan hilos (composición y mezcla), sin
    /// depender del struct `MontajeRenderizable`.
    ///
    /// Es la vía que usa el segundo plano: la lectura completa puede durar minutos
    /// en un montaje largo, y no puede bloquear el hilo de la UI. Como el lector
    /// aplica el mismo `audioMix` que la exportación, medir aquí es medir lo que
    /// va a exportarse.
    static func medir(
        composicion: AVMutableComposition,
        mezcla: AVMutableAudioMix?,
        timeRange: CMTimeRange? = nil
    ) throws -> MedidaDeSonoridad {
        try medirConCurva(composicion: composicion, mezcla: mezcla, timeRange: timeRange).medida
    }

    static func medirConCurva(
        composicion: AVMutableComposition,
        mezcla: AVMutableAudioMix?,
        timeRange: CMTimeRange? = nil
    ) throws -> (medida: MedidaDeSonoridad, curva: [Double]) {
        let pistas = composicion.tracks(withMediaType: .audio)
        guard !pistas.isEmpty else { throw ErrorDeSonoridadMedia.sinAudio }

        let lector = try AVAssetReader(asset: composicion)
        if let timeRange { lector.timeRange = timeRange }
        let ajustes: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsNonInterleaved: false,
        ]
        let salida = AVAssetReaderAudioMixOutput(audioTracks: pistas, audioSettings: ajustes)
        salida.audioMix = mezcla
        guard lector.canAdd(salida) else { throw ErrorDeSonoridadMedia.sinAudio }
        lector.add(salida)
        guard lector.startReading() else {
            throw ErrorDeSonoridadMedia.lecturaFallida(lector.error?.localizedDescription ?? "sin motivo")
        }

        var medidor: MedidorDeSonoridad?
        defer { lector.cancelReading() }

        while let buffer = salida.copyNextSampleBuffer() {
            let marcos = CMSampleBufferGetNumSamples(buffer)
            guard marcos > 0,
                  let descripcion = CMSampleBufferGetFormatDescription(buffer),
                  let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(descripcion)
            else { continue }

            let canales = Int(asbd.pointee.mChannelsPerFrame)
            let frecuencia = asbd.pointee.mSampleRate
            guard canales > 0, frecuencia > 0 else { continue }

            // El formato del flujo no cambia a mitad de lectura; el medidor se
            // crea con el primero y se reutiliza.
            if medidor == nil {
                medidor = MedidorDeSonoridad(
                    frecuencia: frecuencia,
                    canales: canales,
                    pesos: Self.pesosDeLaDisposicion(descripcion, canales: canales)
                )
            }

            guard let bloque = CMSampleBufferGetDataBuffer(buffer) else { continue }
            let bytes = CMBlockBufferGetDataLength(bloque)
            guard bytes >= marcos * canales * MemoryLayout<Float>.size else { continue }

            var muestras = [Float](repeating: 0, count: marcos * canales)
            let estado = muestras.withUnsafeMutableBytes { destino in
                CMBlockBufferCopyDataBytes(
                    bloque,
                    atOffset: 0,
                    dataLength: bytes,
                    destination: destino.baseAddress!
                )
            }
            guard estado == kCMBlockBufferNoErr else { continue }
            medidor?.procesar(entrelazado: muestras)
        }

        guard let medidor else { throw ErrorDeSonoridadMedia.sinAudio }
        // La curva se pide antes de finalizar: `finalizar` vacía la cola del pico y no
        // debe quedar duda de sobre qué muestras se calculó cada cosa.
        let curva = medidor.curvaMomentanea()
        return (medidor.finalizar(), curva)
    }

    /// Pesos BS.1770-4 deducidos de la disposición real de canales del flujo.
    ///
    /// El peso del surround (+1,5 dB) depende de dónde esté el surround, y eso
    /// solo lo sabe la disposición: un WAV 5.1 ordena L R C LFE Ls Rs y una
    /// pista de cine 5.0 ordena L R C Ls Rs. Con descripciones de canal o un tag
    /// de layout conocido se asigna cada peso al canal correcto —el LFE no
    /// contribuye en BS.1770—. Sin ninguna de las dos, `nil`, y el medidor cae
    /// en su fallback por conteo (la convención 5.0 del estándar).
    static func pesosDeLaDisposicion(
        _ descripcion: CMFormatDescription?,
        canales: Int
    ) -> [Double]? {
        guard let descripcion else { return nil }
        var tamanoDelLayout = 0
        guard let layout = CMAudioFormatDescriptionGetChannelLayout(descripcion, sizeOut: &tamanoDelLayout) else { return nil }

        if let fijos = pesosDeTagConocido(layout.pointee.mChannelLayoutTag, canales: canales) {
            return fijos
        }

        let contadas = Int(layout.pointee.mNumberChannelDescriptions)
        guard contadas == canales, canales > 0 else { return nil }
        var pesos = [Double](repeating: 1.0, count: canales)
        // El array de descripciones sigue a los campos fijos del layout en la
        // memoria que devuelve CoreMedia: se camina con aritmética de punteros,
        // nunca sobre una copia del struct.
        let offset = MemoryLayout<AudioChannelLayout>.offset(of: \AudioChannelLayout.mChannelDescriptions) ?? 0
        let descripciones = UnsafeRawPointer(layout)
            .advanced(by: offset)
            .assumingMemoryBound(to: AudioChannelDescription.self)
        for i in 0..<canales {
            switch descripciones[i].mChannelLabel {
            case kAudioChannelLabel_LFEScreen, kAudioChannelLabel_LFE2:
                pesos[i] = 0
            case kAudioChannelLabel_LeftSurround, kAudioChannelLabel_RightSurround,
                 kAudioChannelLabel_CenterSurround,
                 kAudioChannelLabel_LeftSurroundDirect, kAudioChannelLabel_RightSurroundDirect,
                 kAudioChannelLabel_RearSurroundLeft, kAudioChannelLabel_RearSurroundRight:
                pesos[i] = 1.41
            default:
                pesos[i] = 1.0
            }
        }
        return pesos
    }

    /// Los órdenes fijos de las convenciones más comunes, que el tag declara sin
    /// descripciones. Órdenes documentados de CoreAudio:
    /// A y AudioUnit: L R C LFE Ls Rs · B: L R C Ls Rs LFE · C: L C R Ls Rs LFE ·
    /// D: L C R LFE Ls Rs · 7.1 A: L R C LFE Ls Rs Lc Rc.
    private static func pesosDeTagConocido(_ tag: AudioChannelLayoutTag, canales: Int) -> [Double]? {
        switch tag {
        case kAudioChannelLayoutTag_MPEG_5_1_A,
             kAudioChannelLayoutTag_MPEG_5_1_D,
             kAudioChannelLayoutTag_AudioUnit_5_1:
            return canales == 6 ? [1, 1, 1, 0, 1.41, 1.41] : nil
        case kAudioChannelLayoutTag_MPEG_5_1_B,
             kAudioChannelLayoutTag_MPEG_5_1_C:
            return canales == 6 ? [1, 1, 1, 1.41, 1.41, 0] : nil
        case kAudioChannelLayoutTag_MPEG_7_1_A:
            return canales == 8 ? [1, 1, 1, 0, 1.41, 1.41, 1, 1] : nil
        default:
            return nil
        }
    }
}
