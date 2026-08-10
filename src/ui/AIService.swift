import Foundation
import SwiftUI

enum AIProviderChoice: String, CaseIterable, Identifiable, Sendable {
    case local
    case openCodeGo
    var id: String { rawValue }
    var name: String { self == .local ? "Local privado" : "OpenCode Go" }
}

struct AIModelOption: Identifiable, Hashable { let id: String; let name: String; let api: String }
struct AISelection: Sendable { let provider: AIProviderChoice; let model: String; let api: String }

@MainActor
final class AISettings: ObservableObject {
    @Published var provider: AIProviderChoice {
        didSet { UserDefaults.standard.set(provider.rawValue, forKey: "ai.provider"); selectDefault(); refreshStatus() }
    }
    @Published var model: String {
        didSet { UserDefaults.standard.set(model, forKey: "ai.model.\(provider.rawValue)") }
    }
    @Published private(set) var localModels = [AIModelOption(id: "qwen3.5-9b", name: "Qwen3.5 9B · rápido", api: "chat")]
    @Published private(set) var status = ""

    init() {
        provider = AIProviderChoice(rawValue: UserDefaults.standard.string(forKey: "ai.provider") ?? "") ?? .local
        model = UserDefaults.standard.string(forKey: "ai.model.local") ?? "qwen3.5-9b"
        selectDefault(); refreshStatus()
        Task { await refreshLocalModels() }
    }

    var models: [AIModelOption] { provider == .local ? localModels : Self.goModels }
    var selection: AISelection {
        let option = models.first(where: { $0.id == model }) ?? models[0]
        return AISelection(provider: provider, model: option.id, api: option.api)
    }

    func refreshLocalModels() async {
        guard let (data, response) = try? await URLSession.shared.data(from: URL(string: "http://127.0.0.1:9292/v1/models")!),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let entries = root["data"] as? [[String: Any]] else { status = "Hearthia no responde"; return }
        let found = entries.compactMap { entry -> AIModelOption? in
            guard let id = entry["id"] as? String, !id.contains("embedding"), !id.contains("autocomplete") else { return nil }
            return AIModelOption(id: id, name: entry["name"] as? String ?? id, api: "chat")
        }
        if !found.isEmpty { localModels = found }
        if !localModels.contains(where: { $0.id == model }) { selectDefault() }
        status = "Hearthia disponible · datos en este Mac"
    }

    private func selectDefault() {
        let fallback = provider == .local ? "qwen3.5-9b" : "deepseek-v4-pro"
        let saved = UserDefaults.standard.string(forKey: "ai.model.\(provider.rawValue)") ?? fallback
        let available = provider == .local ? localModels : Self.goModels
        model = available.contains(where: { $0.id == saved }) ? saved : fallback
    }

    private func refreshStatus() {
        status = provider == .local ? "Privado · sin subir tus vídeos" : (OpenCodeCredential.apiKey() == nil ? "Falta conectar OpenCode Go con /connect" : "OpenCode Go conectado · usa tu cuota")
    }

    private static let goModels = [
        AIModelOption(id: "deepseek-v4-flash", name: "DeepSeek V4 Flash · rápido", api: "chat"),
        AIModelOption(id: "deepseek-v4-pro", name: "DeepSeek V4 Pro · equilibrado", api: "chat"),
        AIModelOption(id: "glm-5.2", name: "GLM-5.2 · razonamiento", api: "chat"),
        AIModelOption(id: "glm-5.1", name: "GLM-5.1", api: "chat"),
        AIModelOption(id: "kimi-k3", name: "Kimi K3 · máxima capacidad", api: "chat"),
        AIModelOption(id: "kimi-k2.7-code", name: "Kimi K2.7 Code", api: "chat"),
        AIModelOption(id: "kimi-k2.6", name: "Kimi K2.6", api: "chat"),
        AIModelOption(id: "mimo-v2.5", name: "MiMo V2.5 · económico", api: "chat"),
        AIModelOption(id: "mimo-v2.5-pro", name: "MiMo V2.5 Pro", api: "chat"),
        AIModelOption(id: "hy3", name: "Hy3", api: "chat"),
        AIModelOption(id: "grok-4.5", name: "Grok 4.5", api: "chat"),
        AIModelOption(id: "minimax-m3", name: "MiniMax M3", api: "messages"),
        AIModelOption(id: "minimax-m2.7", name: "MiniMax M2.7", api: "messages"),
        AIModelOption(id: "qwen3.8-max", name: "Qwen3.8 Max", api: "messages"),
        AIModelOption(id: "qwen3.7-max", name: "Qwen3.7 Max", api: "messages"),
        AIModelOption(id: "qwen3.7-plus", name: "Qwen3.7 Plus", api: "messages"),
        AIModelOption(id: "qwen3.6-plus", name: "Qwen3.6 Plus", api: "messages"),
    ]
}

struct AIModelSelector: View {
    @ObservedObject var settings: AISettings
    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Picker("Proveedor", selection: $settings.provider) {
                ForEach(AIProviderChoice.allCases) { Text($0.name).tag($0) }
            }
            Picker("Modelo", selection: $settings.model) {
                ForEach(settings.models) { Text($0.name).tag($0.id) }
            }
            Text(settings.status).font(.system(size: 8)).foregroundStyle(.tertiary)
        }
        .controlSize(.small)
    }
}

enum AITransport {
    static func complete(selection: AISelection, system: String, user: String, maxTokens: Int) async throws -> String {
        try await complete(selection: selection, system: system, user: user, maxTokens: maxTokens, imagenJpeg: nil)
    }

    static func complete(
        selection: AISelection,
        system: String,
        user: String,
        maxTokens: Int,
        imagenJpeg: Data?
    ) async throws -> String {
        let base: String
        let token: String
        if selection.provider == .local { base = "http://127.0.0.1:9292/v1"; token = "local" }
        else { guard let key = OpenCodeCredential.apiKey() else { throw AIServiceError.missingCredential }; base = "https://opencode.ai/zen/go/v1"; token = key }
        let messagesAPI = selection.api == "messages"
        var request = URLRequest(url: URL(string: messagesAPI ? "\(base)/messages" : "\(base)/chat/completions")!)
        request.httpMethod = "POST"; request.timeoutInterval = selection.provider == .local ? 180 : 90
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        // Con imagen, el contenido del usuario es una lista de partes: texto y la
        // miniatura en base64. El dialecto cambia entre proveedores —`image_url`
        // para el compatible con OpenAI y `source`/`base64` para el de Anthropic—
        // y sin imagen el contenido sigue siendo una cadena, como siempre.
        let contenido: Any
        if let imagenJpeg {
            let base64 = imagenJpeg.base64EncodedString()
            let parteDeImagen: [String: Any] = messagesAPI
                ? ["type": "image", "source": ["type": "base64", "media_type": "image/jpeg", "data": base64]]
                : ["type": "image_url", "image_url": ["url": "data:image/jpeg;base64,\(base64)"]]
            contenido = [["type": "text", "text": user], parteDeImagen]
        } else {
            contenido = user
        }

        let body: [String: Any] = messagesAPI
            ? ["model": selection.model, "system": system, "messages": [["role": "user", "content": contenido]], "max_tokens": maxTokens, "temperature": 0.1]
            : ["model": selection.model, "messages": [["role": "system", "content": system], ["role": "user", "content": contenido]], "max_tokens": maxTokens, "temperature": 0.1]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        // La sesión compartida no expone la tarea subyacente: "Cancelar" dejaba
        // la petición viva hasta el timeout con la UI pegada en «Planificando…».
        // Una sesión efímera por petición, invalidada desde el handler de
        // cancelación, aborta el request en vuelo.
        let sesion = URLSession(configuration: .ephemeral)
        do {
            let (data, response) = try await withTaskCancellationHandler(
                operation: {
                    try Task.checkCancellation()
                    return try await sesion.data(for: request)
                },
                onCancel: { sesion.invalidateAndCancel() }
            )
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw AIServiceError.server((response as? HTTPURLResponse)?.statusCode ?? 0) }
            let root = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            if messagesAPI, let content = root?["content"] as? [[String: Any]], let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String { return text }
            if let choices = root?["choices"] as? [[String: Any]], let message = choices.first?["message"] as? [String: Any], let text = message["content"] as? String { return text }
            throw AIServiceError.invalidResponse
        } catch let error as URLError where error.code == .cancelled {
            // La invalidación de la sesión llega como URLError; el flujo de
            // edición la trata como cancelación de tarea.
            throw CancellationError()
        }
    }
}

enum AIServiceError: LocalizedError {
    case missingCredential, invalidResponse, server(Int)
    var errorDescription: String? {
        switch self {
        case .missingCredential: "Conecta OpenCode Go con /connect antes de usarlo aquí."
        case .invalidResponse: "El modelo respondió con un formato no aplicable con seguridad."
        case .server(let code): "El proveedor de IA rechazó la petición (\(code))."
        }
    }
}

private enum OpenCodeCredential {
    static func apiKey() -> String? {
        let url = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/share/opencode/auth.json")
        guard let data = try? Data(contentsOf: url), let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let provider = root["opencode-go"] as? [String: Any], provider["type"] as? String == "api" else { return nil }
        return provider["key"] as? String
    }
}
