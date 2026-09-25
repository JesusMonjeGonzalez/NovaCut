import SwiftUI

enum AccionCentro: Hashable {
    case guardar
    case deshacer
    case rehacer
    case importar
    case exportar
    case copiarAtributos
    case pegarAtributos
    case ajustarSeleccion
    case seleccionarTodo
    case invertirSeleccion
    case seleccionarDesactivados
    case panelTranscript
    case panelSubtitulos
    case abrirMarcadores
    case abrirPlanos
    case atajo
    case marcador(UUID)
    case clip(UUID)
    case subtitulo(UUID)
    case timecode(Int64)
}

struct EntradaCentro: Identifiable {
    let id: String
    let titulo: String
    let detalle: String
    let accion: AccionCentro
    let disponible: Bool
}

struct CentroDeComandos: View {
    @ObservedObject var editor: EditorState
    @Environment(\.dismiss) private var cerrar
    @State private var consulta = ""
    @State private var cursor = 0
    @FocusState private var foco: Bool

    private var resultados: [EntradaCentro] {
        let bruto = consulta.trimmingCharacters(in: .whitespacesAndNewlines)
        let ambito = bruto.first
        let filtro: String
        if ambito == nil { filtro = bruto } else { filtro = String(bruto.dropFirst()) }
        let terminos = filtro.lowercased().split(whereSeparator: { $0 == " " }).map(String.init)
        let entradas = entries
        return entradas.filter { entrada in
            let coincideAmbito: Bool
            switch ambito {
            case "@": coincideAmbito = {
                if case .clip = entrada.accion { return true }
                return false
            }()
            case "#": coincideAmbito = {
                if case .marcador = entrada.accion { return true }
                return false
            }()
            case "=": coincideAmbito = {
                if case .subtitulo = entrada.accion { return true }
                return false
            }()
            case ">": coincideAmbito = {
                switch entrada.accion {
                case .clip, .marcador, .subtitulo, .timecode: return false
                default: return true
                }
            }()
            case ":": coincideAmbito = true
            default: coincideAmbito = true
            }
            guard coincideAmbito else { return false }
            let texto = (entrada.titulo + " " + entrada.detalle).lowercased()
            return terminos.allSatisfy(texto.contains)
        }
    }

    private var entries: [EntradaCentro] {
        var result: [EntradaCentro] = []
        func add(_ titulo: String, _ detalle: String, _ accion: AccionCentro, disponible: Bool = true) {
            result.append(EntradaCentro(id: "\(result.count)-\(titulo)", titulo: titulo, detalle: detalle,
                                        accion: accion, disponible: disponible))
        }
        add("Guardar proyecto", "Documento · ⌘S", .guardar)
        add("Deshacer", "Edición · ⌘Z", .deshacer, disponible: editor.canUndo)
        add("Rehacer", "Edición · ⇧⌘Z", .rehacer, disponible: editor.canRedo)
        add("Importar medios", "Documento · ⌘I", .importar, disponible: !editor.isImporting)
        add("Exportar película", "Entrega · ⇧⌘E", .exportar, disponible: editor.duracionEnFrames > 0 && !editor.isExporting)
        add("Copiar atributos", "Selección · ⌥⌘C", .copiarAtributos, disponible: editor.selectedClip != nil)
        add("Pegar atributos", "Selección · ⌥⌘V", .pegarAtributos,
            disponible: editor.atributosCopiados != nil && !editor.seleccionados().isEmpty)
        add("Ajustar vista a la selección", "Navegación · timeline", .ajustarSeleccion,
            disponible: !editor.seleccionados().isEmpty)
        add("Seleccionar todo", "Selección · todos los clips", .seleccionarTodo, disponible: !editor.montaje.todosLosClips.isEmpty)
        add("Invertir selección", "Selección · clips no elegidos", .invertirSeleccion, disponible: !editor.montaje.todosLosClips.isEmpty)
        add("Seleccionar clips desactivados", "Revisión · material excluido", .seleccionarDesactivados,
            disponible: editor.montaje.todosLosClips.contains { !$0.clip.habilitado })
        add("Mostrar u ocultar texto", "Panel · transcripción", .panelTranscript)
        add("Abrir subtítulos", "Panel · SRT", .panelSubtitulos)
        add("Abrir marcadores", "Panel · notas", .abrirMarcadores)
        add("Abrir lista de planos", "Panel · clips", .abrirPlanos)
        add("Atajos de teclado", "Ayuda · teclado", .atajo)

        for clip in editor.montaje.todosLosClips {
            let nombre = editor.mediaItem(for: clip.clip)?.name ?? clip.clip.nombre
            add(nombre, "Clip · \(editor.timebase.timecode(clip.clip.inicio)) · \(clip.clip.duracion) frames",
                .clip(clip.clip.id))
        }
        for marcador in editor.montaje.marcadores {
            add(marcador.nombre, "Marcador · \(editor.timebase.timecode(marcador.frame))", .marcador(marcador.id))
        }
        for subtitulo in editor.montaje.subtitulos ?? [] {
            add(subtitulo.texto, "Subtítulo · \(editor.timebase.timecode(subtitulo.inicio))", .subtitulo(subtitulo.id))
        }
        if consulta.first == ":",
           let frame = editor.timebase.frames(timecode: String(consulta.dropFirst())),
           frame >= 0, frame <= editor.montaje.duracion {
            add("Ir a \(editor.timebase.timecode(frame))", "Timecode · cabezal", .timecode(frame))
        }
        return result
    }

    private func entrada(at index: Int) -> EntradaCentro? {
        resultados.indices.contains(index) ? resultados[index] : nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Buscar comando, clip, marcador o subtítulo…", text: $consulta)
                    .textFieldStyle(.plain)
                    .focused($foco)
                    .onSubmit { ejecutar(entrada(at: cursor)) }
                Button("Esc") { cerrar() }
                    .buttonStyle(.bordered)
                    .keyboardShortcut(.cancelAction)
            }
            .padding(12)
            .background(Color(nsColor: .windowBackgroundColor))

            if resultados.isEmpty {
                ContentUnavailableView("Sin resultados", systemImage: "command",
                                       description: Text("Prueba con un nombre, @clip, #marcador, =subtítulo o :timecode."))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(Array(resultados.enumerated()), id: \.element.id) { indice, entrada in
                    Button {
                        cursor = indice
                        ejecutar(entrada)
                    } label: {
                        HStack(alignment: .top, spacing: 10) {
                            Image(systemName: entrada.disponible ? "arrow.right.circle" : "minus.circle")
                                .foregroundStyle(entrada.disponible ? Color.accentColor : .secondary)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(entrada.titulo)
                                    .font(.system(size: 12, weight: .semibold))
                                    .foregroundStyle(entrada.disponible ? .primary : .secondary)
                                Text(entrada.detalle)
                                    .font(.system(size: 10))
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if indice == cursor {
                                Image(systemName: "return")
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                    .disabled(!entrada.disponible)
                    .listRowBackground(indice == cursor ? Color.accentColor.opacity(0.12) : Color.clear)
                }
                .listStyle(.sidebar)
            }
            HStack {
                Text("↑ ↓ navegar · Return ejecutar · Esc cerrar")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                Spacer()
                Text("\(resultados.count) resultados")
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
        }
        .frame(minWidth: 560, idealWidth: 720, minHeight: 420, idealHeight: 560)
        .onAppear { foco = true }
        .onChange(of: consulta) { _, _ in cursor = 0 }
        .onKeyPress(.upArrow) {
            cursor = max(0, cursor - 1)
            return .handled
        }
        .onKeyPress(.downArrow) {
            cursor = min(max(0, resultados.count - 1), cursor + 1)
            return .handled
        }
        .onKeyPress(.return) {
            ejecutar(entrada(at: cursor))
            return .handled
        }
    }

    private func ejecutar(_ entrada: EntradaCentro?) {
        guard let entrada, entrada.disponible else { return }
        cerrar()
        switch entrada.accion {
        case .guardar: editor.saveProject()
        case .deshacer: editor.undo()
        case .rehacer: editor.redo()
        case .importar: editor.importMedia()
        case .exportar: editor.exportMovie()
        case .copiarAtributos: editor.copiarAtributos()
        case .pegarAtributos: editor.pegarAtributos()
        case .ajustarSeleccion: editor.ajustarVistaASeleccion()
        case .seleccionarTodo: editor.seleccionarTodosLosClips()
        case .invertirSeleccion: editor.invertirSeleccion()
        case .seleccionarDesactivados: editor.seleccionarClipsDesactivados()
        case .panelTranscript: editor.mostrarTranscript.toggle()
        case .panelSubtitulos: editor.mostrarSubtitulos = true
        case .abrirMarcadores: editor.mostrarTranscript = false
        case .abrirPlanos: editor.mostrarTranscript = false
        case .atajo: editor.status = "Atajos: ⌘⇧P centro de comandos · ⌥⌘C/V atributos · J/K/L transporte"
        case .marcador(let id):
            guard let marcador = editor.montaje.marcadores.first(where: { $0.id == id }) else { return }
            editor.seek(toFrame: marcador.frame)
        case .clip(let id):
            editor.seleccionarClip(id)
            if let clip = editor.montaje.clip(id) { editor.seek(toFrame: clip.inicio) }
        case .subtitulo(let id):
            guard let subtitulo = editor.montaje.subtitulos?.first(where: { $0.id == id }) else { return }
            editor.seek(toFrame: subtitulo.inicio)
            editor.mostrarSubtitulos = true
        case .timecode(let frame): editor.seek(toFrame: frame)
        }
    }
}
