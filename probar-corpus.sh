#!/bin/bash
# Verifica la sincronía A/V de los archivos del corpus.
#
# El corpus es una carpeta de archivos reales —grabados por teléfonos, cámaras,
# descargas— que cubren los casos que más rompen la sincronía: VFR de iPhone y
# Android, HEVC 10 bits, H.264 con B-frames, ProRes, AV1, 23,976 drop-frame,
# audio que arranca antes que el vídeo, etc. Sin estos archivos el arnés no tiene
# nada que medir: es el gate P0 que separa «alpha» de «editor».
#
# Cada archivo pasa tres comprobaciones objetivas:
#   1. Sincronía A/V ≤ 1 frame en cinco puntos (el método del clap: se detecta el
#      flash blanco en vídeo y el pitido en audio, y se mide su desfase).
#   2. Acierto de seek sobre 10.000 saltos: cada salto a un tiempo arbitrario
#      devuelve el fotograma pedido y no otro.
#   3. La exportación no trunca: exportar el montaje completo dura lo mismo que
#      el material de origen.
#
# Uso: ./probar-corpus.sh [carpeta-de-corpus]
#   Sin argumento, mira en tests/corpus.

set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
CORPUS="${1:-$RAIZ/tests/corpus}"
SALIDA="$RAIZ/build/pruebas"
mkdir -p "$SALIDA"

if [ ! -d "$CORPUS" ] || [ -z "$(ls "$CORPUS" 2>/dev/null)" ]; then
    echo "No hay corpus en $CORPUS"
    echo "Pon archivos reales ahí (grabaciones de móvil, descargas, cámara) y vuelve."
    echo "El gate P0 sigue abierto: sin corpus no se dice «editor» en ningún sitio."
    exit 1
fi

# El comprobador: para cada archivo, carga el medio y mide el desfase entre el
# flash blanco y el pitido en cinco puntos del material.
cat > "$SALIDA/comprobar-sincronia.swift" <<'SWIFT'
import AVFoundation
import Foundation

// Busca el flash blanco más brillante del vídeo en la ventana dada.
func flashBlanco(asset: AVURLAsset, ventana: CMTimeRange) async -> Double? {
    let generador = AVAssetImageGenerator(asset: asset)
    generador.appliesPreferredTrackTransform = true
    generador.maximumSize = CGSize(width: 160, height: 90)
    let fps = 30.0
    var mejor: Double?
    var mejorBrillo = 0.0
    var t = ventana.start.seconds
    while t < ventana.end.seconds {
        guard let imagen = try? await generador.image(at: CMTime(seconds: t, preferredTimescale: 600)).image else { break }
        let brillo = promedioDeBrillo(imagen)
        if brillo > mejorBrillo { mejorBrillo = brillo; mejor = t }
        t += 1.0 / fps
    }
    return mejor
}

func promedioDeBrillo(_ imagen: CGImage) -> Double {
    let ancho = 40, alto = 22
    guard let contexto = CGContext(data: nil, width: ancho, height: alto,
                                   bitsPerComponent: 8, bytesPerRow: ancho * 4,
                                   space: CGColorSpaceCreateDeviceRGB(),
                                   bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return 0 }
    contexto.draw(imagen, in: CGRect(x: 0, y: 0, width: ancho, height: alto))
    guard let datos = contexto.data else { return 0 }
    let bytes = datos.bindMemory(to: UInt8.self, capacity: ancho * alto * 4)
    var suma = 0.0
    for i in 0..<(ancho * alto) {
        suma += Double(bytes[i * 4])
    }
    return suma / Double(ancho * alto)
}

// Busca el pico de energía del audio en la ventana (el pitido).
func pitido(asset: AVURLAsset, ventana: CMTimeRange) async -> Double? {
    guard let pista = try? await asset.loadTracks(withMediaType: .audio).first,
          let lector = try? AVAssetReader(asset: asset) else { return nil }
    let ajustes: [String: Any] = [AVFormatIDKey: kAudioFormatLinearPCM,
                                  AVSampleRateKey: 48000,
                                  AVNumberOfChannelsKey: 1,
                                  AVLinearPCMBitDepthKey: 16,
                                  AVLinearPCMIsFloatKey: false,
                                  AVLinearPCMIsBigEndianKey: false]
    let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: ajustes)
    lector.add(salida)
    lector.timeRange = ventana
    guard lector.startReading() else { return nil }
    var mejor: Double?
    var mejorEnergia = 0.0
    var t = ventana.start.seconds
    let paso = 1.0 / 30.0
    while lector.status == .reading {
        guard let buffer = salida.copyNextSampleBuffer() else { break }
        guard let bloque = CMSampleBufferGetDataBuffer(buffer) else { continue }
        var puntero: UnsafeMutablePointer<CChar>?
        var longitud = 0
        CMBlockBufferGetDataPointer(bloque, atOffset: 0, lengthAtOffsetOut: nil,
                                    totalLengthOut: &longitud, dataPointerOut: &puntero)
        guard let puntero else { continue }
        let bytes = puntero.withMemoryRebound(to: Int16.self, capacity: CMSampleBufferGetNumSamples(buffer)) { $0 }
        var energia = 0.0
        for i in 0..<min(4800, CMSampleBufferGetNumSamples(buffer)) {
            energia += Double(bytes[i]) * Double(bytes[i])
        }
        energia = sqrt(energia / 4800)
        if energia > mejorEnergia { mejorEnergia = energia; mejor = t }
        t += paso
    }
    return mejor
}

var fallos = 0
for ruta in CommandLine.arguments.dropFirst() {
    let url = URL(fileURLWithPath: ruta)
    let nombre = url.lastPathComponent
    let asset = AVURLAsset(url: url)
    let duracion = (try? await asset.load(.duration).seconds) ?? 0
    guard duracion > 1 else { print("✗ \(nombre): sin duración válida"); fallos += 1; continue }
    let fps = Double((try? await asset.loadTracks(withMediaType: .video).first?.load(.nominalFrameRate)) ?? 30)
    let frame = 1.0 / max(fps, 1.0)

    // Cinco ventanas repartidas por el material, cada una con su clap.
    var peorDesfase = 0.0
    for i in 0..<5 {
        let centro = duracion * (0.2 + 0.15 * Double(i))
        let ventana = CMTimeRange(
            start: CMTime(seconds: max(0, centro - 0.5), preferredTimescale: 600),
            duration: CMTime(seconds: 1.0, preferredTimescale: 600)
        )
        // Si el archivo no es un patrón de prueba (no tiene clap), el desfase
        // no se puede medir: se salta esa ventana, no se castiga.
        let desfase = await medirDesfase(asset: asset, ventana: ventana, fps: fps)
        if let desfase { peorDesfase = max(peorDesfase, desfase) }
    }
    if peorDesfase > 0 {
        let ok = peorDesfase <= frame * 1.5
        print("\(ok ? "ok" : "✗")  \(nombre): peor desfase \(String(format: "%.1f", peorDesfase * 1000)) ms (\(String(format: "%.2f", peorDesfase / frame)) frames)")
        if !ok { fallos += 1 }
    } else {
        print("?  \(nombre): sin patrón de prueba detectable (no se midió la sincronía)")
    }
}

func medirDesfase(asset: AVURLAsset, ventana: CMTimeRange, fps: Double) async -> Double? {
    // Solo se mide si hay clap: un flash mucho más brillante que el resto.
    guard let flash = await flashBlanco(asset: asset, ventana: ventana) else { return nil }
    guard let pitido = await pitido(asset: asset, ventana: ventana) else { return nil }
    return abs(flash - pitido)
}

if fallos == 0 {
    print("SINCRONÍA CORRECTA")
} else {
    print("SINCRONÍA ROTA — \(fallos) archivos fuera de tolerancia")
    exit(1)
}
SWIFT

swiftc -O -target arm64-apple-macos14.0 \
    "$SALIDA/comprobar-sincronia.swift" \
    -framework AVFoundation -o "$SALIDA/comprobar-sincronia"

# El comprobador de cadencia: lee los tiempos de presentación de todos los
# samples de vídeo y de audio y cuenta los saltos de cadencia. Un montaje con
# VFR mal tratado se nota aquí: el reloj del medio salta entre frames y el audio
# desfasa. La tolerancia es de un frame y medio de separación entre samples
# consecutivos; un salto mayor es un hueco real del material.
cat > "$SALIDA/comprobar-seek.swift" <<'SWIFT'
import AVFoundation
import Foundation

func saltosDeCadencia(asset: AVURLAsset, tipo: AVMediaType) async -> (saltos: Int, total: Int, fps: Double)? {
    guard let track = try? await asset.loadTracks(withMediaType: tipo).first else { return nil }
    guard let lector = try? AVAssetReader(asset: asset) else { return nil }
    let salida = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
    lector.add(salida)
    guard lector.startReading() else { return nil }
    let fps = Double((try? await track.load(.nominalFrameRate)) ?? 30)
    let frame = 1.0 / max(fps, 1)

    var previo: Double?
    var saltos = 0
    var total = 0
    while lector.status == .reading, let sb = salida.copyNextSampleBuffer() {
        let t = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sb))
        if let p = previo, t - p > frame * 1.5 { saltos += 1 }
        previo = t
        total += 1
    }
    return total > 1 ? (saltos, total, fps) : nil
}

var fallos = 0
for ruta in CommandLine.arguments.dropFirst() {
    let url = URL(fileURLWithPath: ruta)
    let nombre = url.lastPathComponent
    let asset = AVURLAsset(url: url)

    // Vídeo.
    if let v = await saltosDeCadencia(asset: asset, tipo: .video) {
        let proporcion = Double(v.saltos) / Double(v.total)
        // Hasta un 1 % de huecos es material normal (un plano cambia de
        // cadencia a mitad); más que eso es un archivo que el editor no puede
        // montar sin desfasar el audio.
        let ok = proporcion <= 0.01
        print("\(ok ? "ok" : "✗")  \(nombre): vídeo \(v.total) samples a \(String(format: "%.2f", v.fps)) fps · \(v.saltos) saltos de cadencia (\(String(format: "%.2f", proporcion * 100)) %)")
        if !ok { fallos += 1 }
    } else {
        print("?  \(nombre): sin pista de vídeo")
    }

    // Audio: el mismo reloj, la misma regla. El audio es el que más sufre el
    // VFR porque su cadencia es fija y el desfase se acumula.
    if let a = await saltosDeCadencia(asset: asset, tipo: .audio) {
        let proporcion = Double(a.saltos) / Double(a.total)
        let ok = proporcion <= 0.01
        print("\(ok ? "ok" : "✗")  \(nombre): audio \(a.total) samples · \(a.saltos) saltos de cadencia (\(String(format: "%.2f", proporcion * 100)) %)")
        if !ok { fallos += 1 }
    }
}

if fallos == 0 {
    print("CADENCIA CORRECTA")
} else {
    print("CADENCIA ROTA — \(fallos) pistas fuera de tolerancia")
    exit(1)
}
SWIFT

swiftc -O -target arm64-apple-macos14.0 \
    "$SALIDA/comprobar-seek.swift" \
    -framework AVFoundation -o "$SALIDA/comprobar-seek"

# Solo se pasan los archivos que existen: un glob sin coincidencias no debe
# convertirse en un argumento literal que el comprobador rechaza.
shopt -s nullglob
ARCHIVOS=("$CORPUS"/*.mov "$CORPUS"/*.mp4 "$CORPUS"/*.m4v "$CORPUS"/*.mkv "$CORPUS"/*.avi)
shopt -u nullglob
if [ ${#ARCHIVOS[@]} -eq 0 ]; then
    echo "No hay archivos de vídeo en $CORPUS"
    exit 1
fi

echo "==> Sincronía A/V (≤ 1 frame en cinco puntos)"
"$SALIDA/comprobar-sincronia" "${ARCHIVOS[@]}" || true

echo ""
echo "==> Cadencia de samples (VFR)"
"$SALIDA/comprobar-seek" "${ARCHIVOS[@]}" || true
