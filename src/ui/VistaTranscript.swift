import SwiftUI

/// El panel de texto: se lee lo que se dice y se borra seleccionando palabras.
///
/// Es la ruta corta de una entrevista, y por eso el panel se parece a un documento y
/// no a una lista de subtítulos: se marca con clic y ⇧clic como en cualquier editor de
/// texto, la palabra que suena queda resaltada, y lo que se borra se borra del montaje.
struct PanelDeTranscript: View {

    @ObservedObject var editor: EditorState

    var body: some View {
        // Se recorre el montaje **una vez** por pintada y el resultado viaja hacia abajo.
        // Como propiedad calculada se volvía a recorrer en cada sitio que la miraba —el
        // hueco, el flujo de palabras y el resumen del pie—, y eso es todo el montaje tres
        // veces por fotograma en cuanto la entrevista pasa de unos minutos.
        let palabras = editor.palabrasDelMontaje

        return VStack(alignment: .leading, spacing: 0) {
            cabecera
            Divider()
            buscador
            Divider()
            if !editor.busquedaDeTexto.isEmpty {
                resultados
            } else if palabras.isEmpty {
                vacio
            } else {
                texto(palabras)
                Divider()
                pie(palabras)
            }
        }
        .frame(minWidth: 280)
        .background(Color(nsColor: .controlBackgroundColor))
    }

    // MARK: Cabecera

    private var cabecera: some View {
        HStack(spacing: 8) {
            Image(systemName: "text.alignleft")
            Text("Texto").font(.headline)
            Spacer()
            Button {
                editor.transcribirMedioSeleccionado()
            } label: {
                if editor.transcribing {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: "waveform.badge.mic")
                }
            }
            .buttonStyle(.borderless)
            .disabled(editor.transcribing || editor.selectedMediaID == nil)
            .help("Transcribir el medio seleccionado en este Mac, sin salir del equipo")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    // MARK: Buscar por lo que se dice

    private var buscador: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Buscar lo que se dice…", text: $editor.busquedaDeTexto)
                .textFieldStyle(.plain)
                .font(.system(size: 12))
            if !editor.busquedaDeTexto.isEmpty {
                Button { editor.busquedaDeTexto = "" } label: { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.tertiary)
            }
            Button("Transcribir todo") { editor.transcribirLoQueFalte() }
                .buttonStyle(.borderless)
                .font(.caption)
                .disabled(editor.transcribing)
                .help("Transcribe los medios de la biblioteca que aún no lo estén, uno detrás de otro")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    private var resultados: some View {
        let hallazgos = editor.buscarEnLoQueSeDice(editor.busquedaDeTexto)
        return Group {
            if hallazgos.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Text("Nada dicho así")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("Solo se busca en lo que se ha transcrito. Pulsa «Transcribir todo» para cubrir la biblioteca.")
                        .font(.caption)
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 20)
                    Spacer()
                }
            } else {
                List {
                    ForEach(hallazgos) { hallazgo in
                        Button {
                            editor.irAHallazgo(hallazgo)
                        } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(hallazgo.contexto)
                                    .font(.system(size: 12))
                                    .lineLimit(2)
                                    .multilineTextAlignment(.leading)
                                HStack(spacing: 6) {
                                    if let frame = hallazgo.frame {
                                        Image(systemName: "film").font(.caption2)
                                        Text(editor.timebase.timecode(frame)).font(.caption2)
                                    } else {
                                        Image(systemName: "tray.full").font(.caption2)
                                        Text("solo en la biblioteca").font(.caption2)
                                    }
                                }
                                .foregroundStyle(.secondary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .listStyle(.plain)
            }
        }
    }

    private var vacio: some View {
        VStack(spacing: 10) {
            Spacer()
            Image(systemName: "text.bubble").font(.system(size: 28)).foregroundStyle(.tertiary)
            Text("Sin texto todavía")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Text("Selecciona un medio y transcríbelo. La transcripción se hace en este Mac y no sale de aquí.")
                .font(.caption)
                .multilineTextAlignment(.center)
                .foregroundStyle(.tertiary)
                .padding(.horizontal, 20)
            Spacer()
        }
    }

    // MARK: El texto

    private func texto(_ lista: [PalabraDelMontaje]) -> some View {
        let muletillas = Set(TranscriptService.muletillas(en: lista))
        let sonando = editor.palabraEnElCabezal(lista)

        return ScrollViewReader { lector in
            ScrollView {
                // Un flujo de palabras y no una lista de filas: se lee como un
                // documento, que es de lo que se trata.
                FlujoDePalabras(espaciado: 5, entreLineas: 7) {
                    ForEach(Array(lista.enumerated()), id: \.element.id) { indice, palabra in
                        PalabraPulsable(
                            texto: palabra.texto,
                            seleccionada: editor.seleccionDeTexto.contains(indice),
                            sonando: indice == sonando,
                            esMuletilla: muletillas.contains(indice),
                            alPulsar: { conMayusculas in marcar(indice, extendiendo: conMayusculas) },
                            alPulsarDoble: { editor.irAPalabra(palabra) }
                        )
                        .id(palabra.id)
                    }
                }
                .padding(12)
            }
            .onChange(of: editor.playhead) { _, _ in
                guard let indice = editor.palabraEnElCabezal(lista), indice < lista.count else { return }
                withAnimation(.easeOut(duration: 0.15)) {
                    lector.scrollTo(lista[indice].id, anchor: .center)
                }
            }
        }
    }

    // MARK: Pie

    private func pie(_ palabras: [PalabraDelMontaje]) -> some View {
        VStack(spacing: 6) {
            HStack(spacing: 8) {
                Button("Muletillas") { editor.proponerMuletillas() }
                    .help("Marca «eh», «em», «o sea» y compañía para que las revises antes de quitarlas")
                Spacer()
                Button(role: .destructive) {
                    editor.borrarPalabras(editor.seleccionDeTexto)
                } label: {
                    Text("Quitar")
                }
                .disabled(editor.seleccionDeTexto.isEmpty)
                .keyboardShortcut(.delete, modifiers: [.command])
                .help("Quita del montaje lo seleccionado y cierra el hueco (⌘⌫)")
            }
            HStack {
                Text(resumen(palabras)).font(.caption).foregroundStyle(.secondary)
                Spacer()
                if !editor.seleccionDeTexto.isEmpty {
                    Button("Quitar selección") { editor.seleccionDeTexto = [] }
                        .buttonStyle(.borderless)
                        .font(.caption)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private func resumen(_ palabras: [PalabraDelMontaje]) -> String {
        let marcadas = editor.seleccionDeTexto.count
        if marcadas == 0 { return "\(palabras.count) palabras · clic para marcar, doble clic para ir" }
        let rangos = TranscriptService.rangos(de: palabras, indices: Array(editor.seleccionDeTexto))
        let total = rangos.reduce(Int64(0)) { $0 + ($1.hasta - $1.desde) }
        return "\(marcadas) marcadas · \(editor.timebase.timecode(total)) de material"
    }

    // MARK: Selección

    /// Con ⇧ se extiende desde la última marcada, como en cualquier editor de texto.
    private func marcar(_ indice: Int, extendiendo: Bool) {
        if extendiendo, let ultima = editor.seleccionDeTexto.max() ?? editor.seleccionDeTexto.min() {
            let desde = min(ultima, indice)
            let hasta = max(ultima, indice)
            editor.seleccionDeTexto.formUnion(Set(desde...hasta))
        } else if editor.seleccionDeTexto.contains(indice) {
            editor.seleccionDeTexto.remove(indice)
        } else {
            editor.seleccionDeTexto.insert(indice)
        }
    }
}

/// Una palabra del panel.
private struct PalabraPulsable: View {
    let texto: String
    let seleccionada: Bool
    let sonando: Bool
    let esMuletilla: Bool
    let alPulsar: (Bool) -> Void
    let alPulsarDoble: () -> Void

    var body: some View {
        Text(texto)
            .font(.system(size: 13))
            .padding(.horizontal, 3)
            .padding(.vertical, 1)
            .background(fondo)
            .foregroundStyle(seleccionada ? Color.white : Color.primary)
            .clipShape(RoundedRectangle(cornerRadius: 3))
            .overlay(
                RoundedRectangle(cornerRadius: 3)
                    .stroke(sonando ? Color.accentColor : .clear, lineWidth: 1.5)
            )
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { alPulsarDoble() }
            .onTapGesture {
                alPulsar(NSEvent.modifierFlags.contains(.shift))
            }
            .accessibilityLabel(texto)
            .accessibilityAddTraits(seleccionada ? [.isSelected] : [])
    }

    private var fondo: Color {
        if seleccionada { return .red.opacity(0.75) }
        if esMuletilla { return .orange.opacity(0.25) }
        return .clear
    }
}

/// Coloca las palabras en líneas como un párrafo.
///
/// SwiftUI no trae un flujo así, y montarlo con `LazyVGrid` de columnas fijas dejaría
/// las palabras en rejilla: se leería como una tabla y no como un texto.
private struct FlujoDePalabras: Layout {
    var espaciado: CGFloat
    var entreLineas: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let ancho = proposal.width ?? 300
        let filas = repartir(subviews: subviews, ancho: ancho)
        let alto = filas.reduce(CGFloat(0)) { $0 + $1.alto + entreLineas }
        return CGSize(width: ancho, height: max(0, alto - entreLineas))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let filas = repartir(subviews: subviews, ancho: bounds.width)
        var y = bounds.minY
        for fila in filas {
            var x = bounds.minX
            for indice in fila.indices {
                let tamano = subviews[indice].sizeThatFits(.unspecified)
                subviews[indice].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(tamano))
                x += tamano.width + espaciado
            }
            y += fila.alto + entreLineas
        }
    }

    private func repartir(subviews: Subviews, ancho: CGFloat) -> [(indices: [Int], alto: CGFloat)] {
        var filas: [(indices: [Int], alto: CGFloat)] = []
        var actual: [Int] = []
        var x: CGFloat = 0
        var alto: CGFloat = 0

        for indice in subviews.indices {
            let tamano = subviews[indice].sizeThatFits(.unspecified)
            if x + tamano.width > ancho, !actual.isEmpty {
                filas.append((actual, alto))
                actual = []
                x = 0
                alto = 0
            }
            actual.append(indice)
            x += tamano.width + espaciado
            alto = max(alto, tamano.height)
        }
        if !actual.isEmpty { filas.append((actual, alto)) }
        return filas
    }
}
