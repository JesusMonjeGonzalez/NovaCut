import Foundation

/// Un tramo callado, en segundos del material medido.
struct TramoDeSilencio: Hashable, Sendable {
    var desde: Double
    var hasta: Double

    var duracion: Double { hasta - desde }
}

/// Decide qué tramos están callados a partir de la curva de sonoridad.
///
/// El trabajo difícil ya estaba hecho: el medidor de `Sonoridad.swift` pasa el set oficial
/// de la EBU (Tech 3341/3342) entero. Esto solo decide, y la decisión tiene tres detalles
/// que separan «funciona» de «inservible»:
///
/// - **Umbral relativo a la sonoridad del propio material.** Un pódcast comprimido a −14
///   LUFS y una voz susurrada a −34 tienen que dar los mismos cortes. Con un umbral fijo en
///   dBFS, en uno se corta todo y en el otro nada, y no hay número que valga para los dos.
/// - **Histéresis.** Con un solo umbral, un golpe de un bloque en medio de una pausa la
///   parte en dos y aparecen dos cortes donde debía haber uno.
/// - **Guarda a los lados.** Cortar justo donde la voz cruza el umbral se come la
///   consonante inicial. Es el defecto que hace que un rough cut automático suene mal
///   aunque los tiempos estén bien.
enum DetectorDeSilencios {

    /// Cuánto por debajo de la sonoridad integrada empieza a considerarse silencio.
    ///
    /// 25 LU es holgado a propósito: el suelo de sala de una entrevista normal está entre
    /// 30 y 45 LU por debajo de la voz, y una respiración ronda los 20. Con menos margen se
    /// cortarían las respiraciones, que forman parte de cómo suena hablar.
    static let umbralPorDefecto = -25.0

    /// Cuánto tiene que subir para dar la pausa por terminada. Es la histéresis.
    static let histeresisPorDefecto = 8.0

    /// Tramos callados de la curva.
    ///
    /// - Parameters:
    ///   - curva: sonoridad momentánea, un valor por paso, en LUFS.
    ///   - pasoEnSegundos: separación entre valores de la curva.
    ///   - integrada: sonoridad integrada del material, que fija el umbral.
    ///   - umbralRelativo: LU por debajo de la integrada para entrar en silencio.
    ///   - histeresis: LU que hay que subir por encima del umbral para salir.
    ///   - minimoEnSegundos: por debajo de esto es respirar, no una pausa.
    ///   - guardaEnSegundos: margen que se deja a cada lado del corte.
    static func silencios(
        curva: [Double],
        pasoEnSegundos: Double,
        integrada: Double,
        umbralRelativo: Double = umbralPorDefecto,
        histeresis: Double = histeresisPorDefecto,
        minimoEnSegundos: Double = 0.5,
        guardaEnSegundos: Double = 0.12
    ) -> [TramoDeSilencio] {
        guard !curva.isEmpty, pasoEnSegundos > 0, integrada.isFinite else { return [] }

        let entrar = integrada + umbralRelativo
        let salir = entrar + histeresis

        var crudos: [TramoDeSilencio] = []
        var inicio: Int?

        for (indice, nivel) in curva.enumerated() {
            if inicio == nil {
                // −infinito es silencio digital, no un error: hay que tratarlo como el
                // silencio más rotundo que existe y no descartarlo por no ser finito.
                if nivel < entrar { inicio = indice }
            } else if nivel > salir {
                crudos.append(TramoDeSilencio(
                    desde: Double(inicio!) * pasoEnSegundos,
                    hasta: Double(indice) * pasoEnSegundos
                ))
                inicio = nil
            }
        }
        if let abierto = inicio {
            crudos.append(TramoDeSilencio(
                desde: Double(abierto) * pasoEnSegundos,
                hasta: Double(curva.count) * pasoEnSegundos
            ))
        }

        let duracionTotal = Double(curva.count) * pasoEnSegundos

        return crudos.compactMap { tramo in
            // Sin guarda en los extremos del material: ahí no hay voz que proteger, y
            // dejarla obligaría a quitar a mano el silencio del principio.
            let porDelante = tramo.desde <= 0 ? 0 : guardaEnSegundos
            let porDetras = tramo.hasta >= duracionTotal ? 0 : guardaEnSegundos

            let recortado = TramoDeSilencio(
                desde: tramo.desde + porDelante,
                hasta: tramo.hasta - porDetras
            )
            // Si la guarda se come el silencio, no se corta: mejor dejar una pausa larga
            // que cortar donde hay voz.
            guard recortado.duracion >= minimoEnSegundos else { return nil }
            return recortado
        }
    }
}
