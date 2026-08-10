import AVFoundation
import CoreGraphics
import Foundation

// Prueba el informe de avisos del constructor: los avisos críticos son los que
// cambian el resultado respecto al montaje, y la exportación debe preguntar
// cuando los hay (un medio offline desaparece del render y el archivo saldría
// con huecos que nadie anunció).

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

let base = Timebase.p25
let id = UUID()
let faltante = UUID()

// Un «medio» con una pista de vídeo vacía: suficiente para que el constructor
// llegue a las decisiones de multicámara, que es lo que se prueba aquí.
func medioFalso(_ id: UUID) -> MedioResuelto {
    let composicion = AVMutableComposition()
    let pista = composicion.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)
    return MedioResuelto(
        id: id, url: URL(fileURLWithPath: "/tmp/prueba.mp4"),
        asset: AVURLAsset(url: URL(fileURLWithPath: "/tmp/prueba.mp4")),
        pistaDeVideo: pista, pistaDeAudio: nil,
        duracion: CMTime(seconds: 30, preferredTimescale: 600),
        tamanoNatural: CGSize(width: 1920, height: 1080),
        transformacionPreferida: .identity, fps: 25
    )
}

func montar(_ clip: Clip, medios: [UUID: MedioResuelto]) -> MontajeRenderizable {
    var linea = LineaDeTiempo.nueva(timebase: base)
    linea.sobrescribir(clip, enPista: linea.pistas.first { $0.tipo == .video }!.id, en: clip.inicio)
    return ConstructorDeMontaje.construir(linea, medios: medios)
}

print("— medio sin archivo —")
// Un clip cuyo medio no está en la mesa: sin aviso crítico, el usuario
// exportaría y recibiría un hueco negro en silencio.
let clip = Clip(
    mediaID: id, nombre: "Perdido", inicio: 0,
    duracion: base.frames(segundos: 2.0), entradaEnOrigen: 0
)
let render = montar(clip, medios: [:])
comprobar(render.avisos.contains { $0.mensaje.contains("no encuentra su archivo") },
          "un clip sin archivo produce el aviso de medio perdido")
comprobar(render.avisos.allSatisfy(\.critico) && !render.avisos.isEmpty,
          "ese aviso es crítico (cambia el resultado)")
comprobar(render.composicion.tracks(withMediaType: .video).isEmpty,
          "el clip no entra en la composición: el hueco es real")

print("— multicámara sin soporte —")
// Retime en un clip multicámara: el constructor lo declara sin soporte y el
// clip no entra en el render — crítico, no un detalle.
let grupoID = UUID()
var retimado = Clip(
    mediaID: id, nombre: "Entrevista", inicio: 0,
    duracion: base.frames(segundos: 2.0), entradaEnOrigen: 0, velocidad: 2
)
retimado.multicam = MulticamDeClip(grupoID: grupoID, inicial: id, cortes: [])
let renderRetime = montar(retimado, medios: [id: medioFalso(id)])
comprobar(renderRetime.avisos.contains { $0.mensaje.contains("retime multicámara") && $0.critico },
          "el retime multicámara sin soporte es un aviso crítico")

// Grupo que no existe: el ángulo no se puede resolver.
var conGrupoFalso = Clip(
    mediaID: id, nombre: "Entrevista", inicio: 0,
    duracion: base.frames(segundos: 2.0), entradaEnOrigen: 0
)
conGrupoFalso.multicam = MulticamDeClip(grupoID: grupoID, inicial: id, cortes: [])
let renderSinGrupo = montar(conGrupoFalso, medios: [id: medioFalso(id)])
comprobar(renderSinGrupo.avisos.contains { $0.mensaje.contains("no encuentra su grupo") && $0.critico },
          "un grupo multicámara desaparecido es un aviso crítico")

// Ángulo offline: el corte se queda sin material.
var linea = LineaDeTiempo.nueva(timebase: base)
linea.gruposMulticam = [GrupoMulticam(id: grupoID, nombre: "Entrevista", mediaIDs: [id, faltante])]
var conCorte = Clip(
    mediaID: id, nombre: "Entrevista", inicio: 0,
    duracion: base.frames(segundos: 2.0), entradaEnOrigen: 0
)
conCorte.multicam = MulticamDeClip(
    grupoID: grupoID, inicial: id,
    cortes: [CorteDeAngulo(frame: base.frames(segundos: 1.0), mediaID: faltante)]
)
linea.sobrescribir(conCorte, enPista: linea.pistas.first { $0.tipo == .video }!.id, en: 0)
let renderAngulo = ConstructorDeMontaje.construir(linea, medios: [id: medioFalso(id)])
comprobar(renderAngulo.avisos.contains { $0.mensaje.contains("no encuentra el ángulo") && $0.critico },
          "un ángulo sin archivo es un aviso crítico")

print("— la decisión de exportación —")
// Lo que mira la cola de exportación: con críticos pregunta, sin críticos no.
comprobar(!render.avisos.filter(\.critico).isEmpty, "la exportación detecta críticos y debe preguntar")
comprobar(render.avisos.filter { !$0.critico }.isEmpty,
          "un montaje sin archivos no produce avisos de ajuste — son otra cosa")

if fallos == 0 {
    print("AVISOS CORRECTO")
} else {
    print("AVISOS ROTO — \(fallos) fallos")
    exit(1)
}
