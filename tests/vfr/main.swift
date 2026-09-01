import AVFoundation
import Foundation

var fallos = 0

func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion {
        print("  ok  \(mensaje)")
    } else {
        print("  FALLO  \(mensaje)")
        fallos += 1
    }
}

func marcasDeVideo(_ asset: AVURLAsset) async -> [Double] {
    guard let pista = try? await asset.loadTracks(withMediaType: .video).first,
          let lector = try? AVAssetReader(asset: asset) else { return [] }
    let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: nil)
    guard lector.canAdd(salida) else { return [] }
    lector.add(salida)
    guard lector.startReading() else { return [] }

    var marcas: [Double] = []
    while lector.status == .reading, let buffer = salida.copyNextSampleBuffer() {
        let tiempo = CMSampleBufferGetPresentationTimeStamp(buffer)
        guard tiempo.isNumeric else { continue }
        marcas.append(tiempo.seconds)
    }
    return marcas.sorted()
}

let argumentos = CommandLine.arguments
guard argumentos.count == 2 else {
    print("uso: pruebaVFR /ruta/al/medio-vfr.mov")
    exit(2)
}

let url = URL(fileURLWithPath: argumentos[1])
let id = UUID()
let medio: MedioResuelto
do {
    medio = try await MedioResuelto.cargar(id: id, url: url)
} catch {
    print("FALLO al cargar el medio: \(error.localizedDescription)")
    exit(1)
}

print("Medio: \(url.lastPathComponent) · VFR=\(medio.esVFR) · \(medio.duracion.seconds) s")
guard medio.esVFR else {
    print("FALLO: el archivo de entrada no fue detectado como VFR")
    exit(1)
}

let timebase = Timebase.p30
let primera = await ConformadorVFR.preparar(medios: [id: medio], para: timebase)
comprobar(primera.fallos.isEmpty, "el conformado termina sin fallos")
comprobar(primera.conformados == 1, "se crea un intermediario CFR")
guard let conformado = primera.medios[id] else {
    print("FALLO: no se devolvió el medio conformado")
    exit(1)
}
comprobar(conformado.estaConformado, "el medio conserva la marca de conformado")
comprobar(conformado.timebaseDeMontaje == timebase, "la caché guarda la base de tiempo objetivo")
comprobar(conformado.url == medio.url, "la referencia documental conserva el archivo original")
comprobar(conformado.assetParaMontaje.url != medio.asset.url, "el montaje usa un asset intermediario distinto")

let marcas = await marcasDeVideo(conformado.assetParaMontaje)
let esperado = timebase.tiempo(1).seconds
var deltas = [Double]()
for indice in 1..<marcas.count {
    let delta = marcas[indice] - marcas[indice - 1]
    if delta > 0 { deltas.append(delta) }
}
let errorMaximo = deltas.map { abs($0 - esperado) }.max() ?? .infinity
comprobar(marcas.count > 30, "la salida contiene suficientes frames para medir")
comprobar(!deltas.isEmpty && errorMaximo < esperado * 0.05, "los PTS de vídeo son CFR a \(timebase.nombre)")

do {
    let video = try await conformado.assetParaMontaje.loadTracks(withMediaType: .video).first
    let audio = try await conformado.assetParaMontaje.loadTracks(withMediaType: .audio).first
    let videoDuration = try await video?.load(.timeRange).duration.seconds ?? 0
    let audioDuration = try await audio?.load(.timeRange).duration.seconds ?? 0
    comprobar(videoDuration > 0, "la salida tiene duración de vídeo válida")
    if audio != nil {
        comprobar(abs(videoDuration - audioDuration) <= esperado, "audio y vídeo quedan alineados dentro de un frame")
    } else {
        print("  -  sin audio: no se mide duración A/V")
    }
} catch {
    print("  FALLO  no se pudieron leer las duraciones de salida: \(error.localizedDescription)")
    fallos += 1
}

var linea = LineaDeTiempo.nueva(timebase: timebase)
let pistaDeVideo = linea.pistas.first { $0.tipo == .video }!.id
let pistaDeAudio = linea.pistas.first { $0.tipo == .audio }!.id
let duracionDeClip = min(90, max(1, timebase.frames(segundos: medio.duracion.seconds)))
let clipDeVideo = Clip(
    mediaID: id,
    nombre: "VFR conformado",
    inicio: 0,
    duracion: duracionDeClip,
    entradaEnOrigen: 0
)
linea.sobrescribir(clipDeVideo, enPista: pistaDeVideo, en: 0)
if medio.tieneAudio {
    let clipDeAudio = Clip(
        mediaID: id,
        nombre: "Audio conformado",
        inicio: 0,
        duracion: duracionDeClip,
        entradaEnOrigen: 0,
        enlace: clipDeVideo.enlace
    )
    linea.sobrescribir(clipDeAudio, enPista: pistaDeAudio, en: 0)
}
let montaje = ConstructorDeMontaje.construir(linea, medios: [id: conformado])
comprobar(montaje.avisos.filter(\.critico).isEmpty, "el constructor no vuelve a advertir sobre el VFR conformado")
comprobar(montaje.composicion.tracks(withMediaType: .video).count == 1, "el montaje usa la pista CFR")
if medio.tieneAudio {
    comprobar(montaje.composicion.tracks(withMediaType: .audio).count == 1, "el montaje conserva el audio del intermediario")
}
if let videoComposition = montaje.composicionDeVideo {
    let generador = AVAssetImageGenerator(asset: montaje.composicion)
    generador.videoComposition = videoComposition
    generador.requestedTimeToleranceBefore = .zero
    generador.requestedTimeToleranceAfter = .zero
    do {
        let (_, tiempoReal) = try await generador.image(at: timebase.tiempo(30))
        comprobar(tiempoReal.isNumeric, "el montaje CFR decodifica un frame real")
    } catch {
        print("  FALLO  el montaje CFR no decodifica: \(error.localizedDescription)")
        fallos += 1
    }
}

let segunda = await ConformadorVFR.preparar(medios: [id: medio], para: timebase)
guard let reutilizada = segunda.medios[id] else {
    print("FALLO: la segunda preparación no devolvió el medio")
    exit(1)
}
comprobar(segunda.fallos.isEmpty, "la segunda preparación reutiliza sin fallos")
comprobar(reutilizada.assetParaMontaje.url == conformado.assetParaMontaje.url, "la segunda preparación reutiliza la misma caché")

if fallos == 0 {
    print("VFR CONFORMADO CORRECTO")
} else {
    print("VFR CONFORMADO ROTO — \(fallos) comprobaciones fallidas")
    exit(1)
}
