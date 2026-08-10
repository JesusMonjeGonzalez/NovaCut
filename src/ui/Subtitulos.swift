import AVFoundation
import Foundation
import Speech

enum SubtitulosService {
    static func leerSRT(_ texto: String, timebase: Timebase) -> [Subtitulo] {
        let normalizado = texto.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        return normalizado.components(separatedBy: "\n\n").compactMap { bloque in
            let lineas = bloque.components(separatedBy: "\n")
            guard let rangoIndex = lineas.firstIndex(where: { $0.contains(" --> ") }) else { return nil }
            let tiempos = lineas[rangoIndex].components(separatedBy: " --> ")
            guard tiempos.count == 2,
                  let inicio = frames(tiempo: tiempos[0], timebase: timebase),
                  let fin = frames(tiempo: tiempos[1], timebase: timebase),
                  fin > inicio else { return nil }
            let texto = lineas.dropFirst(rangoIndex + 1).joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !texto.isEmpty else { return nil }
            return Subtitulo(inicio: inicio, fin: fin, texto: texto)
        }
    }

    static func escribirSRT(_ subtitulos: [Subtitulo], timebase: Timebase) -> String {
        subtitulos.sorted { $0.inicio < $1.inicio }.enumerated().map { indice, subtitulo in
            let inicio = reloj(subtitulo.inicio, timebase: timebase)
            let fin = reloj(subtitulo.fin, timebase: timebase)
            return "\(indice + 1)\n\(inicio) --> \(fin)\n\(subtitulo.texto)"
        }.joined(separator: "\n\n") + (subtitulos.isEmpty ? "" : "\n")
    }

    /// Transcribe y devuelve las dos cosas que salen del mismo reconocimiento.
    ///
    /// El tiempo por palabra ya lo daba `Speech` y se tiraba al agrupar en líneas de
    /// subtítulo. Conservarlo no cuesta nada y es lo que sostiene la edición por texto
    /// y el resalte palabra a palabra: un dato, dos productos.
    static func transcribirCompleto(
        url: URL,
        mediaID: UUID,
        timebase: Timebase,
        idioma: String = "es-ES"
    ) async throws -> (cues: [Subtitulo], transcripcion: Transcripcion) {
        let transcripcion = try await reconocer(url: url, idioma: idioma)
        return (
            cues: cues(desde: transcripcion, timebase: timebase),
            transcripcion: palabras(desde: transcripcion, mediaID: mediaID, idioma: idioma)
        )
    }

    static func transcribir(url: URL, timebase: Timebase) async throws -> [Subtitulo] {
        cues(desde: try await reconocer(url: url, idioma: "es-ES"), timebase: timebase)
    }

    private static func reconocer(url: URL, idioma: String) async throws -> SFTranscription {
        let audioURL = try await prepararAudio(url: url)
        defer { try? FileManager.default.removeItem(at: audioURL) }

        let autorizado = await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { estado in
                continuation.resume(returning: estado == .authorized)
            }
        }
        guard autorizado else { throw SubtitulosError.sinPermiso }
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: idioma)), recognizer.isAvailable else {
            throw SubtitulosError.noDisponible
        }

        guard recognizer.supportsOnDeviceRecognition else { throw SubtitulosError.noDisponible }
        let request = SFSpeechURLRecognitionRequest(url: audioURL)
        request.requiresOnDeviceRecognition = true
        // Sin esto, `segments` llega con un único segmento por frase y la edición por
        // texto no tendría de dónde sacar el tiempo de cada palabra.
        request.shouldReportPartialResults = false

        // La tarea se cancela desde dos sitios: el timeout y la cancelación de la
        // tarea circundante. El guard de `terminado` hace que solo uno de los dos
        // resuma el continuation.
        let caja = CajaDeReconocimiento()
        return try await withTaskCancellationHandler(
            operation: {
                try await withCheckedThrowingContinuation { continuation in
                    var terminado = false
                    caja.tarea = recognizer.recognitionTask(with: request) { resultado, error in
                        if terminado { return }
                        if let error {
                            terminado = true
                            caja.tarea?.cancel()
                            continuation.resume(throwing: error)
                            return
                        }
                        guard let resultado, resultado.isFinal else { return }
                        terminado = true
                        continuation.resume(returning: resultado.bestTranscription)
                    }
                    // Un audio silencioso o un ruido continuo pueden no emitir
                    // nunca un resultado final; sin este timeout, «Transcribiendo…»
                    // quedaría clavado para siempre.
                    Task {
                        try? await Task.sleep(for: .seconds(120))
                        guard !terminado else { return }
                        terminado = true
                        caja.tarea?.cancel()
                        continuation.resume(throwing: SubtitulosError.tiempoAgotado)
                    }
                }
            },
            onCancel: {
                caja.tarea?.cancel()
            }
        )
    }

    /// El transcript palabra a palabra, con los tiempos en segundos del medio.
    private static func palabras(desde transcripcion: SFTranscription, mediaID: UUID, idioma: String) -> Transcripcion {
        Transcripcion(
            mediaID: mediaID,
            palabras: transcripcion.segments.map {
                Palabra(texto: $0.substring, inicio: $0.timestamp, duracion: max($0.duration, 0.02))
            },
            idioma: idioma
        )
    }

    private static func prepararAudio(url: URL) async throws -> URL {
        let asset = AVURLAsset(url: url)
        let salida = FileManager.default.temporaryDirectory
            .appendingPathComponent("editorcito-\(UUID().uuidString).m4a")
        guard let exportador = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetAppleM4A) else {
            throw SubtitulosError.audioNoDisponible
        }
        exportador.outputURL = salida
        exportador.outputFileType = .m4a
        await exportador.export()
        guard exportador.status == .completed else {
            throw exportador.error ?? SubtitulosError.audioNoDisponible
        }
        return salida
    }

    private static func cues(desde transcripcion: SFTranscription, timebase: Timebase) -> [Subtitulo] {
        var resultado: [Subtitulo] = []
        var texto = ""
        var inicio: Double?
        var fin: Double = 0

        for segmento in transcripcion.segments {
            if inicio == nil { inicio = segmento.timestamp }
            texto += (texto.isEmpty ? "" : " ") + segmento.substring
            fin = segmento.timestamp + segmento.duration

            let demasiadasPalabras = texto.split(separator: " ").count >= 8
            let demasiadoLargo = fin - (inicio ?? fin) >= 4.5
            if demasiadasPalabras || demasiadoLargo {
                if let inicio {
                    resultado.append(Subtitulo(
                        inicio: timebase.frames(segundos: inicio),
                        fin: timebase.frames(segundos: fin),
                        texto: texto
                    ))
                }
                texto = ""
                inicio = nil
            }
        }
        if let inicio, !texto.isEmpty {
            resultado.append(Subtitulo(
                inicio: timebase.frames(segundos: inicio),
                fin: timebase.frames(segundos: fin),
                texto: texto
            ))
        }
        return resultado
    }

    private static func frames(tiempo texto: String, timebase: Timebase) -> Int64? {
        let partes = texto.split(separator: ":")
        guard partes.count == 3,
              let horas = Double(partes[0]),
              let minutos = Double(partes[1]),
              let segundos = Double(partes[2].replacingOccurrences(of: ",", with: ".")) else { return nil }
        return timebase.frames(segundos: horas * 3600 + minutos * 60 + segundos)
    }

    private static func reloj(_ frames: Int64, timebase: Timebase) -> String {
        let segundos = timebase.segundos(frames)
        let horas = Int(segundos / 3600)
        let minutos = Int(segundos / 60) % 60
        let enteros = Int(segundos) % 60
        let milis = Int(((segundos - floor(segundos)) * 1000).rounded())
        return String(format: "%02d:%02d:%02d,%03d", horas, minutos, enteros, min(999, milis))
    }
}

enum SubtitulosError: LocalizedError {
    case sinPermiso
    case noDisponible
    case audioNoDisponible
    case tiempoAgotado

    var errorDescription: String? {
        switch self {
        case .sinPermiso: "macOS no ha concedido permiso para transcribir audio."
        case .noDisponible: "El reconocimiento de voz no está disponible en este Mac o idioma."
        case .audioNoDisponible: "No se pudo preparar el audio para transcribirlo."
        case .tiempoAgotado: "La transcripción tardó demasiado; el audio puede ser silencioso o ininteligible."
        }
    }
}

/// Referencia compartida a la tarea de reconocimiento, para cancelarla desde el
/// timeout o desde el handler de cancelación de la tarea circundante.
private final class CajaDeReconocimiento: @unchecked Sendable {
    var tarea: SFSpeechRecognitionTask?
}
