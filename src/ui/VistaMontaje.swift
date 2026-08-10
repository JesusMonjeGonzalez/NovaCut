import AppKit
import SwiftUI

/// La línea de tiempo multipista.
///
/// Las cabeceras van fijas a la izquierda y las pistas se desplazan a la vez con
/// la regla, que es la disposición de cualquier editor y la única que permite
/// seguir sabiendo en qué pista estás cuando el montaje mide diez minutos.
struct VistaMontaje: View {

    @ObservedObject var editor: EditorState

    /// El `NSScrollView` real del lienzo, para el paneo de la herramienta Mano.
    ///
    /// `ScrollViewReader` solo sabe centrar anclas —no leer ni fijar el offset—,
    /// y centrar una ancla sin saber dónde está el viewport salta al arranque en
    /// el primer arrastre. La sonda recupera el `NSScrollView` de AppKit y el
    /// paneo se hace por offset relativo, que es exacto.
    @StateObject private var controlDelLienzo = ControlDelLienzo()

    /// Offset horizontal al empezar un paneo con la Mano.
    @State private var origenDelPaneo: CGFloat?

    /// Píxeles por frame. Se deriva de la escala en segundos para que el zoom se
    /// comporte igual con material a 25 y a 60 fps.
    private var escala: Double { editor.timelineScale / max(editor.timebase.fps, 1) }

    private var anchoTotal: Double {
        Double(editor.duracionEnFrames + Int64(editor.timebase.fps * 6)) * escala
    }

    /// Umbral del imán en frames: siempre unos ocho píxeles, se mire con el zoom
    /// que se mire. Un umbral fijo en frames sería inútil de lejos e insufrible de
    /// cerca.
    private var umbralDeIman: Int64 { max(1, Int64(8 / max(escala, 0.0001))) }

    private static let anchoDeCabeceras: Double = 132

    /// La altura predefinida más parecida a la actual, para que el menú marque una.
    private func alturaMasCercana(_ alto: Double) -> Double {
        [40.0, 58.0, 78.0, 104.0].min { abs($0 - alto) < abs($1 - alto) } ?? 58
    }

    var body: some View {
        VStack(spacing: 0) {
            barraDeHerramientas
            Divider()
            HStack(spacing: 0) {
                cabeceras
                Divider()
                // El lienzo mide lo que dure el montaje, pero nunca menos que lo que
                // se ve: si no, las pistas se cortan a media ventana y queda un vacío
                // a la derecha que parece un fallo de dibujo.
                GeometryReader { hueco in
                    ScrollViewReader { lector in
                        ScrollView([.horizontal]) {
                            ZStack(alignment: .topLeading) {
                                VStack(spacing: 1) {
                                    regla
                                    ForEach(editor.montaje.pistas) { pista in
                                        filaDePista(pista, controlDelLienzo: controlDelLienzo)
                                    }
                                    Spacer(minLength: 0)
                                }
                                Color.clear
                                    .frame(width: 1, height: 1)
                                    .offset(x: Double(editor.cabezal) * escala)
                                    .id("playhead-anchor")
                                cabezal
                                if editor.duracionEnFrames == 0 {
                                    VStack(spacing: 6) {
                                        Image(systemName: "arrow.down.circle").font(.system(size: 26)).foregroundStyle(.tertiary)
                                        Text("Arrastra aquí los medios de la biblioteca")
                                            .font(.system(size: 12, weight: .medium)).foregroundStyle(.secondary)
                                        Text("O selecciónalos y usa «Añadir al timeline» (⌘I para importar)")
                                            .font(.system(size: 10)).foregroundStyle(.tertiary)
                                    }
                                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                                    .padding(.bottom, 60)
                                    .allowsHitTesting(false)
                                }
                            }
                            .frame(width: max(anchoTotal, hueco.size.width), alignment: .topLeading)
                            .contentShape(Rectangle())
                            // Suelta un medio de la biblioteca: el puntero decide el
                            // frame (el lienzo ya incluye el desplazamiento del
                            // scroll) y ⌥ escribe encima en vez de abrir hueco.
                            .onDrop(
                                of: [.utf8PlainText],
                                delegate: SueltaDeMedio(
                                    editor: editor,
                                    escala: escala,
                                    anchoDelLienzo: anchoTotal
                                )
                            )
                            .overlay(alignment: .topLeading) {
                                if let frameDeSuelta = editor.frameDeSuelta {
                                    // Guía de soltado: la línea donde caerá el clip.
                                    Rectangle()
                                        .fill(Color.accentColor.opacity(0.85))
                                        .frame(width: 2)
                                        .offset(x: Double(frameDeSuelta) * escala)
                                        .allowsHitTesting(false)
                                }
                            }
                            // Con la Mano activa, arrastrar sobre el lienzo (fuera
                            // de los clips) panea el timeline. Sobre un clip manda
                            // el gesto del propio clip, que con la Mano también
                            // panea —nunca compiten—.
                            .gesture(editor.herramienta == .mano ? paneoDelLienzo : nil)
                            .onHover { dentro in
                                guard editor.herramienta == .mano else { return }
                                if dentro { NSCursor.openHand.push() } else { NSCursor.pop() }
                            }
                            // La sonda se instala en el lienzo para encontrar el
                            // NSScrollView; no intercepta ningún evento.
                            .overlay(alignment: .topLeading) {
                                SondaDelScroll(control: controlDelLienzo)
                                    .frame(width: 1, height: 1)
                                    .allowsHitTesting(false)
                            }
                        }
                        .onChange(of: editor.playhead) { _, _ in
                            guard editor.isPlaying else { return }
                            withAnimation(.linear(duration: 0.08)) {
                                lector.scrollTo("playhead-anchor", anchor: .center)
                            }
                        }
                    }
                    // La vista le cuenta al estado cuánto sitio hay, que es lo que
                    // necesita «ajustar a la ventana» sin conocer la jerarquía.
                    //
                    // El aviso se aplaza un ciclo a propósito: escribir en el modelo
                    // desde dentro de un `GeometryReader` ocurre en plena pasada de
                    // layout, y AppKit aborta el proceso si se le pide recalcular
                    // restricciones mientras las está calculando.
                    .onChange(of: hueco.size.width, initial: true) { _, ancho in
                        Task { @MainActor in editor.anchoVisibleDelMontaje = ancho }
                    }
                }
                .background(Color(nsColor: NSColor(calibratedWhite: 0.10, alpha: 1)))
            }
        }
    }

    // MARK: - Herramientas

    private var barraDeHerramientas: some View {
        HStack(spacing: 10) {
            ForEach(Herramienta.allCases) { herramienta in
                Button {
                    editor.herramienta = herramienta
                    editor.timelineHasFocus = true
                } label: {
                    Image(systemName: herramienta.icono)
                        .frame(width: 22, height: 20)
                        .background(
                            editor.herramienta == herramienta ? Color.accentColor.opacity(0.35) : .clear,
                            in: RoundedRectangle(cornerRadius: 4)
                        )
                }
                .buttonStyle(.plain)
                .help("\(herramienta.nombre) (\(String(herramienta.atajo).uppercased()))")
                .accessibilityLabel(herramienta.nombre)
                .accessibilityHint("Herramienta de montaje")
            }

            Divider().frame(height: 16)

            Button {
                editor.imanActivo.toggle()
                editor.timelineHasFocus = true
            } label: {
                Image(systemName: editor.imanActivo ? "magnet.fill" : "magnet")
                    .foregroundStyle(editor.imanActivo ? Color.accentColor : Color.secondary)
            }
            .buttonStyle(.plain)
            .help("Imán a cortes y marcadores (S)")
            .accessibilityLabel(editor.imanActivo ? "Desactivar imán" : "Activar imán")

            Button {
                editor.anadirMarcador()
                editor.timelineHasFocus = true
            } label: { Image(systemName: "bookmark.fill") }
                .buttonStyle(.plain).help("Marcador en el cabezal (M)")
                .accessibilityLabel("Añadir marcador en el cabezal")

            Divider().frame(height: 16)

            Text(editor.timecodeDelCabezal)
                .font(.system(size: 12, weight: .semibold, design: .monospaced))
                .foregroundStyle(Color(red: 1, green: 0.45, blue: 0.65))

            Text(editor.timebase.nombre)
                .font(.system(size: 9, design: .monospaced)).foregroundStyle(.tertiary)

            Spacer()

            Menu {
                Section("Pistas") {
                    Button("Añadir pista de vídeo") { editor.anadirPista(.video) }
                    Button("Añadir pista de audio") { editor.anadirPista(.audio) }
                    Button("Añadir pista de ajuste") { editor.anadirPistaDeAjuste() }
                        .help("Su color y LUT se aplican a todo lo que hay debajo")
                }
                Section("Altura de las pistas") {
                    Picker("Altura", selection: Binding(
                        get: { alturaMasCercana(editor.alturaDePistas) },
                        set: { editor.fijarAlturaDePistas($0) }
                    )) {
                        Text("Compacta").tag(40.0)
                        Text("Normal").tag(58.0)
                        Text("Cómoda").tag(78.0)
                        Text("Grande").tag(104.0)
                    }
                    .pickerStyle(.inline)
                }
                Section("Huecos") {
                    Button("Cerrar hueco en el cabezal") { editor.cerrarHuecoEnCabezal() }
                    Button("Cerrar todos los huecos de la pista") { editor.cerrarTodosLosHuecos() }
                }
                // Cambiar la cadencia reescribe todo el montaje, así que va apartado
                // del resto y no pegado a acciones cotidianas.
                Section("Proyecto") {
                    Menu("Base de tiempo · \(editor.timebase.nombre)") {
                        Picker("Base de tiempo", selection: Binding(
                            get: { editor.timebase },
                            set: { editor.cambiarTimebase($0) }
                        )) {
                            ForEach(Timebase.habituales, id: \.self) { Text($0.nombre).tag($0) }
                        }
                        .pickerStyle(.inline)
                    }
                }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton).frame(width: 30)
            .help("Pistas, huecos y ajustes del proyecto")

            Button { editor.ajustarMontajeALaVentana() } label: {
                Image(systemName: "arrow.left.and.right.square")
            }
            .buttonStyle(.plain)
            .help("Ajustar el montaje a la ventana (⇧Z)")

            Image(systemName: "minus.magnifyingglass").foregroundStyle(.tertiary)
                .help("Alejar (⌘[)")
            // El deslizador es logarítmico porque el zoom se percibe así: de 10 a 20
            // px/s se nota lo mismo que de 100 a 200. Con un recorrido lineal, todo
            // el rango de trabajo quedaría apelotonado en el primer palmo.
            Slider(
                value: Binding(
                    get: { log2(max(editor.timelineScale, 0.5)) },
                    set: { editor.timelineScale = pow(2, $0) }
                ),
                in: log2(0.5)...log2(400)
            )
            .frame(width: 120)
            .help("Zoom del montaje")
            Image(systemName: "plus.magnifyingglass").foregroundStyle(.tertiary)
                .help("Acercar (⌘])")
        }
        .font(.system(size: 12))
        .padding(.horizontal, 10).frame(height: 30)
        .background(Color(nsColor: NSColor(calibratedWhite: 0.14, alpha: 1)))
    }

    // MARK: - Cabeceras

    private var cabeceras: some View {
        VStack(spacing: 1) {
            // Hueco que alinea las cabeceras con la regla.
            Color.clear.frame(width: Self.anchoDeCabeceras, height: 26)
            ForEach(editor.montaje.pistas) { pista in
                cabeceraDePista(pista)
            }
            Spacer(minLength: 0)
        }
        .frame(width: Self.anchoDeCabeceras)
        .background(Color(nsColor: NSColor(calibratedWhite: 0.13, alpha: 1)))
    }

    private func cabeceraDePista(_ pista: Pista) -> some View {
        let activa = editor.pistaDeTrabajo == pista.id
        return HStack(spacing: 4) {
            Text(pista.nombre)
                .font(.system(size: 11, weight: .bold, design: .monospaced))
                .frame(width: 26, alignment: .leading)

            Group {
                if pista.tipo == .video {
                    boton(
                        pista.visible ? "eye" : "eye.slash", activo: !pista.visible,
                        ayuda: pista.visible ? "Ocultar esta pista de vídeo" : "Volver a mostrarla",
                    ) { editor.alternarPista(pista.id, .visible) }
                } else {
                    boton(
                        pista.silenciada ? "speaker.slash.fill" : "speaker.wave.2",
                        activo: pista.silenciada,
                        ayuda: pista.silenciada ? "Quitar el silencio" : "Silenciar esta pista",
                    ) { editor.alternarPista(pista.id, .silencio) }
                }
                boton(
                    "s.circle\(pista.solo ? ".fill" : "")", activo: pista.solo,
                    ayuda: "Solo: silencia las demás pistas de su tipo",
                ) { editor.alternarPista(pista.id, .solo) }
                if pista.tipo == .audio {
                    boton(
                        pista.duckingActivo ? "arrow.down.circle.fill" : "arrow.down.circle",
                        activo: pista.duckingActivo,
                        ayuda: "Bajar esta pista bajo el diálogo de A1",
                    ) { editor.alternarPista(pista.id, .ducking) }
                    // Expandir la pista de audio: más altura para leer la forma
                    // de onda y la curva de ganancia, como el chevron de Premiere.
                    boton(
                        pista.altura > 58 ? "chevron.down" : "chevron.up",
                        activo: pista.altura > 58,
                        ayuda: pista.altura > 58 ? "Contraer la pista de audio" : "Expandir: forma de onda grande",
                    ) { editor.alternarAlturaDePista(pista.id) }
                    let conProcesamiento = pista.tieneProcesamientoDeAudio
                    boton(
                        "slider.horizontal.3",
                        activo: conProcesamiento,
                        ayuda: conProcesamiento
                            ? "Con procesamiento (clic derecho para ajustar)"
                            : "Sin procesamiento (clic derecho para añadir)",
                    ) { editor.pistaActiva = pista.id }
                }
                boton(
                    pista.bloqueada ? "lock.fill" : "lock.open", activo: pista.bloqueada,
                    ayuda: pista.bloqueada ? "Desbloquear la pista" : "Bloquear: nada la podrá editar",
                ) { editor.alternarPista(pista.id, .bloqueo) }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 6)
        .frame(width: Self.anchoDeCabeceras, height: pista.altura, alignment: .leading)
        .background(activa ? Color.accentColor.opacity(0.16) : Color(nsColor: NSColor(calibratedWhite: 0.16, alpha: 1)))
        .overlay(alignment: .leading) {
            Rectangle().fill(activa ? Color.accentColor : .clear).frame(width: 2)
        }
        .contentShape(Rectangle())
        .onTapGesture { editor.pistaActiva = pista.id }
        .contextMenu {
            Button("Activar esta pista") { editor.pistaActiva = pista.id }
            Button("Cerrar huecos") { editor.pistaActiva = pista.id; editor.cerrarTodosLosHuecos() }
            if pista.tipo == .audio {
                Divider()
                Menu("Procesamiento") {
                    Menu("Puerta de ruido") {
                        Toggle("Quitar el fondo (−50 dB)", isOn: Binding(
                            get: { pista.puertaDeRuido != nil },
                            set: { on in editor.fijarPuertaDeRuido(on ? PuertaDeRuidoDePista() : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("Multibanda") {
                        Toggle("Graves/medios/agudos", isOn: Binding(
                            get: { pista.multibanda != nil },
                            set: { on in editor.fijarMultibanda(on ? CompresorMultibandaDePista() : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("Reverb") {
                        Toggle("Sala (tamaño 0,5)", isOn: Binding(
                            get: { pista.reverb != nil },
                            set: { on in editor.fijarReverb(on ? ReverbDePista() : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("Retardo") {
                        Toggle("Eco de 300 ms", isOn: Binding(
                            get: { pista.retardo != nil },
                            set: { on in editor.fijarRetardo(on ? RetardoDePista() : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("Limiter") {
                        Toggle("Proteger pico (−3 dBFS)", isOn: Binding(
                            get: { pista.limitador != nil },
                            set: { on in editor.fijarLimiter(on ? LimitadorDePista() : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("Compresor") {
                        Toggle("Suave (2:1, −18 dB)", isOn: Binding(
                            get: { pista.compresor?.ratio == 2 },
                            set: { on in editor.fijarCompresor(on ? CompresorDePista(umbralDB: -18, ratio: 2) : nil, enPista: pista.id) }
                        ))
                        Toggle("Fuerte (4:1, −18 dB)", isOn: Binding(
                            get: { pista.compresor?.ratio == 4 },
                            set: { on in editor.fijarCompresor(on ? CompresorDePista(umbralDB: -18, ratio: 4) : nil, enPista: pista.id) }
                        ))
                    }
                    Menu("EQ") {
                        Toggle("Realce de graves (+3 dB @ 100 Hz)", isOn: Binding(
                            get: { pista.ecualizacion?.first { $0.frecuencia == 100 && $0.gananciaDB > 0 } != nil },
                            set: { on in editor.fijarEcualizacion(on ? [BandaDeEQ(frecuencia: 100, gananciaDB: 3, calidad: 0.8, tipo: .bajo)] : nil, enPista: pista.id) }
                        ))
                        Toggle("Reducción de graves (−3 dB @ 100 Hz)", isOn: Binding(
                            get: { pista.ecualizacion?.first { $0.frecuencia == 100 && $0.gananciaDB < 0 } != nil },
                            set: { on in editor.fijarEcualizacion(on ? [BandaDeEQ(frecuencia: 100, gananciaDB: -3, calidad: 0.8, tipo: .bajo)] : nil, enPista: pista.id) }
                        ))
                        Toggle("Realce de agudos (+3 dB @ 8 kHz)", isOn: Binding(
                            get: { pista.ecualizacion?.first { $0.frecuencia == 8000 } != nil },
                            set: { on in editor.fijarEcualizacion(on ? [BandaDeEQ(frecuencia: 8000, gananciaDB: 3, calidad: 0.8, tipo: .alto)] : nil, enPista: pista.id) }
                        ))
                    }
                }
            }
            Divider()
            Button("Eliminar pista", role: .destructive) { editor.eliminarPista(pista.id) }
        }
    }

    private func boton(
        _ icono: String,
        activo: Bool,
        ayuda: String = "",
        accion: @escaping () -> Void
    ) -> some View {
        Button(action: accion) {
            Image(systemName: icono)
                .font(.system(size: 10))
                .foregroundStyle(activo ? Color.orange : Color.secondary)
                .frame(width: 16, height: 16)
        }
        .buttonStyle(.plain)
        .help(ayuda)
        .accessibilityLabel(ayuda)
    }

    // MARK: - Regla

    private var regla: some View {
        // Se elige el paso de marcas para que nunca queden más juntas de 64 px, que
        // es lo que hace ilegible una regla al alejar el zoom.
        let fps = editor.timebase.fps
        let pasos: [Double] = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600]
        let paso = pasos.first { $0 * editor.timelineScale >= 64 } ?? 3600
        let cuantas = Int(Double(editor.duracionEnFrames) / (paso * fps)) + 4

        return ZStack(alignment: .topLeading) {
            Rectangle().fill(Color(nsColor: NSColor(calibratedWhite: 0.16, alpha: 1)))
                .frame(maxWidth: .infinity, minHeight: 26, maxHeight: 26)

            ForEach(0..<max(1, cuantas), id: \.self) { i in
                let frame = Int64(Double(i) * paso * fps)
                VStack(alignment: .leading, spacing: 0) {
                    Text(editor.timebase.timecode(frame))
                        .font(.system(size: 8, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .fixedSize()
                    Rectangle().fill(.white.opacity(0.22)).frame(width: 1, height: 8)
                }
                .offset(x: Double(frame) * escala + 2, y: 2)
            }

            ForEach(editor.montaje.marcadores) { marcador in
                Image(systemName: "bookmark.fill")
                    .font(.system(size: 9))
                    .foregroundStyle(color(marcador.etiqueta))
                    .offset(x: Double(marcador.frame) * escala - 4, y: 13)
                    .help(marcador.nombre)
                    .onTapGesture { editor.seek(toFrame: marcador.frame) }
                    .contextMenu {
                        Button("Ir al marcador") { editor.seek(toFrame: marcador.frame) }
                        Button("Eliminar", role: .destructive) { editor.eliminarMarcador(marcador.id) }
                    }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 26, maxHeight: 26)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0).onChanged { valor in
                editor.timelineHasFocus = true
                editor.seek(toFrame: Int64(max(0, valor.location.x / escala)))
            }
        )
    }

    // MARK: - Pistas

    /// El paneo de la herramienta Mano sobre el lienzo: mueve el viewport en la
    /// dirección del arrastre, sin tocar clips ni el cabezal.
    private var paneoDelLienzo: some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { valor in
                guard let scroll = controlDelLienzo.scroll else { return }
                if origenDelPaneo == nil {
                    origenDelPaneo = scroll.contentView.bounds.origin.x
                    NSCursor.closedHand.push()
                }
                scroll.contentView.scroll(to: NSPoint(x: origenDelPaneo! - valor.translation.width, y: 0))
                scroll.reflectScrolledClipView(scroll.contentView)
            }
            .onEnded { _ in
                if origenDelPaneo != nil {
                    NSCursor.pop()
                    origenDelPaneo = nil
                }
            }
    }

    private func filaDePista(_ pista: Pista, controlDelLienzo: ControlDelLienzo) -> some View {
        ZStack(alignment: .topLeading) {
            Rectangle()
                .fill(Color(nsColor: NSColor(calibratedWhite: pista.tipo == .video ? 0.145 : 0.125, alpha: 1)))
                .frame(maxWidth: .infinity, minHeight: pista.altura, maxHeight: pista.altura)

            ForEach(pista.clips) { clip in
                VistaDeClip(editor: editor, pista: pista, clip: clip, escala: escala,
                            umbralDeIman: umbralDeIman, controlDelLienzo: controlDelLienzo)
                    .offset(x: Double(clip.inicio) * escala)
            }
        }
        .frame(maxWidth: .infinity, minHeight: pista.altura, maxHeight: pista.altura, alignment: .topLeading)
        .opacity(pista.bloqueada ? 0.55 : 1)
        .contentShape(Rectangle())
        .onDrop(
            of: [.utf8PlainText],
            delegate: SueltaDeMedio(
                editor: editor,
                escala: escala,
                anchoDelLienzo: anchoTotal,
                pistaID: pista.id
            )
        )
        .simultaneousGesture(
            SpatialTapGesture().onEnded { valor in
                // Con la Mano, un clic panea y no debe mover el cabezal.
                guard editor.herramienta != .mano else { return }
                editor.timelineHasFocus = true
                editor.pistaActiva = pista.id
                editor.seek(toFrame: Int64(max(0, valor.location.x / escala)))
            }
        )
    }

    private var cabezal: some View {
        Rectangle()
            .fill(Color(red: 1, green: 0.24, blue: 0.58))
            .frame(width: 1.5)
            .frame(maxHeight: .infinity, alignment: .top)
            .offset(x: Double(editor.cabezal) * escala)
            .allowsHitTesting(false)
    }

    private func color(_ etiqueta: EtiquetaDeColor) -> Color {
        switch etiqueta {
        case .ninguna: .cyan
        case .rojo: .red
        case .naranja: .orange
        case .amarillo: .yellow
        case .verde: .green
        case .azul: .blue
        case .morado: .purple
        case .rosa: .pink
        }
    }
}

// MARK: - El scroll real del lienzo

/// El `NSScrollView` del lienzo, localizado por `SondaDelScroll`.
final class ControlDelLienzo: ObservableObject {
    weak var scroll: NSScrollView?
}

/// Una vista de un píxel que sube por la jerarquía de AppKit hasta encontrar el
/// `NSScrollView` de SwiftUI. Sin esto, la Mano no podría leer ni fijar el
/// offset del viewport (ScrollViewReader no lo expone).
struct SondaDelScroll: NSViewRepresentable {
    let control: ControlDelLienzo

    func makeNSView(context: Context) -> NSView {
        let vista = NSView(frame: .zero)
        DispatchQueue.main.async {
            var actual: NSView? = vista
            while let v = actual {
                if let scroll = v as? NSScrollView {
                    control.scroll = scroll
                    return
                }
                actual = v.superview
            }
        }
        return vista
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

// MARK: - Un clip

/// Un clip dibujado y manipulable.
///
/// Los tres gestos que importan viven aquí: arrastrar el cuerpo, y arrastrar
/// cualquiera de los dos bordes. Lo que hace cada uno depende de la herramienta
/// activa, igual que en cualquier editor: con la herramienta de selección el
/// borde recorta, con ripple arrastra la cola, con roll mueve el corte.
struct VistaDeClip: View {

    @ObservedObject var editor: EditorState
    let pista: Pista
    let clip: Clip
    let escala: Double
    let umbralDeIman: Int64
    let controlDelLienzo: ControlDelLienzo

    @State private var arrastreAcumulado: Double = 0
    @State private var aplicadoHastaAhora: Int64 = 0
    @State private var arrastreIniciado = false
    @State private var inicioDelArrastre: Int64 = 0
    @State private var pistaDelArrastre: UUID?
    @State private var origenDelPaneo: CGFloat?

    private var ancho: Double { max(2, Double(clip.duracion) * escala) }
    private var seleccionado: Bool { editor.selectedClipID == clip.id || editor.selectedClipIDs.contains(clip.id) }
    private var offline: Bool { editor.estaOffline(clip) }

    var body: some View {
        ZStack(alignment: .topLeading) {
            fondo
            if pista.tipo == .video, let miniatura = editor.miniatura(for: clip) {
                Image(decorative: miniatura, scale: 1)
                    .resizable()
                    .scaledToFill()
                    .frame(width: ancho, height: pista.altura - 4)
                    .clipShape(RoundedRectangle(cornerRadius: 3))
                    .opacity(clip.habilitado ? 0.55 : 0.2)
                    .overlay {
                        LinearGradient(
                            colors: [.black.opacity(0.05), .black.opacity(0.58)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .clipShape(RoundedRectangle(cornerRadius: 3))
                    }
                    .allowsHitTesting(false)
            }
            if pista.tipo == .audio, let forma = editor.formaDeOnda(for: clip) {
                FormaDeOndaView(muestras: forma)
                    .padding(.horizontal, 4)
                    .padding(.top, 18)
                    .frame(width: ancho, height: pista.altura - 4, alignment: .center)
                    .opacity(clip.habilitado ? 0.8 : 0.25)
                    .allowsHitTesting(false)
            }
            // Automatización de volumen: la curva de ganancia se dibuja sobre
            // el clip de audio, como la línea de volumen de Premiere. Con
            // keyframes se ve la rampa; sin ellos, la ganancia plana del clip.
            if pista.tipo == .audio, clip.ganancia != 0 || (clip.keyframes?.contains { $0.ganancia != 0 } ?? false) {
                CurvaDeGananciaView(clip: clip, ancho: ancho, alto: pista.altura - 4)
                    .allowsHitTesting(false)
            }
            contenido
            fundidos
            indicadoresDeTransicion
            if seleccionado {
                RoundedRectangle(cornerRadius: 3).stroke(.white, lineWidth: 1.6)
            }
        }
        .frame(width: ancho, height: pista.altura - 4)
        .padding(.vertical, 2)
        .contentShape(Rectangle())
        .onTapGesture {
            // Con la Mano, el clic panea y no debe cambiar la selección.
            guard editor.herramienta != .mano else { return }
            editor.seleccionarClip(clip.id, extender: NSEvent.modifierFlags.contains(.shift))
        }
        .gesture(gestoDelCuerpo)
        .onHover { dentro in
            guard editor.herramienta == .mano, !pista.bloqueada else { return }
            if dentro { NSCursor.openHand.push() } else { NSCursor.pop() }
        }
        .overlay(alignment: .leading) { tirador(.entrada) }
        .overlay(alignment: .trailing) { tirador(.salida) }
        .contextMenu { menu }
        .help(descripcion)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(descripcion)
        .accessibilityValue(offline ? "Offline" : detalle)
        .accessibilityAddTraits(seleccionado ? [.isSelected] : [])
        .accessibilityHint("Doble clic para seleccionar. Usa el menú contextual para editar")
        .accessibilityAction { editor.seleccionarClip(clip.id) }
    }

    // MARK: Aspecto

    private var fondo: some View {
        RoundedRectangle(cornerRadius: 3)
            .fill(
                LinearGradient(
                    colors: clip.esAjuste
                        ? [.purple.opacity(0.65), .purple.opacity(0.35)]
                        : (offline
                           ? [.red.opacity(0.45), .red.opacity(0.25)]
                           : (pista.tipo == .video
                              ? [colorBase.opacity(0.72), colorBase.opacity(0.42)]
                              : [.green.opacity(0.55), .green.opacity(0.3)])),
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
            )
            .opacity(clip.habilitado ? 1 : 0.35)
    }

    /// Una pista de ajuste se dibuja con su nombre fijo: no hay medio que mostrar.
    private var nombreVisible: String {
        clip.esAjuste ? "Ajuste" : (editor.mediaItem(for: clip)?.name ?? clip.nombre)
    }

    private var contenido: some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: 3) {
                if offline && !clip.esAjuste {
                    Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 8))
                } else if !clip.habilitado {
                    Image(systemName: "eye.slash").font(.system(size: 8))
                }
                Text(nombreVisible)
                    .font(.system(size: 9, weight: .semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(clip.nombre)
            }
            if ancho > 90 {
                Text(detalle)
                    .font(.system(size: 7.5, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.75))
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 5).padding(.top, 3)
        .frame(width: ancho, alignment: .leading)
        .clipped()
    }

    /// Los fundidos se dibujan como los triángulos de siempre: se leen de un vistazo
    /// y son la única forma de ver que un clip entra o sale sin abrir el inspector.
    private var fundidos: some View {
        ZStack(alignment: .topLeading) {
            if clip.entradaFundido > 0 {
                Path { p in
                    let w = Double(clip.entradaFundido) * escala
                    p.move(to: CGPoint(x: 0, y: pista.altura - 6))
                    p.addLine(to: CGPoint(x: w, y: 0))
                    p.addLine(to: CGPoint(x: 0, y: 0))
                    p.closeSubpath()
                }
                .fill(.black.opacity(0.45))
            }
            if clip.salidaFundido > 0 {
                Path { p in
                    let w = Double(clip.salidaFundido) * escala
                    p.move(to: CGPoint(x: ancho, y: pista.altura - 6))
                    p.addLine(to: CGPoint(x: ancho - w, y: 0))
                    p.addLine(to: CGPoint(x: ancho, y: 0))
                    p.closeSubpath()
                }
                .fill(.black.opacity(0.45))
            }
        }
        .allowsHitTesting(false)
    }

    private var indicadoresDeTransicion: some View {
        HStack {
            if clip.transicionEntrada != nil {
                Image(systemName: "arrowtriangle.right.fill")
                    .font(.system(size: 8))
                    .foregroundStyle(.white.opacity(0.9))
                    .padding(3)
                    .background(.black.opacity(0.5), in: RoundedRectangle(cornerRadius: 2))
                    .help("Tiene transición de entrada")
            }
            Spacer()
            if clip.transicionSalida != nil {
                Image(systemName: "arrowtriangle.left.fill")
                    .font(.system(size: 8))
                    .foregroundStyle(.white.opacity(0.9))
                    .padding(3)
                    .background(.black.opacity(0.5), in: RoundedRectangle(cornerRadius: 2))
                    .help("Tiene transición de salida")
            }
        }
        .padding(.horizontal, 3)
        .padding(.top, 3)
        .allowsHitTesting(false)
    }

    private var colorBase: Color {
        switch clip.etiqueta {
        case .ninguna: .cyan
        case .rojo: .red
        case .naranja: .orange
        case .amarillo: .yellow
        case .verde: .green
        case .azul: .blue
        case .morado: .purple
        case .rosa: .pink
        }
    }

    private var detalle: String {
        var partes = [editor.timebase.timecode(clip.duracion)]
        if clip.velocidad != 1 { partes.append(String(format: "%.0f%%", clip.velocidad * 100)) }
        if clip.ganancia != 0 { partes.append(String(format: "%+.0fdB", clip.ganancia)) }
        return partes.joined(separator: " · ")
    }

    private var descripcion: String {
        if clip.esAjuste {
            return "Ajuste\n\(editor.timebase.timecode(clip.inicio)) → \(editor.timebase.timecode(clip.fin))"
        }
        let nombre = editor.mediaItem(for: clip)?.name ?? clip.nombre
        return "\(nombre)\n\(editor.timebase.timecode(clip.inicio)) → \(editor.timebase.timecode(clip.fin))"
            + (offline ? "\nMedio no encontrado" : "")
    }

    // MARK: Gestos

    /// Arrastrar el cuerpo. Cambia de significado con la herramienta, y con la de
    /// cuchilla un simple clic corta por donde se pulsa. La Mano panea el
    /// timeline sin entrar en el sistema de edición.
    private var gestoDelCuerpo: some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { valor in
                guard !pista.bloqueada else { return }
                if editor.herramienta == .cuchilla { return }

                if editor.herramienta == .mano {
                    guard let scroll = controlDelLienzo.scroll else { return }
                    if origenDelPaneo == nil {
                        origenDelPaneo = scroll.contentView.bounds.origin.x
                        NSCursor.closedHand.push()
                    }
                    scroll.contentView.scroll(to: NSPoint(x: origenDelPaneo! - valor.translation.width, y: 0))
                    scroll.reflectScrolledClipView(scroll.contentView)
                    return
                }

                if !arrastreIniciado {
                    arrastreIniciado = true
                    editor.beginTrim()
                    editor.selectedClipID = clip.id
                    inicioDelArrastre = clip.inicio
                    pistaDelArrastre = pista.id
                    aplicadoHastaAhora = 0
                }
                arrastreAcumulado = valor.translation.width
                let objetivo = Int64((arrastreAcumulado / escala).rounded())

                switch editor.herramienta {
                case .slip:
                    // El slip va al revés de lo que se arrastra: mover la imagen a la
                    // derecha significa entrar antes en el material.
                    editor.deslizarContenido(clip.id, delta: -(objetivo - aplicadoHastaAhora))
                    aplicadoHastaAhora = objetivo
                case .slide:
                    aplicadoHastaAhora += editor.deslizarPosicion(clip.id, delta: objetivo - aplicadoHastaAhora)
                default:
                    let pistaDestino = editor.pistaDestino(
                        de: pistaDelArrastre ?? pista.id,
                        desplazamientoVertical: valor.translation.height
                    )
                    editor.moverClip(
                        clip.id, aPista: pistaDestino,
                        aFrame: inicioDelArrastre + objetivo,
                        umbralDeIman: umbralDeIman
                    )
                    aplicadoHastaAhora = objetivo
                }
            }
            .onEnded { valor in
                if editor.herramienta == .cuchilla {
                    let frame = clip.inicio + Int64(valor.location.x / escala)
                    editor.cortar(en: frame, pista: pista.id)
                } else if editor.herramienta == .mano {
                    if origenDelPaneo != nil {
                        NSCursor.pop()
                        origenDelPaneo = nil
                    }
                } else if arrastreIniciado {
                    editor.endTrim()
                }
                arrastreAcumulado = 0
                aplicadoHastaAhora = 0
                arrastreIniciado = false
                inicioDelArrastre = 0
                pistaDelArrastre = nil
            }
    }

    private func tirador(_ borde: BordeDeClip) -> some View {
        Rectangle()
            .fill(.white.opacity(0.001))
            .frame(width: min(9, max(3, ancho / 3)))
            .contentShape(Rectangle())
            .onHover { dentro in
                // Con la Mano, el borde no recorta: el cursor sigue siendo de mano.
                if dentro && !pista.bloqueada && editor.herramienta != .mano { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { valor in
                        guard !pista.bloqueada, editor.herramienta != .mano else { return }
                        if aplicadoHastaAhora == 0 && arrastreAcumulado == 0 {
                            editor.beginTrim()
                            editor.selectedClipID = clip.id
                        }
                        arrastreAcumulado = valor.translation.width
                        let objetivo = Int64((arrastreAcumulado / escala).rounded())
                        aplicadoHastaAhora += editor.recortar(
                            clip.id, borde: borde, delta: objetivo - aplicadoHastaAhora
                        )
                    }
                    .onEnded { _ in
                        guard editor.herramienta != .mano else { return }
                        editor.endTrim()
                        arrastreAcumulado = 0
                        aplicadoHastaAhora = 0
                    }
            )
    }

    /// Mandos de la rueda primaria del clic derecho, con su clave en `ColorDeClip`.
    private let mandosDeColor: [(String, String)] = [
        ("Exposición +10", "exposicion"),
        ("Contraste +10", "contraste"),
        ("Saturación +10", "saturacion"),
        ("Temperatura +10", "temperatura"),
        ("Altas +10", "altas"),
        ("Sombras +10", "sombras"),
    ]

    @ViewBuilder
    private var menu: some View {
        Button("Cortar aquí") { editor.cortar(en: editor.cabezal, pista: pista.id) }
        Button(clip.habilitado ? "Desactivar" : "Activar") { editor.alternarHabilitado(clip.id) }
        Button("Fundido de 1 s") { editor.fundidoRapido(clip.id) }
        Divider()
        Button("Copiar atributos") { editor.copiarAtributos() }
            .disabled(editor.selectedClipID != clip.id)
        Button("Pegar atributos") { editor.pegarAtributos() }
            .disabled(editor.atributosCopiados == nil || editor.selectedClipID != clip.id)
        Button("Match frame") { editor.matchFrame() }
        Button("Extend edit hasta el cabezal") { editor.extendEdit() }
        if pista.tipo == .video {
            Divider()
            Button("Reencuadre vertical automático") { editor.reframearVertical(clip.id) }
            Menu("Corrección de color") {
                ForEach(mandosDeColor, id: \.0) { etiqueta, clave in
                    Button(etiqueta) { editor.fijarColorDeClip(clip.id) { color in
                        switch clave {
                        case "exposicion": color.exposicion = min(max(color.exposicion + 10, -100), 100)
                        case "contraste": color.contraste = min(max(color.contraste + 10, -100), 100)
                        case "saturacion": color.saturacion = min(max(color.saturacion + 10, -100), 100)
                        case "temperatura": color.temperatura = min(max(color.temperatura + 10, -100), 100)
                        case "altas": color.altas = min(max(color.altas + 10, -100), 100)
                        case "sombras": color.sombras = min(max(color.sombras + 10, -100), 100)
                        default: break
                        }
                    } }
                }
                Divider()
                Button("Restablecer") { editor.fijarColorDeClip(clip.id) { $0 = .neutro } }
            }
        }
        Divider()
        Menu("Etiqueta") {
            ForEach(EtiquetaDeColor.allCases, id: \.self) { etiqueta in
                Button(etiqueta.nombre) { editor.etiquetar(clip.id, etiqueta) }
            }
        }
        Menu("Velocidad") {
            ForEach([0.25, 0.5, 1.0, 1.5, 2.0, 4.0], id: \.self) { v in
                Button(String(format: "%.0f %%", v * 100)) { editor.fijarVelocidad(clip.id, v) }
            }
        }
        if editor.esNido(clip) {
            Button("Desanidar") { editor.desanidar(clipID: clip.id) }
                .help("Devolver los clips del interior del nido al montaje")
        }
        Divider()
        if offline {
            Button("Revincular medio…") { editor.revincular(clip.mediaID) }
        }
        Button("Quitar dejando hueco") {
            editor.selectedClipID = clip.id
            editor.removeSelectedClip()
        }
        Button("Quitar y cerrar hueco", role: .destructive) {
            editor.selectedClipID = clip.id
            editor.borrarConArrastre()
        }
    }
}

private struct FormaDeOndaView: View {
    let muestras: [Float]

    var body: some View {
        GeometryReader { geometria in
            Path { camino in
                guard !muestras.isEmpty else { return }
                let centro = geometria.size.height / 2
                let paso = geometria.size.width / CGFloat(max(1, muestras.count - 1))
                for (indice, muestra) in muestras.enumerated() {
                    let x = CGFloat(indice) * paso
                    let alto = max(1, CGFloat(muestra) * centro * 0.9)
                    camino.move(to: CGPoint(x: x, y: centro - alto))
                    camino.addLine(to: CGPoint(x: x, y: centro + alto))
                }
            }
            .stroke(.white.opacity(0.72), lineWidth: 1)
        }
    }
}

/// La curva de ganancia de un clip de audio: una línea horizontal al nivel del
/// clip (los decibelios como fracción de la altura) con las rampas de los
/// keyframes dibujadas encima. Es la automatización de volumen visible de los
/// NLE grandes, sin editar: solo informa de cómo suena.
private struct CurvaDeGananciaView: View {
    let clip: Clip
    let ancho: CGFloat
    let alto: CGFloat

    var body: some View {
        GeometryReader { geometria in
            Path { camino in
                let margen = geometria.size.height * 0.18
                let centro = geometria.size.height / 2
                func yDe(_ ganancia: Double) -> CGFloat {
                    // +12 dB arriba, −60 dB abajo, con el 0 en el centro.
                    let normalizada = min(max((ganancia + 12) / 72, 0), 1)
                    return centro + (CGFloat(normalizada) - 0.5) * (geometria.size.height - margen * 2)
                }
                let tramos = tramosDeGanancia
                camino.move(to: CGPoint(x: 0, y: yDe(tramos[0].inicio)))
                for tramo in tramos {
                    camino.addLine(to: CGPoint(x: CGFloat(tramo.frame) / CGFloat(clip.duracion) * geometria.size.width,
                                               y: yDe(tramo.inicio)))
                    camino.addLine(to: CGPoint(x: CGFloat(tramo.frame) / CGFloat(clip.duracion) * geometria.size.width,
                                               y: yDe(tramo.fin)))
                }
            }
            .stroke(.yellow.opacity(0.9), lineWidth: 1.6)
        }
        .frame(width: ancho, height: alto)
    }

    /// Puntos donde cambia la pendiente de la ganancia: los keyframes.
    private var tramosDeGanancia: [(frame: Int64, inicio: Double, fin: Double)] {
        guard let keyframes = clip.keyframes, !keyframes.isEmpty else {
            return [(0, clip.ganancia, clip.ganancia)]
        }
        let ordenadas = keyframes.sorted { $0.frame < $1.frame }
        var tramos: [(frame: Int64, inicio: Double, fin: Double)] = [(0, clip.ganancia, ordenadas[0].ganancia)]
        for i in 1..<ordenadas.count {
            tramos.append((ordenadas[i - 1].frame, ordenadas[i - 1].ganancia, ordenadas[i].ganancia))
        }
        return tramos
    }
}

/// El delegado de soltado del lienzo del timeline: recibe un medio arrastrado
/// desde la biblioteca, enseña la guía bajo el puntero y lo inserta (o
/// superpone con ⌥) en el frame correspondiente.
private struct SueltaDeMedio: DropDelegate {
    let editor: EditorState
    let escala: Double
    let anchoDelLienzo: Double
    let pistaID: UUID?

    init(editor: EditorState, escala: Double, anchoDelLienzo: Double, pistaID: UUID? = nil) {
        self.editor = editor
        self.escala = escala
        self.anchoDelLienzo = anchoDelLienzo
        self.pistaID = pistaID
    }

    /// El identificador del medio arrastrado, para validar el arrastre.
    private let prefijo = "editorcito.media."

    func validateDrop(info: DropInfo) -> Bool {
        info.hasItemsConforming(to: [.utf8PlainText])
    }

    func dropEntered(info: DropInfo) {
        actualizarFrame(info)
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        actualizarFrame(info)
        return DropProposal(operation: .copy)
    }

    func dropExited(info: DropInfo) {
        editor.frameDeSuelta = nil
    }

    func performDrop(info: DropInfo) -> Bool {
        editor.frameDeSuelta = nil
        guard let provider = info.itemProviders(for: [.utf8PlainText]).first else { return false }
        provider.loadObject(ofClass: NSString.self) { objeto, _ in
            guard let texto = objeto as? String,
                  texto.hasPrefix(prefijo),
                  let id = UUID(uuidString: String(texto.dropFirst(prefijo.count))) else { return }
            let frame = Int64((info.location.x / escala).rounded())
            let superponer = NSEvent.modifierFlags.contains(.option)
            Task { @MainActor in
                editor.soltarEnTimeline(
                    mediaID: id,
                    enFrame: frame,
                    superponer: superponer,
                    enPista: pistaID
                )
            }
        }
        return true
    }

    private func actualizarFrame(_ info: DropInfo) {
        let frame = Int64(min(max(info.location.x / escala, 0), anchoDelLienzo / escala))
        if editor.frameDeSuelta != frame {
            editor.frameDeSuelta = frame
        }
    }
}
