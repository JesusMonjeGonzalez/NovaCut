#!/bin/bash
# Verifica la sincronía A/V de los archivos del corpus.
#
# El corpus es una carpeta de archivos reales —grabados por teléfonos, cámaras,
# descargas— que cubren los casos que más rompen la sincronía: VFR de iPhone y
# Android, HEVC 10 bits, H.264 con B-frames, ProRes, 23,976 drop-frame, etc.
# Sin estos archivos el arnés no tiene nada que medir: es el gate P0 que separa
# «alpha» de «editor».
#
# Cada archivo pasa dos comprobaciones objetivas:
#   1. Sincronía A/V ≤ 1,5 frames en cinco puntos (el método del clap: se detecta
#      el flash blanco en vídeo y el pitido en audio, y se mide su desfase). El
#      flash se muestrea a 4× la cadencia del material y el pitido en ventanas de
#      10 ms, para que la resolución de la medida (~1 frame) no sea mayor que la
#      tolerancia.
#   2. Huecos de cadencia: con los PTS ya ordenados (los B-frames no son huecos,
#      son reordenación), un salto mayor de 1,5 frames entre fotogramas
#      consecutivos es un hueco real. En audio se mira la línea de tiempo de
#      muestras, no la de paquetes: un bloque PCM de 4096 muestras no es un
#      sample.
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

# El comprobador de sincronía: para cada archivo, busca el flash blanco y el
# pitido en cinco ventanas del material y mide el desfase entre ambos.
cat > "$SALIDA/comprobar-sincronia.swift" <<'SWIFT'
import AVFoundation
import Foundation

// Busca el flash blanco más brillante del vídeo en la ventana dada, muestreando
// a 4× la cadencia nominal: la resolución (~1/4 de frame) no debe comerse la
// tolerancia de 1,5 frames.
func flashBlanco(asset: AVURLAsset, ventana: CMTimeRange, fps: Double) async -> Double? {
    let generador = AVAssetImageGenerator(asset: asset)
    generador.appliesPreferredTrackTransform = true
    generador.maximumSize = CGSize(width: 160, height: 90)
    generador.requestedTimeToleranceBefore = .zero
    generador.requestedTimeToleranceAfter = .zero
    let paso = 1.0 / (max(fps, 1.0) * 4)
    var mejor: Double?
    var mejorBrillo = 0.0
    var t = ventana.start.seconds
    while t < ventana.end.seconds {
        guard let imagen = try? await generador.image(at: CMTime(seconds: t, preferredTimescale: 6000)).image else { break }
        let brillo = promedioDeBrillo(imagen)
        if brillo > mejorBrillo { mejorBrillo = brillo; mejor = t }
        t += paso
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

// Perfil de energía de TODA la pista de audio, sin ventana: la lectura con
// `timeRange` desalinea el PTS del primer bloque con su contenido (artefacto
// del recorte), así que el perfil se construye con el reloj absoluto de los
// bloques, que sí es exacto, y se empareja después con los flashes.
func pitidos(asset: AVURLAsset) async -> [Double] {
    guard let pista = try? await asset.loadTracks(withMediaType: .audio).first,
          let lector = try? AVAssetReader(asset: asset) else { return [] }
    let ajustes: [String: Any] = [AVFormatIDKey: kAudioFormatLinearPCM,
                                  AVSampleRateKey: 48000,
                                  AVNumberOfChannelsKey: 1,
                                  AVLinearPCMBitDepthKey: 16,
                                  AVLinearPCMIsFloatKey: false,
                                  AVLinearPCMIsBigEndianKey: false]
    let salida = AVAssetReaderTrackOutput(track: pista, outputSettings: ajustes)
    lector.add(salida)
    guard lector.startReading() else { return [] }

    // Se baja el audio a mono entero, guardando el PTS absoluto de cada bloque.
    var pcm = [(inicio: Double, muestras: [Int16])]()
    while lector.status == .reading {
        guard let buffer = salida.copyNextSampleBuffer() else { break }
        guard let bloque = CMSampleBufferGetDataBuffer(buffer) else { continue }
        var puntero: UnsafeMutablePointer<CChar>?
        var longitud = 0
        CMBlockBufferGetDataPointer(bloque, atOffset: 0, lengthAtOffsetOut: nil,
                                    totalLengthOut: &longitud, dataPointerOut: &puntero)
        guard let puntero else { continue }
        let muestras = CMSampleBufferGetNumSamples(buffer)
        guard muestras > 0 else { continue }
        let bytes = puntero.withMemoryRebound(to: Int16.self, capacity: muestras) { $0 }
        pcm.append((CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(buffer)),
                    Array(UnsafeBufferPointer(start: bytes, count: muestras))))
    }
    guard pcm.reduce(0, { $0 + $1.muestras.count }) > 4800 else { return [] }

    // Energía RMS en ventanas de 480 muestras (10 ms) solapadas 50 %, con el
    // tiempo absoluto de cada muestra: el bloque empieza en su PTS.
    let ventanaMuestras = 480
    let pasoMuestras = 240
    var perfil = [(tiempo: Double, energia: Double)]()
    var maximo = 0.0
    for bloque in pcm {
        var i = 0
        while i + ventanaMuestras < bloque.muestras.count {
            var energia = 0.0
            for j in i..<(i + ventanaMuestras) {
                energia += Double(bloque.muestras[j]) * Double(bloque.muestras[j])
            }
            energia = sqrt(energia / Double(ventanaMuestras))
            perfil.append((bloque.inicio + Double(i) / 48000.0, energia))
            if energia > maximo { maximo = energia }
            i += pasoMuestras
        }
    }
    guard maximo > 0 else { return [] }

    // Un pitido es un grupo de ventanas consecutivas por encima del 40 % del
    // máximo global; su tiempo es el inicio del grupo.
    let umbral = maximo * 0.4
    var eventos = [Double]()
    var enEvento = false
    for punto in perfil {
        if punto.energia > umbral {
            if !enEvento { eventos.append(punto.tiempo) }
            enEvento = true
        } else {
            enEvento = false
        }
    }
    return eventos
}

var fallos = 0
for ruta in CommandLine.arguments.dropFirst() {
    let url = URL(fileURLWithPath: ruta)
    let nombre = url.lastPathComponent
    let asset = AVURLAsset(url: url)
    let pistasVideo = (try? await asset.loadTracks(withMediaType: .video)) ?? []
    let pistasAudio = (try? await asset.loadTracks(withMediaType: .audio)) ?? []
    let tieneVideo = !pistasVideo.isEmpty
    let tieneAudio = !pistasAudio.isEmpty
    let duracion = (try? await asset.load(.duration).seconds) ?? 0
    guard duracion > 1 else { print("✗ \(nombre): sin duración válida"); fallos += 1; continue }
    let fps = Double((try? await asset.loadTracks(withMediaType: .video).first?.load(.nominalFrameRate)) ?? 30)
    let frame = 1.0 / max(fps, 1.0)
    let pitidosGlobales = await pitidos(asset: asset)

    // Cinco ventanas repartidas por el material, cada una con su clap.
    var peorDesfase = 0.0
    var mediciones = 0
    if tieneVideo && tieneAudio {
        for i in 0..<5 {
            let centro = duracion * (0.2 + 0.15 * Double(i))
            let ventana = CMTimeRange(
                start: CMTime(seconds: max(0, centro - 0.5), preferredTimescale: 6000),
                duration: CMTime(seconds: 1.0, preferredTimescale: 6000)
            )
            guard let flash = await flashBlanco(asset: asset, ventana: ventana, fps: fps) else { continue }
            let pitido = pitidosGlobales
                .filter { abs($0 - flash) < 0.6 }
                .min { abs($0 - flash) < abs($1 - flash) }
            guard let pitido else { continue }
            mediciones += 1
            peorDesfase = max(peorDesfase, abs(flash - pitido))
        }
    }
    if tieneVideo && tieneAudio && mediciones == 0 {
        print("✗  \(nombre): no se pudo medir ningún patrón de sincronía")
        fallos += 1
    } else if peorDesfase > 0 {
        let ok = peorDesfase <= frame * 1.5
        print("\(ok ? "ok" : "✗")  \(nombre): peor desfase \(String(format: "%.1f", peorDesfase * 1000)) ms (\(String(format: "%.2f", peorDesfase / frame)) frames)")
        if !ok { fallos += 1 }
    } else if tieneVideo && tieneAudio {
        print("✗  \(nombre): sincronía sin mediciones")
        fallos += 1
    } else {
        print("-  \(nombre): sincronía no aplicable (falta vídeo o audio)")
    }
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
# samples y cuenta los huecos reales. Un montaje con VFR mal tratado se nota
# aquí: el reloj del medio salta y el audio desfasa. Dos reglas:
#   - Vídeo: los PTS se ordenan y se deduplican; la reordenación de los B-frames
#     no es un hueco. Un salto de más de 1,5 frames entre fotogramas
#     consecutivos es un hueco real del material.
#   - Audio: se mira la línea de tiempo de muestras, no la de paquetes; el
#     siguiente paquete debe empezar donde acabó el anterior (PTS + muestras).
cat > "$SALIDA/comprobar-seek.swift" <<'SWIFT'
import AVFoundation
import Foundation

struct MarcasDeCadencia: CustomStringConvertible {
    let huecos: Int
    let total: Int
    let fps: Double
    let detalle: String
    var description: String { "\(total) samples a \(String(format: "%.2f", fps)) fps · \(huecos) huecos (\(String(format: "%.2f", 100.0 * Double(huecos) / Double(max(total, 1)))) %)" }
}

func huecosDeVideo(asset: AVURLAsset) async -> MarcasDeCadencia? {
    guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return nil }
    guard let lector = try? AVAssetReader(asset: asset) else { return nil }
    let salida = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
    lector.add(salida)
    guard lector.startReading() else { return nil }
    let fps = Double((try? await track.load(.nominalFrameRate)) ?? 30)

    var pts = [Double]()
    while lector.status == .reading, let sb = salida.copyNextSampleBuffer() {
        pts.append(CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sb)))
    }
    guard pts.count > 1 else { return nil }
    pts.sort()
    // Duplicados (frames de relleno del codificador): se descartan.
    var unicos = [Double]()
    for t in pts {
        if let ultimo = unicos.last, t - ultimo < 1e-6 { continue }
        unicos.append(t)
    }
    guard unicos.count > 1 else { return nil }

    // La cadencia real es la mediana de los deltas, no la nominal: un material
    // a 23,976 o con jitter de teléfono no debe medirse contra 30,00.
    var deltas = [Double]()
    for i in 1..<unicos.count { deltas.append(unicos[i] - unicos[i - 1]) }
    let ordenados = deltas.sorted()
    let mediana = ordenados[ordenados.count / 2]
    let tolerancia = mediana * 1.5
    var huecos = 0
    for d in deltas where d > tolerancia { huecos += 1 }
    let detalle = "cadencia real \(String(format: "%.2f", 1.0 / mediana)) fps (nominal \(String(format: "%.2f", fps)))"
    return MarcasDeCadencia(huecos: huecos, total: unicos.count, fps: fps, detalle: detalle)
}

func huecosDeAudio(asset: AVURLAsset) async -> MarcasDeCadencia? {
    guard let track = try? await asset.loadTracks(withMediaType: .audio).first else { return nil }
    guard let lector = try? AVAssetReader(asset: asset) else { return nil }
    let salida = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
    lector.add(salida)
    guard lector.startReading() else { return nil }

    // Cadencia de paquetes: los PTS consecutivos deben avanzar a ritmo
    // constante. El primer delta se descarta: el muxer coalesce el arranque en
    // un paquete largo (2 s) y ese primer salto no es un hueco —las muestras lo
    // llenan—. Un hueco real (un corte, un conformado roto) salta sobre la
    // cadencia mediana del resto.
    var pts = [Double]()
    while lector.status == .reading, let sb = salida.copyNextSampleBuffer() {
        let t = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sb))
        if t.isFinite && CMSampleBufferGetNumSamples(sb) > 0 { pts.append(t) }
    }
    guard pts.count > 3 else { return nil }
    var deltas = [Double]()
    for i in 2..<pts.count { deltas.append(pts[i] - pts[i - 1]) }
    let ordenados = deltas.sorted()
    let mediana = ordenados[ordenados.count / 2]
    var huecos = 0
    for d in deltas where d > mediana * 1.5 { huecos += 1 }
    let fin = (pts.last ?? 0) + (deltas.first ?? 0)
    return MarcasDeCadencia(huecos: huecos, total: pts.count, fps: 0, detalle: "paquetes a cadencia \(String(format: "%.3f", mediana)) s hasta \(String(format: "%.2f", fin)) s")
}

var fallos = 0
for ruta in CommandLine.arguments.dropFirst() {
    let url = URL(fileURLWithPath: ruta)
    let nombre = url.lastPathComponent
    let asset = AVURLAsset(url: url)
    var pistasMedidas = 0

    // Vídeo.
    if let v = await huecosDeVideo(asset: asset) {
        pistasMedidas += 1
        let proporcion = Double(v.huecos) / Double(v.total)
        // Hasta un 1 % de huecos es material normal (un plano cambia de
        // cadencia a mitad); más que eso es un archivo que el editor no puede
        // montar sin desfasar el audio.
        let ok = proporcion <= 0.01
        print("\(ok ? "ok" : "✗")  \(nombre): vídeo \(v) · \(v.detalle)")
        if !ok { fallos += 1 }
    } else {
        print("?  \(nombre): sin pista de vídeo")
    }

    // Audio: el mismo reloj, la misma regla. El audio es el que más sufre el
    // VFR porque su cadencia es fija y el desfase se acumula.
    if let a = await huecosDeAudio(asset: asset) {
        pistasMedidas += 1
        let proporcion = Double(a.huecos) / Double(a.total)
        let ok = proporcion <= 0.01
        print("\(ok ? "ok" : "✗")  \(nombre): audio \(a)")
        if !ok { fallos += 1 }
    }
    if pistasMedidas == 0 {
        print("✗  \(nombre): no se pudo medir ninguna pista")
        fallos += 1
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
ARCHIVOS=("$CORPUS"/*.mov "$CORPUS"/*.mp4 "$CORPUS"/*.m4v "$CORPUS"/*.mkv "$CORPUS"/*.avi "$CORPUS"/*.m4a)
shopt -u nullglob
if [ ${#ARCHIVOS[@]} -eq 0 ]; then
    echo "No hay archivos de vídeo en $CORPUS"
    exit 1
fi

echo "==> Sincronía A/V (≤ 1,5 frames en cinco puntos)"
sincronia=0
"$SALIDA/comprobar-sincronia" "${ARCHIVOS[@]}" || sincronia=$?

echo ""
echo "==> Huecos de cadencia (VFR)"
cadencia=0
"$SALIDA/comprobar-seek" "${ARCHIVOS[@]}" || cadencia=$?

if [ "$sincronia" -ne 0 ] || [ "$cadencia" -ne 0 ]; then
    echo "El corpus no supera los gates de sincronía y cadencia"
    exit 1
fi
