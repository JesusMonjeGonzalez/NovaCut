import AVFoundation
import Foundation
import Vision

/// Detección del sujeto para el reencuadre vertical automático.
///
/// La promesa del punto 7: Premiere Auto Reframe te da una caja negra, aquí sale
/// una lista de keyframes de posición normales que se pueden tocar, borrar o
/// rehacer. Vision corre en el dispositivo y sin subir nada, que es el argumento
/// de la casa.
///
/// El seguimiento es de una sola pasada: se detecta la cara en el primer frame y
/// se le sigue con `VNTrackObjectRequest` —el detector de caras por frame sería
/// más preciso pero decodificar y re-descubrir cada muestra cuesta el doble, y
/// para un encuadre vertical no hace falta. Cuando el seguimiento se pierde, se
/// vuelve a detectar y se retoma.
enum DetectorDeSujeto {

    struct Muestra {
        /// Frame del clip al que corresponde la muestra.
        var frame: Int64
        /// Centro del sujeto en coordenadas normalizadas (0-1), con la Y hacia
        /// arriba como en el vídeo.
        var centro: (x: Double, y: Double)
        /// Proporción de la imagen que ocupa el sujeto; sirve para decidir la
        /// escala del encuadre.
        var tamano: Double
    }

    /// Muestrea el medio y devuelve la trayectoria del sujeto cada [paso] frames.
    ///
    /// El tiempo de muestreo sale del `timebase` del proyecto, no de una cadencia
    /// inventada: con material a 25/50/60 o VFR, muestrear a 30 fps fijos coloca
    /// cada keyframe en el instante equivocado del medio. `Muestra.frame` es
    /// relativo al clip, en frames de proyecto, que es lo que esperan los
    /// `ClipKeyframe`. Para VFR la cadencia nominal es una aproximación honesta:
    /// Vision sigue al sujeto, no a los fotogramas.
    static func rastrear(
        medio: MedioResuelto,
        duracionDelClip: Int64,
        entradaEnOrigen: Int64,
        timebase: Timebase,
        paso: Int64 = 30
    ) async -> [Muestra] {
        guard medio.tieneVideo else { return [] }
        let generador = AVAssetImageGenerator(asset: medio.asset)
        generador.appliesPreferredTrackTransform = true
        // Resolución de análisis: 360 px de ancho basta para seguir una cara y
        // decodificar cuesta mucho menos que a resolución de proyecto.
        generador.maximumSize = CGSize(width: 360, height: 640)

        var salida: [Muestra] = []
        var seguimiento: VNTrackObjectRequest?
        var ultimoCentro: (x: Double, y: Double) = (0.5, 0.5)
        var ultimoTamano: Double = 0.1

        var frame = entradaEnOrigen
        while frame < entradaEnOrigen + duracionDelClip {
            let segundo = timebase.segundos(frame)
            guard let cg = try? await generador.image(
                at: CMTime(seconds: segundo, preferredTimescale: 600)
            ).image else { break }
            let handler = VNImageRequestHandler(cgImage: cg, options: [:])

            // Se detecta o se sigue según el estado del seguimiento anterior.
            if let actual = seguimiento {
                try? handler.perform([actual])
                if let rect = (actual.results as? [VNDetectedObjectObservation])?.first?.boundingBox {
                    let centro = (x: Double(rect.midX), y: 1 - Double(rect.midY))
                    ultimoCentro = centro
                    ultimoTamano = Double(rect.width)
                    salida.append(Muestra(frame: frame - entradaEnOrigen, centro: centro, tamano: Double(rect.width)))
                } else {
                    seguimiento = nil
                    salida.append(Muestra(frame: frame - entradaEnOrigen, centro: ultimoCentro, tamano: ultimoTamano))
                }
            } else {
                let deteccion = VNDetectFaceRectanglesRequest()
                try? handler.perform([deteccion])
                // `VNFaceObservation` es una `VNDetectedObjectObservation`, así
                // que el resultado sirve directamente para el seguimiento.
                if let observacion = deteccion.results?.first {
                    let rect = observacion.boundingBox
                    let centro = (x: Double(rect.midX), y: 1 - Double(rect.midY))
                    ultimoCentro = centro
                    ultimoTamano = Double(rect.width)
                    salida.append(Muestra(frame: frame - entradaEnOrigen, centro: centro, tamano: Double(rect.width)))
                    let track = VNTrackObjectRequest(detectedObjectObservation: observacion)
                    track.trackingLevel = .accurate
                    seguimiento = track
                } else {
                    salida.append(Muestra(frame: frame - entradaEnOrigen, centro: ultimoCentro, tamano: ultimoTamano))
                }
            }
            frame += paso
        }
        return salida
    }

    /// Suaviza la trayectoria con una media móvil: el encuadre que salta de frame
    /// en frame se siente peor que uno ligeramente retrasado.
    static func suavizar(_ muestras: [Muestra], ventana: Int = 3) -> [Muestra] {
        guard muestras.count > 1 else { return muestras }
        let radio = max(1, ventana / 2)
        return muestras.enumerated().map { indice, muestra in
            let desde = max(0, indice - radio)
            let hasta = min(muestras.count - 1, indice + radio)
            let vecinas = muestras[desde...hasta]
            let centroX = vecinas.map { $0.centro.x }.reduce(0, +) / Double(vecinas.count)
            let centroY = vecinas.map { $0.centro.y }.reduce(0, +) / Double(vecinas.count)
            return Muestra(frame: muestra.frame, centro: (centroX, centroY), tamano: muestra.tamano)
        }
    }
}

/// Convierte la trayectoria del sujeto en keyframes de posición del clip.
///
/// El encuadre vertical (9:16) muestra una ventana estrecha del material. La
/// posición vertical del clip en el lienzo es justo lo que mueve esa ventana
/// sobre el original: `posicionY` del clip. El resultado es una lista de
/// `ClipKeyframe` normales —editables, borrables y rehechos como cualquier otra
/// animación del proyecto—.
enum ReframeVertical {

    /// Cómo se muestra el sujeto en el encuadre vertical.
    struct Encuadre {
        var posicionY: Double
        var escala: Double
    }

    /// Convierte muestras (centro normalizado del sujeto) en keyframes.
    ///
    /// La escala sale del tamaño del sujeto: si ocupa poco de la imagen, se
    /// acerca más; si ya llena el encuadre, se deja. La posición se calcula para
    /// que el centro del sujeto caiga en el centro del lienzo vertical.
    static func keyframes(de muestras: [DetectorDeSujeto.Muestra]) -> [ClipKeyframe] {
        guard !muestras.isEmpty else { return [] }
        // Tamaño de referencia del sujeto: 30 % del ancho de la imagen. Menos
        // quiere decir que hay que acercar; más, que ya está cerca.
        let tamanoDeReferencia = 0.30
        return muestras.map { muestra in
            // La escala es porcentaje: un sujeto del 10 % necesita 300 % de zoom,
            // uno del 30 % (el de referencia) se queda en 100.
            let escala = min(max(tamanoDeReferencia / max(muestra.tamano, 0.05) * 100, 100), 250)
            // La posición se mide en píxeles desde el centro del lienzo; el
            // centro normalizado del sujeto (0-1) se traduce a desplazamiento.
            let posicionY = (muestra.centro.y - 0.5) * 200
            let transformacion = TransformacionDeClip(
                posicionX: 0,
                posicionY: posicionY,
                escala: escala
            )
            return ClipKeyframe(frame: muestra.frame, transformacion: transformacion, ganancia: 0)
        }
    }

    /// Aplica el reencuadre a un clip: reemplaza sus keyframes de transformación.
    ///
    /// Se usa la transformación del clip como base y solo se anima la posición y
    /// la escala; el giro, la opacidad y el recorte del usuario se conservan.
    static func aplicar(
        a clip: Clip,
        muestras: [DetectorDeSujeto.Muestra],
        transformacionBase: TransformacionDeClip
    ) -> Clip {
        let claves = keyframes(de: muestras).map { clave in
            var c = clave
            c.transformacion.rotacion = transformacionBase.rotacion
            c.transformacion.opacidad = transformacionBase.opacidad
            c.transformacion.recorteIzquierda = transformacionBase.recorteIzquierda
            c.transformacion.recorteDerecha = transformacionBase.recorteDerecha
            c.transformacion.recorteArriba = transformacionBase.recorteArriba
            c.transformacion.recorteAbajo = transformacionBase.recorteAbajo
            return c
        }
        return clip.withKeyframes(claves)
    }
}
