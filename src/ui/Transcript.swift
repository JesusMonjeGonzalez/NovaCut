import Foundation

/// Una palabra tal y como se oye **en el montaje**.
///
/// El transcript pertenece al medio; esto pertenece al montaje. La diferencia es todo
/// el trabajo: una palabra grabada puede no oírse (quedó fuera del recorte), oírse una
/// vez, u oírse tres si el medio se usó tres veces. El panel de texto tiene que
/// enseñar lo que se oye, en el orden en el que se oye, porque solo así borrar texto
/// significa borrar vídeo.
struct PalabraDelMontaje: Identifiable, Hashable, Sendable {
    var id = UUID()
    var texto: String
    /// Primer frame del montaje en el que se oye.
    var desde: Int64
    /// Primer frame en el que ya no se oye.
    var hasta: Int64
    var clipID: UUID
    var pistaID: UUID
    /// Índice de la palabra dentro del transcript del medio.
    var indiceEnMedio: Int
}

/// Un sitio donde se dijo lo que se buscaba.
struct Hallazgo: Identifiable, Hashable, Sendable {
    var id = UUID()
    var mediaID: UUID
    /// Segundo del archivo en el que empieza.
    var segundoEnElMedio: Double
    /// Frame del montaje donde se oye, o `nil` si ese material no está montado.
    var frame: Int64?
    /// Lo que se dijo alrededor, para poder elegir sin abrir el clip.
    var contexto: String
    var indiceEnMedio: Int
}

enum TranscriptService {

    /// Palabras de contexto a cada lado de una coincidencia.
    static let palabrasDeContexto = 6

    /// Minúsculas y sin acentos: quien busca escribe «cafe» y quiere encontrar «café».
    static func plegar(_ texto: String) -> String {
        texto.folding(options: [.diacriticInsensitive, .caseInsensitive, .widthInsensitive], locale: nil)
            .trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
    }

    /// Muletillas que se pueden proponer para borrar.
    ///
    /// La lista es corta a propósito. «bueno», «pues», «vale» o «claro» son muletilla
    /// a veces y palabra siempre, y una propuesta automática que borra de más cuesta la
    /// confianza en la herramienta entera: se recupera con deshacer, pero ya nadie
    /// vuelve a pulsar el botón. Aquí solo entra lo que no significa nada.
    static let muletillasSueltas: Set<String> = [
        "eh", "ehh", "eeh", "ehm", "em", "emm", "mm", "mmm", "hmm", "este", "esteee",
    ]

    /// Muletillas de dos palabras. Se marcan las dos o ninguna.
    static let muletillasCompuestas: [[String]] = [
        ["o", "sea"],
    ]

    /// Palabras que solo son muletilla cuando vienen con signos: «¿sabes?», «¿no?».
    /// «no» a secas es la palabra más significativa del idioma.
    static let muletillasInterrogativas: Set<String> = ["sabes", "no"]

    /// Rangos de montaje que hay que borrar para quitar las palabras seleccionadas.
    ///
    /// Dentro de un mismo clip, una selección seguida se funde en un solo rango: así se
    /// va también el silencio entre las palabras, que es lo que espera quien selecciona
    /// un párrafo. A través de un corte, no: entre dos clips del mismo medio puede
    /// haber material de otro que el panel de texto no enseña, y fundir ahí lo borraría
    /// sin que nadie lo haya pedido.
    static func rangos(de palabras: [PalabraDelMontaje], indices: [Int]) -> [(desde: Int64, hasta: Int64)] {
        let validos = indices.filter { $0 >= 0 && $0 < palabras.count }.sorted()
        guard let primero = validos.first else { return [] }

        var salida: [(desde: Int64, hasta: Int64)] = []
        var desde = palabras[primero].desde
        var hasta = palabras[primero].hasta
        var anterior = primero

        for indice in validos.dropFirst() {
            let seguida = indice == anterior + 1
            let mismoClip = palabras[indice].clipID == palabras[anterior].clipID
            if seguida && mismoClip {
                hasta = palabras[indice].hasta
            } else {
                salida.append((desde, hasta))
                desde = palabras[indice].desde
                hasta = palabras[indice].hasta
            }
            anterior = indice
        }
        salida.append((desde, hasta))
        return salida
    }

    /// Índices de las palabras que son muletilla.
    static func muletillas(en palabras: [PalabraDelMontaje]) -> [Int] {
        let limpias = palabras.map { normalizar($0.texto) }
        var marcadas = Set<Int>()

        for (indice, limpia) in limpias.enumerated() {
            if muletillasSueltas.contains(limpia) { marcadas.insert(indice) }
            if muletillasInterrogativas.contains(limpia), traeSignos(palabras[indice].texto) {
                marcadas.insert(indice)
            }
        }

        for compuesta in muletillasCompuestas {
            guard compuesta.count > 1, limpias.count >= compuesta.count else { continue }
            for inicio in 0...(limpias.count - compuesta.count) {
                let tramo = Array(limpias[inicio..<(inicio + compuesta.count)])
                guard tramo == compuesta else { continue }
                for desplazamiento in 0..<compuesta.count { marcadas.insert(inicio + desplazamiento) }
            }
        }

        return marcadas.sorted()
    }

    /// Minúsculas y sin signos: «¿Sabes?» y «sabes» son la misma palabra.
    static func normalizar(_ texto: String) -> String {
        texto.lowercased().trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
    }

    private static func traeSignos(_ texto: String) -> Bool {
        texto.contains(where: { "¿?¡!,.…".contains($0) })
    }
}

extension LineaDeTiempo {

    /// Las palabras de un transcript colocadas en el montaje.
    ///
    /// Una palabra entra tantas veces como clips la usen, en el orden en el que suenan.
    /// Las que quedaron fuera del recorte no entran: no se oyen, y enseñarlas haría que
    /// borrar texto no borrase nada.
    func palabrasDelMontaje(_ transcripcion: Transcripcion) -> [PalabraDelMontaje] {
        var salida: [PalabraDelMontaje] = []

        for pista in pistas {
            for clip in pista.clips where clip.mediaID == transcripcion.mediaID {
                // La velocidad convierte tiempo de origen en tiempo de montaje: a 2x,
                // un segundo de habla ocupa medio segundo de línea de tiempo.
                let velocidad = max(abs(clip.velocidad), 0.01)
                let entrada = clip.entradaEnOrigen
                let salidaEnOrigen = clip.salidaEnOrigen

                for (indice, palabra) in transcripcion.palabras.enumerated() {
                    let desdeEnOrigen = timebase.frames(segundos: palabra.inicio)
                    let hastaEnOrigen = max(desdeEnOrigen + 1, timebase.frames(segundos: palabra.fin))
                    // Se acepta el solape parcial: media palabra dentro del recorte se
                    // oye a medias, y esconderla dejaría un hueco inexplicable.
                    guard hastaEnOrigen > entrada, desdeEnOrigen < salidaEnOrigen else { continue }

                    let recortadoDesde = max(desdeEnOrigen, entrada)
                    let recortadoHasta = min(hastaEnOrigen, salidaEnOrigen)
                    let desde = clip.inicio + Int64((Double(recortadoDesde - entrada) / velocidad).rounded())
                    let hasta = clip.inicio + Int64((Double(recortadoHasta - entrada) / velocidad).rounded())
                    guard hasta > desde else { continue }

                    salida.append(PalabraDelMontaje(
                        texto: palabra.texto,
                        desde: desde,
                        hasta: min(hasta, clip.fin),
                        clipID: clip.id,
                        pistaID: pista.id,
                        indiceEnMedio: indice
                    ))
                }
            }
        }

        return salida.sorted { $0.desde < $1.desde }
    }

    /// Todas las palabras del proyecto, de todos los medios, en orden de montaje.
    func palabrasDelMontaje() -> [PalabraDelMontaje] {
        (transcripciones ?? [])
            .flatMap { palabrasDelMontaje($0) }
            .sorted { $0.desde < $1.desde }
    }

    /// El transcript de un medio, si se ha transcrito.
    func transcripcion(de mediaID: UUID) -> Transcripcion? {
        transcripciones?.first { $0.mediaID == mediaID }
    }

    /// Busca una palabra o una frase en todo lo que se ha transcrito.
    ///
    /// Es la respuesta honesta al `Media Intelligence` de Premiere y al `IntelliSearch`
    /// de Resolve para el material que nos importa: en una entrevista o un pódcast, casi
    /// todo lo que uno busca lo dijo alguien. Reconocer objetos y planos es otro
    /// proyecto y aquí no se promete.
    ///
    /// Encuentra también en medios que **no están montados**: buscar sirve justamente
    /// para localizar el trozo que todavía no se ha usado.
    func buscarEnLoQueSeDice(_ consulta: String) -> [Hallazgo] {
        let buscadas = consulta.split(separator: " ")
            .map { TranscriptService.plegar(String($0)) }
            .filter { !$0.isEmpty }
        guard !buscadas.isEmpty else { return [] }

        var salida: [Hallazgo] = []

        for transcripcion in transcripciones ?? [] {
            let plegadas = transcripcion.palabras.map { TranscriptService.plegar($0.texto) }
            guard plegadas.count >= buscadas.count else { continue }
            // Donde se oye cada palabra de este medio, para poder dar el frame.
            let enMontaje = palabrasDelMontaje(transcripcion)

            for inicio in 0...(plegadas.count - buscadas.count) {
                let tramo = Array(plegadas[inicio..<(inicio + buscadas.count)])
                guard tramo == buscadas else { continue }

                let desde = max(0, inicio - TranscriptService.palabrasDeContexto)
                let hasta = min(plegadas.count, inicio + buscadas.count + TranscriptService.palabrasDeContexto)
                salida.append(Hallazgo(
                    mediaID: transcripcion.mediaID,
                    segundoEnElMedio: transcripcion.palabras[inicio].inicio,
                    frame: enMontaje.first { $0.indiceEnMedio == inicio }?.desde,
                    contexto: transcripcion.palabras[desde..<hasta].map(\.texto).joined(separator: " "),
                    indiceEnMedio: inicio
                ))
            }
        }

        // Lo que ya está montado primero y en orden de montaje: es lo que se está
        // mirando. Lo que solo está en la biblioteca va detrás.
        return salida.sorted { a, b in
            switch (a.frame, b.frame) {
            case let (x?, y?): return x < y
            case (nil, _?): return false
            case (_?, nil): return true
            case (nil, nil): return a.segundoEnElMedio < b.segundoEnElMedio
            }
        }
    }
}
