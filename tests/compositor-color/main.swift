import AVFoundation
import CoreGraphics
import Foundation

// Verifica que `CompositorDeColor` compone las capas como el compositor nativo
// de AVFoundation y que la cadena de color se aplica sobre el compuesto.
//
// El compositor custom no es opcional: con un `customVideoCompositorClass`
// activo, AVFoundation entrega los frames en bruto y el compositor lo hace
// todo. El compositor viejo tomaba solo la primera pista y cualquier clip con
// color en el montaje hacía desaparecer las capas inferiores de todos los
// segmentos —el bug que este test existe para que no vuelva.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    print("\(condicion ? "  ok  " : "  FALLO ")\(mensaje)")
    if !condicion { fallos += 1 }
}

let carpeta = FileManager.default.temporaryDirectory.appendingPathComponent("editorcito-compositor-color")
try? FileManager.default.removeItem(at: carpeta)
try FileManager.default.createDirectory(at: carpeta, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: carpeta) }

func generarVideoColor(nombre: String, rojo: CGFloat, verde: CGFloat, azul: CGFloat) async throws -> URL {
    let url = carpeta.appendingPathComponent(nombre)
    let escritor = try AVAssetWriter(outputURL: url, fileType: .m4v)
    let entrada = AVAssetWriterInput(mediaType: .video, outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 320,
        AVVideoHeightKey: 180,
    ])
    let adaptador = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: entrada, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: 320,
        kCVPixelBufferHeightKey as String: 180,
    ])
    escritor.add(entrada)
    guard escritor.startWriting() else { throw NSError(domain: "test", code: 1) }
    escritor.startSession(atSourceTime: .zero)
    while !entrada.isReadyForMoreMediaData { usleep(1000) }
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32BGRA,
                        [kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180] as CFDictionary, &buffer)
    guard let buffer else { throw NSError(domain: "test", code: 2) }
    CVPixelBufferLockBaseAddress(buffer, [])
    let base = CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to: UInt8.self)
    for y in 0..<180 {
        for x in 0..<320 {
            let i = (y * 320 + x) * 4
            base[i] = UInt8(azul * 255); base[i + 1] = UInt8(verde * 255)
            base[i + 2] = UInt8(rojo * 255); base[i + 3] = 255
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer, [])
    for f in 0..<50 {
        while !entrada.isReadyForMoreMediaData { usleep(1000) }
        adaptador.append(buffer, withPresentationTime: CMTime(value: Int64(f), timescale: 25))
    }
    entrada.markAsFinished()
    await escritor.finishWriting()
    guard escritor.status == .completed else { throw escritor.error ?? NSError(domain: "test", code: 3) }
    return url
}

struct Pixel: Equatable {
    let r: Int, g: Int, b: Int
    func distancia(_ otro: Pixel) -> Int {
        abs(r - otro.r) + abs(g - otro.g) + abs(b - otro.b)
    }
}

func pixelDe(_ cg: CGImage, en x: Int, _ y: Int) -> Pixel {
    let datos = cg.dataProvider!.data! as Data
    let bytes = [UInt8](datos)
    let i = (y * cg.width + x) * 4
    return Pixel(r: Int(bytes[i + 2]), g: Int(bytes[i + 1]), b: Int(bytes[i]))
}

func frameDe(_ composicion: AVComposition, con composicionDeVideo: AVVideoComposition?, en segundo: Double) async -> CGImage? {
    let generador = AVAssetImageGenerator(asset: composicion)
    generador.videoComposition = composicionDeVideo
    generador.requestedTimeToleranceBefore = .zero
    generador.requestedTimeToleranceAfter = .zero
    return try? await generador.image(at: CMTime(seconds: segundo, preferredTimescale: 600)).image
}

func comparar(
    _ nativo: CGImage, _ custom: CGImage,
    en puntos: [(Int, Int)], _ mensaje: String, tolerancia: Int = 8
) {
    for (x, y) in puntos {
        let a = pixelDe(nativo, en: x, y)
        let b = pixelDe(custom, en: x, y)
        comprobar(a.distancia(b) <= tolerancia,
                  "\(mensaje) en (\(x),\(y)): nativo (\(a.r),\(a.g),\(a.b)) vs custom (\(b.r),\(b.g),\(b.b))")
    }
}

let roja = try await generarVideoColor(nombre: "roja.m4v", rojo: 1, verde: 0, azul: 0)
let azul = try await generarVideoColor(nombre: "azul.m4v", rojo: 0, verde: 0, azul: 1)
let assetRojo = AVURLAsset(url: roja)
let assetAzul = AVURLAsset(url: azul)
let pistaRoja = try await assetRojo.loadTracks(withMediaType: .video).first!
let pistaAzul = try await assetAzul.loadTracks(withMediaType: .video).first!

func montajeConDosCapas() -> (AVMutableComposition, AVMutableVideoCompositionInstruction, AVMutableVideoCompositionLayerInstruction, AVMutableVideoCompositionLayerInstruction) {
    let composicion = AVMutableComposition()
    let t1 = composicion.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
    let t2 = composicion.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
    try! t1.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaRoja, at: .zero)
    try! t2.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaAzul, at: .zero)
    let instruccion = AVMutableVideoCompositionInstruction()
    instruccion.timeRange = CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600))
    instruccion.backgroundColor = CGColor(red: 0, green: 0, blue: 0, alpha: 1)
    let capaRoja = AVMutableVideoCompositionLayerInstruction(assetTrack: t1)
    let capaAzul = AVMutableVideoCompositionLayerInstruction(assetTrack: t2)
    instruccion.layerInstructions = [capaAzul, capaRoja]
    return (composicion, instruccion, capaRoja, capaAzul)
}

func composicionDeVideo(con instruccion: AVMutableVideoCompositionInstruction, color: ColorDeClip? = nil, custom: Bool) -> AVMutableVideoComposition {
    let vc = AVMutableVideoComposition()
    vc.renderSize = CGSize(width: 320, height: 180)
    vc.frameDuration = CMTime(value: 1, timescale: 25)
    if color == nil {
        vc.instructions = [instruccion]
    } else {
        let conColor = InstruccionConColor()
        conColor.timeRange = instruccion.timeRange
        conColor.layerInstructions = instruccion.layerInstructions
        conColor.backgroundColor = instruccion.backgroundColor
        conColor.colorDeClip = color
        vc.instructions = [conColor]
    }
    if custom { vc.customVideoCompositorClass = CompositorDeColor.self }
    return vc
}

print("— mezcla con opacidad —")
let (comp1, ins1, _, capaAzul1) = montajeConDosCapas()
capaAzul1.setOpacity(0.5, at: .zero)
let nativo1 = await frameDe(comp1, con: composicionDeVideo(con: ins1, custom: false), en: 1)!
let custom1 = await frameDe(comp1, con: composicionDeVideo(con: ins1, custom: true), en: 1)!
let centro = pixelDe(nativo1, en: 160, 90)
comprobar(centro.r > 100 && centro.b > 100, "el nativo mezcla las dos capas (magenta)")
comparar(nativo1, custom1, en: [(160, 90), (20, 20), (300, 170)], "el custom mezcla igual")

print("— transformación —")
let (comp2, ins2, _, capaAzul2) = montajeConDosCapas()
capaAzul2.setTransform(CGAffineTransform(translationX: 80, y: 40), at: .zero)
let nativo2 = await frameDe(comp2, con: composicionDeVideo(con: ins2, custom: false), en: 1)!
let custom2 = await frameDe(comp2, con: composicionDeVideo(con: ins2, custom: true), en: 1)!
comparar(nativo2, custom2, en: [(160, 90), (160 + 80, 90 + 40), (5, 5)], "la transformación coincide")

print("— recorte —")
let (comp3, ins3, _, capaAzul3) = montajeConDosCapas()
capaAzul3.setCropRectangle(CGRect(x: 0, y: 0, width: 160, height: 180), at: .zero)
let nativo3 = await frameDe(comp3, con: composicionDeVideo(con: ins3, custom: false), en: 1)!
let custom3 = await frameDe(comp3, con: composicionDeVideo(con: ins3, custom: true), en: 1)!
comparar(nativo3, custom3, en: [(80, 90), (240, 90)], "el recorte coincide")

print("— color sobre dos capas —")
let (comp4, ins4, _, capaAzul4) = montajeConDosCapas()
capaAzul4.setOpacity(0.5, at: .zero)
let color = ColorDeClip(exposicion: 40, contraste: 0, saturacion: 0, temperatura: 0, altas: 0, sombras: 0)
let nativo4 = await frameDe(comp4, con: composicionDeVideo(con: ins4, custom: false), en: 1)!
let custom4 = await frameDe(comp4, con: composicionDeVideo(con: ins4, color: color, custom: true), en: 1)!
let sinColor = pixelDe(nativo4, en: 160, 90)
let conColor = pixelDe(custom4, en: 160, 90)
comprobar(conColor.r > sinColor.r + 10 && conColor.b > sinColor.b + 10,
          "el color sube la exposición sobre el compuesto (no solo sobre una capa)")
comprobar(conColor.r > 100 && conColor.b > 100, "y las dos capas siguen presentes bajo el color")

print("— pista de ajuste sobre material real —")
// Una pista de ajuste (sin medio) manda su color sobre el compuesto de debajo,
// igual que el adjustment layer de cualquier NLE.
let idRojo = UUID()
let medioRojo = MedioResuelto(
    id: idRojo, url: roja, asset: assetRojo,
    pistaDeVideo: pistaRoja, pistaDeAudio: nil,
    duracion: CMTime(seconds: 2, preferredTimescale: 600),
    tamanoNatural: CGSize(width: 320, height: 180),
    transformacionPreferida: .identity, fps: 25
)
var linea = LineaDeTiempo.nueva(timebase: .p25)
let pistaV2 = linea.pistas.first { $0.nombre == "V2" }!.id
let pistaV1 = linea.pistas.first { $0.nombre == "V1" }!.id
var ajuste = Clip(mediaID: UUID(), nombre: "Ajuste", inicio: 0,
                  duracion: Timebase.p25.frames(segundos: 2), entradaEnOrigen: 0)
ajuste.esAjuste = true
ajuste.color = ColorDeClip(exposicion: 40, contraste: 0, saturacion: 0, temperatura: 0, altas: 0, sombras: 0)
linea.sobrescribir(ajuste, enPista: pistaV2, en: 0)
let clipRojo = Clip(mediaID: idRojo, nombre: "Rojo", inicio: 0,
                    duracion: Timebase.p25.frames(segundos: 2), entradaEnOrigen: 0)
linea.sobrescribir(clipRojo, enPista: pistaV1, en: 0)
let renderAjuste = ConstructorDeMontaje.construir(linea, medios: [idRojo: medioRojo])
let ajustado = await frameDe(renderAjuste.composicion, con: renderAjuste.composicionDeVideo, en: 1)!
let pixelAjustado = pixelDe(ajustado, en: 160, 90)
comprobar(pixelAjustado.r > 240, "el color de la pista de ajuste se aplica sobre el clip de debajo (\(pixelAjustado))")

print("— LUT .cube de un clip —")
// Una LUT 2×2 que mapea todo a verde: el clip que la lleva sale verde.
let rutaDeLut = carpeta.appendingPathComponent("a-verde.cube")
let contenidoDeLut = "LUT_3D_SIZE 2\n" + (0..<8).map { _ in "0.0 1.0 0.0" }.joined(separator: "\n") + "\n"
try! contenidoDeLut.write(to: rutaDeLut, atomically: true, encoding: .utf8)
var clipConLut = Clip(mediaID: idRojo, nombre: "Rojo", inicio: 0,
                      duracion: Timebase.p25.frames(segundos: 2), entradaEnOrigen: 0)
clipConLut.lutDeColor = rutaDeLut.path
var lineaLut = LineaDeTiempo.nueva(timebase: .p25)
lineaLut.sobrescribir(clipConLut, enPista: lineaLut.pistas.first { $0.nombre == "V2" }!.id, en: 0)
let renderLut = ConstructorDeMontaje.construir(lineaLut, medios: [idRojo: medioRojo])
let conLut = await frameDe(renderLut.composicion, con: renderLut.composicionDeVideo, en: 1)!
let pixelConLut = pixelDe(conLut, en: 160, 90)
comprobar(pixelConLut.g > 200 && pixelConLut.r < 60,
          "la LUT del clip mapea el material a verde (\(pixelConLut))")

print("— diagnóstico: tinte del render —")
// El canal alfa del buffer decodificado: determina cómo hay que mezclar.
let genDirecto = AVAssetImageGenerator(asset: assetAzul)
genDirecto.requestedTimeToleranceBefore = .zero
genDirecto.requestedTimeToleranceAfter = .zero
if let cgDirecto = try? await genDirecto.image(at: CMTime(seconds: 1, preferredTimescale: 600)).image {
    let datos = cgDirecto.dataProvider!.data! as Data
    let bytes = [UInt8](datos)
    let i = (90 * cgDirecto.width + 160) * 4
    print("  azul directo (B,G,R,A): \(bytes[i]),\(bytes[i+1]),\(bytes[i+2]),\(bytes[i+3])")
}
let compD = AVMutableComposition()
let tD = compD.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
try! tD.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaAzul, at: .zero)
let insD = AVMutableVideoCompositionInstruction()
insD.timeRange = CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600))
insD.backgroundColor = CGColor(red: 0, green: 0, blue: 0, alpha: 1)
let capaD = AVMutableVideoCompositionLayerInstruction(assetTrack: tD)
insD.layerInstructions = [capaD]
let conAtajo = await frameDe(compD, con: composicionDeVideo(con: insD, custom: true), en: 1)!
print("  atajo (passthrough): \(pixelDe(conAtajo, en: 160, 90))")
capaD.setTransform(CGAffineTransform(translationX: 0.01, y: 0.01), at: .zero)
let sinAtajo = await frameDe(compD, con: composicionDeVideo(con: insD, custom: true), en: 1)!
print("  camino completo: \(pixelDe(sinAtajo, en: 160, 90))")
capaD.setTransform(CGAffineTransform(translationX: 0.01, y: 0.01), at: .zero)
capaD.setOpacity(1.0, at: .zero)
let conOpacidad = await frameDe(compD, con: composicionDeVideo(con: insD, custom: true), en: 1)!
print("  con opacidad 1.0 explícita: \(pixelDe(conOpacidad, en: 160, 90))")

print("— sin color, una sola capa —")
let composicionUnica = AVMutableComposition()
let tU = composicionUnica.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
try! tU.insertTimeRange(CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600)), of: pistaRoja, at: .zero)
let insU = AVMutableVideoCompositionInstruction()
insU.timeRange = CMTimeRange(start: .zero, duration: CMTime(seconds: 2, preferredTimescale: 600))
let capaU = AVMutableVideoCompositionLayerInstruction(assetTrack: tU)
insU.layerInstructions = [capaU]
let nativoU = await frameDe(composicionUnica, con: composicionDeVideo(con: insU, custom: false), en: 1)!
let customU = await frameDe(composicionUnica, con: composicionDeVideo(con: insU, custom: true), en: 1)!
comparar(nativoU, customU, en: [(160, 90)], "una sola capa neutra pasa igual", tolerancia: 4)

print("— modo de fusión: multiplicar oscurece —")
// Rojo sobre azul en modo multiplicar: (1,0,0)·(0,0,1) = negro. Se monta con
// el constructor del modelo, que es quien enruta `efectosPorCapa`.
do {
    let id = UUID()
    let medio = try await MedioResuelto.cargar(id: id, url: roja)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v2 = linea.pistas.first { $0.nombre == "V2" }!.id
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var abajo = Clip(mediaID: id, nombre: "Azul", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    // La pista de abajo necesita un medio azul: se genera con el escritor.
    let azulURL = try await generarVideoColor(nombre: "azul2.m4v", rojo: 0, verde: 0, azul: 1)
    let idAzul = UUID()
    let medioAzul = try await MedioResuelto.cargar(id: idAzul, url: azulURL)
    abajo.mediaID = idAzul
    linea.sobrescribir(abajo, enPista: v1, en: 0)
    var arriba = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    arriba.modoDeFusion = .multiplicar
    linea.sobrescribir(arriba, enPista: v2, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio, idAzul: medioAzul])
    comprobar(render.composicionDeVideo?.customVideoCompositorClass != nil,
              "con un modo de fusión el compositor custom se activa")
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    let p = pixelDe(frame, en: 160, 90)
    comprobar(p.r < 60 && p.g < 60 && p.b < 60,
              "rojo × azul en multiplicar da negro (\(p))")
}

print("— máscara elíptica: fuera de la forma no se ve —")
do {
    let id = UUID()
    let medio = try await MedioResuelto.cargar(id: id, url: roja)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    // Elipse pequeña en el centro: el borde del frame debe quedar negro.
    clip.mascara = MascaraDeClip(forma: .elipse, posicionX: 0.5, posicionY: 0.5,
                                 tamanoX: 0.2, tamanoY: 0.2, pluma: 0.05)
    linea.sobrescribir(clip, enPista: v1, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio])
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    let centro = pixelDe(frame, en: 160, 90)
    let esquina = pixelDe(frame, en: 10, 10)
    comprobar(centro.r > 200, "el centro de la máscara sigue rojo (\(centro))")
    comprobar(esquina.r < 40 && esquina.g < 40 && esquina.b < 40,
              "fuera de la elipse se ve el fondo negro (\(esquina))")
}

print("— máscara invertida: se ve todo menos la forma —")
do {
    let id = UUID()
    let medio = try await MedioResuelto.cargar(id: id, url: roja)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    clip.mascara = MascaraDeClip(forma: .elipse, posicionX: 0.5, posicionY: 0.5,
                                 tamanoX: 0.2, tamanoY: 0.2, pluma: 0.05, invertida: true)
    linea.sobrescribir(clip, enPista: v1, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio])
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    let centro = pixelDe(frame, en: 160, 90)
    let esquina = pixelDe(frame, en: 10, 10)
    comprobar(esquina.r > 200, "fuera de la elipse se ve el rojo (\(esquina))")
    comprobar(centro.r < 40 && centro.g < 40 && centro.b < 40,
              "dentro de la elipse se ve el fondo (\(centro))")
}

print("— viñeta y desenfoque —")
do {
    let id = UUID()
    let medio = try await MedioResuelto.cargar(id: id, url: roja)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var conViñeta = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    conViñeta.color.vignette = 0.8
    conViñeta.color.radioDeVignette = 0.5
    linea.sobrescribir(conViñeta, enPista: v1, en: 0)
    let renderV = ConstructorDeMontaje.construir(linea, medios: [id: medio])
    let frameV = await frameDe(renderV.composicion, con: renderV.composicionDeVideo, en: 1)!
    let centroV = pixelDe(frameV, en: 160, 90)
    let bordeV = pixelDe(frameV, en: 10, 90)
    comprobar(centroV.r > bordeV.r + 60,
              "la viñeta oscurece el borde y deja el centro (\(centroV) vs \(bordeV))")

    var conDesenfoque = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    conDesenfoque.color.desenfoque = 0.1
    var lineaB = LineaDeTiempo.nueva(timebase: .p25)
    let v1b = lineaB.pistas.first { $0.nombre == "V1" }!.id
    lineaB.sobrescribir(conDesenfoque, enPista: v1b, en: 0)
    let renderB = ConstructorDeMontaje.construir(lineaB, medios: [id: medio])
    let frameB = await frameDe(renderB.composicion, con: renderB.composicionDeVideo, en: 1)!
    let pixelB = pixelDe(frameB, en: 160, 90)
    comprobar(pixelB.r > 150, "el desenfoque suaviza pero mantiene el rojo (\(pixelB))")
}

print("— curvas RGB —")
do {
    let id = UUID()
    let medio = try await MedioResuelto.cargar(id: id, url: roja)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: id, nombre: "Rojo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    // Curva de luminancia plana a 0,5: todo se oscurece a la mitad.
    var curvas = CurvasDeClip()
    curvas.luma = [.init(x: 0, y: 0.5), .init(x: 0.5, y: 0.5), .init(x: 1, y: 0.5)]
    clip.color.curvas = curvas
    linea.sobrescribir(clip, enPista: v1, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio])
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    let p = pixelDe(frame, en: 160, 90)
    comprobar(p.r > 100 && p.r < 200, "la curva plana a 0,5 deja el rojo a media luz (\(p))")
}

print("— ruedas de color —")
// Una rueda de sombras azul +0,5: los píxeles oscuros ganan azul, el blanco
// no cambia. Se comprueba con un degradado para tener píxeles oscuros y
// brillantes en el mismo frame.
do {
    let id = UUID()
    let url = carpeta.appendingPathComponent("degradado.m4v")
    let escritor = try AVAssetWriter(outputURL: url, fileType: .m4v)
    let entrada = AVAssetWriterInput(mediaType: .video, outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 320, AVVideoHeightKey: 180,
    ])
    let adaptador = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: entrada, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180,
    ])
    escritor.add(entrada)
    escritor.startWriting()
    escritor.startSession(atSourceTime: .zero)
    while !entrada.isReadyForMoreMediaData { usleep(1000) }
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32BGRA,
                        [kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180] as CFDictionary, &buffer)
    CVPixelBufferLockBaseAddress(buffer!, [])
    let base = CVPixelBufferGetBaseAddress(buffer!)!.assumingMemoryBound(to: UInt8.self)
    for y in 0..<180 {
        let gris = UInt8(Double(y) / 180 * 255)
        for x in 0..<320 {
            let i = (y * 320 + x) * 4
            base[i] = gris; base[i + 1] = gris; base[i + 2] = gris; base[i + 3] = 255
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer!, [])
    for f in 0..<50 {
        while !entrada.isReadyForMoreMediaData { usleep(1000) }
        adaptador.append(buffer!, withPresentationTime: CMTime(value: Int64(f), timescale: 25))
    }
    entrada.markAsFinished()
    await escritor.finishWriting()

    let medio = try await MedioResuelto.cargar(id: id, url: url)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: id, nombre: "Degradado", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    var ruedas = RuedasDeColor()
    ruedas.sombrasAzul = 0.5
    clip.color.ruedas = ruedas
    linea.sobrescribir(clip, enPista: v1, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio])
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    // Fila 10 (oscura): el azul sube; fila 170 (clara): casi no cambia.
    let oscuro = pixelDe(frame, en: 160, 10)
    let claro = pixelDe(frame, en: 160, 170)
    comprobar(oscuro.b > oscuro.r + 20, "las sombras ganan azul (\(oscuro))")
    comprobar(abs(claro.b - claro.r) < 15, "y las altas apenas cambian (\(claro))")
}

print("— chroma key (pantalla verde) —")
// Un clip verde con un sujeto rojo en el centro: al aplicar el chroma key, el
// verde se vuelve transparente y el rojo se queda. Se comprueba montando el
// clip con el key sobre un fondo azul: el centro debe verse rojo y el borde
// azul (el verde ya no tapa).
do {
    let id = UUID()
    let url = carpeta.appendingPathComponent("verde.m4v")
    let escritor = try AVAssetWriter(outputURL: url, fileType: .m4v)
    let entrada = AVAssetWriterInput(mediaType: .video, outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 320, AVVideoHeightKey: 180,
    ])
    let adaptador = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: entrada, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180,
    ])
    escritor.add(entrada)
    escritor.startWriting()
    escritor.startSession(atSourceTime: .zero)
    while !entrada.isReadyForMoreMediaData { usleep(1000) }
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32BGRA,
                        [kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180] as CFDictionary, &buffer)
    CVPixelBufferLockBaseAddress(buffer!, [])
    let base = CVPixelBufferGetBaseAddress(buffer!)!.assumingMemoryBound(to: UInt8.self)
    for y in 0..<180 {
        for x in 0..<320 {
            let i = (y * 320 + x) * 4
            let enCentro = abs(x - 160) < 30 && abs(y - 90) < 30
            if enCentro {
                base[i] = 0; base[i + 1] = 0; base[i + 2] = 255; base[i + 3] = 255      // rojo
            } else {
                base[i] = 0; base[i + 1] = 255; base[i + 2] = 0; base[i + 3] = 255      // verde
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer!, [])
    for f in 0..<50 {
        while !entrada.isReadyForMoreMediaData { usleep(1000) }
        adaptador.append(buffer!, withPresentationTime: CMTime(value: Int64(f), timescale: 25))
    }
    entrada.markAsFinished()
    await escritor.finishWriting()

    let medio = try await MedioResuelto.cargar(id: id, url: url)
    let idFondo = UUID()
    let medioFondo = try await MedioResuelto.cargar(id: idFondo, url: azul)
    var linea = LineaDeTiempo.nueva(timebase: .p25)
    let v2 = linea.pistas.first { $0.nombre == "V2" }!.id
    let v1 = linea.pistas.first { $0.nombre == "V1" }!.id
    var fondo = Clip(mediaID: idFondo, nombre: "Fondo", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    linea.sobrescribir(fondo, enPista: v1, en: 0)
    var sujeto = Clip(mediaID: id, nombre: "Sujeto", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    sujeto.croma = ChromaKeyDeClip(rojo: 0, verde: 1, azul: 0, tolerancia: 0.3, suavizado: 0.1, suprimirDerrame: 0.5)
    linea.sobrescribir(sujeto, enPista: v2, en: 0)
    let render = ConstructorDeMontaje.construir(linea, medios: [id: medio, idFondo: medioFondo])
    comprobar(render.composicionDeVideo?.customVideoCompositorClass != nil,
              "con chroma key el compositor custom se activa")
    let frame = await frameDe(render.composicion, con: render.composicionDeVideo, en: 1)!
    let centro = pixelDe(frame, en: 160, 90)
    let borde = pixelDe(frame, en: 10, 10)
    comprobar(centro.r > 150 && centro.b < 100, "el sujeto rojo se queda sobre el fondo (\(centro))")
    comprobar(borde.b > 150 && borde.r < 100, "y el verde se hace transparente: se ve el azul de debajo (\(borde))")
}

if fallos == 0 {
    print("COMPOSITOR COLOR CORRECTO")
} else {
    print("COMPOSITOR COLOR ROTO — \(fallos) fallos")
    exit(1)
}
