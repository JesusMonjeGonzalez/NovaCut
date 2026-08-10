import AVFoundation
import CoreGraphics
import Foundation

// EXPERIMENTO: ¿AVFoundation compone las capas cuando hay un compositor custom?
//
// La pregunta: `CompositorDeColor.startRequest` coge `sourceTrackIDs.first`,
// le aplica color y devuelve el frame. Con dos pistas de vídeo y opacidad al
// 50 %, ¿el resultado tiene las dos capas compuestas (mezcla magenta) o solo la
// primera (azul puro)? La respuesta decide si adjustment layers y LUT pueden
// apoyarse en el compositor actual o hay que rehacerlo.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    print("\(condicion ? "  ok  " : "  FALLO ")\(mensaje)")
    if !condicion { fallos += 1 }
}

let carpeta = FileManager.default.temporaryDirectory.appendingPathComponent("editorcito-experimento")
try? FileManager.default.removeItem(at: carpeta)
try FileManager.default.createDirectory(at: carpeta, withIntermediateDirectories: true)

func generarVideoColor(nombre: String, rojo: CGFloat, verde: CGFloat, azul: CGFloat) async throws -> URL {
    let url = carpeta.appendingPathComponent(nombre)
    let escritor = try AVAssetWriter(outputURL: url, fileType: .m4v)
    let ajustes: [String: Any] = [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 320,
        AVVideoHeightKey: 180,
    ]
    let entrada = AVAssetWriterInput(mediaType: .video, outputSettings: ajustes)
    let adaptador = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: entrada, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: 320,
        kCVPixelBufferHeightKey as String: 180,
    ])
    escritor.add(entrada)
    guard escritor.startWriting() else { throw NSError(domain: "exp", code: 1) }
    escritor.startSession(atSourceTime: .zero)
    let fps = 25
    let frames = 50
    while !entrada.isReadyForMoreMediaData { usleep(1000) }
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32BGRA,
                        [kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180] as CFDictionary, &buffer)
    guard let buffer else { throw NSError(domain: "exp", code: 2) }
    CVPixelBufferLockBaseAddress(buffer, [])
    let base = CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to: UInt8.self)
    let bgra: (CGFloat, CGFloat, CGFloat) = (azul * 255, verde * 255, rojo * 255)
    for y in 0..<180 {
        for x in 0..<320 {
            let i = (y * 320 + x) * 4
            base[i] = UInt8(bgra.0); base[i + 1] = UInt8(bgra.1); base[i + 2] = UInt8(bgra.2); base[i + 3] = 255
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer, [])
    for f in 0..<frames {
        while !entrada.isReadyForMoreMediaData { usleep(1000) }
        adaptador.append(buffer, withPresentationTime: CMTime(value: Int64(f), timescale: Int32(fps)))
    }
    entrada.markAsFinished()
    await escritor.finishWriting()
    guard escritor.status == .completed else { throw escritor.error ?? NSError(domain: "exp", code: 3) }
    return url
}

let roja = try await generarVideoColor(nombre: "roja.m4v", rojo: 1, verde: 0, azul: 0)
let azul = try await generarVideoColor(nombre: "azul.m4v", rojo: 0, verde: 0, azul: 1)

let assetRojo = AVURLAsset(url: roja)
let assetAzul = AVURLAsset(url: azul)
let pistaRoja = try await assetRojo.loadTracks(withMediaType: .video).first!
let pistaAzul = try await assetAzul.loadTracks(withMediaType: .video).first!

let composicion = AVMutableComposition()
let t1 = composicion.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
let t2 = composicion.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
try t1.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaRoja, at: .zero)
try t2.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaAzul, at: .zero)

let vc = AVMutableVideoComposition()
vc.renderSize = CGSize(width: 320, height: 180)
vc.frameDuration = CMTime(value: 1, timescale: 25)
let instruccion = AVMutableVideoCompositionInstruction()
instruccion.timeRange = CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600))
let capaRoja = AVMutableVideoCompositionLayerInstruction(assetTrack: t1)
let capaAzul = AVMutableVideoCompositionLayerInstruction(assetTrack: t2)
capaAzul.setOpacity(0.5, at: .zero)
instruccion.layerInstructions = [capaAzul, capaRoja]
vc.instructions = [instruccion]

print("== Sin compositor custom (AVFoundation compone) ==")
let generador1 = AVAssetImageGenerator(asset: composicion)
generador1.videoComposition = vc
generador1.requestedTimeToleranceBefore = .zero
generador1.requestedTimeToleranceAfter = .zero
if let cg = try? await generador1.image(at: CMTime(seconds: 1, preferredTimescale: 600)).image {
    let datos = cg.dataProvider!.data! as Data
    let bytes = [UInt8](datos)
    let i = (90 * 320 + 160) * 4
    let (b, g, r) = (bytes[i], bytes[i + 1], bytes[i + 2])
    print("  pixel central: R=\(r) G=\(g) B=\(b) (esperado magenta ≈ R128 G0 B128)")
    comprobar(r > 100 && b > 100, "sin compositor custom, las dos capas se mezclan")
}

print("== Con CompositorDeColor (el compositor de la app) ==")
vc.customVideoCompositorClass = CompositorDeColor.self
let generador2 = AVAssetImageGenerator(asset: composicion)
generador2.videoComposition = vc
generador2.requestedTimeToleranceBefore = .zero
generador2.requestedTimeToleranceAfter = .zero
if let cg = try? await generador2.image(at: CMTime(seconds: 1, preferredTimescale: 600)).image {
    let datos = cg.dataProvider!.data! as Data
    let bytes = [UInt8](datos)
    let i = (90 * 320 + 160) * 4
    let (b, g, r) = (bytes[i], bytes[i + 1], bytes[i + 2])
    print("  pixel central: R=\(r) G=\(g) B=\(b) (si solo usa la primera pista: azul puro R0 B255)")
    comprobar(r > 100 && b > 100, "con el compositor de color, las dos capas se mezclan")
}

try? FileManager.default.removeItem(at: carpeta)
if fallos == 0 {
    print("EXPERIMENTO: composición multicapa intacta")
} else {
    print("EXPERIMENTO: las capas se pierden con el compositor — bug crítico")
    exit(1)
}
