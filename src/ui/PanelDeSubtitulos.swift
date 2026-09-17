import SwiftUI

/// Búsqueda y sincronización sobre los cues del proyecto, con historial normal.
struct PanelDeSubtitulos: View {
    @ObservedObject var editor: EditorState
    @Environment(\.dismiss) private var cerrar
    @State private var consulta = ""
    @State private var milisegundos = "0"
    @State private var aviso: String?

    private var todos: [Subtitulo] { editor.montaje.subtitulos ?? [] }
    private var visibles: [Subtitulo] { SubtitulosService.buscar(todos, consulta: consulta) }
    private var desplazamiento: Int64? {
        guard let valor = Double(milisegundos.trimmingCharacters(in: .whitespaces)
            .replacingOccurrences(of: ",", with: ".")), valor.isFinite else { return nil }
        let frames = (valor / 1000 * editor.timebase.fps).rounded()
        guard frames.isFinite, frames > Double(Int64.min), frames < Double(Int64.max) else { return nil }
        return Int64(frames)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("Subtítulos").font(.title2.bold())
                Spacer()
                Button("Deshacer") { editor.undo() }.disabled(!editor.canUndo)
                Button("Rehacer") { editor.redo() }.disabled(!editor.canRedo)
                Button("Cerrar") { cerrar() }.keyboardShortcut(.cancelAction)
            }
            HStack {
                TextField("Buscar texto…", text: $consulta).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Buscar en los subtítulos")
                if !consulta.isEmpty { Button("Limpiar") { consulta = "" } }
                Text("\(visibles.count) de \(todos.count)").monospacedDigit().foregroundStyle(.secondary)
            }
            GroupBox("Sincronización de todos los subtítulos") {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        TextField("Desplazamiento", text: $milisegundos).frame(width: 110)
                            .textFieldStyle(.roundedBorder).accessibilityLabel("Desplazamiento en milisegundos")
                        Text("ms")
                        Button("Aplicar ajuste") { if let frames = desplazamiento { aplicar(frames) } }
                            .disabled(todos.isEmpty || desplazamiento == nil || desplazamiento == 0)
                        Spacer()
                        Button("Alinear primero al cabezal") {
                            guard let primero = todos.map(\.inicio).min() else { return }
                            let (frames, desborde) = editor.cabezal.subtractingReportingOverflow(primero)
                            if desborde { aviso = "No se puede alinear este intervalo." } else { aplicar(frames) }
                        }.disabled(todos.isEmpty)
                    }
                    Text("Negativo adelanta; positivo retrasa. Se aplica a todos, aunque haya una búsqueda activa.")
                        .font(.caption).foregroundStyle(.secondary)
                    if let frames = desplazamiento {
                        Text("Ajuste a la cadencia del proyecto: \(frames) frames").font(.caption).foregroundStyle(.secondary)
                    } else {
                        Text("Introduce un número válido de milisegundos.").font(.caption).foregroundStyle(.orange)
                    }
                    if let aviso { Text(aviso).font(.caption).foregroundStyle(.orange).accessibilityLabel(aviso) }
                }.padding(5)
            }
            HStack {
                Button("Añadir en el cabezal") { consulta = ""; editor.agregarSubtitulo() }
                Spacer()
                Text("\(editor.timebase.timecode(editor.cabezal))").monospaced().foregroundStyle(.secondary)
            }
            if visibles.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "captions.bubble").font(.largeTitle)
                    Text(todos.isEmpty ? "Añade un subtítulo o importa un SRT desde el menú Subtítulos." : "No hay subtítulos que coincidan.")
                }.foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(visibles) { cue in
                            FilaDeSubtitulo(cue: cue, editor: editor) {
                                editor.seek(toFrame: cue.inicio)
                                cerrar()
                            }
                        }
                    }
                }
            }
        }.padding(20).frame(minWidth: 700, idealWidth: 760, minHeight: 480, idealHeight: 580)
    }

    private func aplicar(_ frames: Int64) {
        if frames == 0 { aviso = "Los subtítulos ya están alineados."; return }
        if editor.desplazarSubtitulos(frames: frames) {
            aviso = nil
            milisegundos = "0"
        } else {
            aviso = editor.status
        }
    }
}

private struct FilaDeSubtitulo: View {
    let cue: Subtitulo
    @ObservedObject var editor: EditorState
    let ir: () -> Void
    @State private var editando = false
    @State private var borrador = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("\(editor.timebase.timecode(cue.inicio)) → \(editor.timebase.timecode(cue.fin))")
                    .font(.caption.monospaced()).foregroundStyle(.secondary)
                Spacer()
                Button("Ir", action: ir).help("Llevar el cabezal al subtítulo y volver al montaje")
                Button("Editar") { borrador = cue.texto; editando = true }.disabled(editando)
                Button { editor.eliminarSubtitulo(cue.id) } label: { Image(systemName: "trash") }
                    .help("Eliminar subtítulo (se puede deshacer)").accessibilityLabel("Eliminar subtítulo")
            }
            if editando {
                TextField("Texto", text: $borrador, axis: .vertical).lineLimit(2...6).textFieldStyle(.roundedBorder)
                HStack {
                    Button("Guardar texto") { editor.editarSubtitulo(cue.id, texto: borrador); editando = false }
                        .disabled(borrador.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Cancelar") { editando = false }
                }
            } else {
                Text(cue.texto).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            }
        }.padding(12).background(.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
    }
}
