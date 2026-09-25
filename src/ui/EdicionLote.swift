import Foundation

struct ColorLote: Equatable {
    var exposicion = 0.0
    var contraste = 0.0
    var saturacion = 0.0
    var vineta = 0.0
    var desenfoque = 0.0

    static let neutro = ColorLote()
}

struct TransformacionLote: Equatable {
    var posicionX = 0.0
    var posicionY = 0.0
    var escala = 100.0
    var rotacion = 0.0
    var opacidad = 100.0
}

struct AjustesLote: Equatable {
    var transformacion: TransformacionLote?
    var color: ColorLote?
    var ganancia: Double?
    var entradaFundido: Int64?
    var salidaFundido: Int64?
    var etiqueta: EtiquetaDeColor?
    var habilitado: Bool?

    static let vacio = AjustesLote()

    var hayCambios: Bool {
        transformacion != nil || color != nil || ganancia != nil || entradaFundido != nil
            || salidaFundido != nil || etiqueta != nil || habilitado != nil
    }
}

struct InformeLote {
    var selected = 0
    var changed = 0
    var unchanged = 0
    var locked = 0
    var incompatible = 0

    var vacio: Bool { changed == 0 && locked == 0 && incompatible == 0 }
}

func clipsLote(_ timeline: LineaDeTiempo, ids: [UUID]) -> [Clip] {
    var vistos = Set<UUID>()
    return ids.compactMap { id in
        guard vistos.insert(id).inserted else { return nil }
        return timeline.clip(id)
    }
}

func valorComun<T: Equatable>(_ clips: [Clip], _ valor: (Clip) -> T) -> T? {
    guard let primero = clips.first.map(valor) else { return nil }
    return clips.dropFirst().allSatisfy { valor($0) == primero } ? primero : nil
}

func aplicarLote(_ ids: [UUID], ajustes: AjustesLote, en timeline: inout LineaDeTiempo) -> InformeLote {
    var informe = InformeLote()
    var vistos = Set<UUID>()
    let objetivos = ids.filter { vistos.insert($0).inserted }
    informe.selected = objetivos.count
    guard !objetivos.isEmpty, ajustes.hayCambios else { return informe }

    for id in objetivos {
        guard let (pistaIndice, clipIndice) = timeline.indiceDeClip(id) else { continue }
        let pista = timeline.pistas[pistaIndice]
        var clip = pista.clips[clipIndice]
        let original = clip
        let bloqueada = pista.bloqueada
        let anidado = clip.nido != nil || clip.esTitulo
        var soportable = false

        if let transformacion = ajustes.transformacion {
            if !anidado && !clip.esAjuste {
                soportable = true
                clip.transformacion.posicionX = transformacion.posicionX
                clip.transformacion.posicionY = transformacion.posicionY
                clip.transformacion.escala = transformacion.escala
                clip.transformacion.rotacion = transformacion.rotacion
                clip.transformacion.opacidad = transformacion.opacidad
            }
        }
        if let color = ajustes.color {
            if !anidado {
                soportable = true
                clip.color.exposicion = color.exposicion
                clip.color.contraste = color.contraste
                clip.color.saturacion = color.saturacion
                clip.color.vignette = color.vineta
                clip.color.desenfoque = color.desenfoque
            }
        }
        if let ganancia = ajustes.ganancia {
            if pista.tipo == .audio && !anidado {
                soportable = true
                clip.ganancia = min(max(ganancia, -60), 12)
            }
        }
        if let entrada = ajustes.entradaFundido {
            if !anidado {
                soportable = true
                clip.entradaFundido = max(0, min(entrada, clip.duracion / 2))
            }
        }
        if let salida = ajustes.salidaFundido {
            if !anidado {
                soportable = true
                clip.salidaFundido = max(0, min(salida, clip.duracion / 2))
            }
        }
        if let etiqueta = ajustes.etiqueta {
            soportable = true
            clip.etiqueta = etiqueta
        }
        if let habilitado = ajustes.habilitado {
            soportable = true
            clip.habilitado = habilitado
        }

        if bloqueada {
            informe.locked += 1
        } else if !soportable {
            informe.incompatible += 1
        } else if clip == original {
            informe.unchanged += 1
        } else {
            timeline.pistas[pistaIndice].clips[clipIndice] = clip
            informe.changed += 1
        }
    }
    return informe
}

func colorLoteComun(_ clips: [Clip]) -> ColorLote? {
    guard let color = valorComun(clips, { $0.color }) else { return nil }
    return ColorLote(exposicion: color.exposicion, contraste: color.contraste,
                     saturacion: color.saturacion, vineta: color.vignette,
                     desenfoque: color.desenfoque)
}

func transformacionLoteComun(_ clips: [Clip]) -> TransformacionLote? {
    guard let transformacion = valorComun(clips, { $0.transformacion }) else { return nil }
    return TransformacionLote(posicionX: transformacion.posicionX, posicionY: transformacion.posicionY,
                              escala: transformacion.escala, rotacion: transformacion.rotacion,
                              opacidad: transformacion.opacidad)
}
