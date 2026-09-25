import Foundation

struct PruebaLote {
    static func main() {
        let timeline = LineaDeTiempo.nueva(timebase: .p25)
        let videoA = Clip(mediaID: UUID(), nombre: "A", inicio: 0, duracion: 50, entradaEnOrigen: 10)
        let videoB = Clip(mediaID: UUID(), nombre: "B", inicio: 60, duracion: 30, entradaEnOrigen: 20)
        var montage = timeline
        montage.pistas[1].clips = [videoA, videoB]
        let ids = [videoA.id, videoB.id]
        let comun = valorComun(clipsLote(montage, ids: ids), { $0.transformacion })
        precondition(comun == .identidad)
        var informe = aplicarLote(ids, ajustes: AjustesLote(
            transformacion: TransformacionLote(posicionX: 20, posicionY: -10, escala: 125, rotacion: 4, opacidad: 80),
            color: ColorLote(exposicion: 12, contraste: 4, saturacion: 8, vineta: 0.2, desenfoque: 0.1),
            entradaFundido: 100,
            salidaFundido: 100
        ), en: &montage)
        precondition(informe.changed == 2)
        precondition(montage.clip(videoA.id)?.transformacion.posicionX == 20)
        precondition(montage.clip(videoB.id)?.color.exposicion == 12)
        precondition(montage.clip(videoA.id)?.entradaFundido == 25)
        precondition(montage.clip(videoA.id)?.inicio == 0)
        precondition(montage.clip(videoA.id)?.entradaEnOrigen == 10)

        var bloqueada = montage
        bloqueada.pistas[1].bloqueada = true
        let antes = bloqueada.clip(videoA.id)
        informe = aplicarLote([videoA.id], ajustes: AjustesLote(ganancia: 3), en: &bloqueada)
        precondition(informe.locked == 1)
        precondition(bloqueada.clip(videoA.id) == antes)

        let idRepetido = videoA.id
        precondition(clipsLote(montage, ids: [idRepetido, idRepetido]).count == 1)
        print("OK: edición por lotes")
    }
}

PruebaLote.main()
