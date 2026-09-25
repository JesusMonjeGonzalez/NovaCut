import Foundation

// MARK: - Intercambio con otros editores
//
// Un NLE profesional no es una isla: el montaje entra y sale por los formatos
// que toda la industria comparte. Estos dos exportadores son funciones puras
// sobre el modelo (no tocan UI ni AVFoundation), así que se verifican en el
// arnés sin abrir la aplicación.
//
//   EDL (CMX 3600)   — el formato de corte más antiguo que Premiere, Resolve y
//                      cualquier sala de máster siguen leyendo hoy.
//   FCPXML (1.11)    — el intercambio moderno de Final Cut y Premiere.
//
// Alcance honesto, documentado en FORMATOS.md:
//   - EDL: cortes con sus timecodes de origen y montaje, canales V/A1/A2,
//     velocidad constante (efecto de movimiento M2). Fundidos, rampas de
//     velocidad, títulos, anidados y multicámara no se representan: se anotan
//     como comentarios `*` para que nadie crea que viajaron.
//   - FCPXML: secuencia de una sola espina con los clips enlazados A/V unidos,
//     velocidades constantes y fundidos de opacidad. Las capas apiladas se
//     aplastan (gana la superior); títulos, anidados y multicámara se omiten
//     con nota. Los proyectos con esos elementos exportan igualmente el resto.

/// Lo mínimo que un exportador necesita saber de un medio. Se pasa desde la
/// app (App.swift) sin acoplar los exportadores a sus tipos.
struct MedioParaExportar {
    var nombre: String
    var url: URL
    var duracionSegundos: Double
    var tamano: CGSize?
    var fps: Double?
}
enum EDLDeEditorcito {

    /// Genera un EDL CMX 3600 del montaje.
    ///
    /// - Parameters:
    ///   - montaje: la línea de tiempo completa.
    ///   - medios: tabla de medios por id, solo los que usa el montaje.
    ///   - titulo: nombre del proyecto, para la cabecera `TITLE:`.
    static func exportar(montaje: LineaDeTiempo, medios: [UUID: MedioParaExportar], titulo: String) -> String {
        var lineas: [String] = []
        lineas.append("TITLE: \(titulo)")
        lineas.append("FCM: \(montaje.timebase.dropFrame ? "DROP FRAME" : "NON-DROP FRAME")")
        lineas.append("")

        var numero = 0
        var reelesUsados: [String: String] = [:]
        let pistasDeAudio = montaje.pistas.enumerated().filter { $0.element.tipo == .audio }

        func reelPara(_ medio: MedioParaExportar?) -> String {
            guard let medio else { return "AX" }
            // El mismo medio usa el mismo reel en todos sus eventos (vídeo y
            // audio juntos): la memo va por archivo, no por evento.
            let clave = medio.url.absoluteString
            if let existente = reelesUsados[clave] { return existente }
            let crudo = medio.nombre.uppercased().filter { $0.isLetter || $0.isNumber }
            var base = crudo.isEmpty ? "AX" : String(crudo.prefix(7))
            // Dos archivos con el mismo nombre no pueden compartir reel: se
            // distingue el segundo con un sufijo.
            let yaUsado = reelesUsados.values.contains(base)
            if yaUsado {
                var sufijo = 2
                while reelesUsados.values.contains(String(base.prefix(6)) + String(sufijo)) {
                    sufijo += 1
                }
                base = String(base.prefix(6)) + String(sufijo)
            }
            reelesUsados[clave] = base
            return base
        }

        for pista in montaje.pistas {
            for clip in pista.clips.sorted(by: { $0.inicio < $1.inicio }) {
                guard clip.habilitado else { continue }
                let medio = clip.esAjuste || clip.esTitulo ? nil : medios[clip.mediaID]

                numero += 1
                let reel = reelPara(medio)
                let canal: String
                switch pista.tipo {
                case .video: canal = "V"
                case .audio:
                    let indice = (pistasDeAudio.firstIndex { $0.element.id == pista.id } ?? -1) + 1
                    canal = "A\(max(1, indice))"
                }

                let fuenteIn = montaje.timebase.timecode(clip.entradaEnOrigen)
                let fuenteOut = montaje.timebase.timecode(clip.entradaEnOrigen + clip.duracionEnOrigen)
                let grabacionIn = montaje.timebase.timecode(clip.inicio)
                let grabacionOut = montaje.timebase.timecode(clip.fin)

                // Formato de columnas CMX: las líneas de evento no pasan de 80
                // caracteres, que es el ancho que las salas de máster siguen
                // esperando.
                let reelColumna = reel.padding(toLength: 7, withPad: " ", startingAt: 0)
                let canalColumna = canal.padding(toLength: 3, withPad: " ", startingAt: 0)
                lineas.append("\(String(format: "%03d", numero))  \(reelColumna)   \(canalColumna)  C      \(fuenteIn) \(fuenteOut) \(grabacionIn) \(grabacionOut)")
                lineas.append("* FROM CLIP NAME: \(clip.nombre.isEmpty ? (medio?.nombre ?? "sin medio") : clip.nombre)")

                // Solo la velocidad constante cabe en un efecto de movimiento.
                // Las rampas, fundidos, títulos y anidados viajan como nota:
                // mejor un EDL que no miente que uno que silencia el resto.
                if clip.velocidad != 1, clip.rampasDeVelocidad?.isEmpty != false {
                    lineas.append("* SPEED CHANGE RATE: \(Int(clip.velocidad * 100))")
                } else if clip.velocidad != 1 {
                    lineas.append("* RAMPA DE VELOCIDAD (1×…\(String(format: "%.2f", clip.velocidad))×): no representable en EDL — exportado sin retime")
                }
                if clip.transicionEntrada != nil || clip.transicionSalida != nil {
                    lineas.append("* FUNDIDO/TRANSICIÓN en un corte: no representable en EDL")
                }
                if clip.esTitulo { lineas.append("* TÍTULO (texto): no representable en EDL") }
                if clip.nido != nil { lineas.append("* CLIP ANIDADO: no representable en EDL") }
                if clip.multicam != nil { lineas.append("* MULTICÁMARA: solo viaja el montaje actual, sin ángulos") }
                if pista.silenciada { lineas.append("* PISTA SILENCIADA: el medio suena, la mezcla no se exporta") }
                lineas.append("")
            }
        }
        return lineas.joined(separator: "\n") + "\n"
    }
}

/// Un evento leído de un EDL, antes de convertirlo en clips.
struct EventoDeEDL: Equatable {
    var numero: Int
    var reel: String
    var canal: String
    /// `C` es corte; `D` disolución; `W` cortinilla. Solo el corte es exacto.
    var tipo: String
    var fuenteIn: Int64
    var fuenteOut: Int64
    var grabacionIn: Int64
    var grabacionOut: Int64
    var nombre: String?
    /// 1 salvo que un `M2` o un comentario de velocidad diga otra cosa.
    var velocidad: Double = 1
}

struct ResultadoDeImportacionEDL {
    var titulo: String?
    var montaje: LineaDeTiempo
    var eventos: [EventoDeEDL]
    /// Medio inventado para cada reel: el EDL no trae archivos, así que los
    /// clips entran offline y se localizan con la revinculación en lote.
    var mediosPorReel: [String: UUID]
    var nombresPorReel: [String: String]
    var avisos: [String]
}

extension EDLDeEditorcito {

    /// Lee un EDL CMX 3600 y lo convierte en montaje.
    ///
    /// Un EDL describe cortes, no medios: no dice dónde están los archivos ni
    /// qué códec traen. Lo que entra es la estructura —qué trozo de qué cinta va
    /// en qué punto—, y los medios quedan offline hasta revincularlos. Es
    /// preferible a rechazar el archivo: la lista de cortes es justo el trabajo
    /// que costaría rehacer a mano.
    ///
    /// Alcance: cortes con timecodes de origen y montaje, canales V/A1..A4/B/AA,
    /// nombres `FROM CLIP NAME` y velocidad constante (`M2` o
    /// `* SPEED CHANGE RATE`). Las disoluciones y cortinillas entran como corte
    /// con aviso, porque un EDL no lleva la forma de la transición.
    static func importar(_ texto: String, timebase: Timebase) -> ResultadoDeImportacionEDL {
        var titulo: String?
        var dropFrame: Bool?
        var eventos: [EventoDeEDL] = []
        var avisos: [String] = []

        for cruda in texto.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n") {
            let linea = cruda.trimmingCharacters(in: .whitespaces)
            if linea.isEmpty { continue }

            if linea.uppercased().hasPrefix("TITLE:") {
                titulo = String(linea.dropFirst("TITLE:".count)).trimmingCharacters(in: .whitespaces)
                continue
            }
            if linea.uppercased().hasPrefix("FCM:") {
                dropFrame = linea.uppercased().contains("NON-DROP") ? false : linea.uppercased().contains("DROP")
                continue
            }
            if linea.hasPrefix("*") {
                aplicarComentario(String(linea.dropFirst()).trimmingCharacters(in: .whitespaces), a: &eventos)
                continue
            }
            if linea.uppercased().hasPrefix("M2") {
                aplicarM2(linea, a: &eventos, timebase: timebase, avisos: &avisos)
                continue
            }
            if let evento = leerEvento(linea, timebase: timebase) {
                if evento.tipo != "C" {
                    avisos.append("Evento \(evento.numero): «\(evento.tipo)» entra como corte; un EDL no lleva la forma de la transición")
                }
                eventos.append(evento)
            } else if linea.first?.isNumber == true {
                avisos.append("Línea ilegible, omitida: \(linea.prefix(40))")
            }
        }

        var base = timebase
        if let dropFrame, dropFrame != timebase.dropFrame {
            let candidata = Timebase(numerador: timebase.numerador, denominador: timebase.denominador, dropFrame: dropFrame)
            if candidata.dropFrame == dropFrame {
                base = candidata
            } else {
                avisos.append("El EDL declara \(dropFrame ? "DROP FRAME" : "NON-DROP FRAME"), que no aplica a \(timebase.nombre)")
            }
        }
        return montar(eventos, timebase: base, titulo: titulo, avisos: avisos)
    }

    // MARK: Lectura

    private static func leerEvento(_ linea: String, timebase: Timebase) -> EventoDeEDL? {
        let campos = linea.split(whereSeparator: \.isWhitespace).map(String.init)
        // Número, reel, canal, tipo, [duración de transición] y cuatro timecodes.
        guard campos.count >= 8, let numero = Int(campos[0]), numero >= 0 else { return nil }
        let tiempos = campos.suffix(4).compactMap { timebase.frames(timecode: $0) }
        guard tiempos.count == 4 else { return nil }
        let tipo = campos[3].uppercased()
        guard tipo.first?.isLetter == true else { return nil }
        return EventoDeEDL(
            numero: numero,
            reel: campos[1],
            canal: campos[2].uppercased(),
            tipo: String(tipo.prefix(1)),
            fuenteIn: tiempos[0], fuenteOut: tiempos[1],
            grabacionIn: tiempos[2], grabacionOut: tiempos[3]
        )
    }

    private static func aplicarComentario(_ comentario: String, a eventos: inout [EventoDeEDL]) {
        guard !eventos.isEmpty else { return }
        let mayusculas = comentario.uppercased()
        if mayusculas.hasPrefix("FROM CLIP NAME:") {
            let nombre = String(comentario.dropFirst("FROM CLIP NAME:".count)).trimmingCharacters(in: .whitespaces)
            if !nombre.isEmpty { eventos[eventos.count - 1].nombre = nombre }
        } else if mayusculas.hasPrefix("SPEED CHANGE RATE:") {
            let valor = String(comentario.dropFirst("SPEED CHANGE RATE:".count))
                .trimmingCharacters(in: .whitespaces)
                .replacingOccurrences(of: "%", with: "")
            if let porcentaje = Double(valor), porcentaje != 0, porcentaje.isFinite {
                eventos[eventos.count - 1].velocidad = porcentaje / 100
            }
        }
    }

    /// `M2  REEL  048.0  01:00:00:00`: la velocidad va en fotogramas por segundo
    /// del origen, así que se compara con la cadencia del montaje.
    private static func aplicarM2(_ linea: String, a eventos: inout [EventoDeEDL], timebase: Timebase, avisos: inout [String]) {
        guard let ultimo = eventos.indices.last else { return }
        let campos = linea.split(whereSeparator: \.isWhitespace).map(String.init)
        guard campos.count >= 3, let ritmo = Double(campos[2]), ritmo.isFinite else { return }
        let nominal = Double(timebase.fpsNominal)
        guard nominal > 0 else { return }
        if ritmo == 0 {
            avisos.append("Evento \(eventos[ultimo].numero): congelado (M2 a 0 fps); entra a velocidad normal")
            return
        }
        eventos[ultimo].velocidad = ritmo / nominal
    }

    // MARK: Montaje

    private static func montar(
        _ eventos: [EventoDeEDL],
        timebase: Timebase,
        titulo: String?,
        avisos: [String]
    ) -> ResultadoDeImportacionEDL {
        var montaje = LineaDeTiempo.nueva(timebase: timebase)
        var avisos = avisos
        var mediosPorReel: [String: UUID] = [:]
        var nombresPorReel: [String: String] = [:]

        for evento in eventos {
            let duracion = evento.grabacionOut - evento.grabacionIn
            guard duracion > 0, evento.grabacionIn >= 0 else {
                avisos.append("Evento \(evento.numero): duración no positiva en el montaje, omitido")
                continue
            }
            let medio = mediosPorReel[evento.reel] ?? UUID()
            mediosPorReel[evento.reel] = medio
            if let nombre = evento.nombre, nombresPorReel[evento.reel] == nil {
                nombresPorReel[evento.reel] = nombre
            }

            var clip = Clip(
                mediaID: medio,
                nombre: evento.nombre ?? evento.reel,
                inicio: evento.grabacionIn,
                duracion: duracion,
                entradaEnOrigen: max(0, evento.fuenteIn)
            )
            clip.velocidad = evento.velocidad

            let canales = canalesDe(evento.canal)
            if canales.isEmpty {
                avisos.append("Evento \(evento.numero): canal «\(evento.canal)» desconocido, omitido")
                continue
            }
            // Vídeo y audio del mismo evento viajan enlazados, como el plano que
            // eran en la sala de montaje.
            let enlace = canales.count > 1 ? UUID() : nil
            for canal in canales {
                var copia = clip
                copia.id = UUID()
                copia.enlace = enlace
                colocar(copia, canal: canal, en: &montaje)
            }
        }
        return ResultadoDeImportacionEDL(
            titulo: titulo,
            montaje: montaje,
            eventos: eventos,
            mediosPorReel: mediosPorReel,
            nombresPorReel: nombresPorReel,
            avisos: avisos
        )
    }

    /// `B` es vídeo y audio; `AA` los dos primeros canales de audio; `A` es A1.
    private static func canalesDe(_ canal: String) -> [(tipo: TipoDePista, indice: Int)] {
        switch canal {
        case "V": return [(.video, 1)]
        case "B", "VA", "AV": return [(.video, 1), (.audio, 1)]
        case "A", "A1": return [(.audio, 1)]
        case "AA", "A1A2", "AA/V": return [(.audio, 1), (.audio, 2)]
        case "NONE": return []
        default:
            guard canal.hasPrefix("A"), let indice = Int(canal.dropFirst()), indice > 0, indice <= 16 else { return [] }
            return [(.audio, indice)]
        }
    }

    /// Coloca el clip en la pista pedida; si ese hueco está ocupado, baja a la
    /// siguiente del mismo tipo y crea una nueva si hace falta. Solapar dos
    /// clips en la misma pista corrompería el montaje.
    private static func colocar(_ clip: Clip, canal: (tipo: TipoDePista, indice: Int), en montaje: inout LineaDeTiempo) {
        let candidatas = montaje.pistas.indices.filter { montaje.pistas[$0].tipo == canal.tipo }
        // Las pistas de vídeo se listan de arriba abajo (V2, V1): el canal 1 es
        // la de más abajo, que es donde un EDL espera el corte principal.
        let ordenadas = canal.tipo == .video ? candidatas.reversed().map { $0 } : candidatas
        let desde = min(canal.indice - 1, max(0, ordenadas.count - 1))
        for posicion in ordenadas.dropFirst(desde) {
            let ocupada = montaje.pistas[posicion].clips.contains { $0.inicio < clip.fin && clip.inicio < $0.fin }
            if !ocupada {
                montaje.pistas[posicion].clips.append(clip)
                montaje.pistas[posicion].clips.sort { $0.inicio < $1.inicio }
                return
            }
        }
        let numero = montaje.pistas.filter { $0.tipo == canal.tipo }.count + 1
        var pista = Pista(tipo: canal.tipo, nombre: "\(canal.tipo == .video ? "V" : "A")\(numero)")
        pista.clips = [clip]
        if canal.tipo == .video {
            montaje.pistas.insert(pista, at: 0)
        } else {
            montaje.pistas.append(pista)
        }
    }
}

enum FCPXMLDeEditorcito {

    /// Genera un documento FCPXML 1.11 del montaje, listo para abrir en Final
    /// Cut Pro o importar en Premiere Pro.
    static func exportar(montaje: LineaDeTiempo, medios: [UUID: MedioParaExportar], titulo: String) -> Data {
        let raiz = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n"
        let esDrop = montaje.timebase.dropFrame ? "DF" : "NDF"

        // Los elementos se montan con un acumulador: los clips de vídeo van a la
        // espina y los de audio se cuelgan del mismo elemento cuando el enlace
        // A/V los une; los no enlazados van aparte.
        var cuerpos: [String] = []
        var audioSuelto: [String] = []
        var videoEmitido = Set<UUID>()
        var notas: [String] = []
        var refs: [UUID: String] = [:]


        // Clips de vídeo, por pista y posición. Los apilados se aplastan
        // recortando el clip inferior a los tramos que ninguna pista superior
        // cubre: gana la pista superior (la que el compositor dibuja encima).
        let pistasDeVideo = montaje.pistas.filter { $0.tipo == .video }
        var ocupado: [(desde: Int64, hasta: Int64)] = []
        for pista in pistasDeVideo {
            for clip in pista.clips.sorted(by: { $0.inicio < $1.inicio }) {
                guard clip.habilitado else { continue }
                guard let medio = medios[clip.mediaID] else { continue }
                if clip.esTitulo {
                    notas.append("Título «\(clip.nombre)» omitido: FCPXML de títulos requiere plantillas")
                    continue
                }
                if clip.nido != nil {
                    notas.append("Clip anidado omitido: FCPXML 1.11 no transporta secuencias internas")
                    continue
                }
                if clip.multicam != nil {
                    notas.append("Multicámara exportada con el ángulo activo")
                }

                // Tramos del clip que ninguna pista superior cubre.
                var libres = tramosLibres(desde: clip.inicio, hasta: clip.fin, ocupados: ocupado)
                if libres.isEmpty {
                    notas.append("Capa apilada «\(clip.nombre)» en \(montaje.timebase.timecode(clip.inicio)): la superior la cubre entera, se aplana")
                    if let enlace = clip.enlace, let audio = audioEnlazado(montaje: montaje, enlace: enlace, video: clip) {
                        audioSuelto.append(contenidoDeAudio(audio, medios: medios, montaje: montaje, refs: &refs))
                    }
                    continue
                }
                ocupado = fundir(ocupado, con: (desde: clip.inicio, hasta: clip.fin))

                // El clip recortado conserva la velocidad constante: la parte
                // emitida avanza su entrada en origen lo que consumió la parte
                // cubierta. Las rampas quedan aproximadas (ya lo anuncia la nota).
                if libres.count > 1 || libres[0].0 > clip.inicio || libres[0].1 < clip.fin {
                    notas.append("Capa apilada «\(clip.nombre)»: la superior cubre un tramo, se recorta")
                }
                for (desde, hasta) in libres {
                    var pieza = clip
                    pieza.inicio = desde
                    pieza.duracion = hasta - desde
                    if clip.velocidad != 1, clip.rampasDeVelocidad?.isEmpty != false {
                        pieza.entradaEnOrigen += Int64((Double(desde - clip.inicio) * clip.velocidad).rounded())
                    } else if clip.velocidad != 1 {
                        pieza.entradaEnOrigen += desde - clip.inicio
                    } else {
                        pieza.entradaEnOrigen += desde - clip.inicio
                    }
                    pieza.keyframes = nil
                    cuerpos.append(contenidoDeVideo(pieza, medios: medios, montaje: montaje, refs: &refs, notas: &notas))
                }
                videoEmitido.insert(clip.id)
            }
        }

        // Audio no enlazado a vídeo (o enlazado a un vídeo descartado).
        for pista in montaje.pistas where pista.tipo == .audio {
            for clip in pista.clips.sorted(by: { $0.inicio < $1.inicio }) {
                guard clip.habilitado else { continue }
                if let enlace = clip.enlace {
                    // Ya viaja con su vídeo si este se emitió.
                    let viajaConVideo = montaje.pistas
                        .filter { $0.tipo == .video }
                        .flatMap(\.clips)
                        .contains { $0.enlace == enlace && videoEmitido.contains($0.id) }
                    if viajaConVideo { continue }
                }
                audioSuelto.append(contenidoDeAudio(clip, medios: medios, montaje: montaje, refs: &refs))
            }
        }
        cuerpos.append(contentsOf: audioSuelto)

        // Recursos: un formato por cadencia (con el tamaño del primer vídeo) y
        // un asset por medio usado.
        var recursos: [String] = []
        let formato = refDeFormato(montaje, medios: medios)
        recursos.append("<format id=\"\(formato.id)\" name=\"\(formato.nombre)\" frameDuration=\"\(formato.duracionDeFrame)\"\(formato.dimensiones)/>")
        var recursosDeMedio = Set<UUID>()
        for pista in montaje.pistas {
            for clip in pista.clips where clip.habilitado && !clip.esAjuste && !clip.esTitulo {
                guard let medio = medios[clip.mediaID], recursosDeMedio.insert(clip.mediaID).inserted else { continue }
                let r = refDe(clip.mediaID, refs: &refs)
                let duracion = String(format: "%.6f", medio.duracionSegundos) + "s"
                let conVideo = (medio.tamano?.width ?? 0) > 0
                let conAudio = true
                let atrFormato = conVideo ? " format=\"\(formato.id)\"" : ""
                let src = escape(medio.url.absoluteString)
                recursos.append("<asset id=\"\(r)\" name=\"\(escape(medio.nombre))\" uid=\"\(clip.mediaID.uuidString)\" start=\"0s\" duration=\"\(duracion)\" hasVideo=\"\(conVideo ? 1 : 0)\" hasAudio=\"\(conAudio ? 1 : 0)\"\(atrFormato)>")
                recursos.append("<media-rep kind=\"original-media\" src=\"\(src)\"/>")
                recursos.append("</asset>")
            }
        }

        let duracionSecuencia = racional(montaje.duracion, montaje.timebase)
        var xml = raiz
        xml += "<fcpxml version=\"1.11\">\n"
        xml += " <resources>\n" + recursos.map { "  \($0)" }.joined(separator: "\n") + "\n </resources>\n"
        xml += " <library>\n  <event name=\"\(escape(titulo))\">\n   <project name=\"\(escape(titulo))\" uid=\"\(UUID().uuidString)\">\n"
        xml += "    <sequence format=\"\(formato.id)\" duration=\"\(duracionSecuencia)\" tcStart=\"0s\" tcFormat=\"\(esDrop)\">\n"
        xml += "     <spine>\n"
        xml += cuerpos.map { "      \($0)" }.joined(separator: "\n") + "\n"
        xml += "     </spine>\n"
        if !notas.isEmpty {
            xml += "     <note>Exportado por Editorcito. Limitaciones: \(escape(notas.joined(separator: " · ")))</note>\n"
        }
        xml += "    </sequence>\n   </project>\n  </event>\n </library>\n"
        xml += "</fcpxml>\n"
        return Data(xml.utf8)
    }

    // MARK: - Piezas internas

    /// Tramo del clip que ninguna pista superior cubre.
    private static func tramosLibres(desde: Int64, hasta: Int64, ocupados: [(desde: Int64, hasta: Int64)]) -> [(Int64, Int64)] {
        var resultado: [(Int64, Int64)] = []
        var cursor = desde
        for o in ocupados where o.hasta > cursor {
            if o.desde > cursor { resultado.append((cursor, min(o.desde, hasta))) }
            cursor = max(cursor, o.hasta)
            if cursor >= hasta { break }
        }
        if cursor < hasta { resultado.append((cursor, hasta)) }
        return resultado
    }

    /// Añade un tramo al mapa de ocupado manteniéndolo ordenado y fundido.
    private static func fundir(_ ocupados: [(desde: Int64, hasta: Int64)], con nuevo: (desde: Int64, hasta: Int64)) -> [(desde: Int64, hasta: Int64)] {
        var mezclado = ocupados + [nuevo]
        mezclado.sort { $0.desde < $1.desde }
        var resultado: [(desde: Int64, hasta: Int64)] = []
        for o in mezclado {
            if let ultimo = resultado.last, o.desde <= ultimo.hasta {
                resultado[resultado.count - 1].hasta = max(ultimo.hasta, o.hasta)
            } else {
                resultado.append(o)
            }
        }
        return resultado
    }

    /// Un `asset-clip` de vídeo (con su audio enlazado si lo hay).
    private static func contenidoDeVideo(_ clip: Clip, medios: [UUID: MedioParaExportar], montaje: LineaDeTiempo, refs: inout [UUID: String], notas: inout [String]) -> String {
        guard let medio = medios[clip.mediaID] else { return "" }
        let velocidad = clip.velocidad
        let atrVelocidad = velocidad != 1 ? " speed=\"\(Int((velocidad * 100).rounded()))\"" : ""
        // Con `speed`, la duración se mide en tiempo de origen.
        let framesDeDuracion = velocidad != 1 ? clip.duracionEnOrigen : clip.duracion
        let duracion = racional(framesDeDuracion, montaje.timebase)
        let inicio = racional(clip.entradaEnOrigen, montaje.timebase)
        let offset = racional(clip.inicio, montaje.timebase)

        var audioXml = ""
        if let enlace = clip.enlace, let audio = audioEnlazado(montaje: montaje, enlace: enlace, video: clip) {
            audioXml = contenidoDeAudio(audio, medios: medios, montaje: montaje, comoAtributos: true, refs: &refs)
        }
        var apertura = "<asset-clip name=\"\(escape(clip.nombre.isEmpty ? medio.nombre : clip.nombre))\" ref=\"\(refDe(clip.mediaID, refs: &refs))\"" +
            " offset=\"\(offset)\" start=\"\(inicio)\" duration=\"\(duracion)\"\(atrVelocidad)"
        if !audioXml.isEmpty { apertura += "\(audioXml)>" } else { apertura += "/>" }

        if clip.rampasDeVelocidad?.isEmpty == false {
            notas.append("Rampa de velocidad en «\(clip.nombre)» exportada a velocidad constante media")
        }
        return apertura
    }

    /// Referencia estable por medio dentro de un mismo documento.
    private static func refDe(_ id: UUID, refs: inout [UUID: String]) -> String {
        if let existente = refs[id] { return existente }
        let nuevo = "medio-\(refs.count + 1)"
        refs[id] = nuevo
        return nuevo
    }

    /// El clip de audio enlazado al vídeo: mismo `enlace` y tramo solapado.
    /// El `enlace` puede repetirse por error de montaje, así que la posición
    /// decide; si sigue sin haber pareja, el audio viaja solo.
    private static func audioEnlazado(montaje: LineaDeTiempo, enlace: UUID, video: Clip) -> Clip? {
        montaje.pistas
            .first { $0.tipo == .audio }?
            .clips
            .filter { $0.enlace == enlace }
            .first { $0.solapaCon(inicio: video.inicio, fin: video.fin) }
            ?? montaje.pistas.first { $0.tipo == .audio }?.clips.first { $0.enlace == enlace }
    }

    /// Atributos de audio de un clip: `audioStart`/`audioDuration`, o el
    /// elemento `<audio>` si `comoAtributos` es falso.
    private static func contenidoDeAudio(_ clip: Clip, medios: [UUID: MedioParaExportar], montaje: LineaDeTiempo, comoAtributos: Bool = false, refs: inout [UUID: String]) -> String {
        guard let medio = medios[clip.mediaID] else { return "" }
        let inicio = racional(clip.entradaEnOrigen, montaje.timebase)
        let duracion = racional(clip.duracionEnOrigen, montaje.timebase)
        let offset = racional(clip.inicio, montaje.timebase)
        if comoAtributos {
            return " audioStart=\"\(inicio)\" audioDuration=\"\(duracion)\""
        }
        return "<asset-clip name=\"\(escape(clip.nombre.isEmpty ? medio.nombre : clip.nombre))\" ref=\"\(refDe(clip.mediaID, refs: &refs))\"" +
            " offset=\"\(offset)\" start=\"\(inicio)\" duration=\"\(duracion)\" audioStart=\"\(inicio)\" audioDuration=\"\(duracion)\"/>"
    }

    /// `1200/24000s` exacto para frames enteros sobre la base de tiempo.
    private static func racional(_ frames: Int64, _ timebase: Timebase) -> String {
        let valor = frames * Int64(timebase.denominador)
        return "\(valor)/\(timebase.numerador)s"
    }

    private struct Formato {
        let id: String
        let nombre: String
        let duracionDeFrame: String
        let dimensiones: String
    }

    private static func refDeFormato(_ montaje: LineaDeTiempo, medios: [UUID: MedioParaExportar]) -> Formato {
        let tb = montaje.timebase
        let dimension = medios.values.first { ($0.tamano?.width ?? 0) > 0 }?.tamano
        let ancho = Int(dimension?.width ?? 1920)
        let alto = Int(dimension?.height ?? 1080)
        let nombre = "FFVideoFormat\(ancho)p\(tb.fpsNominal)"
        return Formato(
            id: "formato",
            nombre: nombre,
            duracionDeFrame: "\(tb.denominador)/\(tb.numerador)s",
            dimensiones: " width=\"\(ancho)\" height=\"\(alto)\""
        )
    }

        private static func escape(_ texto: String) -> String {
        texto
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&apos;")
    }
}
