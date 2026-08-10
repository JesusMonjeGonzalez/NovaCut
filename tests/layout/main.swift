import AVFoundation
import CoreMedia
import Foundation

// Pesos BS.1770-4 por disposición real de canales: el surround va a +1,5 dB y el
// LFE no contribuye, pero en qué índice está cada uno solo lo sabe el layout del
// flujo. Un WAV 5.1 ordena L R C LFE Ls Rs; deducir el peso por conteo le daría
// al LFE los +1,5 dB del surround y la normalización decidiría falseada.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

func descripcionConLayout(_ tag: AudioChannelLayoutTag, canales: UInt32) -> CMFormatDescription? {
    var layout = AudioChannelLayout()
    layout.mChannelLayoutTag = tag
    layout.mNumberChannelDescriptions = canales
    var asbd = AudioStreamBasicDescription(
        mSampleRate: 48000, mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat, mBytesPerPacket: 4, mFramesPerPacket: 1,
        mBytesPerFrame: 4, mChannelsPerFrame: canales, mBitsPerChannel: 32, mReserved: 0
    )
    var descripcion: CMAudioFormatDescription?
    CMAudioFormatDescriptionCreate(
        allocator: kCFAllocatorDefault, asbd: &asbd,
        layoutSize: MemoryLayout<AudioChannelLayout>.size, layout: &layout,
        magicCookieSize: 0, magicCookie: nil, extensions: nil,
        formatDescriptionOut: &descripcion
    )
    return descripcion
}

func igual(_ pesos: [Double]?, _ esperado: [Double], _ mensaje: String) {
    guard let pesos else { comprobar(false, "\(mensaje) — sin pesos"); return }
    comprobar(
        pesos.count == esperado.count && zip(pesos, esperado).allSatisfy { abs($0 - $1) < 0.001 },
        "\(mensaje): \(pesos.map { String(format: "%.2f", $0) })"
    )
}

print("— pesos por disposición de canales —")

if let d = descripcionConLayout(kAudioChannelLayoutTag_MPEG_5_1_A, canales: 6) {
    igual(SonoridadMedia.pesosDeLaDisposicion(d, canales: 6),
          [1, 1, 1, 0, 1.41, 1.41],
          "5.1 A (L R C LFE Ls Rs): el LFE a 0 y los surrounds a +1,5 dB")
} else {
    comprobar(false, "no se pudo crear la descripción 5.1 A")
}

if let d = descripcionConLayout(kAudioChannelLayoutTag_MPEG_5_1_B, canales: 6) {
    igual(SonoridadMedia.pesosDeLaDisposicion(d, canales: 6),
          [1, 1, 1, 1.41, 1.41, 0],
          "5.1 B (L R C Ls Rs LFE): el LFE sigue excluido aunque esté al final")
} else {
    comprobar(false, "no se pudo crear la descripción 5.1 B")
}

if let d = descripcionConLayout(kAudioChannelLayoutTag_MPEG_7_1_A, canales: 8) {
    igual(SonoridadMedia.pesosDeLaDisposicion(d, canales: 8),
          [1, 1, 1, 0, 1.41, 1.41, 1, 1],
          "7.1 A: LFE excluido, surrounds a +1,5 dB, el resto a uno")
} else {
    comprobar(false, "no se pudo crear la descripción 7.1 A")
}

print("— sin layout —")

// Sin layout no hay pesos: el medidor cae en su fallback por conteo, que es la
// convención 5.0 del estándar. Devolver nil aquí es el contrato honesto.
let estéreo = descripcionConLayout(kAudioChannelLayoutTag_Stereo, canales: 2)
comprobar(SonoridadMedia.pesosDeLaDisposicion(estéreo, canales: 2) == nil,
          "sin descripciones ni tag conocido, nil (fallback por conteo)")
comprobar(SonoridadMedia.pesosDeLaDisposicion(nil, canales: 2) == nil,
          "sin descripción, nil")

if fallos == 0 {
    print("LAYOUT CORRECTO")
} else {
    print("LAYOUT ROTO — \(fallos) fallos")
    exit(1)
}
