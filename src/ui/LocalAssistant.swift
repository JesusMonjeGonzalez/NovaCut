import Foundation

/// Una orden de montaje propuesta por el modelo.
///
/// El clip se identifica por el número que se le enseña en el contexto, no por su
/// UUID: un modelo copia mal un identificador de 36 caracteres y acierta siempre
/// con «el 3». El estado traduce el número al clip real antes de tocar nada.
struct NovaEditAction: Decodable {
    let kind: String
    let clip: Int?
    /// Posiciones en timecode `HH:MM:SS:FF` o en segundos, según lo que escriba.
    let entrada: String?
    let salida: String?
    let destino: String?
    let valor: Double?
    let texto: String?

    private enum CodingKeys: String, CodingKey {
        case kind, clip, entrada, salida, destino, valor, texto
        case sourceIn, sourceOut, to, value, name, action, index
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = ((try? c.decode(String.self, forKey: .kind))
            ?? (try? c.decode(String.self, forKey: .action)) ?? "")
            .trimmingCharacters(in: .whitespaces).lowercased()
        clip = (try? c.decode(Int.self, forKey: .clip)) ?? (try? c.decode(Int.self, forKey: .index))
        entrada = Self.texto(c, .entrada) ?? Self.texto(c, .sourceIn)
        salida = Self.texto(c, .salida) ?? Self.texto(c, .sourceOut)
        destino = Self.texto(c, .destino) ?? Self.texto(c, .to)
        valor = (try? c.decode(Double.self, forKey: .valor)) ?? (try? c.decode(Double.self, forKey: .value))
        texto = (try? c.decode(String.self, forKey: .texto)) ?? (try? c.decode(String.self, forKey: .name))
    }

    /// Lee un campo que el modelo puede haber escrito como número o como texto.
    private static func texto(_ c: KeyedDecodingContainer<CodingKeys>, _ clave: CodingKeys) -> String? {
        if let s = try? c.decode(String.self, forKey: clave) { return s }
        if let d = try? c.decode(Double.self, forKey: clave) { return String(d) }
        return nil
    }
}

struct NovaEditPlan: Decodable {
    let summary: String
    let actions: [NovaEditAction]

    private enum CodingKeys: String, CodingKey { case summary, resumen, actions, acciones, operaciones }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        summary = (try? c.decode(String.self, forKey: .summary))
            ?? (try? c.decode(String.self, forKey: .resumen)) ?? ""
        actions = (try? c.decode([NovaEditAction].self, forKey: .actions))
            ?? (try? c.decode([NovaEditAction].self, forKey: .acciones))
            ?? (try? c.decode([NovaEditAction].self, forKey: .operaciones)) ?? []
    }
}

enum NovaAssistant {

    /// Las órdenes que el estado sabe ejecutar. Es la lista que valida el plan y la
    /// que se le enseña al modelo, en la misma constante para que no diverjan.
    static let ordenes = [
        "recortar", "mover", "quitar", "quitar_cerrando", "cortar",
        "silenciar", "ganancia", "fundido", "velocidad", "marcador", "etiquetar",
    ]

    static func plan(request: String, timeline: String, selection: AISelection) async throws -> NovaEditPlan {
        let instruccion = instruccionDeSistema()
        let texto = try await AITransport.complete(
            selection: selection,
            system: instruccion,
            user: "Montaje actual:\n\(timeline)\n\nPetición:\n\(request)",
            maxTokens: 2000
        )
        return try decodificar(texto)
    }

    /// Lo mismo que [plan], con una miniatura del cabezal para que el modelo
    /// pueda referirse a lo que ve. El presupuesto sube porque describir la
    /// imagen se lleva tokens, igual que en Yunkil.
    static func planConImagen(
        request: String,
        timeline: String,
        selection: AISelection,
        imagenJpeg: Data
    ) async throws -> NovaEditPlan {
        let instruccion = instruccionDeSistema()
        let texto = try await AITransport.complete(
            selection: selection,
            system: instruccion,
            user: "Montaje actual:\n\(timeline)\n\n" +
                "El frame del cabezal se adjunta como imagen; la orden puede " +
                "referirse a lo que se ve en él.\n\nPetición:\n\(request)",
            maxTokens: 3000,
            imagenJpeg: imagenJpeg
        )
        return try decodificar(texto)
    }

    private static func instruccionDeSistema() -> String {
        """
        Eres el asistente de montaje de Editorcito. Devuelves SOLO JSON, sin markdown
        ni explicaciones, con esta forma:

        {"resumen":"...","acciones":[ ... ]}

        Órdenes disponibles (campo "kind"):
        {"kind":"recortar","clip":1,"entrada":"00:00:02:00","salida":"00:00:08:12"}
        {"kind":"mover","clip":2,"destino":"00:00:10:00"}
        {"kind":"quitar","clip":3}
        {"kind":"quitar_cerrando","clip":3}
        {"kind":"cortar","clip":1,"destino":"00:00:05:00"}
        {"kind":"silenciar","clip":2}
        {"kind":"ganancia","clip":2,"valor":-6}
        {"kind":"fundido","clip":1,"valor":1.5}
        {"kind":"velocidad","clip":1,"valor":2}
        {"kind":"marcador","destino":"00:00:30:00","texto":"Entrevista"}
        {"kind":"etiquetar","clip":1,"texto":"verde"}

        Reglas:
        - "clip" es el número con el que aparece en el listado, empezando en 1.
        - Los tiempos van en timecode HH:MM:SS:FF del montaje, no en segundos.
        - "recortar" usa entrada y salida **en la línea de tiempo**, no del archivo.
        - "quitar" deja el hueco; "quitar_cerrando" lo cierra arrastrando la cola.
        - "ganancia" va en decibelios, "fundido" en segundos, "velocidad" es un
          multiplicador (2 es el doble de rápido).
        - No inventes órdenes ni clips que no estén en el listado.
        - Conserva el material salvo que se pida quitarlo explícitamente.
        - Si el transcript viene en el contexto, puedes usar "recortar" con los
          timecodes de lo que se dice para quitar un tramo concreto.
        """
    }

    private static func decodificar(_ texto: String) throws -> NovaEditPlan {
        guard let inicio = texto.firstIndex(of: "{"), let fin = texto.lastIndex(of: "}") else {
            throw NovaAssistantError.invalidPlan
        }
        let plan = try JSONDecoder().decode(NovaEditPlan.self, from: Data(texto[inicio...fin].utf8))
        let validas = plan.actions.filter { ordenes.contains($0.kind) }
        guard !validas.isEmpty, plan.actions.count <= 40 else { throw NovaAssistantError.invalidPlan }
        return NovaEditPlan(summary: plan.summary, actions: validas)
    }
}

extension NovaEditPlan {
    init(summary: String, actions: [NovaEditAction]) {
        self.summary = summary
        self.actions = actions
    }
}

enum NovaAssistantError: LocalizedError {
    case unavailable
    case invalidPlan

    var errorDescription: String? {
        switch self {
        case .unavailable: "No se pudo conectar con Hearthia en este Mac."
        case .invalidPlan: "El modelo no devolvió un plan de montaje válido."
        }
    }
}
