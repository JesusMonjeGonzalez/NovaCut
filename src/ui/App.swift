import AppKit
import AVFoundation
import AVKit
import SwiftUI
import UniformTypeIdentifiers

struct MediaItem: Identifiable, Hashable {
    let id: UUID
    let url: URL
    let duration: Double
    let size: CGSize
    let fileSize: Int64
    let frameRate: Double
    var variableFrameRate: Bool = false
    var bin: String = "Todos"
    /// Si es un subclip, qué recorte del medio base representa.
    var subclipDe: SubclipOrigen? = nil

    init(id: UUID = UUID(), url: URL, duration: Double, size: CGSize, fileSize: Int64 = 0, frameRate: Double = 0, variableFrameRate: Bool = false, bin: String = "Todos", subclipDe: SubclipOrigen? = nil) {
        self.id = id
        self.url = url
        self.duration = duration
        self.size = size
        self.fileSize = fileSize
        self.frameRate = frameRate
        self.variableFrameRate = variableFrameRate
        self.bin = bin
        self.subclipDe = subclipDe
    }

    var name: String { url.deletingPathExtension().lastPathComponent }
    var detail: String {
        let resolution = size == .zero ? "Audio" : "\(Int(size.width))×\(Int(size.height))"
        let bytes = ByteCountFormatter.string(fromByteCount: fileSize, countStyle: .file)
        let fps = frameRate > 0 ? String(format: " · %.3g fps", frameRate) : ""
        let vfr = variableFrameRate ? " · VFR" : ""
        return "\(resolution)  ·  \(duration.timecode)\(fps)\(vfr)  ·  \(bytes)"
    }
}

struct RecuperacionPendiente: Identifiable {
    let id = UUID()
    let nombre: String
    let fecha: Date?

    var detalle: String {
        if let fecha {
            return "Hay una recuperación automática de «\(nombre)» del \(fecha.formatted(date: .abbreviated, time: .shortened))."
        }
        return "Hay una recuperación automática de «\(nombre)»."
    }
}

struct TimelineClip: Identifiable, Hashable {
    let id: UUID
    let mediaID: UUID
    var sourceIn: Double
    var sourceOut: Double

    init(id: UUID = UUID(), mediaID: UUID, sourceIn: Double, sourceOut: Double) {
        self.id = id
        self.mediaID = mediaID
        self.sourceIn = sourceIn
        self.sourceOut = sourceOut
    }

    var duration: Double { max(0, sourceOut - sourceIn) }
}

/// Interruptores de una pista. Van juntos porque comparten el mismo camino: no
/// tocan el documento, pero sí lo que se ve y se oye.
enum CampoDePista { case silencio, solo, bloqueo, visible, ducking }

/// El instrumento que se dibuja bajo el monitor de programa.
enum InstrumentoDeMonitor: String, CaseIterable, Identifiable {
    case ninguno
    case formaDeOnda
    case vectorscopio
    case paradeRGB
    case histograma

    var id: String { rawValue }

    var nombre: String {
        switch self {
        case .ninguno: "Ninguno"
        case .formaDeOnda: "Forma de onda"
        case .vectorscopio: "Vectorscopio"
        case .paradeRGB: "Parade RGB"
        case .histograma: "Histograma"
        }
    }
}

/// El canal cuya curva se edita en el editor de curvas.
enum CanalDeCurva: String, CaseIterable, Identifiable {
    case luma, rojo, verde, azul

    var id: String { rawValue }

    var nombre: String {
        switch self {
        case .luma: "Luminancia"
        case .rojo: "Rojo"
        case .verde: "Verde"
        case .azul: "Azul"
        }
    }

    var color: Color {
        switch self {
        case .luma: .white
        case .rojo: .red
        case .verde: .green
        case .azul: .blue
        }
    }
}

enum ModoDeMonitor: String, CaseIterable, Identifiable {
    case programa
    case origen

    var id: String { rawValue }

    var nombre: String {
        switch self {
        case .programa: "Programa"
        case .origen: "Origen"
        }
    }
}

@MainActor
final class EditorState: ObservableObject {
    let aiSettings = AISettings()
    /// Transcribir automáticamente los medios al importarlos. Preferencia del
    /// usuario: el reconocimiento on-device consume CPU y el Mac puede estar
    /// ocupado montando.
    @AppStorage("editorcito.transcribirAlImportar") var transcribirAlImportar = false
    @Published private(set) var media: [MediaItem] = []
    /// El montaje. Multipista, en frames enteros y con historial por instantánea.
    @Published private(set) var montaje = LineaDeTiempo.nueva()
    /// Medios con sus pistas ya resueltas, para no rehacer el `AVURLAsset` en cada
    /// reconstrucción de la vista previa.
    private var medios: [UUID: MedioResuelto] = [:]
    private var mediosOriginales: [UUID: MedioResuelto] = [:]
    private var proxyURLs: [UUID: URL] = [:]
    @Published private(set) var miniaturas: [UUID: CGImage] = [:]
    @Published private(set) var formasDeOnda: [UUID: [Float]] = [:]
    @Published var selectedMediaID: UUID?
    @Published var selectedMediaIDs: Set<UUID> = []
    @Published var selectedClipID: UUID?
    @Published var timelineHasFocus = false
    @Published var selectedClipIDs: Set<UUID> = []
    /// Pista donde caen las inserciones y los cortes.
    @Published var pistaActiva: UUID?
    @Published var herramienta: Herramienta = .seleccion
    @Published var imanActivo = true
    @Published var playhead = 0.0
    /// Cabezal del monitor de origen, para marcar entrada y salida antes de montar.
    @Published var cabezalDeOrigen: Int64 = 0
    @Published private(set) var entradaDeOrigen: [UUID: Int64] = [:]
    @Published private(set) var salidaDeOrigen: [UUID: Int64] = [:]
    @Published var isPlaying = false
    @Published private(set) var playbackRate: Float = 0
    /// ¿Se está arrastrando el cabezal con el audio sonando (audio scrubbing)?
    @Published var scrubbing = false
    /// Zoom del monitor de programa (1 = ajustar, 2 = 100 % sobre el lienzo).
    @Published var zoomDeMonitor: Double = 1
    /// Frame bajo el puntero mientras se arrastra un medio sobre el timeline,
    /// para la guía de soltado; `nil` fuera del arrastre.
    @Published var frameDeSuelta: Int64? = nil
    @Published var isPlayingOrigen = false
    @Published var monitorActual: ModoDeMonitor = .programa
    /// Instrumento del monitor: forma de onda, vectorscopio o ninguno.
    @Published var instrumentoDeMonitor: InstrumentoDeMonitor = .ninguno
    /// Distribución de la forma de onda del frame actual del monitor, o `nil`
    /// si aún no hay nada que medir.
    @Published private(set) var formaDeOnda: [[Float]]?
    /// Densidad de croma del vectorscopio del frame actual, o `nil`.
    @Published private(set) var vectorscopio: [[Float]]?
    /// Parade RGB del frame actual, o `nil`.
    @Published private(set) var paradeRGB: [[[Float]]]?
    /// Histograma de luminancia del frame actual, o `nil`.
    @Published private(set) var histograma: [Float]?
    /// Cuándo se pidió el último instrumento, para no recalcularlo en cada
    /// fotograma del cabezal.
    private var ultimoFrameDeOnda: Int64 = -1
    @Published var mediaSearch = ""
    @Published var selectedBin = "Todos"
    @Published var isExporting = false
    @Published var exportProgress = 0.0
    @Published private(set) var exportQueueCount = 0
    /// Destino de sonoridad elegido para el próximo trabajo de exportación.
    @Published var normalizacionDeExportacion: ObjetivoDeSonoridad = .ninguno
    @Published var status = "Importa vídeo o audio para empezar"
    @Published var aiRequest = ""
    @Published var aiWorking = false
    @Published var aiResult: String?
    @Published var transcribing = false
    /// Medios en espera de transcripción (por orden de llegada).
    @Published private(set) var colaDeTranscripcion: [UUID] = []
    /// La tarea que está transcribiendo ahora mismo, para poder cancelarla.
    private var tareaDeTranscripcion: Task<Void, Never>?
    /// Palabras marcadas en el panel de texto, por índice en `palabrasDelMontaje`.
    /// Se vacía tras cada edición porque la lista se reconstruye y los índices
    /// dejarían de señalar lo que el usuario había marcado.
    @Published var seleccionDeTexto: Set<Int> = []
    @Published var mostrarTranscript = false
    /// Lo que se busca en el panel de texto. Con algo escrito, el panel enseña
    /// coincidencias en vez del transcript del montaje.
    @Published var busquedaDeTexto = ""
    @Published private(set) var midiendoSilencios = false
    @Published var projectName = "Sin título"
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false
    @Published private(set) var isDirty = false
    @Published var timelineScale = 24.0
    @Published private(set) var isImporting = false
    @Published private(set) var importProgress = 0.0
    @Published var proxiesActivos = false
    @Published var generandoProxies = false
    @Published var proxyProgress = 0.0
    @Published var recuperacionPendiente: RecuperacionPendiente?

    let player = AVPlayer()
    let sourcePlayer = AVPlayer()
    /// Reproductores vivos del visor multiángulo: uno por cámara del grupo,
    /// mudos (el audio lo pone la composición del programa). Solo existen
    /// mientras hay un clip multicámara seleccionado.
    private var reproductoresDeAngulos: [UUID: AVPlayer] = [:]
    /// Generación del visor: un grupo distinto (o el mismo recién reconstruido)
    /// invalida los reproductores viejos sin esperar a que se liberen solos.
    private var generacionDelVisor = 0
    private var temporizadorDeAngulos: Timer?
    /// Grupo al que pertenecen los reproductores del visor, para saber si al
    /// reconstruir el preview hay que rehacerlos o basta con re-sincronizarlos.
    private var grupoDelVisor: UUID?
    private var composition: AVMutableComposition?
    private var timeObserver: Any?
    private var sourceTimeObserver: Any?
    private var keyMonitor: Any?
    private var sourceMediaID: UUID?
    private var exportTimer: Timer?
    private var activeExportSession: AVAssetExportSession?
    private var exportQueue: [TrabajoDeExportacion] = []
    private var undoStack: [EditSnapshot] = []
    private var redoStack: [EditSnapshot] = []
    private var trimSnapshot: EditSnapshot?
    private var autosaveTask: Task<Void, Never>?
    private var previewGeneration = 0
    /// El último render instalado en el reproductor. La forma de onda del monitor
    /// y la miniatura de la IA se regeneran desde él: el `asset` del item nunca
    /// es un `AVURLAsset` (siempre es la composición), así que es la única fuente
    /// de un frame que coincide con lo que se ve.
    private var ultimoRender: MontajeRenderizable?
    private var documentRevision = 0
    private var aiTask: Task<Void, Never>?
    /// Generación de peticiones de IA: cada `editWithAI` la incrementa, y el
    /// final de una petición vieja (cancelada) no toca el estado si ya hay una
    /// nueva en curso.
    private var aiGeneracion = 0
    private var proyectoPendienteDeRecuperacion: ProyectoEditorcito?
    /// Carpeta del proyecto guardado, base de las rutas relativas de los medios.
    private var carpetaDelProyecto: URL?
    lazy var windowDelegate = EditorWindowDelegate(editor: self)

    init() {
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 1.0 / 20.0, preferredTimescale: 600),
            queue: .main
        ) { [weak self] time in
            Task { @MainActor in
                guard let self else { return }
                self.playhead = time.seconds.isFinite ? time.seconds : 0
                self.isPlaying = self.player.rate != 0
                self.playbackRate = self.player.rate
            }
        }
        sourceTimeObserver = sourcePlayer.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 1.0 / 20.0, preferredTimescale: 600),
            queue: .main
        ) { [weak self] time in
            Task { @MainActor in
                guard let self else { return }
                let frame = self.timebase.frames(segundos: time.seconds)
                self.cabezalDeOrigen = max(0, min(frame, self.duracionDeOrigenEnFrames))
                self.isPlayingOrigen = self.sourcePlayer.rate != 0
            }
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.procesarTeclaDeEditor(event) else { return event }
            return nil
        }
        recoverAutosave()
    }

    deinit {
        if let timeObserver { player.removeTimeObserver(timeObserver) }
        if let sourceTimeObserver { sourcePlayer.removeTimeObserver(sourceTimeObserver) }
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        exportTimer?.invalidate()
        autosaveTask?.cancel()
    }

    // MARK: Lectura del montaje

    var timebase: Timebase { montaje.timebase }

    var selectedClip: Clip? { selectedClipID.flatMap { montaje.clip($0) } }

    var selectedMedia: MediaItem? { media.first { $0.id == selectedMediaID } }

    var nombresDeBins: [String] {
        let definidos = montaje.bins?.map(\.nombre) ?? []
        let usados = media.map(\.bin)
        return ["Todos"] + Array(Set(definidos + usados).subtracting(["Todos"])).sorted()
    }

    var mediosVisibles: [MediaItem] {
        let termino = mediaSearch.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        // Un bin inteligente filtra por su `filtro`; los bins normales por
        // pertenencia manual.
        let binInteligente = montaje.bins?.first { $0.nombre == selectedBin && $0.esInteligente }
        return media.filter { item in
            let coincideBin: Bool
            if let inteligente = binInteligente {
                coincideBin = inteligente.contiene((item.name, item.variableFrameRate, item.size == .zero))
            } else {
                coincideBin = selectedBin == "Todos" || item.bin == selectedBin
            }
            let coincideTexto = termino.isEmpty || item.name.lowercased().contains(termino)
            return coincideBin && coincideTexto
        }
    }

    /// Cabezal en frames. Es la posición verdadera; `playhead` en segundos existe
    /// solo porque el reproductor habla en segundos.
    var cabezal: Int64 { timebase.frames(segundos: playhead) }

    var duracionEnFrames: Int64 { montaje.duracion }
    var timelineDuration: Double { timebase.segundos(montaje.duracion) }

    var duracionDeOrigenEnFrames: Int64 {
        guard let selectedMedia else { return 0 }
        return max(0, timebase.frames(segundos: selectedMedia.duration))
    }

    var timecodeDelCabezal: String { timebase.timecode(cabezal) }
    var timecodeDeOrigen: String { timebase.timecode(cabezalDeOrigen) }
    var subtituloActivo: Subtitulo? {
        montaje.subtitulos?.first { $0.inicio <= cabezal && cabezal < $0.fin }
    }

    /// Pista sobre la que actúan las acciones sin destino explícito.
    var pistaDeTrabajo: UUID {
        if let pistaActiva, montaje.pista(pistaActiva) != nil { return pistaActiva }
        return montaje.pistas.first { $0.tipo == .video }?.id ?? montaje.pistas[0].id
    }

    func mediaItem(for clip: Clip) -> MediaItem? { media.first { $0.id == clip.mediaID } }

    func medioResuelto(_ id: UUID) -> MedioResuelto? { medios[id] }

    func fijarProxies(_ activar: Bool) {
        guard !generandoProxies else { return }
        proxiesActivos = activar
        if !activar {
            medios = mediosOriginales
            ultimoRender = nil
            cargarMedioEnOrigen(selectedMediaID)
            rebuildPreview(keepPosition: true)
            status = "Preview con originales"
            return
        }
        if proxyURLs.count < mediosOriginales.values.filter(\.tieneVideo).count {
            generarProxies()
        } else {
            cargarProxiesEnPreview()
        }
    }

    func generarProxies() {
        guard !generandoProxies else { return }
        let candidatos = mediosOriginales.values.filter(\.tieneVideo)
        guard !candidatos.isEmpty else { status = "No hay vídeo para generar proxies"; return }
        generandoProxies = true
        proxyProgress = 0
        proxiesActivos = true
        Task {
            var creados = 0
            for medio in candidatos {
                do {
                    let url = try await ProxyService.crear(id: medio.id, asset: medio.asset)
                    proxyURLs[medio.id] = url
                    let proxy = try await MedioResuelto.cargar(id: medio.id, url: url)
                    medios[medio.id] = proxy
                    creados += 1
                } catch {
                    status = "Proxy fallido: \(medio.url.lastPathComponent)"
                }
                proxyProgress = Double(creados) / Double(candidatos.count)
            }
            generandoProxies = false
            ultimoRender = nil
            cargarMedioEnOrigen(selectedMediaID)
            rebuildPreview(keepPosition: true)
            status = "\(creados) proxy\(creados == 1 ? "" : "s") listo\(creados == 1 ? "" : "s")"
        }
    }

    private func cargarProxiesEnPreview() {
        Task {
            medios = mediosOriginales
            for id in mediosOriginales.keys {
                guard let url = proxyURLs[id], let proxy = try? await MedioResuelto.cargar(id: id, url: url) else { continue }
                medios[id] = proxy
            }
            ultimoRender = nil
            cargarMedioEnOrigen(selectedMediaID)
            rebuildPreview(keepPosition: true)
        }
    }

    func anadirBin() {
        let existentes = Set(nombresDeBins)
        var indice = 1
        while existentes.contains("Bin \(indice)") { indice += 1 }
        let nombre = "Bin \(indice)"
        performEdit(keepPosition: true) {
            if montaje.bins == nil { montaje.bins = [] }
            montaje.bins?.append(MediaBin(nombre: nombre))
        }
        selectedBin = nombre
    }

    /// Añade un bin inteligente: se llena solo con los medios que cumplen el
    /// filtro. «VFR» y «Audio» tienen significado propio; cualquier otra
    /// palabra busca en el nombre del medio.
    func anadirBinInteligente(filtro: String) {
        let limpio = filtro.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !limpio.isEmpty else { return }
        performEdit(keepPosition: true) {
            if montaje.bins == nil { montaje.bins = [] }
            montaje.bins?.append(MediaBin(nombre: "Smart: \(limpio)", filtro: limpio))
        }
        selectedBin = "Smart: \(limpio)"
        status = "Bin inteligente «\(limpio)» · se llena solo"
    }

    func moverMedio(_ id: UUID, aBin bin: String) {
        guard let indice = media.firstIndex(where: { $0.id == id }) else { return }
        performEdit(keepPosition: true) { media[indice].bin = bin }
    }

    func crearGrupoMulticamConMediosVisibles() {
        let fuentes = mediosVisibles.filter { selectedMediaIDs.contains($0.id) && medios[$0.id]?.tieneVideo == true }
        guard fuentes.count >= 2 else {
            status = "Selecciona al menos dos vídeos en la biblioteca para crear un grupo multicámara"
            return
        }
        let grupoID = UUID()
        let inicios = fuentes.map { inicioDeAudio($0) }
        let referencia = inicios.max() ?? 0
        // Los desfases viven en el grupo, no solo en las pistas: el clip
        // multicámara necesita saber cuándo arranca el material de cada
        // cámara para reproducirla fuera de su pista.
        var desfases: [UUID: Int64] = [:]
        for fuente in fuentes {
            let indice = fuentes.firstIndex { $0.id == fuente.id } ?? 0
            desfases[fuente.id] = max(0, timebase.frames(segundos: referencia - inicios[indice]))
        }
        let grupo = GrupoMulticam(
            id: grupoID,
            nombre: "Multicam \((montaje.gruposMulticam?.count ?? 0) + 1)",
            mediaIDs: fuentes.map(\.id),
            sincronizadoPorAudio: inicios.contains { $0 > 0 },
            desfases: desfases
        )
        performEdit(keepPosition: true) {
            if montaje.gruposMulticam == nil { montaje.gruposMulticam = [] }
            montaje.gruposMulticam?.append(grupo)
            for (indice, fuente) in fuentes.enumerated() {
                let pista = Pista(tipo: .video, nombre: "MC\(indice + 1)")
                montaje.pistas.insert(pista, at: 0)
                var clip = clipDe(fuente, enlace: grupoID)
                let desfase = desfases[fuente.id] ?? 0
                clip.inicio = desfase
                montaje.sobrescribir(clip, enPista: pista.id, en: desfase)
                if indice == 0 { selectedClipID = clip.id }
            }
        }
        status = "Grupo multicámara creado · \(fuentes.count) ángulos"
    }

    /// Inserta un clip multicámara en el cabezal: un solo clip en el timeline
    /// cuyo ángulo se cambia desde el visor, con el audio enlazado siguiendo
    /// los mismos cortes. La duración se limita al ángulo con menos material
    /// restante: el clip no puede pedir lo que la cámara más corta no tiene.
    func insertarClipMulticam(grupoID: UUID) {
        guard let grupo = montaje.gruposMulticam?.first(where: { $0.id == grupoID }),
              let inicial = grupo.mediaIDs.first else {
            status = "El grupo multicámara ya no existe"
            return
        }
        var duracion = Int64.max
        for id in grupo.mediaIDs {
            guard let item = media.first(where: { $0.id == id }) else { continue }
            let desfaseSegundos = timebase.segundos(grupo.desfases[id] ?? 0)
            let disponible = max(0, item.duration - desfaseSegundos)
            duracion = min(duracion, timebase.frames(segundos: disponible))
        }
        guard duracion > 0 else { status = "El grupo no tiene material suficiente"; return }

        let destino = max(0, cabezal)
        let enlace = UUID()
        let multicam = MulticamDeClip(grupoID: grupoID, inicial: inicial)
        performEdit(keepPosition: true) {
            var video = Clip(mediaID: inicial, nombre: grupo.nombre, inicio: destino, duracion: duracion, entradaEnOrigen: 0, enlace: enlace)
            video.multicam = multicam
            montaje.insertar(video, enPista: pistaDeTrabajo, en: destino)
            if let audio = pistaDeAudioParaEnlace() {
                var audioClip = Clip(mediaID: inicial, nombre: grupo.nombre, inicio: destino, duracion: duracion, entradaEnOrigen: 0, enlace: enlace)
                audioClip.multicam = multicam
                montaje.pistas[montaje.indiceDePista(audio)!].clips.append(audioClip)
                montaje.pistas[montaje.indiceDePista(audio)!].ordenar()
            }
            selectedClipID = video.id
        }
        status = "Clip multicámara «\(grupo.nombre)» insertado · cambia de ángulo en el visor"
    }

    /// Cambia el ángulo activo del clip multicámara desde el frame dado.
    /// Llamado desde el visor: durante la reproducción el frame es el cabezal
    /// en curso, así que el corte cae donde suena la acción.
    func cambiarAnguloMulticam(clipID: UUID, a mediaID: UUID, en frame: Int64) {
        guard let clip = montaje.clip(clipID), let pistaID = montaje.pistaDe(clip: clipID),
              let indice = montaje.indiceDePista(pistaID),
              let indiceClip = montaje.pistas[indice].clips.firstIndex(where: { $0.id == clipID }) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].clips[indiceClip].multicam?.cambiar(
                en: frame - clip.inicio, a: mediaID
            )
        }
        let nombre = media.first { $0.id == mediaID }?.name ?? "ángulo"
        status = "Ángulo «\(nombre)» desde \(timebase.timecode(frame))"
    }

    /// El visor multicámara del clip seleccionado, si lo es.
    func visorMulticam() -> (clip: Clip, grupo: GrupoMulticam)? {
        guard let clip = selectedClip, let multicam = clip.multicam,
              let grupo = montaje.gruposMulticam?.first(where: { $0.id == multicam.grupoID }) else {
            return nil
        }
        return (clip, grupo)
    }

    func anguloActivoDelVisor(_ visor: (clip: Clip, grupo: GrupoMulticam)) -> UUID? {
        guard let multicam = visor.clip.multicam else { return nil }
        return multicam.medioActivo(en: max(0, cabezal - visor.clip.inicio))
    }

    /// Convierte el clip multicámara en clips normales: el «flatten» de
    /// Premiere. Cada tramo de ángulo queda como clip independiente con su
    /// medio, su entrada en origen y su enlace de vídeo/audio. Después de
    /// esto el clip deja de ser multicámara, a propósito.
    func aplanarMulticam(clipID: UUID) {
        guard let clip = montaje.clip(clipID), clip.multicam != nil else {
            status = "El clip seleccionado no es multicámara"
            return
        }
        var creados: [Clip] = []
        performEdit(keepPosition: true) {
            creados = montaje.aplanarMulticam(clipID: clipID)
        }
        guard !creados.isEmpty else {
            status = "No se pudo aplanar: el clip multicámara no tiene tramos con material"
            return
        }
        selectedClipID = creados.first?.id
        status = "Multicámara aplanada · \(creados.count) tramos convertidos en clips normales"
    }

    /// Sincronía manual: corrige el desfase de un ángulo del grupo en frames
    /// (+1/−1, o ±10 con ⌥). Es el volante de sincronía del visor: cuando el
    /// onset de la forma de onda no basta, se ajusta a mano contra la señal.
    func ajustarDesfase(grupoID: UUID, medioID: UUID, delta: Int64) {
        var nuevo: Int64 = 0
        performEdit(keepPosition: true) {
            nuevo = montaje.ajustarDesfase(grupoID: grupoID, medioID: medioID, delta: delta)
        }
        let nombre = media.first { $0.id == medioID }?.name ?? "ángulo"
        status = "«\(nombre)» · desfase \(timebase.timecode(nuevo))"
        sincronizarAngulosDelVisor()
    }

    /// Recalcula la sincronización de un grupo desde el onset de la forma de
    /// onda, como al crearlo: el arreglo automático sigue disponible si el
    /// manual se fue por las ramas.
    func resincronizarGrupo(grupoID: UUID) {
        guard let indice = montaje.gruposMulticam?.firstIndex(where: { $0.id == grupoID }) else { return }
        let grupo = montaje.gruposMulticam?[indice] ?? GrupoMulticam(nombre: "", mediaIDs: [])
        var iniciosPorMedio: [UUID: Double] = [:]
        for id in grupo.mediaIDs {
            if let item = media.first(where: { $0.id == id }) {
                iniciosPorMedio[id] = inicioDeAudio(item)
            }
        }
        let referencia = iniciosPorMedio.values.max() ?? 0
        var nuevosDesfases: [UUID: Int64] = [:]
        for (id, inicio) in iniciosPorMedio {
            nuevosDesfases[id] = max(0, timebase.frames(segundos: referencia - inicio))
        }
        performEdit(keepPosition: true) {
            montaje.gruposMulticam?[indice].desfases = nuevosDesfases
        }
        status = "Grupo «\(grupo.nombre)» resincronizado por audio"
        sincronizarAngulosDelVisor()
    }

    // MARK: Clips anidados

    /// ¿Hay un nido a medio renderizar en este momento? Mientras lo haya, el
    /// botón de anidar no debe dejar crear otro sobre la misma selección.
    private(set) var anidando = false

    /// Anida los clips seleccionados en un solo clip: crea la línea de tiempo
    /// interior, la pre-renderiza a la caché (como un proxy) y sustituye la
    /// selección por un clip cuyo medio es ese render. El interior se guarda en
    /// el clip, así que desanidar devuelve los clips originales intactos.
    func crearNido() {
        guard !anidando else { status = "Ya hay un nido renderizándose"; return }
        let objetivos = seleccionados().compactMap { id -> (pista: UUID, clip: Clip)? in
            guard let clip = montaje.clip(id), !clip.esAjuste, !clip.esTitulo,
                  clip.multicam == nil else { return nil }
            guard let pista = montaje.pistaDe(clip: id) else { return nil }
            return (pista, clip)
        }
        guard objetivos.count >= 2 else {
            status = "Selecciona al menos dos clips del mismo montaje para anidar"
            return
        }
        // La selección tiene que estar contigua en el tiempo para que el nido
        // sea una pieza continua (como la secuencia dentro de la secuencia).
        let ordenados = objetivos.map(\.clip).sorted { $0.inicio < $1.inicio }
        let huecos = zip(ordenados, ordenados.dropFirst()).contains { $0.fin != $1.inicio }
        guard !huecos else {
            status = "Los clips seleccionados tienen que ser contiguos (sin huecos entre ellos)"
            return
        }
        guard let pista = objetivos.first?.pista, objetivos.allSatisfy({ $0.pista == pista }) else {
            status = "Anidar funciona sobre clips de una misma pista"
            return
        }

        // La línea de tiempo interior: los mismos clips, rebasados a cero.
        let arranque = ordenados.first?.inicio ?? 0
        var interior = LineaDeTiempo.nueva(timebase: timebase)
        interior.pistas = montaje.pistas.map { original in
            var copia = original
            copia.clips = original.clips.compactMap { clip in
                guard objetivos.contains(where: { $0.clip.id == clip.id }) else { return nil }
                var rebasado = clip
                rebasado.inicio -= arranque
                return rebasado
            }
            copia.ordenar()
            return copia
        }
        let duracionDelNido = ordenados.last?.fin ?? arranque
        let duracion = duracionDelNido - arranque
        guard duracion > 0 else { return }

        // El medio del nido: el archivo renderizado, resuelto como un medio más.
        let nidoID = UUID()
        let nombre = "Nido \(media.count + 1)"
        anidando = true
        status = "Renderizando nido…"

        Task {
            do {
                let url = try await ServicioDeNidos.renderizar(interior, medios: medios, id: nidoID)
                let medio = try await MedioResuelto.cargar(id: nidoID, url: url)
                let item = MediaItem(id: nidoID, url: url, duration: medio.duracion.seconds,
                                     size: medio.tamanoVisible, fileSize: 0, frameRate: medio.fps)
                let ids = Set(objetivos.map(\.clip.id))
                await MainActor.run {
                    media.append(item)
                    mediosOriginales[nidoID] = medio
                    medios[nidoID] = medio
                    prepararMiniatura(medio)
                    performEdit(keepPosition: true) {
                        // Quita los clips originales y deja el nido en su lugar.
                        for i in montaje.pistas.indices {
                            montaje.pistas[i].clips.removeAll { ids.contains($0.id) }
                        }
                        var nido = Clip(
                            mediaID: nidoID, nombre: nombre,
                            inicio: arranque, duracion: duracion, entradaEnOrigen: 0
                        )
                        nido.nido = interior
                        if let indice = montaje.indiceDePista(pista) {
                            montaje.pistas[indice].clips.append(nido)
                            montaje.pistas[indice].ordenar()
                        }
                        selectedClipID = nido.id
                        selectedClipIDs = [nido.id]
                    }
                    anidando = false
                    status = "Nido «\(nombre)» creado · \(duracion) frames · doble clic para desanidar"
                }
            } catch {
                await MainActor.run {
                    anidando = false
                    status = "No se pudo renderizar el nido: \(error.localizedDescription)"
                }
            }
        }
    }

    /// Desanida un clip: devuelve su línea de tiempo interior al montaje, en el
    /// sitio del nido. El nido se va y los clips originales vuelven intactos.
    func desanidar(clipID: UUID) {
        guard let nido = montaje.clip(clipID), let interior = nido.nido,
              let pista = montaje.pistaDe(clip: clipID),
              let indice = montaje.indiceDePista(pista) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].clips.removeAll { $0.id == clipID }
            for original in interior.pistas {
                let destino = montaje.pistas[indice].tipo == original.tipo ? indice : indice
                var clipsRebasados: [Clip] = []
                for clip in original.clips {
                    var rebasado = clip
                    rebasado.inicio += nido.inicio
                    clipsRebasados.append(rebasado)
                }
                montaje.pistas[destino].clips.append(contentsOf: clipsRebasados)
                montaje.pistas[destino].ordenar()
            }
        }
        status = "Nido desanidado · los clips originales vuelven a su sitio"
    }

    /// ¿El clip seleccionado es un nido (tiene línea de tiempo interior)?
    func esNido(_ clip: Clip) -> Bool { clip.nido != nil }

    /// Re-renderiza el interior de un nido tras editarlo. La app no abre el
    /// nido a editar todavía: el camino es desanidar, editar y volver a anidar.
    /// Este método queda como el puente para el día en que el interior se
    /// edite dentro del propio nido.
    func reRenderizarNido(clipID: UUID) async {
        guard let nido = montaje.clip(clipID), let interior = nido.nido else { return }
        anidando = true
        status = "Re-renderizando nido…"
        do {
            let url = try await ServicioDeNidos.renderizar(interior, medios: medios, id: nido.mediaID)
            let medio = try await MedioResuelto.cargar(id: nido.mediaID, url: url)
            await MainActor.run {
                mediosOriginales[nido.mediaID] = medio
                medios[nido.mediaID] = medio
                anidando = false
                rebuildPreview(keepPosition: true)
                status = "Nido re-renderizado"
            }
        } catch {
            await MainActor.run {
                anidando = false
                status = "No se pudo re-renderizar el nido: \(error.localizedDescription)"
            }
        }
    }

    // MARK: Títulos

    /// Inserta un clip de título en la pista de trabajo, en el cabezal.
    ///
    /// La duración por defecto es de cinco segundos —la carta de presentación
    /// típica— y el texto empieza vacío para que se edite en el inspector, como
    /// hace Premiere al arrastrar el «Legacy title» al timeline.
    func anadirTitulo() {
        let pista = pistaDeTrabajo
        let inicio = max(0, cabezal)
        let duracion = timebase.frames(segundos: 5)
        performEdit(keepPosition: true) {
            var titulo = Clip(mediaID: UUID(), nombre: "Título", inicio: inicio, duracion: duracion, entradaEnOrigen: 0)
            titulo.esTitulo = true
            titulo.titulo = TituloDeClip(texto: "Título", posicionY: 0.5)
            montaje.sobrescribir(titulo, enPista: pista, en: inicio)
            selectedClipID = titulo.id
            selectedClipIDs = [titulo.id]
        }
        status = "Título añadido · edítalo en el inspector"
    }

    /// Añade una imagen superpuesta (logotipo, PNG con transparencia) en el
    /// cabezal: un clip de título en forma de imagen que se quema sobre el
    /// vídeo con su relación de aspecto.
    func anadirImagen() {
        let panel = NSOpenPanel()
        panel.title = "Añadir imagen"
        panel.prompt = "Añadir"
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.image]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let pista = pistaDeTrabajo
        let inicio = max(0, cabezal)
        let duracion = timebase.frames(segundos: 5)
        performEdit(keepPosition: true) {
            var titulo = Clip(mediaID: UUID(), nombre: url.lastPathComponent, inicio: inicio, duracion: duracion, entradaEnOrigen: 0)
            titulo.esTitulo = true
            titulo.titulo = TituloDeClip(
                texto: "", posicionX: 0.5, posicionY: 0.5,
                forma: .imagen, ancho: 0.2, alto: 0.3,
                rutaDeImagen: url.path
            )
            montaje.sobrescribir(titulo, enPista: pista, en: inicio)
            selectedClipID = titulo.id
            selectedClipIDs = [titulo.id]
        }
        status = "Imagen añadida · tamaño y posición en el inspector"
    }

    /// Actualiza las propiedades del título del clip seleccionado.
    func fijarTitulo(_ id: UUID, _ mutacion: (inout TituloDeClip) -> Void) {
        guard let indice = montaje.indiceDeClip(id) else { return }
        performEdit(keepPosition: true) {
            guard var titulo = montaje.pistas[indice.0].clips[indice.1].titulo else { return }
            mutacion(&titulo)
            montaje.pistas[indice.0].clips[indice.1].titulo = titulo
        }
    }

    // MARK: Visor multiángulo en vivo

    /// El reproductor vivo del ángulo dado, creándolo si hace falta.
    ///
    /// Se usa el archivo real del medio (o el proxy si los proxies están
    /// activos): el visor enseña lo que cada cámara está grabando en este
    /// instante de grupo, mudo para que solo suene la composición.
    func reproductorDelAngulo(_ medioID: UUID) -> AVPlayer? {
        if let existente = reproductoresDeAngulos[medioID] { return existente }
        guard let item = media.first(where: { $0.id == medioID }) else { return nil }
        let url = proxiesActivos ? (proxyURLs[medioID] ?? item.url) : item.url
        let reproductor = AVPlayer(playerItem: AVPlayerItem(asset: AVURLAsset(url: url)))
        reproductor.isMuted = true
        reproductoresDeAngulos[medioID] = reproductor
        return reproductor
    }

    /// Prepara el visor para el clip multicámara dado: crea los reproductores
    /// de sus ángulos y los lleva al instante de grupo del cabezal.
    func prepararVisorMultiAngulo(_ visor: (clip: Clip, grupo: GrupoMulticam)) {
        let clipInicio = visor.clip.inicio
        for id in visor.grupo.mediaIDs {
            guard let reproductor = reproductorDelAngulo(id) else { continue }
            let tiempoDeGrupo = max(clipInicio, cabezal)
            let posicion = visor.grupo.posicionDeAngulo(id, enTiempoDeGrupo: tiempoDeGrupo)
            reproductor.seek(
                to: CMTime(seconds: timebase.segundos(posicion), preferredTimescale: 600),
                toleranceBefore: .zero, toleranceAfter: .zero
            )
            if isPlaying { reproductor.play(); reproductor.rate = playbackRate }
        }
        arrancarTemporizadorDeAngulos()
    }

    /// Lleva cada ángulo al instante de grupo del cabezal. Se llama al buscar
    /// en el programa y tras cada edición de sincronía.
    func sincronizarAngulosDelVisor() {
        guard let visor = visorMulticam() else { return }
        let tiempoDeGrupo = max(visor.clip.inicio, cabezal)
        for (id, reproductor) in reproductoresDeAngulos {
            let posicion = visor.grupo.posicionDeAngulo(id, enTiempoDeGrupo: tiempoDeGrupo)
            reproductor.seek(
                to: CMTime(seconds: timebase.segundos(posicion), preferredTimescale: 600),
                toleranceBefore: .zero, toleranceAfter: .zero
            )
        }
    }

    /// Arranca el re-sincronizado periódico del visor: los relojes de
    /// AVFoundation son independientes y sin corrección las cámaras se
    /// descuelgan del cabezal en cuestión de segundos.
    private func arrancarTemporizadorDeAngulos() {
        temporizadorDeAngulos?.invalidate()
        let objetivo = generacionDelVisor
        temporizadorDeAngulos = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                guard self.generacionDelVisor == objetivo, self.isPlaying else { return }
                self.recorregirAngulosDelVisor()
            }
        }
    }

    private func recorregirAngulosDelVisor() {
        guard let visor = visorMulticam() else { return }
        let cabezalActual = self.cabezal
        let tiempoDeGrupo = max(visor.clip.inicio, cabezalActual)
        let tolerancia = timebase.tiempo(2) // dos frames: la re-corrección no debe ser visible
        for (id, reproductor) in reproductoresDeAngulos {
            let posicion = visor.grupo.posicionDeAngulo(id, enTiempoDeGrupo: tiempoDeGrupo)
            let esperada = CMTime(seconds: timebase.segundos(posicion), preferredTimescale: 600)
            let actual = reproductor.currentTime()
            let deriva = CMTime(seconds: max(0, abs(CMTimeGetSeconds(actual) - CMTimeGetSeconds(esperada))), preferredTimescale: 600)
            if CMTimeCompare(deriva, tolerancia) > 0 {
                reproductor.seek(to: esperada, toleranceBefore: .zero, toleranceAfter: .zero)
            }
        }
    }

    /// Pone en reproducción o pausa los ángulos del visor con el programa.
    func reproducirAngulosDelVisor(rate: Float) {
        for reproductor in reproductoresDeAngulos.values {
            if rate == 0 { reproductor.pause() } else { reproductor.play(); reproductor.rate = rate }
        }
    }

    /// Libera los reproductores del visor al dejar de verlo.
    func liberarVisorMultiAngulo() {
        temporizadorDeAngulos?.invalidate()
        temporizadorDeAngulos = nil
        generacionDelVisor += 1
        for reproductor in reproductoresDeAngulos.values { reproductor.pause() }
        reproductoresDeAngulos.removeAll()
    }

    private func inicioDeAudio(_ item: MediaItem) -> Double {
        guard let muestras = formasDeOnda[item.id], let indice = muestras.firstIndex(where: { $0 > 0.08 }), !muestras.isEmpty else {
            return 0
        }
        return Double(indice) / Double(muestras.count) * item.duration
    }

    func miniatura(for item: MediaItem) -> CGImage? { miniaturas[item.id] }

    func miniatura(for clip: Clip) -> CGImage? {
        guard let item = mediaItem(for: clip) else { return nil }
        return miniaturas[item.id]
    }

    func formaDeOnda(for clip: Clip) -> [Float]? { formasDeOnda[clip.mediaID] }

    private func prepararMiniatura(_ medio: MedioResuelto) {
        Task { [weak self] in
            guard let imagen = await MedioResuelto.miniatura(medio) else { return }
            guard let self else { return }
            self.miniaturas[medio.id] = imagen
        }
    }

    private func prepararFormaDeOnda(_ medio: MedioResuelto) {
        let url = medio.url
        let id = medio.id
        Task { [weak self] in
            let forma = await Task.detached(priority: .utility) {
                await MedioResuelto.formaDeOnda(url: url)
            }.value
            guard let forma else { return }
            self?.formasDeOnda[id] = forma
        }
    }

    /// Duración del medio en frames de proyecto. El recorte la necesita para no
    /// dejar alargar un clip más allá del final del archivo.
    func duracionDelMedio(_ id: UUID) -> Int64 {
        guard let item = media.first(where: { $0.id == id }) else { return 0 }
        return timebase.frames(segundos: item.duration)
    }

    func duracionDelMedio(deClip clip: Clip) -> Int64 { duracionDelMedio(clip.mediaID) }

    /// Carga el medio seleccionado en el monitor de origen. El programa sigue
    /// usando la composición del montaje; ambos reproductores son independientes.
    func cargarMedioEnOrigen(_ id: UUID?) {
        guard let id, let medio = medios[id], media.contains(where: { $0.id == id }) else {
            sourceMediaID = nil
            sourcePlayer.pause()
            sourcePlayer.replaceCurrentItem(with: nil)
            cabezalDeOrigen = 0
            isPlayingOrigen = false
            return
        }

        let limite = duracionDeOrigenEnFrames
        let posicion = min(max(entradaDeOrigen[id] ?? 0, 0), limite)
        sourcePlayer.pause()
        sourceMediaID = id
        sourcePlayer.replaceCurrentItem(with: AVPlayerItem(asset: medio.asset))
        cabezalDeOrigen = posicion
        sourcePlayer.seek(
            to: timebase.tiempo(posicion),
            toleranceBefore: .zero,
            toleranceAfter: .zero
        )
        isPlayingOrigen = false
    }

    func seekOrigen(toFrame frame: Int64) {
        let destino = max(0, min(frame, duracionDeOrigenEnFrames))
        cabezalDeOrigen = destino
        sourcePlayer.seek(
            to: timebase.tiempo(destino),
            toleranceBefore: .zero,
            toleranceAfter: .zero
        )
    }

    func stepFrameOrigen(_ direction: Int) {
        seekOrigen(toFrame: cabezalDeOrigen + Int64(direction))
    }

    func togglePlaybackOrigen() {
        guard sourcePlayer.currentItem != nil else { return }
        if sourcePlayer.rate == 0 {
            if cabezalDeOrigen >= duracionDeOrigenEnFrames { seekOrigen(toFrame: 0) }
            sourcePlayer.play()
        } else {
            sourcePlayer.pause()
        }
        isPlayingOrigen = sourcePlayer.rate != 0
    }

    func importMedia() {
        let panel = NSOpenPanel()
        panel.title = "Importar medios"
        panel.prompt = "Importar"
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.audiovisualContent]
        guard panel.runModal() == .OK else { return }

        importURLs(panel.urls)
    }

    func importarSubtitulos() {
        let panel = NSOpenPanel()
        panel.title = "Importar subtítulos"
        panel.prompt = "Importar"
        panel.allowedContentTypes = [UTType(filenameExtension: "srt") ?? .text]
        guard panel.runModal() == .OK, let url = panel.url,
              let texto = try? String(contentsOf: url, encoding: .utf8) else { return }
        let cues = SubtitulosService.leerSRT(texto, timebase: timebase)
        guard !cues.isEmpty else { status = "El SRT no contiene subtítulos válidos"; return }
        performEdit(keepPosition: true) { montaje.subtitulos = cues }
        status = "Importados \(cues.count) subtítulos"
    }

    func exportarSubtitulos() {
        guard let subtitulos = montaje.subtitulos, !subtitulos.isEmpty else {
            status = "No hay subtítulos que exportar"
            return
        }
        let panel = NSSavePanel()
        panel.title = "Exportar subtítulos"
        panel.prompt = "Exportar"
        panel.nameFieldStringValue = "\(projectName).srt"
        panel.allowedContentTypes = [UTType(filenameExtension: "srt") ?? .text]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try SubtitulosService.escribirSRT(subtitulos, timebase: timebase).write(to: url, atomically: true, encoding: .utf8)
            status = "Subtítulos exportados"
        } catch {
            status = "No se pudieron exportar los subtítulos"
        }
    }

    func transcribirMedioSeleccionado() {
        guard let medio = selectedMedia else { return }
        encolarTranscripcion(medio.id)
    }

    /// Añade un medio a la cola de transcripción y arranca la rueda si está parada.
    ///
    /// La cola es serial: el reconocimiento on-device es caro, y lanzar diez a la
    /// vez deja el Mac inservible. Cada trabajo es cancelable por separado —el
    /// camino de cancelación real de la IA, con `withTaskCancellationHandler` en
    /// el servicio— y cancelar la cola entera corta el trabajo en curso y vacía
    /// lo que quedaba.
    func encolarTranscripcion(_ mediaID: UUID) {
        guard let medio = media.first(where: { $0.id == mediaID }) else { return }
        guard montaje.transcripcion(de: mediaID) == nil else {
            status = "«\(medio.name)» ya está transcrito"
            return
        }
        guard !colaDeTranscripcion.contains(mediaID) else { return }
        colaDeTranscripcion.append(mediaID)
        status = "«\(medio.name)» en cola para transcribir"
        procesarSiguienteTranscripcion()
    }

    /// La rueda de la cola: transcribe el primer medio pendiente, y al terminar
    /// pasa al siguiente hasta vaciar la cola. Si un trabajo falla, no para la
    /// cola: se registra y se sigue.
    private func procesarSiguienteTranscripcion() {
        guard !transcribing, let siguiente = colaDeTranscripcion.first else {
            if colaDeTranscripcion.isEmpty { transcribing = false }
            return
        }
        colaDeTranscripcion.removeFirst()
        guard let medio = media.first(where: { $0.id == siguiente }) else {
            procesarSiguienteTranscripcion()
            return
        }
        transcribing = true
        status = "Transcribiendo «\(medio.name)» en este Mac…"
        tareaDeTranscripcion = Task { [weak self] in
            guard let self else { return }
            do {
                let salida = try await SubtitulosService.transcribirCompleto(
                    url: medio.url,
                    mediaID: medio.id,
                    timebase: self.timebase
                )
                guard !Task.isCancelled else { throw CancellationError() }
                guard !salida.cues.isEmpty else { throw SubtitulosError.noDisponible }
                self.performEdit(keepPosition: true) {
                    self.montaje.subtitulos = salida.cues
                    var todas = self.montaje.transcripciones ?? []
                    todas.removeAll { $0.mediaID == medio.id }
                    todas.append(salida.transcripcion)
                    self.montaje.transcripciones = todas
                }
                self.status = "Transcripción de «\(medio.name)» lista · \(salida.cues.count) subtítulos · \(salida.transcripcion.palabras.count) palabras"
            } catch is CancellationError {
                self.status = "Transcripción de «\(medio.name)» cancelada"
            } catch {
                self.status = "No se pudo transcribir «\(medio.name)»: \(error.localizedDescription)"
            }
            self.transcribing = false
            self.tareaDeTranscripcion = nil
            self.procesarSiguienteTranscripcion()
        }
    }

    /// Cancela la cola de transcripción: el trabajo en curso se corta y lo que
    /// quedaba en espera se vacía.
    func cancelarColaDeTranscripcion() {
        tareaDeTranscripcion?.cancel()
        tareaDeTranscripcion = nil
        colaDeTranscripcion.removeAll()
        transcribing = false
        status = "Cola de transcripción cancelada"
    }

    /// Transcribe todos los medios de la biblioteca que aún no lo estén.
    ///
    /// Uno detrás de otro y no todos a la vez: el reconocimiento on-device es caro y
    /// lanzar diez a la vez deja el Mac inservible mientras se monta.
    func transcribirLoQueFalte() {
        let pendientes = media.filter { montaje.transcripcion(de: $0.id) == nil }
        guard !pendientes.isEmpty else { status = "Todos los medios están transcritos"; return }
        for medio in pendientes where !colaDeTranscripcion.contains(medio.id) {
            colaDeTranscripcion.append(medio.id)
        }
        status = "\(pendientes.count) medio\(pendientes.count == 1 ? "" : "s") en cola para transcribir"
        procesarSiguienteTranscripcion()
    }

    // MARK: Edición por transcript

    /// Lo que se oye en el montaje, palabra a palabra y en orden.
    var palabrasDelMontaje: [PalabraDelMontaje] { montaje.palabrasDelMontaje() }

    /// Índice de la palabra que suena en el cabezal, para seguir la lectura.
    func palabraEnElCabezal(_ palabras: [PalabraDelMontaje]) -> Int? {
        let frame = cabezal
        return palabras.firstIndex { frame >= $0.desde && frame < $0.hasta }
    }

    /// Lleva el cabezal al principio de una palabra.
    func irAPalabra(_ palabra: PalabraDelMontaje) {
        seek(toFrame: palabra.desde)
    }

    /// Borra del montaje las palabras seleccionadas en el panel de texto.
    ///
    /// Una sola entrada de deshacer para toda la selección: quitar veinte muletillas y
    /// tener que pulsar ⌘Z veinte veces sería peor que no tener el botón.
    func borrarPalabras(_ indices: Set<Int>) {
        let palabras = palabrasDelMontaje
        let rangos = TranscriptService.rangos(de: palabras, indices: Array(indices))
        guard !rangos.isEmpty else { return }
        let borrados = rangos.reduce(Int64(0)) { $0 + ($1.hasta - $1.desde) }

        performEdit(keepPosition: true) { montaje.extraerRangos(rangos) }
        seleccionDeTexto = []
        status = "Quitado \(indices.count == 1 ? "1 tramo" : "\(rangos.count) tramos") · \(timebase.timecode(borrados)) menos"
    }

    /// Resultados de buscar por lo que se dice, en todo lo transcrito.
    func buscarEnLoQueSeDice(_ consulta: String) -> [Hallazgo] {
        montaje.buscarEnLoQueSeDice(consulta)
    }

    /// Lleva la vista a donde se dijo eso.
    ///
    /// Si el material está montado manda el monitor de programa, porque es lo que se está
    /// mirando; si solo está en la biblioteca se abre en el de origen, que es donde se
    /// decide si entra o no.
    func irAHallazgo(_ hallazgo: Hallazgo) {
        if let frame = hallazgo.frame {
            monitorActual = .programa
            seek(toFrame: frame)
            return
        }
        selectedMediaID = hallazgo.mediaID
        selectedMediaIDs = [hallazgo.mediaID]
        monitorActual = .origen
        seekOrigen(toFrame: timebase.frames(segundos: hallazgo.segundoEnElMedio))
    }

    // MARK: Silencios

    /// Ejecuta una operación pesada (lectura completa de audio) fuera del hilo de
    /// la UI. El resultado vuelve al actor con `await`.
    private func enSegundoPlano<Resultado: Sendable>(
        _ operacion: @escaping @Sendable () throws -> Resultado
    ) async throws -> Resultado {
        try await Task.detached(priority: .userInitiated) { try operacion() }.value
    }

    /// Quita los silencios del montaje y cierra los huecos.
    ///
    /// Todo en **una sola entrada de deshacer**, y con un marcador en cada corte: cortar
    /// cuarenta veces y no poder revisar dónde sería un rough cut que nadie se atreve a
    /// usar. El umbral es relativo a la sonoridad del propio material, así que funciona
    /// igual en una entrevista susurrada y en un pódcast comprimido.
    func quitarSilencios() {
        guard !midiendoSilencios else { return }
        guard montaje.duracion > 0 else { status = "No hay montaje que recortar"; return }

        midiendoSilencios = true
        status = "Midiendo el audio para encontrar los silencios…"
        Task {
            do {
                let render = ConstructorDeMontaje.construir(
                    montaje,
                    medios: mediosOriginales.isEmpty ? medios : mediosOriginales,
                    tamanoDeSalida: CGSize(width: 1920, height: 1080)
                )
                // La lectura del audio entero puede durar minutos; va a una tarea
                // desacoplada del actor para que la UI no se congele.
                let composicion = render.composicion
                let mezcla = render.mezclaDeAudio
                let salida = try await enSegundoPlano {
                    try SonoridadMedia.medirConCurva(composicion: composicion, mezcla: mezcla)
                }
                let tramos = DetectorDeSilencios.silencios(
                    curva: salida.curva,
                    pasoEnSegundos: MedidorDeSonoridad.pasoDeLaCurva,
                    integrada: salida.medida.integrada
                )
                await MainActor.run { aplicarSilencios(tramos) }
            } catch {
                status = "No se pudo medir el audio: \(error.localizedDescription)"
            }
            midiendoSilencios = false
        }
    }

    private func aplicarSilencios(_ tramos: [TramoDeSilencio]) {
        guard !tramos.isEmpty else { status = "No hay silencios que quitar"; return }

        let rangos = tramos.map { tramo in
            (desde: timebase.frames(segundos: tramo.desde), hasta: timebase.frames(segundos: tramo.hasta))
        }.filter { $0.hasta > $0.desde }
        guard !rangos.isEmpty else { status = "Los silencios son más cortos que un frame"; return }

        let quitado = rangos.reduce(Int64(0)) { $0 + ($1.hasta - $1.desde) }

        performEdit(keepPosition: true) {
            montaje.extraerRangos(rangos)
            // Un marcador donde quedó cada costura, para poder repasarlas. Se ponen
            // después de cortar, así que hay que descontar lo ya quitado por delante.
            var acumulado: Int64 = 0
            var nuevos: [Marcador] = []
            for rango in rangos.sorted(by: { $0.desde < $1.desde }) {
                nuevos.append(Marcador(
                    frame: rango.desde - acumulado,
                    nombre: "Silencio quitado",
                    etiqueta: .amarillo
                ))
                acumulado += rango.hasta - rango.desde
            }
            montaje.marcadores.append(contentsOf: nuevos)
        }
        status = "Quitados \(rangos.count) silencios · \(timebase.timecode(quitado)) menos · ⌘Z lo deshace de una vez"
    }

    /// Índices de las muletillas que hay ahora mismo en el montaje.
    func muletillasDelMontaje() -> [Int] { TranscriptService.muletillas(en: palabrasDelMontaje) }

    /// Selecciona las muletillas para que se vean antes de borrarlas.
    ///
    /// No se aplica directamente a propósito: una propuesta automática que borra sola es
    /// la forma más rápida de que alguien deje de usar el botón. Se marca, se mira y se
    /// confirma.
    func proponerMuletillas() {
        let encontradas = muletillasDelMontaje()
        seleccionDeTexto = Set(encontradas)
        status = encontradas.isEmpty
            ? "No hay muletillas que quitar"
            : "\(encontradas.count) muletillas marcadas · revisa y pulsa Quitar"
    }

    func editarSubtitulo(_ id: UUID, texto: String) {
        guard let indice = montaje.subtitulos?.firstIndex(where: { $0.id == id }) else { return }
        montaje.subtitulos?[indice].texto = texto
        objectWillChange.send()
        scheduleAutosave()
    }

    /// Aplica un estilo a un subtítulo. El «default» es el que está por omisión.
    func aplicarEstiloDeSubtitulo(_ id: UUID, estilo: String) {
        guard let indice = montaje.subtitulos?.firstIndex(where: { $0.id == id }) else { return }
        montaje.subtitulos?[indice].estilo = estilo
        objectWillChange.send()
        scheduleAutosave()
    }

    /// Añade o reemplaza un estilo de subtítulo en el proyecto.
    func guardarEstiloDeSubtitulo(_ estilo: EstiloDeSubtitulo) {
        if let indice = montaje.estilosDeSubtitulo.firstIndex(where: { $0.nombre == estilo.nombre }) {
            montaje.estilosDeSubtitulo[indice] = estilo
        } else {
            montaje.estilosDeSubtitulo.append(estilo)
        }
        objectWillChange.send()
        scheduleAutosave()
    }

    func borrarEstiloDeSubtitulo(_ nombre: String) {
        guard nombre != "default" else { return }
        montaje.estilosDeSubtitulo.removeAll { $0.nombre == nombre }
        // Los subtítulos que lo usaban vuelven al estilo por omisión.
        if let subtitulos = montaje.subtitulos {
            montaje.subtitulos = subtitulos.map { s in
                s.estilo == nombre ? Subtitulo(id: s.id, inicio: s.inicio, fin: s.fin, texto: s.texto) : s
            }
        }
        objectWillChange.send()
        scheduleAutosave()
    }

    func importURLs(_ urls: [URL]) {
        guard !isImporting else { status = "Espera a que termine la importación actual"; return }
        let unique = urls.filter { candidate in
            !media.contains(where: { $0.url.standardizedFileURL == candidate.standardizedFileURL })
        }
        guard !unique.isEmpty else { status = "Esos medios ya están en la biblioteca"; return }

        Task {
            isImporting = true
            importProgress = 0
            var loaded: [MediaItem] = []
            var resueltos: [UUID: MedioResuelto] = [:]
            var failures: [String] = []
            for (index, url) in unique.enumerated() {
                status = "Analizando \(url.lastPathComponent)…"
                let accessing = url.startAccessingSecurityScopedResource()
                defer { if accessing { url.stopAccessingSecurityScopedResource() } }
                let asset = AVURLAsset(url: url)
                do {
                    let playable = try await asset.load(.isPlayable)
                    let protected = try await asset.load(.hasProtectedContent)
                    guard playable, !protected else { throw MediaImportError.notPlayable }
                    let duration = try await asset.load(.duration).seconds
                    guard duration.isFinite, duration > 0 else { throw MediaImportError.invalidDuration }
                    let videoTracks = try await asset.loadTracks(withMediaType: .video)
                    let audioTracks = try await asset.loadTracks(withMediaType: .audio)
                    guard !videoTracks.isEmpty || !audioTracks.isEmpty else { throw MediaImportError.noTracks }
                    let size: CGSize
                    let frameRate: Double
                    let variableFrameRate: Bool
                    if let track = videoTracks.first {
                        guard try await track.load(.isDecodable) else { throw MediaImportError.videoNotDecodable }
                        let naturalSize = try await track.load(.naturalSize)
                        let transform = try await track.load(.preferredTransform)
                        let transformed = naturalSize.applying(transform)
                        size = CGSize(width: abs(transformed.width), height: abs(transformed.height))
                        frameRate = Double(try await track.load(.nominalFrameRate))
                        variableFrameRate = await MedioResuelto.esCadenciaVFR(pista: track)
                    } else {
                        size = .zero
                        frameRate = 0
                        variableFrameRate = false
                    }
                    let values = try url.resourceValues(forKeys: [.fileSizeKey, .totalFileAllocatedSizeKey])
                    let bytes = Int64(values.fileSize ?? values.totalFileAllocatedSize ?? 0)
                    let item = MediaItem(url: url, duration: duration, size: size, fileSize: bytes, frameRate: frameRate, variableFrameRate: variableFrameRate)
                    // Se guarda el medio ya resuelto: montar la vista previa deja de
                    // necesitar E/S y pasa a ser aritmética sobre estructuras.
                     resueltos[item.id] = MedioResuelto(
                        id: item.id, url: url, asset: asset,
                        pistaDeVideo: videoTracks.first, pistaDeAudio: audioTracks.first,
                        duracion: try await asset.load(.duration),
                        tamanoNatural: videoTracks.first == nil ? .zero : try await videoTracks[0].load(.naturalSize),
                        transformacionPreferida: videoTracks.first == nil ? .identity : try await videoTracks[0].load(.preferredTransform),
                        fps: frameRate, esVFR: variableFrameRate
                    )
                    if let medio = resueltos[item.id] {
                        prepararMiniatura(medio)
                        prepararFormaDeOnda(medio)
                    }
                    loaded.append(item)
                } catch {
                    failures.append("\(url.lastPathComponent): \(error.localizedDescription)")
                }
                importProgress = Double(index + 1) / Double(unique.count)
            }
            if !loaded.isEmpty {
                mediosOriginales.merge(resueltos) { _, nuevo in nuevo }
                medios.merge(resueltos) { _, nuevo in nuevo }
                performEdit {
                    media.append(contentsOf: loaded)
                    selectedMediaID = loaded.last?.id
                    selectedMediaIDs = loaded.last.map { Set<UUID>([$0.id]) } ?? Set<UUID>()
                    // El primer medio con imagen fija la cadencia del proyecto, como
                    // hace cualquier editor al crear una secuencia arrastrando un
                    // clip: es la pregunta de configuración que nadie sabe contestar
                    // antes de haber visto el material.
                    if montaje.duracion == 0, let primero = loaded.first(where: { $0.frameRate > 0 }) {
                        montaje.timebase = Self.timebaseMasCercana(a: primero.frameRate)
                    }
                    if montaje.duracion == 0, let first = loaded.first {
                        let pista = first.size == .zero
                            ? montaje.pistas.first { $0.tipo == .audio }!.id
                            : montaje.pistas.first { $0.tipo == .video }!.id
                        var clip = clipDe(first)
                        clip.duracion = max(1, montaje.timebase.frames(segundos: first.duration))
                        montaje.sobrescribir(clip, enPista: pista, en: 0)
                        selectedClipID = clip.id
                        pistaActiva = pista
                    }
                }
                cargarMedioEnOrigen(loaded.last?.id)
                let imported = loaded.count
                let vfr = loaded.filter(\.variableFrameRate).count
                status = "\(imported) archivo\(imported == 1 ? "" : "s") importado\(imported == 1 ? "" : "s")"
                    + (vfr > 0 ? " · \(vfr) VFR detectado\(vfr == 1 ? "" : "s")" : "")
            }
            if !failures.isEmpty {
                let alert = NSAlert()
                alert.messageText = failures.count == 1 ? "No se pudo importar un archivo" : "No se pudieron importar \(failures.count) archivos"
                alert.informativeText = failures.joined(separator: "\n\n")
                alert.alertStyle = .warning
                alert.addButton(withTitle: "Entendido")
                alert.runModal()
                if loaded.isEmpty { status = "Importación fallida · revisa formato y codec" }
            }
            isImporting = false
            // Transcripción al importar: con la preferencia activa, los medios
            // nuevos entran en la cola cancelable, uno detrás de otro.
            if transcribirAlImportar {
                let porTranscribir = loaded.filter {
                    montaje.transcripcion(de: $0.id) == nil && !$0.size.equalTo(.zero)
                }
                for medio in porTranscribir {
                    colaDeTranscripcion.append(medio.id)
                }
                if !porTranscribir.isEmpty {
                    status = "\(porTranscribir.count) medio\(porTranscribir.count == 1 ? "" : "s") en cola para transcribir"
                    procesarSiguienteTranscripcion()
                }
            }
        }
    }

    // MARK: - Proyecto

    func saveProject() {
        let panel = NSSavePanel()
        panel.title = "Guardar proyecto Editorcito"
        panel.prompt = "Guardar"
        panel.nameFieldStringValue = "\(projectName == "Sin título" ? "Mi montaje" : projectName).editorcito"
        panel.allowedContentTypes = [UTType(filenameExtension: "editorcito") ?? .json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            let backup = url.appendingPathExtension("bak")
            if FileManager.default.fileExists(atPath: url.path) {
                try? FileManager.default.removeItem(at: backup)
                try FileManager.default.copyItem(at: url, to: backup)
            }
            try encoder.encode(projectFile(carpeta: url.deletingLastPathComponent()))
                .write(to: url, options: .atomic)
            projectName = url.deletingPathExtension().lastPathComponent
            carpetaDelProyecto = url.deletingLastPathComponent()
            isDirty = false
            status = "Proyecto guardado"
            try? FileManager.default.removeItem(at: autosaveURL)
        } catch {
            status = "No se pudo guardar: \(error.localizedDescription)"
        }
    }

    /// Guarda una instantánea del proyecto en la carpeta «Versiones» junto al
    /// archivo actual: el «guardar como versión» de las suites grandes.    ///
    /// El nombre lleva fecha y hora para no pisar nada: `Nombre_2026-08-08_163045`.
    /// No toca el archivo de trabajo —editar, versión, seguir editando— y el
    /// autosave sigue siendo el de siempre.
    func guardarVersion() {
        guard let carpeta = carpetaDelProyecto else {
            status = "Guarda el proyecto una vez antes de crear versiones"
            return
        }
        let fecha = DateFormatter()
        fecha.dateFormat = "yyyy-MM-dd_HHmmss"
        let base = carpeta.appendingPathComponent("Versiones", isDirectory: true)
        let raiz = "\(projectName)_\(fecha.string(from: Date()))"
        do {
            try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            var nombre = "\(raiz).editorcito"
            var indice = 2
            while FileManager.default.fileExists(atPath: base.appendingPathComponent(nombre).path) {
                nombre = "\(raiz)_\(indice).editorcito"
                indice += 1
            }
            try encoder.encode(projectFile(carpeta: carpeta))
                .write(to: base.appendingPathComponent(nombre), options: .atomic)
            status = "Versión guardada en Versiones/\(nombre)"
        } catch {
            status = "No se pudo guardar la versión: \(error.localizedDescription)"
        }
    }

    func openProject() {
        let panel = NSOpenPanel()
        panel.title = "Abrir proyecto Editorcito"
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "editorcito") ?? .json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await cargarProyecto(desde: url) }
    }

    /// Exporta el montaje como EDL CMX3600: el formato de listas de decisión
    /// que importan Premiere, Resolve y las mesas de edición. Es el puente de
    /// salida para terminar el montaje en otro lado.
    func exportarEDL() {
        guard montaje.duracion > 0 else { status = "No hay montaje que exportar"; return }
        let panel = NSSavePanel()
        panel.title = "Exportar EDL"
        panel.prompt = "Exportar"
        panel.nameFieldStringValue = "\(projectName).edl"
        panel.allowedContentTypes = [UTType(filenameExtension: "edl") ?? .plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let edl = EDLDeEditorcito.exportar(montaje: montaje, medios: mediosParaExportar(), titulo: projectName)
        do {
            try edl.write(to: url, atomically: true, encoding: .utf8)
            status = "EDL exportado · \(url.lastPathComponent) — léelo en Premiere o Resolve"
        } catch {
            status = "No se pudo exportar el EDL: \(error.localizedDescription)"
        }
    }

    /// Exporta el montaje como FCPXML 1.11: el intercambio moderno con Final
    /// Cut Pro y Premiere Pro.
    func exportarFCPXML() {
        guard montaje.duracion > 0 else { status = "No hay montaje que exportar"; return }
        let panel = NSSavePanel()
        panel.title = "Exportar FCPXML"
        panel.prompt = "Exportar"
        panel.nameFieldStringValue = "\(projectName).fcpxml"
        panel.allowedContentTypes = [UTType(filenameExtension: "fcpxml") ?? .xml]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let datos = FCPXMLDeEditorcito.exportar(montaje: montaje, medios: mediosParaExportar(), titulo: projectName)
        do {
            try datos.write(to: url)
            status = "FCPXML exportado · \(url.lastPathComponent) — abrible en Final Cut o Premiere"
        } catch {
            status = "No se pudo exportar el FCPXML: \(error.localizedDescription)"
        }
    }

    /// Los medios del montaje, en la forma mínima que piden los exportadores.
    private func mediosParaExportar() -> [UUID: MedioParaExportar] {
        let ids = Set(montaje.todosLosClips.map(\.clip.mediaID))
        var resultado: [UUID: MedioParaExportar] = [:]
        for item in media where ids.contains(item.id) {
            resultado[item.id] = MedioParaExportar(
                nombre: item.name,
                url: item.url,
                duracionSegundos: item.duration,
                tamano: item.size,
                fps: item.frameRate
            )
        }
        return resultado
    }

    /// Importa otro proyecto: añade sus medios a la biblioteca y pega su
    /// montaje al final del actual, pista a pista por tipo.
    ///
    /// Es el «combinar proyectos» de las suites grandes: dos entrevistas
    /// montadas por separado se unen en una sola línea de tiempo sin copiar
    /// archivos. Los medios que ya están (mismo archivo) no se duplican; sus
    /// clips se remapean al medio existente.
    func importarOtroProyecto() {
        let panel = NSOpenPanel()
        panel.title = "Importar proyecto Editorcito"
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "editorcito") ?? .json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard let proyecto = try? ProyectoEditorcito.leer(Data(contentsOf: url)) else {
            status = "No se pudo leer «\(url.lastPathComponent)»"
            return
        }
        Task {
            let carpeta = url.deletingLastPathComponent()
            var cargados: [MediaItem] = []
            var resueltos: [UUID: MedioResuelto] = [:]
            var reasignados: [UUID: UUID] = [:]
            var perdidos: [String] = []

            for guardado in proyecto.medios {
                // Un archivo que ya está en la biblioteca no se importa dos
                // veces: los clips del proyecto importado apuntan al existente.
                if let existente = media.first(where: { $0.url.standardizedFileURL == {
                    (proyecto.localizar(guardado, carpetaDelProyecto: carpeta) ?? URL(fileURLWithPath: guardado.ruta)).standardizedFileURL
                }() }) {
                    reasignados[guardado.id] = existente.id
                    continue
                }
                guard let archivo = proyecto.localizar(guardado, carpetaDelProyecto: carpeta) else {
                    cargados.append(
                        MediaItem(
                            id: guardado.id,
                            url: URL(fileURLWithPath: guardado.ruta),
                            duration: guardado.duracion,
                            size: CGSize(width: guardado.ancho, height: guardado.alto),
                            fileSize: guardado.bytes ?? 0,
                            frameRate: guardado.fps ?? 0,
                            variableFrameRate: guardado.vfr ?? false,
                            bin: guardado.bin ?? "Todos",
                            subclipDe: guardado.subclip
                        )
                    )
                    perdidos.append(guardado.nombre)
                    continue
                }
                let item = MediaItem(
                    id: guardado.id, url: archivo, duration: guardado.duracion,
                    size: CGSize(width: guardado.ancho, height: guardado.alto),
                    fileSize: guardado.bytes ?? 0, frameRate: guardado.fps ?? 0,
                    variableFrameRate: guardado.vfr ?? false,
                    bin: guardado.bin ?? "Todos",
                    subclipDe: guardado.subclip
                )
                cargados.append(item)
                if let medio = try? await MedioResuelto.cargar(id: guardado.id, url: archivo) {
                    resueltos[guardado.id] = medio
                    prepararMiniatura(medio)
                    prepararFormaDeOnda(medio)
                }
            }

            let offset = montaje.duracion
            await MainActor.run {
                media.append(contentsOf: cargados)
                mediosOriginales.merge(resueltos) { _, nuevo in nuevo }
                medios.merge(resueltos) { _, nuevo in nuevo }
                ultimoRender = nil
                performEdit(keepPosition: true) {
                    for pistaImportada in proyecto.montaje.pistas {
                        // El destino: una pista del mismo tipo de nuestro montaje.
                        guard let destino = montaje.pistas.first(where: {
                            $0.tipo == pistaImportada.tipo && !$0.bloqueada
                        })?.id, let indice = montaje.indiceDePista(destino) else { continue }
                        for clip in pistaImportada.clips {
                            var copia = clip
                            copia.id = UUID()
                            copia.inicio += offset
                            copia.mediaID = reasignados[clip.mediaID] ?? clip.mediaID
                            // Sin el medio no hay clip que valga: se salta con aviso.
                            montaje.pistas[indice].clips.append(copia)
                        }
                        montaje.pistas[indice].ordenar()
                    }
                }
                let importados = cargados.count + reasignados.count
                let perdidosTexto = perdidos.isEmpty ? "" : " · faltan \(perdidos.count) archivos"
                status = "Proyecto importado · \(importados) medios, \(offset > 0 ? "montaje pegado al final" : "montaje copiado")\(perdidosTexto)"
            }
        }
    }

    /// Abre un proyecto revinculando los medios y avisando de los que falten.
    /// Un medio que no aparece no invalida el montaje: el clip se queda en su sitio
    /// marcado como offline. Rechazar el proyecto entero por un archivo movido
    /// obligaría a rehacer el trabajo, que es exactamente lo contrario de lo que
    /// debe hacer un editor.
    func cargarProyecto(desde url: URL) async {
        do {
            let proyecto = try ProyectoEditorcito.leer(try Data(contentsOf: url))
            await instalarProyecto(
                proyecto,
                carpeta: url.deletingLastPathComponent(),
                nombre: proyecto.nombre ?? url.deletingPathExtension().lastPathComponent,
                recuperado: false
            )
        } catch {
            status = "No se pudo abrir: \(error.localizedDescription)"
        }
    }

    /// Instala un proyecto manteniendo también los medios que no se pudieron
    /// resolver. Así los clips offline siguen visibles, seleccionables y
    /// revinculables en vez de desaparecer de la biblioteca.
    private func instalarProyecto(
        _ proyecto: ProyectoEditorcito,
        carpeta: URL?,
        nombre: String,
        recuperado: Bool
    ) async {
        var cargados: [MediaItem] = []
        var resueltos: [UUID: MedioResuelto] = [:]
        var perdidos: [String] = []

        for guardado in proyecto.medios {
            let archivo = proyecto.localizar(guardado, carpetaDelProyecto: carpeta)
            let url = archivo ?? URL(fileURLWithPath: guardado.ruta)
            cargados.append(
                MediaItem(
                    id: guardado.id, url: url, duration: guardado.duracion,
                    size: CGSize(width: guardado.ancho, height: guardado.alto),
                    fileSize: guardado.bytes ?? 0, frameRate: guardado.fps ?? 0,
                    variableFrameRate: guardado.vfr ?? false,
                    bin: guardado.bin ?? "Todos",
                    subclipDe: guardado.subclip
                )
            )
            guard let archivo else {
                perdidos.append(guardado.nombre)
                continue
            }
            if let medio = try? await MedioResuelto.cargar(id: guardado.id, url: archivo) {
                resueltos[guardado.id] = medio
                prepararMiniatura(medio)
                prepararFormaDeOnda(medio)
            } else {
                perdidos.append(guardado.nombre)
            }
        }

        undoStack.removeAll()
        redoStack.removeAll()
        media = cargados
        mediosOriginales = resueltos
        medios = resueltos
        proxyURLs.removeAll()
        proxiesActivos = false
        montaje = proyecto.montaje
        projectName = nombre
        carpetaDelProyecto = carpeta
        selectedMediaID = media.first?.id
        selectedMediaIDs = media.first.map { Set<UUID>([$0.id]) } ?? Set<UUID>()
        selectedClipID = montaje.todosLosClips.first?.clip.id
        pistaActiva = montaje.pistas.first { $0.tipo == .video }?.id
        cargarMedioEnOrigen(selectedMediaID)
        documentRevision += 1
        updateHistoryState()
        isDirty = recuperado
        ultimoRender = nil
        rebuildPreview(keepPosition: false)
        let avisoMedios = perdidos.isEmpty
            ? ""
            : " · \(perdidos.count) medio(s) offline: \(perdidos.prefix(3).joined(separator: ", "))"
        status = recuperado
            ? "Montaje recuperado; guárdalo para conservarlo\(avisoMedios)"
            : "Proyecto abierto\(avisoMedios)"
    }

    /// Vuelve a apuntar un medio a otro archivo, conservando el montaje.
    func revincular(_ id: UUID) {
        let panel = NSOpenPanel()
        panel.title = "Localizar el archivo"
        panel.prompt = "Revincular"
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.audiovisualContent]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task {
            guard let medio = try? await MedioResuelto.cargar(id: id, url: url) else {
                status = "Ese archivo no se puede usar"
                return
            }
            medios[id] = medio
            mediosOriginales[id] = medio
            proxyURLs[id] = nil
            if proxiesActivos { proxiesActivos = false }
            miniaturas[id] = await MedioResuelto.miniatura(medio)
            prepararFormaDeOnda(medio)
            if let i = media.firstIndex(where: { $0.id == id }) {
                let anterior = media[i]
                let valores = try? url.resourceValues(forKeys: [.fileSizeKey])
                let bytes = Int64(valores?.fileSize ?? Int(anterior.fileSize))
                media[i] = MediaItem(
                    id: id, url: url,
                    duration: anterior.subclipDe.map { timebase.segundos($0.duracion) } ?? medio.duracion.seconds,
                    size: medio.tamanoVisible, fileSize: bytes, frameRate: medio.fps,
                    variableFrameRate: medio.esVFR, bin: anterior.bin,
                    subclipDe: anterior.subclipDe
                )
            }
            if sourceMediaID == id { cargarMedioEnOrigen(id) }
            ultimoRender = nil
            rebuildPreview(keepPosition: true)
            scheduleAutosave()
            status = "Medio revinculado"
        }
    }

    /// `true` si el clip apunta a un medio que ahora mismo no está disponible.
    func estaOffline(_ clip: Clip) -> Bool { medios[clip.mediaID] == nil }

    static func timebaseMasCercana(a fps: Double) -> Timebase {
        ProyectoEditorcito.timebaseMasCercana(a: fps)
    }

    /// El clip cuyo editor de curvas está abierto, o `nil`.
    @Published var clipConCurvasAbiertas: UUID?
    /// Borrador de las curvas mientras se editan: se aplica al soltar.
    private var borradorDeCurvas: CurvasDeClip?

    /// Abre el editor de curvas del clip (con curvas de partida neutras si no
    /// las tiene).
    func abrirEditorDeCurvas(_ id: UUID) {
        guard let clip = montaje.clip(id) else { return }
        borradorDeCurvas = clip.color.curvas ?? .identidad
        clipConCurvasAbiertas = id
    }

    /// Puntos de la curva que se está editando (para el editor).
    func curvaEnEdicion(_ canal: CanalDeCurva) -> [PuntoDeCurva] {
        guard let borrador = borradorDeCurvas else { return [.init(x: 0, y: 0), .init(x: 1, y: 1)] }
        switch canal {
        case .luma: return borrador.luma
        case .rojo: return borrador.rojo
        case .verde: return borrador.verde
        case .azul: return borrador.azul
        }
    }

    /// Actualiza los puntos de la curva del canal en edición.
    func fijarPuntoDeCurva(_ canal: CanalDeCurva, puntos: [PuntoDeCurva]) {
        guard borradorDeCurvas != nil else { return }
        switch canal {
        case .luma: borradorDeCurvas?.luma = puntos
        case .rojo: borradorDeCurvas?.rojo = puntos
        case .verde: borradorDeCurvas?.verde = puntos
        case .azul: borradorDeCurvas?.azul = puntos
        }
        // Aplicar en vivo: la curva se ve en el monitor mientras se arrastra.
        if let id = clipConCurvasAbiertas, let borrador = borradorDeCurvas {
            modificarClip(id, recompilar: false) { $0.color.curvas = borrador }
            rebuildPreview(keepPosition: true)
        }
    }

    func cerrarEditorDeCurvas() {
        clipConCurvasAbiertas = nil
        borradorDeCurvas = nil
        scheduleAutosave()
    }

    func eliminarPuntoDeCurva(_ canal: CanalDeCurva, en indice: Int) {
        guard var borrador = borradorDeCurvas else { return }
        switch canal {
        case .luma: borrador.luma = puntosSin(borrador.luma, indice)
        case .rojo: borrador.rojo = puntosSin(borrador.rojo, indice)
        case .verde: borrador.verde = puntosSin(borrador.verde, indice)
        case .azul: borrador.azul = puntosSin(borrador.azul, indice)
        }
        borradorDeCurvas = borrador
        if let id = clipConCurvasAbiertas {
            modificarClip(id, recompilar: false) { $0.color.curvas = borrador }
            rebuildPreview(keepPosition: true)
        }
    }

    private func puntosSin(_ puntos: [PuntoDeCurva], _ indice: Int) -> [PuntoDeCurva] {
        guard puntos.count > 2, puntos.indices.contains(indice) else { return puntos }
        return puntos.enumerated().compactMap { $0.offset == indice ? nil : $0.element }
    }

    // MARK: - Montar

    /// Clip nuevo a partir de un medio, respetando su entrada y salida de origen.
    private func clipDe(_ item: MediaItem, enlace: UUID? = nil) -> Clip {
        let entrada = entradaDeOrigen[item.id] ?? 0
        let salida = salidaDeOrigen[item.id] ?? timebase.frames(segundos: item.duration)
        return Clip(
            mediaID: item.id,
            nombre: item.name,
            inicio: 0,
            duracion: max(1, salida - entrada),
            entradaEnOrigen: (item.subclipDe?.entrada ?? 0) + entrada,
            enlace: enlace
        )
    }

    private func pistaDeAudioParaEnlace() -> UUID? {
        montaje.pistas.first { $0.tipo == .audio && !$0.bloqueada }?.id
    }

    private func pistaDeVideoParaEnlace() -> UUID? {
        montaje.pistas.first { $0.tipo == .video && !$0.bloqueada }?.id
    }

    private func anadirAudioEnHueco(_ clip: Clip, pista: UUID) {
        guard let indice = montaje.indiceDePista(pista) else { return }
        montaje.pistas[indice].clips.append(clip)
        montaje.pistas[indice].ordenar()
    }

    /// Corta por un frame concreto en una pista concreta. Es lo que hace la
    /// cuchilla al pulsar sobre un clip.
    func cortar(en frame: Int64, pista: UUID) {
        var creados: [UUID] = []
        performEdit(keepPosition: true) {
            creados = montaje.partir(en: frame, pistas: [pista])
            if let primero = creados.first { selectedClipID = primero }
        }
        if !creados.isEmpty { status = "Cortado en \(timebase.timecode(frame))" }
    }

    /// Cambia la cadencia del proyecto conservando el tiempo real de cada clip.
    ///
    /// Reinterpretar los frames sin convertirlos movería todo el montaje: un clip
    /// que empieza en el frame 250 está en el segundo 10 a 25 fps y en el 4,17 a
    /// 60. Se convierte por tiempo, que es lo que el montador tiene en la cabeza.
    func cambiarTimebase(_ nueva: Timebase) {
        guard nueva != timebase else { return }
        let anterior = timebase
        performEdit(keepPosition: true) {
            func convertir(_ frames: Int64) -> Int64 {
                nueva.frames(segundos: anterior.segundos(frames))
            }
            for p in montaje.pistas.indices {
                for c in montaje.pistas[p].clips.indices {
                    var clip = montaje.pistas[p].clips[c]
                    clip.inicio = convertir(clip.inicio)
                    clip.duracion = max(1, convertir(clip.duracion))
                    clip.entradaEnOrigen = convertir(clip.entradaEnOrigen)
                    clip.entradaFundido = convertir(clip.entradaFundido)
                    clip.salidaFundido = convertir(clip.salidaFundido)
                    clip.keyframes = clip.keyframes?.map {
                        var keyframe = $0
                        keyframe.frame = convertir($0.frame)
                        return keyframe
                    }
                    clip.rampasDeVelocidad = clip.rampasDeVelocidad?.map {
                        var rampa = $0
                        rampa.frame = convertir($0.frame)
                        return rampa
                    }
                    if var titulo = clip.titulo {
                        titulo.fundido = convertir(titulo.fundido)
                        clip.titulo = titulo
                    }
                    if var transicion = clip.transicionEntrada {
                        transicion.duracion = convertir(transicion.duracion)
                        clip.transicionEntrada = transicion
                    }
                    if var transicion = clip.transicionSalida {
                        transicion.duracion = convertir(transicion.duracion)
                        clip.transicionSalida = transicion
                    }
                    montaje.pistas[p].clips[c] = clip
                }
            }
            for m in montaje.marcadores.indices {
                montaje.marcadores[m].frame = convertir(montaje.marcadores[m].frame)
                montaje.marcadores[m].duracion = convertir(montaje.marcadores[m].duracion)
            }
            if let entrada = montaje.entradaDeTrabajo { montaje.entradaDeTrabajo = convertir(entrada) }
            if let salida = montaje.salidaDeTrabajo { montaje.salidaDeTrabajo = convertir(salida) }
            montaje.subtitulos = montaje.subtitulos?.map {
                var subtitulo = $0
                subtitulo.inicio = convertir($0.inicio)
                subtitulo.fin = convertir($0.fin)
                return subtitulo
            }
            // Los desfases de los grupos multicámara viven en frames de proyecto:
            // sin convertirlos, cada ángulo se insertaría con su desfase en la
            // escala antigua y la sincronía del grupo se descuadraría en silencio.
            if let grupos = montaje.gruposMulticam {
                for indice in grupos.indices {
                    montaje.gruposMulticam?[indice].desfases = GrupoMulticam.convertirDesfases(
                        grupos[indice].desfases, de: anterior, a: nueva
                    )
                }
            }
            // Los subclips guardan su rango en frames de proyecto: al cambiar la
            // cadencia, el recorte tiene que seguir señalando el mismo trozo de
            // archivo. `media` es estado de la app, así que se convierte aparte.
            for i in media.indices {
                if let subclip = media[i].subclipDe {
                    let convertido = SubclipOrigen(
                        medioBase: subclip.medioBase,
                        entrada: nueva.frames(segundos: anterior.segundos(subclip.entrada)),
                        salida: nueva.frames(segundos: anterior.segundos(subclip.salida))
                    )
                    media[i].subclipDe = convertido
                }
            }
            montaje.timebase = nueva
        }
        status = "Base de tiempo: \(nueva.nombre)"
    }

    func addSelectedMedia() {
        guard let selectedMedia else { return }
        addToTimeline(selectedMedia)
    }

    func seleccionarClip(_ id: UUID, extender: Bool = false) {
        timelineHasFocus = true
        if extender {
            if selectedClipIDs.contains(id) {
                selectedClipIDs.remove(id)
            } else {
                selectedClipIDs.insert(id)
            }
            selectedClipID = selectedClipIDs.first ?? id
        } else {
            selectedClipIDs = [id]
            selectedClipID = id
        }
    }

    /// Añade al final de la pista activa. Es el gesto de «méteme esto y ya».
    func addToTimeline(_ item: MediaItem) {
        guard medios[item.id] != nil else {
            status = "«\(item.name)» está offline; revincúlalo antes de montarlo"
            return
        }
        let pista = medios[item.id]?.tieneVideo == true
            ? (pistaDeVideoParaEnlace() ?? pistaDeTrabajo)
            : (montaje.pistas.first { $0.tipo == .audio && !$0.bloqueada }?.id ?? pistaDeTrabajo)
        let final = montaje.pista(pista)?.fin ?? 0
        performEdit {
            let enlace = UUID()
            let clip = clipDe(item, enlace: enlace)
            montaje.sobrescribir(clip, enPista: pista, en: final)
            if medios[item.id]?.tieneVideo == true, medios[item.id]?.tieneAudio == true,
               let audio = pistaDeAudioParaEnlace(), audio != pista {
                var audioClip = clipDe(item, enlace: enlace)
                audioClip.id = UUID()
                montaje.sobrescribir(audioClip, enPista: audio, en: final)
            }
            selectedClipID = clip.id
        }
    }

    /// Suelta un medio de la biblioteca sobre el timeline: el gesto de arrastrar
    /// y soltar de cualquier NLE. La posición del puntero decide el frame, y con
    /// el imán activo el frame atrae al corte más cercano.
    ///
    /// ⌥ mientras se suelta escribe encima (superposición); sin él, abre hueco
    /// (inserción), que es lo que no destruye lo que ya había.
    func soltarEnTimeline(mediaID: UUID, enFrame frame: Int64, superponer: Bool, enPista pistaSolicitada: UUID? = nil) {
        guard let item = media.first(where: { $0.id == mediaID }) else { return }
        guard medios[item.id] != nil else {
            status = "«\(item.name)» está offline; revincúlalo antes de montarlo"
            return
        }
        let tipo = medios[item.id]?.tieneVideo == true ? TipoDePista.video : .audio
        let pista = pistaSolicitada.flatMap { solicitada in
            guard let destino = montaje.pista(solicitada), !destino.bloqueada, destino.tipo == tipo else { return nil }
            return destino.id
        } ?? (tipo == .video
            ? (pistaDeVideoParaEnlace() ?? pistaDeTrabajo)
            : (montaje.pistas.first { $0.tipo == .audio && !$0.bloqueada }?.id ?? pistaDeTrabajo))
        let destino = imanActivo
            ? montaje.imantar(max(0, frame), umbral: max(1, Int64(8 / max(timelineScale / timebase.fps, 0.0001))))
            : max(0, frame)
        performEdit(keepPosition: true) {
            let enlace = UUID()
            let clip = clipDe(item, enlace: enlace)
            if superponer {
                montaje.sobrescribir(clip, enPista: pista, en: destino)
            } else {
                montaje.insertar(clip, enPista: pista, en: destino)
            }
            if medios[item.id]?.tieneVideo == true, medios[item.id]?.tieneAudio == true,
               let audio = pistaDeAudioParaEnlace(), audio != pista,
               let indice = montaje.indiceDePista(audio) {
                var audioClip = clipDe(item, enlace: enlace)
                audioClip.id = UUID()
                if superponer {
                    montaje.sobrescribir(audioClip, enPista: audio, en: destino)
                } else {
                    montaje.pistas[indice].clips.append(audioClip)
                    montaje.pistas[indice].ordenar()
                }
            }
            selectedClipID = clip.id
        }
        status = superponer
            ? "Superpuesto «\(item.name)» en \(timebase.timecode(destino))"
            : "Insertado «\(item.name)» en \(timebase.timecode(destino))"
        seek(toFrame: destino)
    }

    /// Edición por inserción de tres puntos: abre hueco en el cabezal.
    func insertarEnCabezal() {
        guard let item = selectedMedia else { status = "Elige un medio en la biblioteca"; return }
        guard medios[item.id] != nil else {
            status = "«\(item.name)» está offline; revincúlalo antes de montarlo"
            return
        }
        let pista = medios[item.id]?.tieneVideo == true
            ? (pistaDeVideoParaEnlace() ?? pistaDeTrabajo)
            : (montaje.pistas.first { $0.tipo == .audio && !$0.bloqueada }?.id ?? pistaDeTrabajo)
        let destino = cabezal
        performEdit(keepPosition: true) {
            let enlace = UUID()
            let clip = clipDe(item, enlace: enlace)
            montaje.insertar(clip, enPista: pista, en: destino)
            if medios[item.id]?.tieneVideo == true, medios[item.id]?.tieneAudio == true,
               let audio = pistaDeAudioParaEnlace(), audio != pista,
               let indice = montaje.indiceDePista(audio) {
                var audioClip = clipDe(item, enlace: enlace)
                audioClip.id = UUID()
                montaje.pistas[indice].clips.append(audioClip)
                montaje.pistas[indice].ordenar()
            }
            selectedClipID = clip.id
        }
        status = "Insertado en \(timebase.timecode(destino))"
    }

    /// Edición por superposición: escribe encima sin mover nada de sitio.
    func sobrescribirEnCabezal() {
        guard let item = selectedMedia else { status = "Elige un medio en la biblioteca"; return }
        guard medios[item.id] != nil else {
            status = "«\(item.name)» está offline; revincúlalo antes de montarlo"
            return
        }
        let pista = medios[item.id]?.tieneVideo == true
            ? (pistaDeVideoParaEnlace() ?? pistaDeTrabajo)
            : (montaje.pistas.first { $0.tipo == .audio && !$0.bloqueada }?.id ?? pistaDeTrabajo)
        let destino = cabezal
        performEdit(keepPosition: true) {
            let enlace = UUID()
            let clip = clipDe(item, enlace: enlace)
            montaje.sobrescribir(clip, enPista: pista, en: destino)
            if medios[item.id]?.tieneVideo == true, medios[item.id]?.tieneAudio == true,
               let audio = pistaDeAudioParaEnlace(), audio != pista {
                var audioClip = clipDe(item, enlace: enlace)
                audioClip.id = UUID()
                montaje.sobrescribir(audioClip, enPista: audio, en: destino)
            }
            selectedClipID = clip.id
        }
        status = "Superpuesto en \(timebase.timecode(destino))"
    }

    // MARK: Cortar y borrar

    /// Parte por el cabezal. Sin selección corta todas las pistas, que es lo que
    /// se espera al pulsar la tecla a secas.
    func splitAtPlayhead() {
        let destino = cabezal
        let objetivo: Set<UUID>? = selectedClipID.flatMap { montaje.pistaDe(clip: $0) }.map { [$0] }
        var creados: [UUID] = []
        performEdit(keepPosition: true) {
            creados = montaje.partir(en: destino, pistas: objetivo)
            if let primero = creados.first { selectedClipID = primero }
        }
        status = creados.isEmpty
            ? "El cabezal no está dentro de ningún clip"
            : "Cortado en \(timebase.timecode(destino)) · \(creados.count) clips"
    }

    /// Borra dejando el hueco.
    func removeSelectedClip() {
        let ids = selectedClipIDs.isEmpty ? selectedClipID.map { [$0] } ?? [] : Array(selectedClipIDs)
        guard !ids.isEmpty else { return }
        if ids.count > 1 {
            performEdit(keepPosition: true) {
                for id in ids { montaje.levantar(id) }
                selectedClipIDs.removeAll()
                selectedClipID = nil
            }
            status = "\(ids.count) clips retirados; los huecos se mantienen"
            return
        }
        let id = ids[0]
        let siguiente = clipVecino(de: id, haciaDelante: true)
        performEdit(keepPosition: true) {
            montaje.levantar(id)
            selectedClipID = siguiente
            selectedClipIDs = siguiente.map { [$0] } ?? []
        }
        status = "Clip retirado; el hueco se mantiene"
    }

    /// Borra y cierra el hueco arrastrando lo que venga detrás.
    func borrarConArrastre() {
        guard let id = selectedClipID else { return }
        let siguiente = clipVecino(de: id, haciaDelante: true)
        performEdit(keepPosition: true) {
            montaje.borrarConArrastre(id)
            selectedClipID = siguiente
        }
        status = "Clip eliminado y hueco cerrado"
    }

    /// Cierra el hueco en el que está el cabezal, en la pista activa.
    func cerrarHuecoEnCabezal() {
        let pista = pistaDeTrabajo
        guard let hueco = montaje.hueco(enPista: pista, en: cabezal) else {
            status = "El cabezal no está sobre un hueco"
            return
        }
        let ancho = hueco.fin - hueco.inicio
        performEdit(keepPosition: true) {
            guard let indice = montaje.indiceDePista(pista) else { return }
            for i in montaje.pistas[indice].clips.indices
            where montaje.pistas[indice].clips[i].inicio >= hueco.fin {
                montaje.pistas[indice].clips[i].inicio -= ancho
            }
        }
        status = "Hueco de \(ancho) frames cerrado"
    }

    func cerrarTodosLosHuecos() {
        let pista = pistaDeTrabajo
        performEdit(keepPosition: true) { montaje.cerrarHuecos(enPista: pista) }
        status = "Huecos cerrados en la pista"
    }

    // MARK: Recorte

    func beginTrim() { if trimSnapshot == nil { trimSnapshot = snapshot() } }

    func endTrim() {
        guard let before = trimSnapshot else { return }
        trimSnapshot = nil
        commit(before: before)
        rebuildPreview(keepPosition: true)
    }

    /// Recorta un borde del clip con el modo que dicte la herramienta activa.
    @discardableResult
    func recortar(_ id: UUID, borde: BordeDeClip, delta: Int64, modo: ModoDeRecorte? = nil) -> Int64 {
        guard montaje.clip(id) != nil else { return 0 }
        let modoReal = modo ?? herramienta.modoDeRecorte
        let objetivos = montaje.grupoEnlazado(de: id)
        var permitido = delta
        // Primero se calcula el margen común. Después se aplica el mismo delta a
        // vídeo y audio; si uno de los dos llega antes al borde del medio, ninguno
        // se separa del otro por un frame.
        for objetivo in objetivos {
            guard let miembro = montaje.clip(objetivo) else { continue }
            var prueba = montaje
            let aplicado = prueba.recortar(
                objetivo,
                borde: borde,
                delta: delta,
                modo: modoReal,
                duracionDelMedio: duracionDelMedio(deClip: miembro)
            )
            permitido = delta >= 0 ? min(permitido, aplicado) : max(permitido, aplicado)
        }
        var aplicado: Int64 = 0
        let mutar = {
            for objetivo in objetivos {
                guard let miembro = self.montaje.clip(objetivo) else { continue }
                _ = self.montaje.recortar(
                    objetivo,
                    borde: borde,
                    delta: permitido,
                    modo: modoReal,
                    duracionDelMedio: self.duracionDelMedio(deClip: miembro)
                )
            }
            aplicado = permitido
        }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
        return aplicado
    }

    /// Recorta hasta el cabezal: es el gesto rápido de «quítame lo de antes» o
    /// «quítame lo de después» sin arrastrar nada.
    func recortarHastaCabezal(borde: BordeDeClip) {
        guard let id = selectedClipID, let clip = montaje.clip(id) else { return }
        let delta = borde == .entrada ? cabezal - clip.inicio : cabezal - clip.fin
        guard delta != 0 else { return }
        recortar(id, borde: borde, delta: delta)
        status = borde == .entrada ? "Recortada la entrada al cabezal" : "Recortada la salida al cabezal"
    }

    // MARK: Acciones de NLE (copiar atributos, match frame, extend edit)

    /// Los atributos que se copian y se pegan: transformación (posición, escala,
    /// rotación, opacidad), recorte, ganancia y fundidos. La velocidad no entra:
    /// pegar velocidad cambiaría la duración del clip, que no es un atributo.
    struct AtributosCopiados {
        var transformacion: TransformacionDeClip
        var ganancia: Double
        var entradaFundido: Int64
        var salidaFundido: Int64
    }

    /// Lo copiado con ⌥⌘C, vivo hasta que se copie otra cosa.
    @Published private(set) var atributosCopiados: AtributosCopiados?

    /// Copia la transformación, la ganancia y los fundidos del clip seleccionado.
    ///
    /// Es el ⌥⌘C de cualquier NLE: «este clip está como quiero, ponme veinte igual».
    /// Se copia del clip, no de la selección, porque atributos tiene un clip.
    func copiarAtributos() {
        guard let id = selectedClipID, let clip = montaje.clip(id) else { return }
        atributosCopiados = AtributosCopiados(
            transformacion: clip.transformacion,
            ganancia: clip.ganancia,
            entradaFundido: clip.entradaFundido,
            salidaFundido: clip.salidaFundido
        )
        status = "Atributos copiados de «\(clip.nombre)»"
    }

    /// Pega los atributos copiados en todos los clips seleccionados.
    ///
    /// Se aplica a la selección entera —ese es el punto: llevar un look a veinte
    /// clips— y se confirma con `performEdit`, así que un ⌘Z lo deshace todo.
    func pegarAtributos() {
        guard let copiados = atributosCopiados else { return }
        let objetivos = seleccionados()
        guard !objetivos.isEmpty else { return }
        for id in objetivos {
            modificarClip(id) { clip in
                clip.transformacion = copiados.transformacion
                clip.ganancia = copiados.ganancia
                clip.entradaFundido = copiados.entradaFundido
                clip.salidaFundido = copiados.salidaFundido
            }
        }
        scheduleAutosave()
        status = "Atributos pegados en \(objetivos.count) \(objetivos.count == 1 ? "clip" : "clips")"
    }

    /// Refresca el instrumento del monitor (forma de onda o vectorscopio).
    ///
    /// Se calcula bajo demanda —cuando el cabezal se mueve o hay un seek— y no en
    /// cada fotograma de reproducción: el histograma cuesta décimas y un
    /// instrumento parpadeando en vivo es menos útil que uno que se puede leer.
    /// El frame se captura del render del reproductor con `AVAssetImageGenerator`,
    /// que es el mismo dato que se ve en el monitor.
    func refrescarFormaDeOnda() {
        guard instrumentoDeMonitor != .ninguno else {
            ultimoFrameDeOnda = -1
            return
        }
        let frameActual = Int64(playhead * timebase.fps)
        guard frameActual != ultimoFrameDeOnda,
              let render = ultimoRender else { return }
        ultimoFrameDeOnda = frameActual

        let segundo = max(0, min(playhead, render.composicion.duration.seconds - 0.001))
        guard segundo.isFinite else { return }

        // El frame se regenera desde la composición del render —el único asset
        // que existe en el reproductor— con el mismo dato que se ve en el monitor.
        let composicion = render.composicion
        let composicionDeVideo = render.composicionDeVideo
        let instrumento = instrumentoDeMonitor
        Task.detached(priority: .utility) {
            // Basta con la mitad de la resolución del monitor para leer el instrumento.
            guard let cg = await CapturadorDeFrames.capturarFrame(
                de: composicion, videoComposition: composicionDeVideo,
                en: segundo, maximo: CGSize(width: 960, height: 540)
            ) else { return }
            var buffer: CVPixelBuffer?
            let atributos: [String: Any] = [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
                kCVPixelBufferWidthKey as String: cg.width,
                kCVPixelBufferHeightKey as String: cg.height,
            ]
            guard CVPixelBufferCreate(kCFAllocatorDefault, cg.width, cg.height,
                                      kCVPixelFormatType_32BGRA, atributos as CFDictionary,
                                      &buffer) == kCVReturnSuccess,
                  let pixelBuffer = buffer else { return }
            CVPixelBufferLockBaseAddress(pixelBuffer, [])
            defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }
            guard let contexto = CGContext(
                data: CVPixelBufferGetBaseAddress(pixelBuffer),
                width: cg.width, height: cg.height,
                bitsPerComponent: 8, bytesPerRow: CVPixelBufferGetBytesPerRow(pixelBuffer),
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
            ) else { return }
            contexto.draw(cg, in: CGRect(x: 0, y: 0, width: cg.width, height: cg.height))

            await MainActor.run {
                switch instrumento {
                case .vectorscopio:
                    self.vectorscopio = Vectorscopio.calcular(del: pixelBuffer)
                case .paradeRGB:
                    self.paradeRGB = ParadeRGB.calcular(del: pixelBuffer)
                case .histograma:
                    self.histograma = HistogramaDeLuminancia.calcular(del: pixelBuffer)
                default:
                    self.formaDeOnda = FormaDeOnda.calcular(del: pixelBuffer)
                }
            }
        }
    }

    /// Abre en el monitor de origen el medio del clip bajo el cabezal, en ese frame.
    ///
    /// Es la tecla F de cualquier NLE: quieres ver el plano entero del que sale el
    /// corte que estás mirando. El cabezal de origen se coloca en el frame del
    /// medio que corresponde al del montaje, respetando velocidad y recorte.
    func matchFrame() {
        guard let clip = clipBajoElCabezal else { return }
        guard let medio = media.first(where: { $0.id == clip.mediaID }) else {
            status = "El medio de «\(clip.nombre)» no está en la biblioteca"
            return
        }
        monitorActual = .origen
        cargarMedioEnOrigen(clip.mediaID)
        let relativo = max(0, cabezal - clip.inicio)
        cabezalDeOrigen = clip.entradaEnOrigen + Int64((Double(relativo) * abs(clip.velocidad)).rounded())
        status = "Match frame: «\(medio.url.deletingPathExtension().lastPathComponent)»"
    }

    /// Extiende el clip seleccionado hasta el cabezal (E).
    ///
    /// Es el *extend edit* de la industria: si el corte está en el sitio equivocado
    /// y sabes dónde tiene que estar, mueves el cabezal y pulsas E —no hay que
    /// agarrar el borde con el ratón. Se estira el borde más cercano al cabezal,
    /// que es el que el usuario está mirando cuando pulsa la tecla.
    func extendEdit() {
        guard let id = selectedClipID, let clip = montaje.clip(id) else { return }
        // Se estira el borde más cercano al cabezal; con empate, el de salida, que
        // es el que se extiende hacia delante —el gesto más común del extend edit.
        let cercaDeLaEntrada = abs(cabezal - clip.inicio) < abs(cabezal - clip.fin)
        recortar(id, borde: cercaDeLaEntrada ? .entrada : .salida, delta: cabezal - (cercaDeLaEntrada ? clip.inicio : clip.fin))
        status = "Extend edit hasta el cabezal"
    }

    /// Los clips seleccionados, o el clip seleccionado suelto si no hay conjunto.
    private func seleccionados() -> [UUID] {
        if !selectedClipIDs.isEmpty { return Array(selectedClipIDs) }
        if let id = selectedClipID { return [id] }
        return []
    }

    /// El clip bajo el cabezal, en cualquier pista visible.
    private var clipBajoElCabezal: Clip? {
        for pista in montaje.pistas {
            if let clip = pista.clips.first(where: { $0.contiene(cabezal) }) { return clip }
        }
        return nil
    }

    @discardableResult
    func deslizarContenido(_ id: UUID, delta: Int64) -> Int64 {
        guard montaje.clip(id) != nil else { return 0 }
        let objetivos = montaje.grupoEnlazado(de: id)
        var permitido = delta
        for objetivo in objetivos {
            guard let miembro = montaje.clip(objetivo) else { continue }
            var prueba = montaje
            let aplicado = prueba.deslizarContenido(
                objetivo, delta: delta, duracionDelMedio: duracionDelMedio(deClip: miembro)
            )
            permitido = delta >= 0 ? min(permitido, aplicado) : max(permitido, aplicado)
        }
        var aplicado: Int64 = 0
        let mutar = {
            for objetivo in objetivos {
                guard let miembro = self.montaje.clip(objetivo) else { continue }
                _ = self.montaje.deslizarContenido(
                    objetivo, delta: permitido, duracionDelMedio: self.duracionDelMedio(deClip: miembro)
                )
            }
            aplicado = permitido
        }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
        return aplicado
    }

    @discardableResult
    func deslizarPosicion(_ id: UUID, delta: Int64) -> Int64 {
        guard montaje.clip(id) != nil else { return 0 }
        let objetivos = montaje.grupoEnlazado(de: id)
        var permitido = delta
        for objetivo in objetivos {
            guard let miembro = montaje.clip(objetivo) else { continue }
            var prueba = montaje
            let aplicado = prueba.deslizarPosicion(
                objetivo, delta: delta, duracionDelMedio: duracionDelMedio(deClip: miembro)
            )
            permitido = delta >= 0 ? min(permitido, aplicado) : max(permitido, aplicado)
        }
        var aplicado: Int64 = 0
        let mutar = {
            for objetivo in objetivos {
                guard let miembro = self.montaje.clip(objetivo) else { continue }
                _ = self.montaje.deslizarPosicion(
                    objetivo, delta: permitido, duracionDelMedio: self.duracionDelMedio(deClip: miembro)
                )
            }
            aplicado = permitido
        }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
        return aplicado
    }

    /// Mueve un clip, imantándolo a los cortes cercanos si el imán está activo.
    func moverClip(_ id: UUID, aPista destino: UUID, aFrame frame: Int64, umbralDeIman: Int64 = 0) {
        guard let clip = montaje.clip(id) else { return }
        var objetivo = max(0, frame)
        if imanActivo && umbralDeIman > 0 {
            // Se prueban las dos puntas del clip: engancha la que quede más cerca,
            // que es como se comporta el imán de cualquier editor.
            let porLaEntrada = montaje.imantar(objetivo, umbral: umbralDeIman, excluyendo: [id], cabezal: cabezal)
            let porLaSalida = montaje.imantar(objetivo + clip.duracion, umbral: umbralDeIman, excluyendo: [id], cabezal: cabezal) - clip.duracion
            objetivo = abs(porLaEntrada - objetivo) <= abs(porLaSalida - objetivo) ? porLaEntrada : porLaSalida
        }
        let destinoFinal = max(0, objetivo)
        let mutar = { self.montaje.mover(id, aPista: destino, en: destinoFinal) }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
    }

    /// Devuelve la pista del mismo tipo cuyo centro queda más cerca del centro
    /// visual del clip tras el desplazamiento vertical. Nunca permite que un
    /// arrastre de audio termine tocando vídeo, ni al revés.
    func pistaDestino(de origen: UUID, desplazamientoVertical: Double) -> UUID {
        guard let indiceOrigen = montaje.indiceDePista(origen) else { return origen }
        let tipo = montaje.pistas[indiceOrigen].tipo
        var centros: [UUID: Double] = [:]
        var y = 0.0
        for pista in montaje.pistas {
            centros[pista.id] = y + pista.altura / 2
            y += pista.altura + 1
        }
        guard let centroOrigen = centros[origen] else { return origen }
        let destinoVisual = centroOrigen + desplazamientoVertical
        return montaje.pistas
            .filter { $0.tipo == tipo }
            .min { abs((centros[$0.id] ?? centroOrigen) - destinoVisual) < abs((centros[$1.id] ?? centroOrigen) - destinoVisual) }?.id ?? origen
    }

    // MARK: Propiedades del clip

    /// Cambia una propiedad del clip seleccionado. Todas pasan por aquí para que
    /// ninguna se escape del historial ni de la reconstrucción de la vista previa.
    func modificarClip(_ id: UUID, recompilar: Bool = true, _ cambio: @escaping (inout Clip) -> Void) {
        guard let pistaID = montaje.pistaDe(clip: id),
              let p = montaje.indiceDePista(pistaID),
              let c = montaje.pistas[p].clips.firstIndex(where: { $0.id == id }) else { return }
        let mutar = { cambio(&self.montaje.pistas[p].clips[c]) }
        if trimSnapshot == nil {
            if recompilar { performEdit(keepPosition: true, mutar) } else { mutar() }
        } else {
            mutar()
        }
    }

    func fijarGanancia(_ id: UUID, _ dB: Double) {
        let valor = min(max(-60, dB), 12)
        guard let clip = montaje.clip(id) else { return }
        let relativo = cabezal - clip.inicio
        modificarClip(id) { clip in
            if let indice = clip.keyframes?.firstIndex(where: { $0.frame == relativo }) {
                clip.keyframes?[indice].ganancia = valor
            } else {
                clip.ganancia = valor
            }
        }
    }

    func anadirKeyframe(_ id: UUID) {
        guard let clip = montaje.clip(id) else { return }
        let relativo = max(0, min(clip.duracion - 1, cabezal - clip.inicio))
        let nuevo = ClipKeyframe(frame: relativo, transformacion: clip.transformacion, ganancia: clip.ganancia)
        modificarClip(id) { clip in
            var claves = clip.keyframes ?? []
            if let indice = claves.firstIndex(where: { $0.frame == relativo }) {
                claves[indice] = nuevo
            } else {
                claves.append(nuevo)
            }
            clip.keyframes = claves.sorted { $0.frame < $1.frame }
        }
        status = "Keyframe en \(timebase.timecode(cabezal))"
    }

    func eliminarKeyframeActual(_ id: UUID) {
        guard let clip = montaje.clip(id) else { return }
        let relativo = cabezal - clip.inicio
        modificarClip(id) { $0.keyframes?.removeAll { $0.frame == relativo } }
        status = "Keyframe eliminado"
    }

    /// Añade un keyframe de velocidad (rampa) en el frame relativo al clip.
    ///
    /// Con velocidad 0 es un freeze frame: la imagen se queda quieta en el
    /// frame del cabezal hasta la siguiente rampa o el final del clip. El
    /// keyframe se inserta ordenado y sustituye al que hubiera en ese mismo
    /// frame, como los keyframes de ganancia.
    func anadirRampaDeVelocidad(_ id: UUID, en frameAbsoluto: Int64, velocidad: Double) {
        guard let clip = montaje.clip(id) else { return }
        let relativo = max(0, min(clip.duracion - 1, frameAbsoluto - clip.inicio))
        modificarClip(id) { clip in
            var rampas = clip.rampasDeVelocidad ?? []
            rampas.removeAll { $0.frame == relativo }
            rampas.append(RampaDeVelocidad(frame: relativo, velocidad: velocidad))
            clip.rampasDeVelocidad = rampas.sorted { $0.frame < $1.frame }
        }
        let nombre = velocidad == 0 ? "congelado" : String(format: "%.0f %%", velocidad * 100)
        status = "Rampa \(nombre) en \(timebase.timecode(frameAbsoluto))"
    }

    func quitarRampaDeVelocidad(_ id: UUID, en frame: Int64) {
        modificarClip(id) { $0.rampasDeVelocidad?.removeAll { $0.frame == frame } }
        status = "Rampa eliminada"
    }

    func quitarRampasDeVelocidad(_ id: UUID) {
        modificarClip(id) { $0.rampasDeVelocidad = nil }
        status = "Velocidad constante de nuevo"
    }

    /// Reencuadre vertical automático: sigue al sujeto y vuelca la trayectoria
    /// como keyframes de posición editables.
    ///
    /// Es la respuesta a Auto Reframe: en Premiere te dan una caja negra, aquí
    /// salen keyframes normales que se pueden tocar, borrar o rehacer. Corre en
    /// el dispositivo (Vision), en segundo plano porque muestrear el medio tarda.
    func reframearVertical(_ id: UUID) {
        guard let clip = montaje.clip(id),
              let medio = media.first(where: { $0.id == clip.mediaID }),
              let resuelto = medios[clip.mediaID] else { return }
        guard resuelto.tieneVideo else {
            status = "«\(clip.nombre)» no tiene vídeo que reencuadrar"
            return
        }
        status = "Siguiendo al sujeto en «\(clip.nombre)»…"
        let base = clip.transformacion
        let clipID = clip.id
        let baseDeTiempo = timebase
        Task.detached(priority: .userInitiated) {
            let muestras = await DetectorDeSujeto.rastrear(
                medio: resuelto,
                duracionDelClip: clip.duracion,
                entradaEnOrigen: clip.entradaEnOrigen,
                timebase: baseDeTiempo
            )
            let suavizadas = DetectorDeSujeto.suavizar(muestras)
            let claves = ReframeVertical.keyframes(de: suavizadas)
            await MainActor.run {
                guard !claves.isEmpty else {
                    self.status = "No se encontró ningún sujeto en «\(clip.nombre)»"
                    return
                }
                self.modificarClip(clipID) { clip in
                    var c = clip
                    c.keyframes = claves.map { clave in
                        var k = clave
                        k.transformacion.rotacion = base.rotacion
                        k.transformacion.opacidad = base.opacidad
                        k.transformacion.recorteIzquierda = base.recorteIzquierda
                        k.transformacion.recorteDerecha = base.recorteDerecha
                        k.transformacion.recorteArriba = base.recorteArriba
                        k.transformacion.recorteAbajo = base.recorteAbajo
                        return k
                    }
                    clip = c
                }
                self.scheduleAutosave()
                self.status = "Reencuadre aplicado: \(claves.count) keyframes editables en «\(clip.nombre)»"
            }
        }
    }

    func fijarOpacidad(_ id: UUID, _ porcentaje: Double) {
        fijarTransformacion(id) { $0.opacidad = min(max(0, porcentaje), 100) }
    }

    func fijarVelocidad(_ id: UUID, _ velocidad: Double) {
        guard montaje.clip(id) != nil else { return }
        let nueva = min(max(0.1, velocidad), 10)
        let objetivos = montaje.grupoEnlazado(de: id)
        // Cambiar la velocidad cambia cuánto ocupa el clip: el material es el mismo
        // y se estira o se encoge en la línea de tiempo. El grupo A/V conserva la
        // misma duración para que la voz no se despegue de la imagen.
        performEdit(keepPosition: true) {
            for objetivo in objetivos {
                guard let miembro = montaje.clip(objetivo) else { continue }
                let materialUsado = Double(miembro.duracion) * abs(miembro.velocidad)
                let nuevaDuracion = max(1, Int64((materialUsado / nueva).rounded()))
                guard let indice = montaje.indiceDeClip(objetivo) else { continue }
                montaje.pistas[indice.0].clips[indice.1].velocidad = nueva
                montaje.pistas[indice.0].clips[indice.1].duracion = nuevaDuracion
            }
        }
        status = String(format: "Velocidad %.0f %%", nueva * 100)
    }

    func fijarFundidos(_ id: UUID, entrada: Int64? = nil, salida: Int64? = nil) {
        modificarClip(id) { clip in
            if let entrada { clip.entradaFundido = max(0, min(entrada, clip.duracion)) }
            if let salida { clip.salidaFundido = max(0, min(salida, clip.duracion - clip.entradaFundido)) }
        }
    }

    func fijarTransicion(_ id: UUID, tipo: TipoDeTransicion = .disolucion, duracion: Int64? = nil) {
        guard let pistaID = montaje.pistaDe(clip: id),
              let indicePista = montaje.indiceDePista(pistaID),
              let indiceClip = montaje.pistas[indicePista].clips.firstIndex(where: { $0.id == id }) else { return }
        let clip = montaje.pistas[indicePista].clips[indiceClip]
        let cantidad = max(1, min(duracion ?? Int64(timebase.fps), clip.duracion / 2))
        let transicion = Transicion(tipo: tipo, duracion: cantidad)
        performEdit(keepPosition: true) {
            montaje.pistas[indicePista].clips[indiceClip].transicionEntrada = transicion
            montaje.pistas[indicePista].clips[indiceClip].transicionSalida = transicion
            if indiceClip > 0,
               montaje.pistas[indicePista].clips[indiceClip - 1].fin == clip.inicio {
                montaje.pistas[indicePista].clips[indiceClip - 1].transicionSalida = transicion
            }
            if indiceClip + 1 < montaje.pistas[indicePista].clips.count,
               clip.fin == montaje.pistas[indicePista].clips[indiceClip + 1].inicio {
                montaje.pistas[indicePista].clips[indiceClip + 1].transicionEntrada = transicion
            }
        }
        status = String(format: "Transición de %.1f s", timebase.segundos(cantidad))
    }

    func quitarTransiciones(_ id: UUID) {
        guard let pistaID = montaje.pistaDe(clip: id),
              let indicePista = montaje.indiceDePista(pistaID),
              let indiceClip = montaje.pistas[indicePista].clips.firstIndex(where: { $0.id == id }) else { return }
        let clip = montaje.pistas[indicePista].clips[indiceClip]
        performEdit(keepPosition: true) {
            montaje.pistas[indicePista].clips[indiceClip].transicionEntrada = nil
            montaje.pistas[indicePista].clips[indiceClip].transicionSalida = nil
            if indiceClip > 0,
               montaje.pistas[indicePista].clips[indiceClip - 1].fin == clip.inicio {
                montaje.pistas[indicePista].clips[indiceClip - 1].transicionSalida = nil
            }
            if indiceClip + 1 < montaje.pistas[indicePista].clips.count,
               clip.fin == montaje.pistas[indicePista].clips[indiceClip + 1].inicio {
                montaje.pistas[indicePista].clips[indiceClip + 1].transicionEntrada = nil
            }
        }
    }

    func alternarHabilitado(_ id: UUID) {
        modificarClip(id) { $0.habilitado.toggle() }
    }

    func etiquetar(_ id: UUID, _ etiqueta: EtiquetaDeColor) {
        modificarClip(id, recompilar: false) { $0.etiqueta = etiqueta }
        scheduleAutosave()
    }

    /// Aplica un mando de la rueda primaria al clip.
    ///
    /// El color se guarda en el clip y lo aplica `CompositorDeColor` en el mismo
    /// `videoComposition` que usa la reproducción y la exportación: el monitor
    /// enseña exactamente lo que saldrá en el archivo.
    func fijarColorDeClip(_ id: UUID, _ cambio: @escaping (inout ColorDeClip) -> Void) {
        modificarClip(id, recompilar: false) { clip in
            cambio(&clip.color)
        }
        scheduleAutosave()
        rebuildPreview(keepPosition: true)
    }

    func fijarModoDeFusion(_ id: UUID, _ modo: ModoDeFusion) {
        modificarClip(id) { $0.modoDeFusion = modo }
        status = "Modo de fusión: \(modo.nombre)"
    }

    /// Cambia la máscara del clip; con `nil` la quita.
    func fijarMascara(_ id: UUID, _ cambio: @escaping (inout MascaraDeClip?) -> Void) {
        modificarClip(id) { clip in
            cambio(&clip.mascara)
        }
        rebuildPreview(keepPosition: true)
    }

    /// Cambia el chroma key del clip; con `nil` lo quita.
    func fijarCroma(_ id: UUID, _ cambio: @escaping (inout ChromaKeyDeClip?) -> Void) {
        modificarClip(id) { clip in
            cambio(&clip.croma)
        }
        rebuildPreview(keepPosition: true)
    }

    /// Ajusta una rueda de color del clip (sombras, medios o altas, por canal).
    func fijarRueda(_ id: UUID, _ cambio: @escaping (inout RuedasDeColor) -> Void) {
        modificarClip(id) { clip in
            var ruedas = clip.color.ruedas ?? .neutras
            cambio(&ruedas)
            clip.color.ruedas = ruedas
        }
        rebuildPreview(keepPosition: true)
    }

    func fijarTransformacion(_ id: UUID, _ cambio: @escaping (inout TransformacionDeClip) -> Void) {
        guard let clip = montaje.clip(id) else { return }
        let relativo = cabezal - clip.inicio
        modificarClip(id) { clip in
            if let indice = clip.keyframes?.firstIndex(where: { $0.frame == relativo }) {
                cambio(&clip.keyframes![indice].transformacion)
            } else {
                cambio(&clip.transformacion)
            }
        }
    }

    /// Fundido de entrada y salida de un segundo, que es el gesto más repetido.
    func fundidoRapido(_ id: UUID) {
        let unSegundo = Int64(timebase.fpsNominal)
        fijarFundidos(id, entrada: unSegundo, salida: unSegundo)
        status = "Fundido de entrada y salida de 1 s"
    }

    // MARK: Pistas

    /// Altura actual de las pistas. Es propiedad del montaje, no preferencia de la
    /// aplicación: se guarda con el proyecto porque un montaje de ocho pistas y uno
    /// de dos no se ven cómodos con la misma altura.
    var alturaDePistas: Double { montaje.pistas.first?.altura ?? 58 }

    func fijarAlturaDePistas(_ alto: Double) {
        let acotada = min(max(alto, 28), 200)
        guard abs(acotada - alturaDePistas) > 0.5 else { return }
        performEdit(keepPosition: true) {
            for i in montaje.pistas.indices { montaje.pistas[i].altura = acotada }
        }
    }

    /// Ancho útil del lienzo del montaje, que informa la propia vista. Hace falta
    /// para poder ajustar el zoom a la ventana sin que el estado conozca la vista.
    var anchoVisibleDelMontaje: Double = 900

    /// Encaja el montaje entero en el ancho disponible.
    ///
    /// Es el atajo más usado de cualquier editor —⇧Z en Premiere y en Resolve— y el
    /// que más se echa de menos: sin él, situarse tras cada corte obliga a pelearse
    /// con el deslizador de zoom.
    func ajustarMontajeALaVentana() {
        let segundos = max(timebase.segundos(montaje.duracion), 1)
        // Un 4 % de aire a la derecha: pegar el último frame al borde hace pensar
        // que el montaje sigue más allá.
        let objetivo = (anchoVisibleDelMontaje * 0.96) / segundos
        timelineScale = min(max(objetivo, 0.5), 400)
    }

    func acercarMontaje(_ factor: Double) {
        timelineScale = min(max(timelineScale * factor, 0.5), 400)
    }

    func anadirPista(_ tipo: TipoDePista) {
        let cuantas = montaje.pistas.filter { $0.tipo == tipo }.count + 1
        let nombre = (tipo == .video ? "V" : "A") + String(cuantas)
        performEdit(keepPosition: true) {
            let nueva = Pista(tipo: tipo, nombre: nombre)
            if tipo == .video {
                montaje.pistas.insert(nueva, at: 0)
            } else {
                montaje.pistas.append(nueva)
            }
            pistaActiva = nueva.id
        }
    }

    /// Añade una pista de ajuste arriba del todo, con un clip que cubre el
    /// montaje: el color (y la LUT) que se le apliquen afectarán a todo lo que
    /// haya debajo, como el adjustment layer de cualquier NLE.
    func anadirPistaDeAjuste() {
        let cuantas = montaje.pistas.filter { $0.tipo == .video }.count + 1
        let duracion = max(montaje.duracion, Int64(timebase.fps * 2))
        performEdit(keepPosition: true) {
            var nueva = Pista(tipo: .video, nombre: "Ajuste")
            var clip = Clip(
                mediaID: UUID(), nombre: "Ajuste", inicio: 0,
                duracion: duracion, entradaEnOrigen: 0
            )
            clip.esAjuste = true
            nueva.clips.append(clip)
            montaje.pistas.insert(nueva, at: 0)
            pistaActiva = nueva.id
            selectedClipID = clip.id
        }
        status = "Pista de ajuste añadida: su color se aplica a lo que hay debajo"
    }

    func eliminarPista(_ id: UUID) {
        guard montaje.pistas.count > 1 else { return }
        performEdit(keepPosition: true) {
            montaje.pistas.removeAll { $0.id == id }
            if pistaActiva == id { pistaActiva = montaje.pistas.first?.id }
        }
    }

    func alternarPista(_ id: UUID, _ campo: CampoDePista) {
        guard let i = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            switch campo {
            case .silencio: montaje.pistas[i].silenciada.toggle()
            case .solo: montaje.pistas[i].solo.toggle()
            case .bloqueo: montaje.pistas[i].bloqueada.toggle()
            case .visible: montaje.pistas[i].visible.toggle()
            case .ducking: montaje.pistas[i].ducking = !montaje.pistas[i].duckingActivo
            }
        }
    }

    func fijarVolumenPista(_ id: UUID, _ dB: Double) {
        let mutar = {
            guard let indice = self.montaje.indiceDePista(id) else { return }
            self.montaje.pistas[indice].volumen = min(max(-60, dB), 12)
        }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
    }

    func fijarPaneoPista(_ id: UUID, _ paneo: Double) {
        let mutar = {
            guard let indice = self.montaje.indiceDePista(id) else { return }
            self.montaje.pistas[indice].paneo = min(max(-1, paneo), 1)
        }
        if trimSnapshot == nil { performEdit(keepPosition: true, mutar) } else { mutar() }
    }

    /// Elige qué pista dispara el ducking de otra (el lado del sidechain).
    func fijarFuenteDeDucking(_ id: UUID, _ fuenteID: UUID) {
        performEdit(keepPosition: true) {
            guard let indice = montaje.indiceDePista(id) else { return }
            montaje.pistas[indice].fuenteDeDucking = fuenteID
        }
        let nombre = montaje.pistasDeAudio.first { $0.id == fuenteID }?.nombre ?? "primera"
        status = "El ducking de esta pista lo dispara \(nombre)"
    }

    /// El nivel en vivo de una pista, en decibelios (0 = pico). `nil` si el
    /// tap aún no ha visto señal (pausa o pista sin audio).
    func nivelEnVivoDe(_ pistaID: UUID) -> Double? {
        guard let pico = MedidorEnVivo.compartido.picoDe(pistaID.uuidString), pico > 0 else { return nil }
        return 20 * log10(pico)
    }

    /// Alterna la altura de una pista de audio entre la normal y la expandida,
    /// para leer la forma de onda con más detalle.
    func alternarAlturaDePista(_ id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        let actual = montaje.pistas[indice].altura
        let nueva = actual > 58 ? 58.0 : 96.0
        montaje.pistas[indice].altura = nueva
        objectWillChange.send()
    }

    func fijarLimiter(_ limitador: LimitadorDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].limitador = limitador
        }
    }

    func fijarCompresor(_ compresor: CompresorDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].compresor = compresor
        }
    }

    func fijarEcualizacion(_ bandas: [BandaDeEQ]?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].ecualizacion = bandas
        }
    }

    func fijarPuertaDeRuido(_ puerta: PuertaDeRuidoDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].puertaDeRuido = puerta
        }
    }

    func fijarMultibanda(_ multibanda: CompresorMultibandaDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].multibanda = multibanda
        }
    }

    func fijarReverb(_ reverb: ReverbDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].reverb = reverb
        }
    }

    func fijarRetardo(_ retardo: RetardoDePista?, enPista id: UUID) {
        guard let indice = montaje.indiceDePista(id) else { return }
        performEdit(keepPosition: true) {
            montaje.pistas[indice].retardo = retardo
        }
    }

    // MARK: Marcadores

    func anadirMarcador(etiqueta: EtiquetaDeColor = .azul) {
        let frame = cabezal
        guard !montaje.marcadores.contains(where: { $0.frame == frame }) else { return }
        performEdit(keepPosition: true) {
            montaje.marcadores.append(
                Marcador(frame: frame, nombre: timebase.timecode(frame), etiqueta: etiqueta)
            )
        }
        status = "Marcador en \(timebase.timecode(frame))"
    }

    func eliminarMarcador(_ id: UUID) {
        performEdit(keepPosition: true) { montaje.marcadores.removeAll { $0.id == id } }
    }

    func renombrarMarcador(_ id: UUID, _ nombre: String) {
        guard let i = montaje.marcadores.firstIndex(where: { $0.id == id }) else { return }
        montaje.marcadores[i].nombre = nombre
        objectWillChange.send()
        scheduleAutosave()
    }

    func irAlMarcador(haciaDelante: Bool) {
        guard let marcador = montaje.marcador(desde: cabezal, haciaDelante: haciaDelante) else { return }
        seek(toFrame: marcador.frame)
    }

    func marcarEntradaTrabajo() {
        performEdit(keepPosition: true) {
            montaje.entradaDeTrabajo = cabezal
            if let salida = montaje.salidaDeTrabajo, salida <= cabezal { montaje.salidaDeTrabajo = nil }
        }
        status = "Entrada de trabajo en \(timecodeDelCabezal)"
    }

    func marcarSalidaTrabajo() {
        guard let entrada = montaje.entradaDeTrabajo, cabezal > entrada else {
            status = "Marca primero una entrada de trabajo"
            return
        }
        performEdit(keepPosition: true) { montaje.salidaDeTrabajo = cabezal }
        status = "Salida de trabajo en \(timecodeDelCabezal)"
    }

    func limpiarRangoTrabajo() {
        performEdit(keepPosition: true) {
            montaje.entradaDeTrabajo = nil
            montaje.salidaDeTrabajo = nil
        }
        status = "Rango de trabajo limpiado"
    }

    // MARK: Navegación

    func stepFrame(_ direction: Int) {
        seek(toFrame: cabezal + Int64(direction))
    }

    func irAlCorte(haciaDelante: Bool) {
        guard let corte = montaje.corte(desde: cabezal, haciaDelante: haciaDelante) else {
            seek(toFrame: haciaDelante ? montaje.duracion : 0)
            return
        }
        seek(toFrame: corte)
    }

    func seek(toFrame frame: Int64) {
        seek(to: timebase.segundos(max(0, min(frame, montaje.duracion))))
    }

    /// Selecciona el clip anterior o siguiente dentro de su misma pista.
    func seleccionarVecino(haciaDelante: Bool) {
        guard let id = selectedClipID else {
            selectedClipID = montaje.pista(pistaDeTrabajo)?.clips.first?.id
            return
        }
        if let vecino = clipVecino(de: id, haciaDelante: haciaDelante) {
            selectedClipID = vecino
            if let clip = montaje.clip(vecino) { seek(toFrame: clip.inicio) }
        }
    }

    private func clipVecino(de id: UUID, haciaDelante: Bool) -> UUID? {
        guard let pistaID = montaje.pistaDe(clip: id),
              let pista = montaje.pista(pistaID),
              let indice = pista.clips.firstIndex(where: { $0.id == id }) else { return nil }
        let destino = haciaDelante ? indice + 1 : indice - 1
        return pista.clips.indices.contains(destino) ? pista.clips[destino].id : nil
    }

    // MARK: Entrada y salida en el origen

    /// Marca de entrada y salida sobre el medio seleccionado, para el montaje de
    /// tres puntos: se elige el trozo en la biblioteca y se inserta en el cabezal.
    func marcarEntradaDeOrigen() {
        guard let id = selectedMediaID else { return }
        entradaDeOrigen[id] = max(0, min(cabezalDeOrigen, duracionDeOrigenEnFrames))
        objectWillChange.send()
        status = "Entrada de origen en \(timecodeDeOrigen)"
    }

    func marcarSalidaDeOrigen() {
        guard let id = selectedMediaID else { return }
        let salida = min(duracionDeOrigenEnFrames, max(cabezalDeOrigen, (entradaDeOrigen[id] ?? 0) + 1))
        salidaDeOrigen[id] = salida
        objectWillChange.send()
        status = "Salida de origen en \(timebase.timecode(salida))"
    }

    func limpiarEntradaYSalida() {
        guard let id = selectedMediaID else { return }
        entradaDeOrigen[id] = nil
        salidaDeOrigen[id] = nil
        seekOrigen(toFrame: 0)
        objectWillChange.send()
    }

    func rangoDeOrigen(_ id: UUID) -> (entrada: Int64, salida: Int64)? {
        guard entradaDeOrigen[id] != nil || salidaDeOrigen[id] != nil else { return nil }
        let item = media.first { $0.id == id }
        let limite = max(0, timebase.frames(segundos: item?.duration ?? 0))
        let entrada = min(max(entradaDeOrigen[id] ?? 0, 0), limite)
        let salida = min(max(salidaDeOrigen[id] ?? limite, entrada), limite)
        return (entrada, salida)
    }

    /// Crea un subclip del medio seleccionado con la entrada y salida marcadas
    /// en el monitor de origen: el recorte entra en la biblioteca como un medio
    /// más, con su propio nombre y duración, sin copiar el archivo.
    ///
    /// Es el «make subclip» de Premiere: marcar la pieza buena de una entrevista
    /// de dos horas y trabajar solo con ella, sin cargar el máster cada vez.
    func crearSubclip() {
        guard let baseID = selectedMediaID,
              let base = media.first(where: { $0.id == baseID }) else {
            status = "Selecciona un medio en el origen para crear un subclip"
            return
        }
        let limite = max(1, timebase.frames(segundos: base.duration))
        let entrada = min(max(entradaDeOrigen[baseID] ?? 0, 0), max(0, limite - 1))
        let salida = min(max(salidaDeOrigen[baseID] ?? limite, entrada + 1), limite)
        let origen = SubclipOrigen(medioBase: baseID, entrada: entrada, salida: salida)
        // El recorte mide la duración visible; el archivo es el mismo del base.
        let nombreBase = base.name
        let nuevo = MediaItem(
            id: UUID(),
            url: base.url,
            duration: timebase.segundos(origen.duracion),
            size: base.size,
            fileSize: base.fileSize,
            frameRate: base.frameRate,
            variableFrameRate: base.variableFrameRate,
            bin: base.bin,
            subclipDe: origen
        )
        media.append(nuevo)
        if let medio = medios[baseID] {
            // El mismo asset, pero el subclip solo enseña su rango: la duración
            // efectiva se acota en el montaje con `duracionDelMedio`.
            medios[nuevo.id] = medio
            mediosOriginales[nuevo.id] = medio
        }
        selectedMediaIDs = [nuevo.id]
        selectedMediaID = nuevo.id
        selectedBin = base.bin
        status = "Subclip «\(nombreBase)» creado · \(timebase.timecode(origen.entrada))→\(timebase.timecode(origen.salida))"
        scheduleAutosave()
    }

    func undo() {
        // Un deshacer inmediato sobre lo que acaba de proponer la IA es el «no»
        // más rotundo: se corrige el asiento de la bitácora antes de deshacer.
        if let pendiente = ultimaPropuestaIA,
           Date().timeIntervalSince(pendiente.momento) < Self.ventanaDeArrepentimiento {
            anotarPropuesta(pendiente.plan, peticion: pendiente.peticion, desenlace: "DESHECHO")
        }
        ultimaPropuestaIA = nil
        guard let previous = undoStack.popLast() else { return }
        redoStack.append(snapshot())
        restore(previous)
    }

    func redo() {
        guard let next = redoStack.popLast() else { return }
        undoStack.append(snapshot())
        restore(next)
    }

    func togglePlayback() {
        if player.rate == 0 {
            if playhead >= max(0, timelineDuration - 0.05) { seek(to: 0) }
            reproducir(a: 1)
        } else {
            reproducir(a: 0)
        }
    }

    func reproducir(a velocidad: Float) {
        guard player.currentItem != nil else { return }
        if velocidad == 0 {
            player.pause()
        } else {
            player.play()
            player.rate = velocidad
        }
        playbackRate = player.rate
        isPlaying = player.rate != 0
        reproducirAngulosDelVisor(rate: player.rate)
    }

    func teclaJ() { reproducir(a: playbackRate <= -1 ? -2 : -1) }
    func teclaK() { reproducir(a: 0) }
    func teclaL() { reproducir(a: playbackRate >= 1 ? 2 : 1) }

    @discardableResult
    private func procesarTeclaDeEditor(_ evento: NSEvent) -> Bool {
        guard timelineHasFocus,
              let ventana = NSApp.keyWindow,
              !(ventana.firstResponder is NSTextField),
              !(ventana.firstResponder is NSTextView) else { return false }
        let modificadores = evento.modifierFlags.intersection([.command, .option, .control, .function])
        guard modificadores.isEmpty, !evento.modifierFlags.contains(.shift) else { return false }

        if evento.keyCode == 49 {
            togglePlayback()
            return true
        }
        guard let tecla = evento.charactersIgnoringModifiers?.lowercased() else { return false }
        switch tecla {
        case "j": teclaJ()
        case "k": teclaK()
        case "l": teclaL()
        case "i": monitorActual == .origen ? marcarEntradaDeOrigen() : marcarEntradaTrabajo()
        case "o": monitorActual == .origen ? marcarSalidaDeOrigen() : marcarSalidaTrabajo()
        case "m": anadirMarcador()
        case "s": imanActivo.toggle()
        case "v": herramienta = .seleccion
        case "c": herramienta = .cuchilla
        case "b": herramienta = .ripple
        case "n": herramienta = .roll
        case "y": herramienta = .slip
        case "u": herramienta = .slide
        case "h": herramienta = .mano
        case ",": insertarEnCabezal()
        case ".": sobrescribirEnCabezal()
        default: return false
        }
        return true
    }

    func seek(to seconds: Double) {
        let target = min(max(0, seconds), timelineDuration)
        player.seek(to: CMTime(seconds: target, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero)
        playhead = target
        refrescarFormaDeOnda()
        sincronizarAngulosDelVisor()
    }

    /// Rehace la vista previa desde el montaje.
    ///
    /// Es síncrono a propósito: los medios ya están resueltos, así que construir la
    /// composición son unas operaciones sobre estructuras en memoria. Hacerlo en una
    /// tarea asíncrona obligaría a controlar carreras entre ediciones seguidas —el
    /// contador de generación que hacía falta antes— sin ganar nada.
    func rebuildPreview(keepPosition: Bool) {
        let previous = keepPosition ? min(playhead, timelineDuration) : 0
        previewGeneration += 1
        player.pause()
        if let visor = visorMulticam() {
            if grupoDelVisor != visor.grupo.id {
                liberarVisorMultiAngulo()
                grupoDelVisor = visor.grupo.id
            } else {
                sincronizarAngulosDelVisor()
            }
        } else {
            liberarVisorMultiAngulo()
            grupoDelVisor = nil
        }

        let render = ConstructorDeMontaje.construir(
            montaje, medios: medios, reutilizando: ultimoRender
        )
        guard !render.estaVacio else {
            player.replaceCurrentItem(with: nil)
            ultimoRender = nil
            status = "Timeline vacío"
            return
        }
        let item = AVPlayerItem(asset: render.composicion)
        item.videoComposition = render.composicionDeVideo
        item.audioMix = render.mezclaDeAudio
        player.replaceCurrentItem(with: item)
        ultimoRender = render
        seek(to: previous)

        let resolucion = "\(Int(render.tamano.width))×\(Int(render.tamano.height))"
        if let aviso = render.avisos.first {
            status = "Aviso: \(aviso.mensaje)"
        } else {
            status = "\(timebase.timecode(montaje.duracion)) · \(resolucion) · \(timebase.nombre)"
        }
    }

    func exportMovie() {
        guard montaje.duracion > 0 else { status = "Añade al menos un clip al timeline"; return }
        let alerta = NSAlert()
        alerta.messageText = "Formato de exportación"
        alerta.informativeText = "Elige el destino de este trabajo. Se puede seguir editando mientras la cola exporta."
        alerta.alertStyle = .informational
        alerta.addButton(withTitle: PresetExportacion.mp4.nombre)
        alerta.addButton(withTitle: PresetExportacion.hevc.nombre)
        alerta.addButton(withTitle: PresetExportacion.vertical.nombre)
        alerta.addButton(withTitle: PresetExportacion.prores.nombre)
        alerta.addButton(withTitle: PresetExportacion.audio.nombre)
        alerta.addButton(withTitle: PresetExportacion.master.nombre)
        alerta.addButton(withTitle: "Cancelar")

        // La normalización se elige aquí, antes de medir, porque medir un
        // montaje largo lleva su tiempo y no tiene sentido pedir la medición
        // para que luego se descarte en la misma ventana.
        let etiqueta = NSTextField(labelWithString: "Normalizar audio:")
        let selector = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 240, height: 26), pullsDown: false)
        selector.addItems(withTitles: ObjetivoDeSonoridad.allCases.map(\.nombre))
        selector.selectItem(at: max(0, ObjetivoDeSonoridad.allCases.firstIndex(of: normalizacionDeExportacion) ?? 0))
        let fila = NSStackView(views: [etiqueta, selector])
        fila.orientation = .horizontal
        fila.spacing = 8
        let envoltorio = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 30))
        fila.frame = envoltorio.bounds
        envoltorio.addSubview(fila)
        alerta.accessoryView = envoltorio

        let respuesta = alerta.runModal()
        let cancelacion = NSApplication.ModalResponse(rawValue: NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + 6)
        guard respuesta != cancelacion else { return }
        let presets = PresetExportacion.allCases
        let indice = max(0, min(presets.count - 1, respuesta.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue))
        normalizacionDeExportacion = ObjetivoDeSonoridad.allCases[selector.indexOfSelectedItem]
        exportMovie(preset: presets[indice])
    }

    func exportMovie(preset: PresetExportacion) {
        guard montaje.duracion > 0 else { status = "Añade al menos un clip al timeline"; return }
        let panel = NSSavePanel()
        panel.title = "Exportar película"
        panel.prompt = "Exportar"
        panel.nameFieldStringValue = "Editorcito Export.\(preset.extensionDeArchivo)"
        panel.allowedContentTypes = [preset.esSoloAudio ? (UTType(filenameExtension: "m4a") ?? .audio) : (preset == .master ? .quickTimeMovie : .mpeg4Movie)]
        guard panel.runModal() == .OK, let outputURL = panel.url else { return }

        let objetivo = normalizacionDeExportacion
        guard objetivo != .ninguno else {
            encolar(preset: preset, url: outputURL, ganancia: nil)
            return
        }

        // Medir el montaje real cuesta una pasada de audio; se hace en segundo
        // plano y después se enseña el resumen honesto del plan antes de que
        // nada toque la cola.
        status = "Midiendo sonoridad…"
        Task {
            do {
                let render = ConstructorDeMontaje.construir(
                    montaje,
                    medios: mediosOriginales.isEmpty ? medios : mediosOriginales,
                    tamanoDeSalida: preset.tamano
                )
                let rango: CMTimeRange?
                if let entrada = montaje.entradaDeTrabajo, let salida = montaje.salidaDeTrabajo, salida > entrada {
                    rango = CMTimeRange(start: timebase.tiempo(entrada), duration: timebase.tiempo(salida - entrada))
                } else {
                    rango = nil
                }
                let composicion = render.composicion
                let mezcla = render.mezclaDeAudio
                let medida = try await enSegundoPlano {
                    try SonoridadMedia.medir(composicion: composicion, mezcla: mezcla, timeRange: rango)
                }
                presentarPlan(objetivo.plan(para: medida), preset: preset, url: outputURL)
            } catch {
                status = "No se pudo medir el audio (\(error.localizedDescription)); se exporta sin tocar"
                encolar(preset: preset, url: outputURL, ganancia: nil)
            }
        }
    }

    /// Enseña el resumen del plan de normalización y deja decidir al usuario.
    ///
    /// La decisión es del usuario porque el plan a veces no llega al objetivo:
    /// cuando el techo de pico lo impide, `PlanDeNormalizacion.resumen` lo dice
    /// y queda exportar con lo que hay o sin tocar nada.
    private func presentarPlan(_ plan: PlanDeNormalizacion?, preset: PresetExportacion, url: URL) {
        guard let plan else {
            status = "El montaje no tiene audio medible; se exporta sin tocar"
            encolar(preset: preset, url: url, ganancia: nil)
            return
        }
        let alerta = NSAlert()
        alerta.messageText = "Sonoridad medida"
        alerta.informativeText = plan.resumen
        alerta.alertStyle = .informational
        if plan.ganancia != 0 {
            alerta.addButton(withTitle: "Normalizar y exportar")
            alerta.addButton(withTitle: "Exportar tal cual")
            let respuesta = alerta.runModal()
            let normalizar = respuesta == .alertFirstButtonReturn
            status = normalizar
                ? "Normalizando \(String(format: "%+.1f dB", plan.ganancia)) al exportar"
                : "Exportando sin normalizar"
            encolar(preset: preset, url: url, ganancia: normalizar ? plan.ganancia : nil)
        } else {
            alerta.addButton(withTitle: "Exportar")
            _ = alerta.runModal()
            encolar(preset: preset, url: url, ganancia: nil)
        }
    }

    private func encolar(preset: PresetExportacion, url: URL, ganancia: Double?) {
        exportQueue.append(TrabajoDeExportacion(preset: preset, url: url, ganancia: ganancia))
        exportQueueCount = exportQueue.count
        if ganancia == nil { status = "Trabajo añadido a la cola · \(preset.nombre)" }
        procesarSiguienteExportacion()
    }

    private func procesarSiguienteExportacion() {
        guard !isExporting, let trabajo = exportQueue.first else {
            exportQueueCount = exportQueue.count
            return
        }
        exportQueue.removeFirst()
        exportQueueCount = exportQueue.count
        isExporting = true
        exportProgress = 0
        status = "Preparando exportación…"

        Task {
            let temporal = EscrituraAtomica.temporal(para: trabajo.url)
            try? FileManager.default.removeItem(at: temporal)
            defer {
                exportTimer?.invalidate()
                exportTimer = nil
                activeExportSession = nil
                try? FileManager.default.removeItem(at: temporal)
                isExporting = false
                procesarSiguienteExportacion()
            }
            do {
                // La ganancia del máster va doblada en el volumen de las pistas
                // de audio de una copia del montaje: el constructor la convierte
                // en rampas como a cualquier otro ajuste de mezcla, y el montaje
                // de trabajo no se toca.
                var paraExportar = montaje
                if let ganancia = trabajo.ganancia, ganancia != 0 {
                    for i in paraExportar.pistas.indices where paraExportar.pistas[i].tipo == .audio {
                        paraExportar.pistas[i].volumen = (paraExportar.pistas[i].volumen ?? 0) + ganancia
                    }
                }
                let render = ConstructorDeMontaje.construir(
                    paraExportar,
                    medios: mediosOriginales.isEmpty ? medios : mediosOriginales,
                    tamanoDeSalida: trabajo.preset.tamano
                )
                // Los avisos críticos cambian el resultado respecto al montaje: un
                // medio offline desaparece del render y el archivo saldría con
                // huecos que nadie anunció. La decisión es del usuario, como con
                // la normalización.
                let criticos = render.avisos.filter(\.critico)
                if !criticos.isEmpty {
                    let alerta = NSAlert()
                    alerta.messageText = "La exportación saldrá incompleta"
                    alerta.informativeText = "El montaje tiene problemas que cambiarán el resultado:\n\n"
                        + criticos.map(\.mensaje).joined(separator: "\n")
                        + "\n\n¿Exportar de todas formas?"
                    alerta.addButton(withTitle: "Exportar de todas formas")
                    alerta.addButton(withTitle: "Cancelar este trabajo")
                    guard alerta.runModal() == .alertFirstButtonReturn else {
                        status = "Trabajo cancelado: no se exportó «\(trabajo.url.lastPathComponent)»"
                        exportProgress = 0
                        return
                    }
                }
                guard let session = AVAssetExportSession(asset: render.composicion, presetName: trabajo.preset.presetAV) else {
                    throw EditorError.exportUnavailable
                }
                session.outputURL = temporal
                session.outputFileType = trabajo.preset.tipoDeArchivo
                if let entrada = montaje.entradaDeTrabajo, let salida = montaje.salidaDeTrabajo, salida > entrada {
                    session.timeRange = CMTimeRange(
                        start: timebase.tiempo(entrada),
                        duration: timebase.tiempo(salida - entrada)
                    )
                }
                // Sin esto la exportación ignora capas, opacidad, encuadre y mezcla:
                // saldría solo la pista de abajo y a volumen plano.
                session.videoComposition = trabajo.preset.esSoloAudio ? nil : render.composicionDeVideo
                session.audioMix = render.mezclaDeAudio
                session.shouldOptimizeForNetworkUse = true
                activeExportSession = session
                exportTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
                    Task { @MainActor in self?.exportProgress = Double(self?.activeExportSession?.progress ?? 0) }
                }
                await session.export()
                if session.status == .completed {
                    try EscrituraAtomica.instalar(temporal, en: trabajo.url)
                    exportProgress = 1
                    let avisosDeAjuste = render.avisos.filter { !$0.critico }
                    status = avisosDeAjuste.isEmpty
                        ? "Exportado: \(trabajo.url.lastPathComponent)"
                        : "Exportado: \(trabajo.url.lastPathComponent) · \(avisosDeAjuste.count) \(avisosDeAjuste.count == 1 ? "aviso de ajuste" : "avisos de ajuste") (metraje recortado)"
                    NSWorkspace.shared.activateFileViewerSelecting([trabajo.url])
                } else if session.status == .cancelled {
                    status = "Exportación cancelada: se conservó el archivo anterior"
                    exportProgress = 0
                } else {
                    throw session.error ?? EditorError.exportFailed
                }
            } catch {
                status = "Error al exportar: \(error.localizedDescription)"
            }
        }
    }

    /// Cancela solo el trabajo que está procesándose. Lo que ya existe en el
    /// destino permanece intacto y los trabajos siguientes siguen en la cola.
    func cancelarExportacion() {
        guard isExporting else { return }
        activeExportSession?.cancelExport()
        status = "Cancelando exportación…"
    }

    func editWithAI() {
        let peticion = aiRequest.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !peticion.isEmpty, !aiWorking, montaje.duracion > 0 else { return }
        aiWorking = true
        aiResult = "Planificando…"
        aiTask?.cancel()
        aiGeneracion += 1
        let generacion = aiGeneracion
        let revisionPedida = documentRevision
        let listado = clipsNumerados()
        let contexto = contextoParaIA(listado)
        let seleccion = aiSettings.selection

        aiTask = Task {
            do {
                // La miniatura solo sale del Mac cuando el proveedor es local. El
                // proveedor remoto recibe el contexto textual del montaje, nunca
                // un frame del vídeo, aunque el usuario tenga un frame visible.
                let miniatura = seleccion.provider == .local
                    ? await miniaturaDelCabezal()
                    : nil
                if seleccion.provider != .local {
                    aiResult = "OpenCode Go recibe solo el contexto textual; el frame se queda en este Mac."
                }
                try Task.checkCancellation()
                let plan: NovaEditPlan
                if let miniatura {
                    plan = try await NovaAssistant.planConImagen(
                        request: peticion, timeline: contexto, selection: seleccion, imagenJpeg: miniatura
                    )
                } else {
                    plan = try await NovaAssistant.plan(request: peticion, timeline: contexto, selection: seleccion)
                }
                try Task.checkCancellation()
                guard documentRevision == revisionPedida else {
                    aiResult = "El montaje cambió mientras respondía la IA. No se aplicó nada."
                    return
                }

                let confirmacion = NSAlert()
                confirmacion.messageText = "¿Aplicar esta edición?"
                confirmacion.informativeText = "\(plan.summary)\n\n"
                    + previsualizar(plan, listado: listado)
                    + "\n\nSe deshace de una vez con ⌘Z."
                confirmacion.alertStyle = .informational
                confirmacion.addButton(withTitle: "Aplicar")
                confirmacion.addButton(withTitle: "Cancelar")
                guard confirmacion.runModal() == .alertFirstButtonReturn else {
                    aiResult = "Propuesta descartada; el montaje no cambió."
                    anotarPropuesta(plan, peticion: peticion, desenlace: "DESCARTADO")
                    return
                }
                // El usuario pudo cancelar mientras leía el aviso de confirmación:
                // sin esta comprobación, la propuesta se aplicaría igualmente.
                try Task.checkCancellation()

                var aplicadas = 0
                performEdit(keepPosition: true) {
                    for accion in plan.actions.prefix(40) where ejecutar(accion, listado: listado) {
                        aplicadas += 1
                    }
                }
                aiRequest = ""
                aiResult = "\(plan.summary) · \(aplicadas) de \(plan.actions.count) aplicadas"
                // Queda pendiente de arrepentimiento: si el siguiente deshacer llega
                // enseguida, el desenlace se corrige a DESHECHO.
                ultimaPropuestaIA = (plan, peticion, Date())
                anotarPropuesta(plan, peticion: peticion, desenlace: "APLICADO")
            } catch is CancellationError {
                aiResult = "Edición cancelada; el montaje quedó intacto."
            } catch {
                aiResult = error.localizedDescription
            }
            // Una petición vieja cancelada no pisa el estado de una nueva.
            if aiGeneracion == generacion {
                aiWorking = false
            }
        }
    }

    // MARK: Bitácora de propuestas

    /// La última propuesta aplicada, viva solo hasta que se sabe si se deshace.
    private var ultimaPropuestaIA: (plan: NovaEditPlan, peticion: String, momento: Date)?

    /// Ventana de arrepentimiento: un deshacer inmediato es el «no» más rotundo.
    private static let ventanaDeArrepentimiento: TimeInterval = 45

    /// Dónde vive la bitácora: junto a los ajustes, en Application Support.
    private static var rutaDeLaBitacora: String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Editorcito", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.appendingPathComponent("propuestas.jsonl").path
    }

    /// Registra qué propuso la IA y qué hizo la persona con ello.
    ///
    /// Es el único dato de calidad que no se puede fabricar sintéticamente ni
    /// reconstruir después: sin él, cualquier afinado de prompts sería adivinar.
    /// Que falle no puede romper nada.
    private func anotarPropuesta(_ plan: NovaEditPlan, peticion: String, desenlace: String) {
        let registro: [String: Any] = [
            "momento": Int64(Date().timeIntervalSince1970),
            "peticion": peticion,
            "resumen": plan.summary,
            "acciones": plan.actions.map { accion in
                [
                    "kind": accion.kind,
                    "clip": accion.clip ?? 0,
                    "entrada": accion.entrada ?? "",
                    "salida": accion.salida ?? "",
                    "destino": accion.destino ?? "",
                    "valor": accion.valor ?? 0,
                    "texto": accion.texto ?? "",
                ]
            },
            "desenlace": desenlace,
        ]
        guard let datos = try? JSONSerialization.data(withJSONObject: registro),
              let linea = String(data: datos, encoding: .utf8) else { return }
        if let manejador = FileHandle(forWritingAtPath: Self.rutaDeLaBitacora) {
            manejador.seekToEndOfFile()
            manejador.write((linea + "\n").data(using: .utf8)!)
            try? manejador.close()
        } else {
            try? (linea + "\n").write(toFile: Self.rutaDeLaBitacora, atomically: true, encoding: .utf8)
        }
    }

    /// Captura el frame del cabezal como JPEG reducido a 1024 px.
    ///
    /// Es el camino ya resuelto en Yunkil: reducir antes de codificar. Doce
    /// megapíxeles en base64 son millones de caracteres y el modelo local tiene
    /// 16K de contexto; sin reducir no falla «lento», falla con un error de
    /// servidor que no menciona la imagen. El frame sale del último render
    /// (la composición del reproductor), no de un `AVURLAsset` que no existe.
    private func miniaturaDelCabezal() async -> Data? {
        guard let render = ultimoRender else { return nil }
        let segundo = max(0, min(playhead, render.composicion.duration.seconds - 0.001))
        guard let cg = await CapturadorDeFrames.capturarFrame(
            de: render.composicion, videoComposition: render.composicionDeVideo,
            en: segundo, maximo: CGSize(width: 1024, height: 1024)
        ) else { return nil }

        let bitmap = NSBitmapImageRep(cgImage: cg)
        return bitmap.representation(using: .jpeg, properties: [.compressionFactor: 0.7])
    }

    /// Los clips en el orden en el que se le enseñan al modelo.
    private func clipsNumerados() -> [Clip] {
        montaje.pistas.flatMap { pista in pista.clips.map { $0 } }
            .sorted { $0.inicio < $1.inicio }
    }

    /// El montaje descrito en timecode, que es el idioma en el que se habla de
    /// edición. Un modelo razona mucho mejor sobre `00:01:12:04` que sobre 72,16 s.
    private func contextoParaIA(_ listado: [Clip]) -> String {
        var salida = "Base de tiempo: \(timebase.nombre). Duración total: \(timebase.timecode(montaje.duracion)).\n"
        salida += "Cabezal en \(timecodeDelCabezal).\n\n"
        for (indice, clip) in listado.enumerated() {
            let pista = montaje.pistaDe(clip: clip.id).flatMap { montaje.pista($0)?.nombre } ?? "?"
            let nombre = mediaItem(for: clip)?.name ?? clip.nombre
            salida += "\(indice + 1). [\(pista)] \(nombre) · "
            salida += "\(timebase.timecode(clip.inicio)) → \(timebase.timecode(clip.fin)) "
            salida += "(dura \(timebase.timecode(clip.duracion)))"
            if clip.ganancia != 0 { salida += String(format: " · %+.0f dB", clip.ganancia) }
            if clip.velocidad != 1 { salida += String(format: " · %.0f %%", clip.velocidad * 100) }
            if !clip.habilitado { salida += " · desactivado" }
            salida += "\n"
        }
        if !montaje.marcadores.isEmpty {
            salida += "\nMarcadores: " + montaje.marcadores
                .sorted { $0.frame < $1.frame }
                .map { "\(timebase.timecode($0.frame)) \($0.nombre)" }
                .joined(separator: ", ")
        }
        // El transcript es lo que convierte «quita donde se equivoca al
        // presentarse» en una orden ejecutable: el modelo ve lo que se dice y
        // cuándo, y con qué clip, sin adivinar.
        let palabras = montaje.palabrasDelMontaje()
        if !palabras.isEmpty {
            salida += "\n\nLo que se dice (transcript del montaje):\n"
            var porLinea = ""
            var desdeLinea = palabras[0].desde
            var clipDeLaLinea = 0
            for palabra in palabras {
                let numero = (listado.firstIndex { $0.id == palabra.clipID }).map { $0 + 1 } ?? 0
                let nueva = porLinea.isEmpty ? palabra.texto : porLinea + " " + palabra.texto
                if nueva.count > 90 {
                    salida += "\(timebase.timecode(desdeLinea)): [clip \(clipDeLaLinea)] \(porLinea)\n"
                    porLinea = palabra.texto
                    desdeLinea = palabra.desde
                    clipDeLaLinea = numero
                } else {
                    porLinea = nueva
                    if clipDeLaLinea == 0 { clipDeLaLinea = numero }
                }
            }
            if !porLinea.isEmpty {
                salida += "\(timebase.timecode(desdeLinea)): [clip \(clipDeLaLinea)] \(porLinea)\n"
            }
            salida += "\nEl timecode que se oye es el del montaje. Para quitar un " +
                "tramo donde se dice algo, usa «recortar» con la entrada y la salida " +
                "de ese tramo en el clip indicado, o «quitar» el clip entero."
        }
        return salida
    }

    private func previsualizar(_ plan: NovaEditPlan, listado: [Clip]) -> String {
        plan.actions.prefix(8).map { accion in
            let nombre = accion.clip.flatMap { listado[safe: $0 - 1] }
                .flatMap { mediaItem(for: $0)?.name } ?? "—"
            return "· \(accion.kind) \(nombre)"
        }.joined(separator: "\n")
    }

    /// Traduce lo que escribe el modelo a frames del proyecto.
    ///
    /// Acepta timecode y también segundos sueltos, porque un modelo mezcla las dos
    /// notaciones aunque se le pida una. Rechazar por eso sería tirar un plan bueno.
    private func aFrames(_ texto: String?) -> Int64? {
        guard let texto = texto?.trimmingCharacters(in: .whitespaces), !texto.isEmpty else { return nil }
        if texto.contains(":") || texto.contains(";") { return timebase.frames(timecode: texto) }
        guard let segundos = Double(texto.replacingOccurrences(of: ",", with: ".")) else { return nil }
        return timebase.frames(segundos: segundos)
    }

    @discardableResult
    private func ejecutar(_ accion: NovaEditAction, listado: [Clip]) -> Bool {
        // El marcador no necesita clip; el resto sí.
        if accion.kind == "marcador" {
            guard let frame = aFrames(accion.destino) else { return false }
            montaje.marcadores.append(
                Marcador(frame: frame, nombre: accion.texto ?? timebase.timecode(frame))
            )
            return true
        }

        guard let numero = accion.clip, let referencia = listado[safe: numero - 1],
              let actual = montaje.clip(referencia.id) else { return false }
        let id = actual.id
        let limite = duracionDelMedio(deClip: actual)

        switch accion.kind {
        case "recortar":
            var cambio = false
            if let entrada = aFrames(accion.entrada) {
                cambio = montaje.recortar(id, borde: .entrada, delta: entrada - actual.inicio,
                                          modo: .normal, duracionDelMedio: limite) != 0 || cambio
            }
            if let salida = aFrames(accion.salida), let refrescado = montaje.clip(id) {
                cambio = montaje.recortar(id, borde: .salida, delta: salida - refrescado.fin,
                                          modo: .normal, duracionDelMedio: limite) != 0 || cambio
            }
            return cambio

        case "mover":
            guard let destino = aFrames(accion.destino),
                  let pista = montaje.pistaDe(clip: id) else { return false }
            montaje.mover(id, aPista: pista, en: destino)
            return true

        case "quitar":
            montaje.levantar(id)
            return true

        case "quitar_cerrando":
            montaje.borrarConArrastre(id)
            return true

        case "cortar":
            guard let destino = aFrames(accion.destino),
                  let pista = montaje.pistaDe(clip: id) else { return false }
            return !montaje.partir(en: destino, pistas: [pista]).isEmpty

        case "silenciar":
            aplicarAlClip(id) { $0.ganancia = -60 }
            return true

        case "ganancia":
            guard let valor = accion.valor else { return false }
            aplicarAlClip(id) { $0.ganancia = min(max(-60, valor), 12) }
            return true

        case "fundido":
            let frames = Int64(((accion.valor ?? 1) * timebase.fps).rounded())
            aplicarAlClip(id) {
                $0.entradaFundido = max(0, min(frames, $0.duracion / 2))
                $0.salidaFundido = max(0, min(frames, $0.duracion / 2))
            }
            return true

        case "velocidad":
            guard let valor = accion.valor, valor > 0 else { return false }
            let nueva = min(max(0.1, valor), 10)
            let material = Double(actual.duracion) * abs(actual.velocidad)
            aplicarAlClip(id) {
                $0.velocidad = nueva
                $0.duracion = max(1, Int64((material / nueva).rounded()))
            }
            return true

        case "etiquetar":
            guard let nombre = accion.texto?.lowercased(),
                  let etiqueta = EtiquetaDeColor(rawValue: nombre) else { return false }
            aplicarAlClip(id) { $0.etiqueta = etiqueta }
            return true

        default:
            return false
        }
    }

    private func aplicarAlClip(_ id: UUID, _ cambio: (inout Clip) -> Void) {
        guard let pistaID = montaje.pistaDe(clip: id),
              let p = montaje.indiceDePista(pistaID),
              let c = montaje.pistas[p].clips.firstIndex(where: { $0.id == id }) else { return }
        cambio(&montaje.pistas[p].clips[c])
    }

    func cancelAI() {
        aiTask?.cancel()
        // La UI se despega al momento: el catch de la tarea ya no debe dejar el
        // botón clavado en «Planificando…» hasta que el servidor responda.
        guard aiWorking else { return }
        aiWorking = false
        aiResult = "Edición cancelada; el montaje quedó intacto."
    }

    private func snapshot() -> EditSnapshot {
        EditSnapshot(media: media, montaje: montaje, selectedMediaID: selectedMediaID, selectedClipID: selectedClipID)
    }

    private func performEdit(keepPosition: Bool = false, _ mutation: () -> Void) {
        let before = snapshot()
        mutation()
        commit(before: before)
        rebuildPreview(keepPosition: keepPosition)
    }

    private func commit(before: EditSnapshot) {
        guard before.media != media || before.montaje != montaje else { return }
        selectedClipIDs = Set(selectedClipIDs.filter { montaje.clip($0) != nil })
        isDirty = true
        // La entrada de la historia se describe comparando los dos montajes.
        var entrada = before
        entrada.descripcion = LineaDeTiempo.describirCambio(antes: before.montaje, despues: montaje)
        undoStack.append(entrada)
        if undoStack.count > 100 { undoStack.removeFirst(undoStack.count - 100) }
        redoStack.removeAll()
        documentRevision += 1
        updateHistoryState()
        scheduleAutosave()
    }

    private func restore(_ value: EditSnapshot) {
        media = value.media
        montaje = value.montaje
        selectedMediaID = value.selectedMediaID
        selectedMediaIDs = value.selectedMediaID.map { Set<UUID>([$0]) } ?? Set<UUID>()
        selectedClipID = value.selectedClipID
        selectedClipIDs = value.selectedClipID.map { [$0] } ?? []
        documentRevision += 1
        updateHistoryState()
        scheduleAutosave()
        rebuildPreview(keepPosition: false)
    }

    /// El panel de historia de deshacer: las ediciones pasadas, de la más
    /// reciente a la más antigua, con su descripción. Se deriva de la pila de
    /// deshacer para no poder desincronizarse de ella.
    var historiaDeEdicion: [String] {
        undoStack.reversed().map(\.descripcion)
    }

    /// Deshace hasta dejar la historia en la posición pedida (1 = la última
    /// edición, N = N ediciones atrás). El panel de historia lo usa para saltar
    /// a un punto concreto en lugar de pulsar ⌘Z muchas veces.
    func deshacerHasta(_ posicion: Int) {
        let pasos = max(0, min(posicion, undoStack.count))
        guard pasos > 0 else { return }
        for _ in 0..<pasos { undo() }
    }

    private func updateHistoryState() {
        canUndo = !undoStack.isEmpty
        canRedo = !redoStack.isEmpty
    }

    private var autosaveURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Editorcito/Autosave/recovery.editorcito")
    }

    /// Elimina la recuperación automática. Lo llama el cierre de ventana cuando
    /// el usuario decide no guardar: la decisión se respeta.
    func descartarRecuperacion() {
        try? FileManager.default.removeItem(at: autosaveURL)
        proyectoPendienteDeRecuperacion = nil
        recuperacionPendiente = nil
    }

    /// Recupera el montaje solo después de que el usuario lo haya confirmado en
    /// la interfaz. Nunca se sustituye el documento actual de forma silenciosa.
    func recuperarRecuperacion() {
        guard let proyecto = proyectoPendienteDeRecuperacion else { return }
        proyectoPendienteDeRecuperacion = nil
        recuperacionPendiente = nil
        Task { await instalarProyecto(proyecto, carpeta: nil, nombre: proyecto.nombre ?? "Recuperado", recuperado: true) }
    }

    private func scheduleAutosave() {
        isDirty = true
        autosaveTask?.cancel()
        let project = projectFile()
        let url = autosaveURL
        autosaveTask = Task {
            try? await Task.sleep(for: .seconds(1.2))
            guard !Task.isCancelled else { return }
            do {
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try JSONEncoder().encode(project).write(to: url, options: .atomic)
            } catch {
                status = "No se pudo crear la recuperación automática"
            }
        }
    }

    private func recoverAutosave() {
        guard let datos = try? Data(contentsOf: autosaveURL),
              let proyecto = try? ProyectoEditorcito.leer(datos),
              proyecto.montaje.duracion > 0 else { return }
        proyectoPendienteDeRecuperacion = proyecto
        let atributos = try? FileManager.default.attributesOfItem(atPath: autosaveURL.path)
        let fecha = atributos?[.modificationDate] as? Date
        recuperacionPendiente = RecuperacionPendiente(
            nombre: proyecto.nombre ?? "Sin título",
            fecha: fecha
        )
    }

    private func projectFile(carpeta: URL? = nil) -> ProyectoEditorcito {
        let base = carpeta ?? carpetaDelProyecto
        return ProyectoEditorcito(
            version: 2,
            nombre: projectName,
            medios: media.map {
                .init(
                    id: $0.id, ruta: $0.url.path,
                    rutaRelativa: ProyectoEditorcito.rutaRelativa(de: $0.url, respectoA: base),
                    duracion: $0.duration, ancho: $0.size.width, alto: $0.size.height,
                    bytes: $0.fileSize, fps: $0.frameRate, vfr: $0.variableFrameRate,
                    nombre: $0.url.lastPathComponent,
                    bin: $0.bin,
                    subclip: $0.subclipDe
                )
            },
            montaje: montaje
        )
    }
}

private struct EditSnapshot {
    let media: [MediaItem]
    let montaje: LineaDeTiempo
    let selectedMediaID: UUID?
    let selectedClipID: UUID?
    /// Descripción del cambio que representa, para el panel de historia.
    var descripcion: String = ""
}

enum EditorError: LocalizedError {
    case exportUnavailable
    case exportFailed
    case invalidProject
    case missingMedia(Int)

    var errorDescription: String? {
        switch self {
        case .exportUnavailable: "El sistema no ofrece un exportador compatible."
        case .exportFailed: "La exportación no pudo completarse."
        case .invalidProject: "El proyecto está dañado o usa una versión incompatible. El montaje actual no se modificó."
        case .missingMedia(let count): "Faltan \(count) archivos originales. El montaje actual no se modificó."
        }
    }
}

enum MediaImportError: LocalizedError {
    case notPlayable, invalidDuration, noTracks, videoNotDecodable
    var errorDescription: String? {
        switch self {
        case .notPlayable: "macOS no puede reproducir este codec o el archivo está protegido."
        case .invalidDuration: "el archivo no contiene una duración válida."
        case .noTracks: "el contenedor no incluye ninguna pista de vídeo o audio reconocible."
        case .videoNotDecodable: "macOS reconoce la pista de vídeo, pero no puede decodificar su codec."
        }
    }
}

struct PlayerView: NSViewRepresentable {
    let player: AVPlayer
    /// Si la vista tiene controles del sistema (para el monitor a pantalla
    /// completa); por defecto ninguno, como el monitor incrustado.
    var conControles: Bool = false

    func makeNSView(context: Context) -> AVPlayerView {
        let view = AVPlayerView()
        view.player = player
        view.controlsStyle = conControles ? .floating : .none
        view.videoGravity = .resizeAspect
        return view
    }

    func updateNSView(_ view: AVPlayerView, context: Context) {
        view.player = player
        view.controlsStyle = conControles ? .floating : .none
    }
}

extension NSView {
    /// Encuentra la vista del reproductor dentro de la jerarquía, para pedirle
    /// pantalla completa sin guardar referencias.
    func vistaDeReproductor() -> AVPlayerView? {
        if let reproductor = self as? AVPlayerView { return reproductor }
        return subviews.compactMap { $0.vistaDeReproductor() }.first
    }
}

@MainActor
final class EditorWindowDelegate: NSObject, NSWindowDelegate {
    weak var editor: EditorState?

    init(editor: EditorState) {
        self.editor = editor
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard let editor, editor.isDirty else { return true }
        let alerta = NSAlert()
        alerta.messageText = "Hay cambios sin guardar"
        alerta.informativeText = "¿Quieres guardar el proyecto antes de cerrar Editorcito?"
        alerta.addButton(withTitle: "Guardar")
        alerta.addButton(withTitle: "No guardar")
        alerta.addButton(withTitle: "Cancelar")
        switch alerta.runModal() {
        case .alertFirstButtonReturn:
            editor.saveProject()
            return !editor.isDirty
        case .alertSecondButtonReturn:
            // La decisión de no guardar se respeta: la recuperación automática
            // no puede reaparecer al arrancar el montaje que se acaba de
            // rechazar explícitamente.
            editor.descartarRecuperacion()
            return true
        default:
            return false
        }
    }
}

struct EditorWindowAccessor: NSViewRepresentable {
    @ObservedObject var editor: EditorState

    func makeNSView(context: Context) -> NSView { NSView(frame: .zero) }

    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async {
            view.window?.delegate = editor.windowDelegate
            // DIAGNÓSTICO TEMPORAL: volcar la geometría de la ventana a un archivo.
            guard let ventana = view.window else { return }
            var lineas = ["ventana: \(ventana.frame)"]
            func recorrer(_ v: NSView, nivel: Int) {
                let nombre = String(describing: type(of: v))
                lineas.append("\(String(repeating: "  ", count: nivel))\(nombre) frame=\(v.frame) hidden=\(v.isHidden)")
                if v is NSSplitView {
                    let sv = v as! NSSplitView
                    lineas.append("\(String(repeating: "  ", count: nivel))  SPLIT arrangesAllSubviews=\(sv.arrangesAllSubviews) dividers=\(sv.dividerStyle.rawValue)")
                }
                for sub in v.subviews { recorrer(sub, nivel: nivel + 1) }
            }
            recorrer(ventana.contentView ?? view, nivel: 0)
            try? lineas.joined(separator: "\n").write(toFile: "/tmp/editorcito-frames.txt", atomically: true, encoding: .utf8)
        }
    }
}

struct ContentView: View {
    @ObservedObject var editor: EditorState

    /// Zoom de toda la interfaz.
    ///
    /// Los tamaños de un editor están escritos en píxeles porque una regla de
    /// timecode o una pista tienen que medir lo que miden. Eso los deja clavados a
    /// la densidad de la pantalla, y lo que se lee bien en un portátil de 14" es
    /// diminuto en un monitor de 32". Escalar el lienzo entero —midiendo primero y
    /// escalando después— respeta la disposición y mantiene el ratón donde toca.
    @AppStorage("ui.escala") private var escalaDeInterfaz = 1.0
    @State private var iaExpandida = false
    @State private var historiaExpandida = false

    var body: some View {
        GeometryReader { geo in
            contenido
                // El lienzo mide exactamente lo que hay, dividido por el zoom: si se
                // le fuerza un mínimo mayor que la ventana, el contenido se dibuja
                // fuera del marco y acaba pintando encima de lo que haya detrás.
                .frame(
                    width: geo.size.width / escalaDeInterfaz,
                    height: geo.size.height / escalaDeInterfaz,
                    alignment: .topLeading
                )
                .scaleEffect(escalaDeInterfaz, anchor: .topLeading)
        }
        // El mínimo de la ventana crece con el zoom, porque con el zoom crece lo que
        // ocupa cada panel. Y se recorta, que es el cinturón de seguridad.
        .frame(
            minWidth: Self.anchoMinimoDelLienzo * escalaDeInterfaz,
            minHeight: Self.altoMinimoDelLienzo * escalaDeInterfaz
        )
        .clipped()
        .background(Color(nsColor: NSColor(calibratedWhite: 0.075, alpha: 1)))
        .preferredColorScheme(.dark)
        .overlay { EditorWindowAccessor(editor: editor).frame(width: 0, height: 0) }
        .onDeleteCommand { editor.removeSelectedClip() }
        .dropDestination(for: URL.self) { urls, _ in
            editor.importURLs(urls)
            return true
        }
        .onOpenURL { editor.importURLs([$0]) }
        .alert(item: $editor.recuperacionPendiente) { pendiente in
            Alert(
                title: Text("Recuperación disponible"),
                message: Text(pendiente.detalle + " El proyecto actual no se cambiará hasta que lo confirmes."),
                primaryButton: .default(Text("Recuperar montaje")) {
                    editor.recuperarRecuperacion()
                },
                secondaryButton: .destructive(Text("Descartar recuperación")) {
                    editor.descartarRecuperacion()
                }
            )
        }
    }

    /// Lo que necesita el lienzo para que ningún panel se salga de su sitio: la
    /// suma de los mínimos de los tres paneles más los divisores.
    private static let anchoMinimoDelLienzo: Double = 700
    private static let altoMinimoDelLienzo: Double = 520

    private var contenido: some View {
        VStack(spacing: 0) {
            topBar
            // Un Divider dentro de un VStack es flexible: se expande llenando el
            // espacio sobrante y dibuja su línea en medio, que es el hueco que
            // crecía al arrastrar el split. Fijo a un píxel.
            Divider().frame(height: 1)
            // Vertical arrastrable: el reparto entre monitor y montaje depende de si
            // se está buscando un plano o afinando un corte, y eso cambia cada rato.
            VSplitView {
                HSplitView {
                    // Los paneles deben llenar la altura que les asigna el split.
                    // Si conservan solo su altura ideal, fullscreen deja un hueco
                    // vacío encima de la segunda barra.
                    mediaPanel
                        .frame(minWidth: 170, idealWidth: 250, maxWidth: 460, maxHeight: .infinity, alignment: .top)
                    monitor
                        .frame(minWidth: 280, maxHeight: .infinity, alignment: .top)
                    if editor.mostrarTranscript {
                        PanelDeTranscript(editor: editor)
                            .frame(minWidth: 260, idealWidth: 320, maxWidth: 520, maxHeight: .infinity, alignment: .top)
                    }
                    inspector
                        .frame(minWidth: 190, idealWidth: 260, maxWidth: 460, maxHeight: .infinity, alignment: .top)
                }
                .frame(minHeight: 200, maxHeight: .infinity)

                timeline.frame(minHeight: 170, idealHeight: 300, maxHeight: .infinity)
            }
            .frame(maxHeight: .infinity)
            statusBar
        }
        .frame(maxHeight: .infinity)
    }

    /// Controles de tamaño de la interfaz. Con los atajos de siempre: ⌘+, ⌘− y ⌘0.
    /// Zoom de la interfaz.
    ///
    /// Solo zoom: la altura de las pistas se quedó en el menú del montaje, donde
    /// pertenece. Una es preferencia de la aplicación y viaja en los ajustes; la
    /// otra es propiedad del documento y viaja con el proyecto, y mezclarlas hacía
    /// imposible saber qué se guarda con qué.
    private var controlesDeTamano: some View {
        Menu {
            // Un selector marca cuál está activo. Una lista de botones no dice en
            // qué tamaño estás, que es justo lo primero que uno quiere saber al
            // abrir el menú.
            Picker("Tamaño de la interfaz", selection: $escalaDeInterfaz) {
                ForEach([0.8, 0.9, 1.0, 1.15, 1.3, 1.5], id: \.self) { valor in
                    Text(valor == 1.0 ? "100 % (original)" : String(format: "%.0f %%", valor * 100))
                        .tag(valor)
                }
            }
            .pickerStyle(.inline)
            Divider()
            Button("Ampliar") { ajustarEscala(0.1) }
            Button("Reducir") { ajustarEscala(-0.1) }
        } label: {
            Label(String(format: "%.0f %%", escalaDeInterfaz * 100), systemImage: "textformat.size")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Tamaño de toda la interfaz (⌘+ / ⌘− / ⌘0 en el menú Ver)")
    }

    private func ajustarEscala(_ delta: Double) {
        escalaDeInterfaz = min(max(escalaDeInterfaz + delta, 0.7), 1.8)
    }

    private var topBar: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                barraSuperior.padding(.horizontal, 12).frame(height: 40)
            }
            Divider()
            exportButton
                .padding(.horizontal, 10)
        }
        .frame(height: 40)
        .background(Color(nsColor: NSColor(calibratedWhite: 0.115, alpha: 1)))
    }

    private var barraSuperior: some View {
        HStack(spacing: 14) {
            HStack(spacing: 8) {
                Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 25, height: 25)
                Text("EDITORCITO").font(.system(size: 16, weight: .black, design: .rounded)).tracking(1.2)
            }
            Divider().frame(height: 22)
            Button {
                editor.undo()
            } label: {
                Label("Deshacer", systemImage: "arrow.uturn.backward")
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 11))
            }
            .buttonStyle(.plain)
            .disabled(!editor.canUndo)
            .help("Deshacer el último cambio (⌘Z)")
            Button {
                editor.redo()
            } label: {
                Label("Rehacer", systemImage: "arrow.uturn.forward")
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 11))
            }
            .buttonStyle(.plain)
            .disabled(!editor.canRedo)
            .help("Rehacer lo deshecho (⇧⌘Z)")
            Button {
                editor.openProject()
            } label: {
                Label("Abrir", systemImage: "folder")
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 11))
            }
            .buttonStyle(.plain)
            .help("Abrir un proyecto .editorcito (⌘O)")
            Button {
                editor.saveProject()
            } label: {
                Label("Guardar", systemImage: "square.and.arrow.down")
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 11))
            }
            .buttonStyle(.plain)
            .help("Guardar el proyecto, con copia .bak del anterior (⌘S)")
            Button {
                editor.importMedia()
            } label: {
                Label(editor.isImporting ? "Analizando…" : "Importar", systemImage: "plus")
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 11))
            }
            .buttonStyle(.plain)
            .disabled(editor.isImporting)
            .help("Importar vídeo o audio a la biblioteca (⌘I)")
            Menu("Subtítulos", systemImage: "captions.bubble") {
                Button("Importar SRT…") { editor.importarSubtitulos() }
                Button(editor.transcribing ? "Transcribiendo…" : "Transcribir medio seleccionado") {
                    editor.transcribirMedioSeleccionado()
                }
                .disabled(editor.selectedMedia == nil || editor.transcribing)
                Button("Exportar SRT…") { editor.exportarSubtitulos() }
                    .disabled(editor.montaje.subtitulos?.isEmpty != false)
            }
            Menu("Proxies", systemImage: "gauge.with.dots.needle.67percent") {
                Button("Generar proxies") { editor.generarProxies() }
                    .disabled(editor.generandoProxies || editor.media.isEmpty)
                Toggle("Usar proxies en preview", isOn: Binding(
                    get: { editor.proxiesActivos },
                    set: { editor.fijarProxies($0) }
                ))
                if editor.generandoProxies {
                    ProgressView(value: editor.proxyProgress)
                }
            }
            Button("Añadir al timeline", systemImage: "rectangle.stack.badge.plus") { editor.addSelectedMedia() }
                .disabled(editor.selectedMedia == nil)
                .help("Añadir el medio seleccionado al final de la pista activa")
            Button("Dividir", systemImage: "scissors") { editor.splitAtPlayhead() }
                .disabled(editor.duracionEnFrames == 0)
                .help("Cortar por donde está el cabezal (⌘B)")
            Divider().frame(height: 22)
            controlesDeTamano
            Divider().frame(height: 22)
            HStack(spacing: 4) {
                Circle().fill(editor.isDirty ? .orange : .green).frame(width: 6, height: 6)
                Text(editor.projectName + (editor.isDirty ? " *" : ""))
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(editor.isDirty ? .primary : .secondary)
            }
            .help(editor.isDirty ? "Cambios sin guardar" : "Todos los cambios están guardados")
            if editor.isExporting {
                ProgressView(value: editor.exportProgress).frame(width: 150)
            } else if editor.isImporting {
                ProgressView(value: editor.importProgress).frame(width: 150)
            }
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .fixedSize()
    }

    private var exportButton: some View {
        Button(editor.exportQueueCount > 0 ? "Exportar · \(editor.exportQueueCount) en cola" : "Exportar", systemImage: "arrow.up.right.square") {
            editor.exportMovie()
        }
        .buttonStyle(.borderedProminent)
        .tint(.cyan)
        .controlSize(.small)
        .disabled(editor.duracionEnFrames == 0)
        .help("Exportar el montaje a MP4 con capas y mezcla aplicadas (⇧⌘E)")
        .accessibilityLabel("Exportar montaje")
    }

    private var mediaPanel: some View {
        VStack(alignment: .leading, spacing: 0) {
            panelTitle("MEDIOS", count: editor.media.count)
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("Buscar medios", text: $editor.mediaSearch)
                    .textFieldStyle(.plain)
                Button { editor.anadirBin() } label: { Image(systemName: "folder.badge.plus") }
                    .buttonStyle(.plain)
                    .help("Crear bin")
                    .accessibilityLabel("Crear bin")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Color(nsColor: NSColor(calibratedWhite: 0.12, alpha: 1)))
            Picker("Bin", selection: $editor.selectedBin) {
                ForEach(editor.nombresDeBins, id: \.self) { nombre in
                    Text(nombre).tag(nombre)
                }
            }
            .pickerStyle(.menu)
            .padding(.horizontal, 10)
            .padding(.bottom, 4)
            HStack {
                Button("Crear grupo multicámara", systemImage: "square.grid.2x2") {
                    editor.crearGrupoMulticamConMediosVisibles()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(editor.selectedMediaIDs.count < 2)
                Menu {
                    Button("Bin normal") { editor.anadirBin() }
                    Divider()
                    Button("Smart: VFR") { editor.anadirBinInteligente(filtro: "VFR") }
                    Button("Smart: Audio") { editor.anadirBinInteligente(filtro: "Audio") }
                    Button("Smart: 4K") { editor.anadirBinInteligente(filtro: "4k") }
                } label: {
                    Image(systemName: "plus.circle")
                }
                .menuStyle(.borderlessButton)
                .help("Crear bin (normal o inteligente)")
                .accessibilityLabel("Crear bin")
            }
            .padding(.horizontal, 10)
            .padding(.bottom, 5)
            if !(editor.montaje.gruposMulticam?.isEmpty ?? true) {
                Menu {
                    ForEach(editor.montaje.gruposMulticam ?? []) { grupo in
                        Button("\(grupo.nombre) · \(grupo.mediaIDs.count) ángulos") {
                            editor.insertarClipMulticam(grupoID: grupo.id)
                        }
                    }
                } label: {
                    Label("Insertar multicámara", systemImage: "square.grid.2x2.fill")
                }
                .menuStyle(.borderlessButton)
                .controlSize(.small)
                .fixedSize()
                .help("Insertar un clip multicámara en el cabezal y cambiarlo desde el visor")
                .padding(.horizontal, 10)
                .padding(.bottom, 5)
            }
            List(editor.mediosVisibles, selection: $editor.selectedMediaIDs) { item in
                VStack(alignment: .leading, spacing: 3) {
                    HStack {
                        if let miniatura = editor.miniatura(for: item) {
                            Image(decorative: miniatura, scale: 1)
                                .resizable()
                                .scaledToFill()
                                .frame(width: 52, height: 30)
                                .clipped()
                                .cornerRadius(3)
                        } else {
                            Image(systemName: item.size == .zero ? "waveform" : "film.fill")
                                .foregroundStyle(.cyan)
                                .frame(width: 52, height: 30)
                        }
                        Text(item.name)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(item.name)
                    }
                    Text(item.detail).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
                .tag(item.id)
                .onTapGesture(count: 2) { editor.addToTimeline(item) }
                // Arrastrar al timeline es el gesto de montaje de cualquier NLE:
                // se arrastra el identificador del medio y el lienzo lo suelta
                // en el frame bajo el puntero.
                .draggable("editorcito.media.\(item.id.uuidString)")
                .help("Arrastra al timeline o doble clic para añadirlo")
                .contextMenu {
                    Menu("Mover a bin") {
                        ForEach(editor.nombresDeBins.dropFirst(), id: \.self) { nombre in
                            Button(nombre) { editor.moverMedio(item.id, aBin: nombre) }
                        }
                    }
                    Button("Quitar del bin") { editor.moverMedio(item.id, aBin: "Todos") }
                }
            }
            .listStyle(.sidebar)
            .onChange(of: editor.selectedMediaIDs) { _, ids in
                editor.timelineHasFocus = false
                guard let id = ids.first else { return }
                editor.selectedMediaID = id
                editor.cargarMedioEnOrigen(id)
            }
            .onChange(of: editor.selectedMediaID, initial: true) { _, id in
                editor.cargarMedioEnOrigen(id)
            }
            if editor.media.isEmpty {
                ContentUnavailableView("Sin medios", systemImage: "film.stack", description: Text("Arrastra o importa MP4, MOV, M4V y audio compatibles con macOS. Sin límite fijo de tamaño."))
            } else if editor.mediosVisibles.isEmpty {
                ContentUnavailableView("Sin resultados", systemImage: "magnifyingglass", description: Text("Prueba otro nombre o cambia de bin."))
            }
        }
    }

    private var monitor: some View {
        VStack(spacing: 0) {
            HStack {
                Text(editor.monitorActual == .programa ? "PROGRAMA" : "ORIGEN")
                    .font(.system(size: 10, weight: .bold))
                    .tracking(0.8)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Text("Monitor")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .fixedSize()
                    Picker("", selection: $editor.monitorActual) {
                        ForEach(ModoDeMonitor.allCases) { modo in
                            Text(modo.nombre).tag(modo)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .accessibilityLabel("Monitor")
                    .frame(minWidth: 120, idealWidth: 150, maxWidth: 170)
                }
                .padding(.horizontal, 10)
                .frame(height: 34)
                .background(Color(nsColor: NSColor(calibratedWhite: 0.095, alpha: 1)))

            if editor.monitorActual == .programa {
                programaMonitor
            } else {
                origenMonitor
            }
        }
    }

    private var programaMonitor: some View {
        VStack(spacing: 0) {
            ZStack {
                Color.black
                PlayerView(player: editor.player)
                    .scaleEffect(editor.zoomDeMonitor)
                    .animation(.easeInOut(duration: 0.15), value: editor.zoomDeMonitor)
                if editor.duracionEnFrames == 0 {
                    VStack(spacing: 10) {
                        Image(systemName: "play.rectangle").font(.system(size: 42)).foregroundStyle(.tertiary)
                        Text(editor.media.isEmpty ? "Importa un medio o arrástralo aquí" : "El monitor mostrará el montaje aquí")
                            .foregroundStyle(.secondary)
                        if editor.media.isEmpty {
                            Text("⌘I o arrastra un archivo a la ventana")
                                .font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary)
                        } else {
                            Text("Arrastra un medio de la biblioteca al timeline")
                                .font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary)
                        }
                    }
                }
            }
    /// Forma de onda del frame actual: el instrumento que revela negros
    /// aplastados, blancos quemados y exposiciones que cambian con el plano.
    switch editor.instrumentoDeMonitor {
    case .formaDeOnda:
        VistaDeFormaDeOnda(distribucion: editor.formaDeOnda)
            .padding(.horizontal, 8).padding(.top, 6)
    case .vectorscopio:
        VistaDeVectorscopio(densidad: editor.vectorscopio)
            .padding(.horizontal, 8).padding(.top, 6)
    case .paradeRGB:
        VistaDeParadeRGB(distribucion: editor.paradeRGB)
            .padding(.horizontal, 8).padding(.top, 6)
    case .histograma:
        VistaDeHistograma(distribucion: editor.histograma)
            .padding(.horizontal, 8).padding(.top, 6)
    case .ninguno:
        EmptyView()
    }
            if let visor = editor.visorMulticam() {
                multicamStrip(visor)
            }
            HStack(spacing: 16) {
                Text(editor.timecodeDelCabezal).monospacedDigit().frame(width: 78)
                Slider(
                    value: Binding(get: { editor.playhead }, set: { editor.seek(to: $0) }),
                    in: 0...max(0.01, editor.timelineDuration)
                ) { arrastrando in
                    // Audio scrubbing: mientras se arrastra el cabezal, la
                    // reproducción va a 0,5× para oír el material por donde
                    // se pasa, como en Premiere.
                    editor.scrubbing = arrastrando
                    if arrastrando && !editor.isPlaying {
                        editor.reproducir(a: 0.5)
                    } else if !arrastrando {
                        editor.reproducir(a: 0)
                    }
                }
                Button { editor.stepFrame(-1) } label: { Image(systemName: "backward.frame.fill") }
                    .help("Un frame atrás (←)")
                    .accessibilityLabel("Un frame atrás")
                Button { editor.togglePlayback() } label: {
                    Image(systemName: editor.isPlaying ? "pause.fill" : "play.fill").frame(width: 22)
                }
                .help(editor.isPlaying ? "Pausar (espacio)" : "Reproducir (espacio)")
                .accessibilityLabel(editor.isPlaying ? "Pausar programa" : "Reproducir programa")
                Button { editor.stepFrame(1) } label: { Image(systemName: "forward.frame.fill") }
                    .help("Un frame adelante (→)")
                    .accessibilityLabel("Un frame adelante")
                Text(editor.timebase.timecode(editor.duracionEnFrames)).monospacedDigit().frame(width: 78)
                Spacer()
                Menu {
                    Picker("Zoom del monitor", selection: $editor.zoomDeMonitor) {
                        ForEach([1.0, 1.25, 1.5, 1.75, 2.0], id: \.self) { zoom in
                            Text(zoom == 1.0 ? "Ajustar" : String(format: "%.0f %%", zoom * 100))
                                .tag(zoom)
                        }
                    }
                    .pickerStyle(.inline)
                } label: {
                    Text(String(format: "%.0f %%", editor.zoomDeMonitor * 100)).frame(width: 44)
                }
                .menuStyle(.borderlessButton)
                .help("Zoom del monitor (1× = ajustar, 2× = tamaño real)")
                .accessibilityLabel("Zoom del monitor")
                Button {
                    // Pantalla completa de la ventana: el monitor ocupa todo.
                    NSApp.keyWindow?.toggleFullScreen(nil)
                } label: {
                    Image(systemName: "arrow.up.left.and.arrow.down.right").font(.system(size: 11))
                }
                .help("Pantalla completa (esc para salir)")
                .accessibilityLabel("Pantalla completa")
            }
            .font(.system(size: 11, design: .monospaced))
            .buttonStyle(.plain).padding(10)
        }
    }

    /// El visor multiángulo del clip multicámara seleccionado.
    ///
    /// Una cuadrícula de reproductores vivos, uno por cámara, sincronizados al
    /// instante de grupo del cabezal (cada ángulo enseña su material en
    /// `t − desfase`, la misma cuenta del constructor). El que manda en el
    /// cabezal queda marcado; pulsar otro cambia el ángulo —también durante la
    /// reproducción—. Debajo, la sincronía manual (±1 frame, ±10 con ⌥, o el
    /// arreglo automático por audio) y el corte destructivo («aplanar»).
    private func multicamStrip(_ visor: (clip: Clip, grupo: GrupoMulticam)) -> some View {
        let activo = editor.anguloActivoDelVisor(visor)
        return VStack(spacing: 6) {
            HStack(spacing: 8) {
                ForEach(visor.grupo.mediaIDs, id: \.self) { id in
                    let esActivo = activo == id
                    let nombre = editor.media.first { $0.id == id }?.name ?? "Ángulo"
                    Button {
                        editor.cambiarAnguloMulticam(clipID: visor.clip.id, a: id, en: editor.cabezal)
                    } label: {
                        VStack(spacing: 2) {
                            ZStack {
                                Rectangle().fill(.black)
                                if let reproductor = editor.reproductorDelAngulo(id) {
                                    PlayerView(player: reproductor)
                                } else {
                                    VStack(spacing: 4) {
                                        Image(systemName: "video.slash").foregroundStyle(.tertiary)
                                        Text("Sin medio").font(.system(size: 9)).foregroundStyle(.secondary)
                                    }
                                }
                            }
                            .frame(width: 110, height: 62)
                            .clipShape(RoundedRectangle(cornerRadius: 3))
                            .overlay(
                                RoundedRectangle(cornerRadius: 3)
                                    .stroke(esActivo ? Color.accentColor : Color(nsColor: NSColor(calibratedWhite: 0.3, alpha: 1)), lineWidth: esActivo ? 2 : 1)
                            )
                            HStack(spacing: 3) {
                                Text(nombre).font(.system(size: 9)).lineLimit(1)
                                Spacer()
                                let desfase = visor.grupo.desfases[id] ?? 0
                                Text(editor.timebase.timecode(desfase))
                                    .font(.system(size: 8, design: .monospaced))
                                    .foregroundStyle(.secondary)
                            }
                            .frame(width: 110)
                        }
                        .padding(4)
                        .background(esActivo ? Color.accentColor.opacity(0.15) : Color.clear)
                        .cornerRadius(5)
                    }
                    .buttonStyle(.plain)
                    .help("Cambiar a este ángulo desde el cabezal")
                    .accessibilityLabel("Cambiar al ángulo \(nombre)")
                }
                Spacer()
            }

            HStack(spacing: 8) {
                let grupoID = visor.grupo.id
                let clipID = visor.clip.id
                ForEach(visor.grupo.mediaIDs, id: \.self) { id in
                    let nombre = editor.media.first { $0.id == id }?.name ?? "ángulo"
                    HStack(spacing: 3) {
                        Button {
                            let paso: Int64 = NSEvent.modifierFlags.contains(.option) ? 10 : 1
                            editor.ajustarDesfase(grupoID: grupoID, medioID: id, delta: -paso)
                        } label: {
                            Image(systemName: "minus.circle").font(.system(size: 11))
                        }
                        .buttonStyle(.plain)
                        .help("«\(nombre)» un frame antes (⌥: 10)")
                        .accessibilityLabel("Retrasar el ángulo \(nombre) un frame")
                        Text(nombre).font(.system(size: 9)).foregroundStyle(.secondary).lineLimit(1)
                        Button {
                            let paso: Int64 = NSEvent.modifierFlags.contains(.option) ? 10 : 1
                            editor.ajustarDesfase(grupoID: grupoID, medioID: id, delta: paso)
                        } label: {
                            Image(systemName: "plus.circle").font(.system(size: 11))
                        }
                        .buttonStyle(.plain)
                        .help("«\(nombre)» un frame después (⌥: 10)")
                        .accessibilityLabel("Adelantar el ángulo \(nombre) un frame")
                    }
                }
                Spacer()
                Button {
                    editor.resincronizarGrupo(grupoID: grupoID)
                } label: {
                    Label("Sincronizar por audio", systemImage: "waveform")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("Recalcular la sincronización con el onset de la forma de onda")
                .accessibilityLabel("Resincronizar el grupo por audio")
                Button {
                    editor.aplanarMulticam(clipID: clipID)
                } label: {
                    Label("Aplanar multicámara", systemImage: "square.stack.3d.up.slash")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("Convertir cada tramo de ángulo en un clip normal (corte destructivo)")
                .accessibilityLabel("Aplanar el clip multicámara en clips normales")
            }
            .font(.system(size: 9))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color(nsColor: NSColor(calibratedWhite: 0.09, alpha: 1)))
        .onAppear {
            editor.prepararVisorMultiAngulo(visor)
        }
        .onDisappear {
            editor.liberarVisorMultiAngulo()
        }
        .onChange(of: visor.clip.id) {
            editor.prepararVisorMultiAngulo(visor)
        }
        .onChange(of: visor.grupo.desfases) {
            editor.sincronizarAngulosDelVisor()
        }
    }

    private var origenMonitor: some View {
        VStack(spacing: 0) {
            ZStack {
                Color.black
                if editor.selectedMedia != nil {
                    PlayerView(player: editor.sourcePlayer)
                } else {
                    VStack(spacing: 10) {
                        Image(systemName: "film").font(.system(size: 42)).foregroundStyle(.tertiary)
                        Text("Selecciona un medio para marcar entrada y salida")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            HStack(spacing: 10) {
                Text(editor.timecodeDeOrigen).monospacedDigit().frame(width: 78)
                Slider(
                    value: Binding(
                        get: { Double(editor.cabezalDeOrigen) },
                        set: { editor.seekOrigen(toFrame: Int64($0.rounded())) }
                    ),
                    in: 0...Double(max(1, editor.duracionDeOrigenEnFrames))
                )
                Button { editor.stepFrameOrigen(-1) } label: { Image(systemName: "backward.frame.fill") }
                    .help("Un frame atrás en el origen")
                    .accessibilityLabel("Un frame atrás en el origen")
                Button { editor.togglePlaybackOrigen() } label: {
                    Image(systemName: editor.isPlayingOrigen ? "pause.fill" : "play.fill").frame(width: 22)
                }
                .help(editor.isPlayingOrigen ? "Pausar origen" : "Reproducir origen")
                .accessibilityLabel(editor.isPlayingOrigen ? "Pausar origen" : "Reproducir origen")
                Button { editor.stepFrameOrigen(1) } label: { Image(systemName: "forward.frame.fill") }
                    .help("Un frame adelante en el origen")
                    .accessibilityLabel("Un frame adelante en el origen")
                Text(editor.timebase.timecode(editor.duracionDeOrigenEnFrames)).monospacedDigit().frame(width: 78)
            }
            HStack(spacing: 8) {
                Button("Entrada", systemImage: "arrow.down.to.line") { editor.marcarEntradaDeOrigen() }
                Button("Salida", systemImage: "arrow.up.to.line") { editor.marcarSalidaDeOrigen() }
                if let id = editor.selectedMediaID, let rango = editor.rangoDeOrigen(id) {
                    Text("I/O \(editor.timebase.timecode(rango.entrada)) → \(editor.timebase.timecode(rango.salida))")
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundStyle(.secondary)
                    Button("Limpiar") { editor.limpiarEntradaYSalida() }
                    Button("Subclip") { editor.crearSubclip() }
                        .help("Crear un recorte con nombre en la biblioteca (make subclip)")
                }
                Spacer()
                Button("Insertar", systemImage: "arrow.down.to.line.compact") { editor.insertarEnCabezal() }
                    .disabled(editor.selectedMedia == nil)
                Button("Superponer", systemImage: "square.stack.3d.up") { editor.sobrescribirEnCabezal() }
                    .disabled(editor.selectedMedia == nil)
            }
            .controlSize(.small)
            .buttonStyle(.bordered)
            .padding(.horizontal, 10)
            .padding(.bottom, 10)
        }
    }

    private var inspector: some View {
        VStack(alignment: .leading, spacing: 0) {
            panelTitle("INSPECTOR", count: nil)
            DisclosureGroup(isExpanded: $iaExpandida) {
                VStack(alignment: .leading, spacing: 8) {
                    AIModelSelector(settings: editor.aiSettings)
                    TextField("Recorta el inicio y mueve el último clip…", text: $editor.aiRequest, axis: .vertical)
                        .textFieldStyle(.roundedBorder).lineLimit(2...4)
                        .onSubmit { editor.editWithAI() }
                    Button(editor.aiWorking ? "Planificando…" : "Aplicar edición") { editor.editWithAI() }
                        .buttonStyle(.borderedProminent).tint(.cyan).controlSize(.small)
                        .disabled(editor.aiRequest.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || editor.aiWorking || editor.duracionEnFrames == 0)
                    if editor.aiWorking {
                        Button("Cancelar") { editor.cancelAI() }.controlSize(.small)
                    }
                    if let result = editor.aiResult {
                        Text(result).font(.system(size: 9)).foregroundStyle(.secondary)
                    }
                    Text(editor.aiSettings.provider == .local
                         ? "Privado · Hearthia · sin subir tus vídeos"
                         : "Cloud · se envían tu instrucción, nombres, tiempos, marcadores y transcripciones; nunca el vídeo")
                        .font(.system(size: 8)).foregroundStyle(.tertiary)
                }
                .padding(.top, 6)
            } label: {
                Label("Montaje con IA local", systemImage: "sparkles")
                    .font(.system(size: 9, weight: .bold)).foregroundStyle(.cyan)
            }
            .padding(12)
            Divider()
            if let subtitulo = editor.subtituloActivo {
                VStack(alignment: .leading, spacing: 6) {
                    Text("SUBTÍTULO ACTIVO").font(.system(size: 9, weight: .bold)).foregroundStyle(.cyan)
                    TextEditor(text: Binding(
                        get: { subtitulo.texto },
                        set: { editor.editarSubtitulo(subtitulo.id, texto: $0) }
                    ))
                    .frame(minHeight: 42, maxHeight: 90)
                    .font(.system(size: 11))
                }
                .padding(10)
                Divider()
            }
            if let clip = editor.selectedClip {
                inspectorDeClip(clip)
            } else {
                ContentUnavailableView(
                    "Sin clip",
                    systemImage: "slider.horizontal.3",
                    description: Text("Selecciona un clip del montaje")
                )
            }
            Divider()
            historiaDeEdicion
        }
    }

    /// Propiedades del clip seleccionado.
    ///
    /// Todo lo que se puede tocar de un clip está aquí y en las mismas unidades en
    /// las que se habla de ello: timecode para el tiempo, decibelios para el sonido,
    /// porcentaje para la imagen.
    @ViewBuilder
    private func inspectorDeClip(_ clip: Clip) -> some View {
        let base = editor.timebase
        let medio = editor.mediaItem(for: clip)
        Form {
            if clip.esTitulo, let titulo = clip.titulo {
                Section("Título") {
                    Picker("Forma", selection: Binding(
                        get: { titulo.forma },
                        set: { nueva in editor.fijarTitulo(clip.id) { $0.forma = nueva } }
                    )) {
                        ForEach(FormaDeTitulo.allCases, id: \.self) { Text($0.nombre).tag($0) }
                    }
                    if titulo.forma == .texto {
                        TextField("Texto del título", text: Binding(
                            get: { titulo.texto },
                            set: { nuevo in editor.fijarTitulo(clip.id) { $0.texto = nuevo } }
                        ))
                        .textFieldStyle(.roundedBorder)
                        deslizador("Tamaño", titulo.tamano, 16...300, "pt") { valor in
                            editor.fijarTitulo(clip.id) { $0.tamano = valor }
                        }
                    } else {
                        deslizador("Ancho", titulo.ancho * 100, 1...100, "%") { valor in
                            editor.fijarTitulo(clip.id) { $0.ancho = valor / 100 }
                        }
                        if titulo.forma != .linea {
                            deslizador("Alto", titulo.alto * 100, 1...100, "%") { valor in
                                editor.fijarTitulo(clip.id) { $0.alto = valor / 100 }
                            }
                        }
                    }
                    deslizador("Posición X", titulo.posicionX * 100, 0...100, "%") { valor in
                        editor.fijarTitulo(clip.id) { $0.posicionX = valor / 100 }
                    }
                    deslizador("Posición Y", titulo.posicionY * 100, 0...100, "%") { valor in
                        editor.fijarTitulo(clip.id) { $0.posicionY = valor / 100 }
                    }
                    HStack {
                        Text("Color").font(.system(size: 11)).frame(width: 70, alignment: .leading)
                        ColorPicker("", selection: Binding(
                            get: { Color(red: titulo.rojo, green: titulo.verde, blue: titulo.azul) },
                            set: { color in
                                let ns = NSColor(color).usingColorSpace(.sRGB) ?? NSColor.white
                                editor.fijarTitulo(clip.id) {
                                    $0.rojo = Double(ns.redComponent)
                                    $0.verde = Double(ns.greenComponent)
                                    $0.azul = Double(ns.blueComponent)
                                }
                            }
                        ))
                        .labelsHidden()
                    }
                    deslizador("Contorno", titulo.contorno, 0...12, "pt") { valor in
                        editor.fijarTitulo(clip.id) { $0.contorno = valor }
                    }
                    deslizador("Fundido", Double(titulo.fundido) / base.fps, 0...2, "s") { valor in
                        editor.fijarTitulo(clip.id) { $0.fundido = Int64(valor * base.fps) }
                    }
                }
                Section("Posición en el montaje") {
                    LabeledContent("Entrada", value: base.timecode(clip.inicio))
                    LabeledContent("Salida", value: base.timecode(clip.fin))
                    LabeledContent("Duración", value: base.timecode(clip.duracion))
                }
            } else {
                Section("Clip") {
                Text(medio?.name ?? clip.nombre).lineLimit(2)
                LabeledContent("Entrada", value: base.timecode(clip.inicio))
                LabeledContent("Salida", value: base.timecode(clip.fin))
                LabeledContent("Duración", value: base.timecode(clip.duracion))
                LabeledContent("En origen", value: base.timecode(clip.entradaEnOrigen))
                if editor.estaOffline(clip) {
                    Button("Revincular medio…") { editor.revincular(clip.mediaID) }
                        .tint(.orange)
                }
            }

            Section("Recorte") {
                HStack {
                    Button("Entrada al cabezal") { editor.recortarHastaCabezal(borde: .entrada) }
                    Button("Salida al cabezal") { editor.recortarHastaCabezal(borde: .salida) }
                }
                .controlSize(.small)
                Toggle("Activo", isOn: Binding(
                    get: { clip.habilitado },
                    set: { _ in editor.alternarHabilitado(clip.id) }
                ))
            }

            Section("Imagen") {
                deslizador("Opacidad", clip.transformacion.opacidad, 0...100, "%") { valor in
                    editor.fijarOpacidad(clip.id, valor)
                }
                deslizador("Escala", clip.transformacion.escala, 10...400, "%") { valor in
                    editor.fijarTransformacion(clip.id) { $0.escala = valor }
                }
                deslizador("Giro", clip.transformacion.rotacion, -180...180, "°") { valor in
                    editor.fijarTransformacion(clip.id) { $0.rotacion = valor }
                }
                deslizador("Posición X", clip.transformacion.posicionX, -2000...2000, "px") { valor in
                    editor.fijarTransformacion(clip.id) { $0.posicionX = valor }
                }
                deslizador("Posición Y", clip.transformacion.posicionY, -2000...2000, "px") { valor in
                    editor.fijarTransformacion(clip.id) { $0.posicionY = valor }
                }
            }

            Section("Composición") {
                Picker("Modo de fusión", selection: Binding(
                    get: { clip.modoDeFusion },
                    set: { editor.fijarModoDeFusion(clip.id, $0) }
                )) {
                    ForEach(ModoDeFusion.allCases, id: \.self) { modo in
                        Text(modo.nombre).tag(modo)
                    }
                }
                if let mascara = clip.mascara {
                    Toggle("Máscara", isOn: Binding(
                        get: { mascara.activa },
                        set: { activa in editor.fijarMascara(clip.id) {
                            if activa {
                                if $0 == nil { $0 = MascaraDeClip() }
                            } else {
                                $0 = nil
                            }
                        } }
                    ))
                    if mascara.activa {
                        Picker("Forma", selection: Binding(
                            get: { mascara.forma },
                            set: { nueva in editor.fijarMascara(clip.id) { $0?.forma = nueva } }
                        )) {
                            ForEach(MascaraDeClip.Forma.allCases, id: \.self) { Text($0.nombre).tag($0) }
                        }
                        deslizador("Posición X", mascara.posicionX * 100, 0...100, "%") { valor in
                            editor.fijarMascara(clip.id) { $0?.posicionX = valor / 100 }
                        }
                        deslizador("Posición Y", mascara.posicionY * 100, 0...100, "%") { valor in
                            editor.fijarMascara(clip.id) { $0?.posicionY = valor / 100 }
                        }
                        deslizador("Tamaño X", mascara.tamanoX * 100, 1...100, "%") { valor in
                            editor.fijarMascara(clip.id) { $0?.tamanoX = valor / 100 }
                        }
                        deslizador("Tamaño Y", mascara.tamanoY * 100, 1...100, "%") { valor in
                            editor.fijarMascara(clip.id) { $0?.tamanoY = valor / 100 }
                        }
                        deslizador("Pluma", mascara.pluma * 100, 0...100, "%") { valor in
                            editor.fijarMascara(clip.id) { $0?.pluma = valor / 100 }
                        }
                        Toggle("Invertida", isOn: Binding(
                            get: { mascara.invertida },
                            set: { invertida in editor.fijarMascara(clip.id) { $0?.invertida = invertida } }
                        ))
                    }
                } else {
                    Button("Añadir máscara") { editor.fijarMascara(clip.id) { $0 = MascaraDeClip() } }
                        .controlSize(.small)
                }
                if let croma = clip.croma {
                    Toggle("Chroma key", isOn: Binding(
                        get: { !croma.esNeutro },
                        set: { activo in editor.fijarCroma(clip.id) {
                            if activo {
                                if $0 == nil { $0 = ChromaKeyDeClip() }
                            } else {
                                $0 = nil
                            }
                        } }
                    ))
                    if !croma.esNeutro {
                        deslizador("Tolerancia", croma.tolerancia * 100, 1...99, "%") { valor in
                            editor.fijarCroma(clip.id) { $0?.tolerancia = valor / 100 }
                        }
                        deslizador("Suavizado", croma.suavizado * 100, 0...50, "%") { valor in
                            editor.fijarCroma(clip.id) { $0?.suavizado = valor / 100 }
                        }
                        deslizador("Suprimir derrame", croma.suprimirDerrame * 100, 0...100, "%") { valor in
                            editor.fijarCroma(clip.id) { $0?.suprimirDerrame = valor / 100 }
                        }
                        HStack {
                            Text("Color de clave").font(.system(size: 11)).frame(width: 90, alignment: .leading)
                            ColorPicker("", selection: Binding(
                                get: { Color(red: croma.rojo, green: croma.verde, blue: croma.azul) },
                                set: { color in
                                    let ns = NSColor(color).usingColorSpace(.sRGB) ?? NSColor.green
                                    editor.fijarCroma(clip.id) {
                                        $0?.rojo = Double(ns.redComponent)
                                        $0?.verde = Double(ns.greenComponent)
                                        $0?.azul = Double(ns.blueComponent)
                                    }
                                }
                            ))
                            .labelsHidden()
                        }
                    }
                } else {
                    Button("Añadir chroma key", systemImage: "camera.metering.center.weighted") {
                        editor.fijarCroma(clip.id) { $0 = ChromaKeyDeClip() }
                    }
                    .controlSize(.small)
                    .help("Pantalla verde o azul: el color de clave se hace transparente")
                }
            }

            Section("Efectos") {
                deslizador("Viñeta", clip.color.vignette * 100, 0...100, "%") { valor in
                    editor.fijarColorDeClip(clip.id) { $0.vignette = valor / 100 }
                }
                if clip.color.vignette > 0.001 {
                    deslizador("Radio viñeta", clip.color.radioDeVignette * 100, 20...100, "%") { valor in
                        editor.fijarColorDeClip(clip.id) { $0.radioDeVignette = valor / 100 }
                    }
                }
                deslizador("Desenfoque", clip.color.desenfoque * 100, 0...50, "%") { valor in
                    editor.fijarColorDeClip(clip.id) { $0.desenfoque = valor / 100 }
                }
                if clip.color.curvas == nil {
                    Button("Curvas…") {
                        editor.abrirEditorDeCurvas(clip.id)
                    }
                    .controlSize(.small)
                    .help("Curvas RGB: la curva de luminancia y la de cada canal")
                } else {
                    Button("Quitar curvas") {
                        editor.fijarColorDeClip(clip.id) { $0.curvas = nil }
                    }
                    .controlSize(.small)
                }

                DisclosureGroup {
                    ruedasDeColor(clip)
                } label: {
                    Label("Ruedas de color", systemImage: "circle.hexagongrid.fill")
                        .font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                }
            }

            Section("Animación") {
                HStack {
                    Button("Añadir keyframe", systemImage: "diamond.fill") {
                        editor.anadirKeyframe(clip.id)
                    }
                    if let claves = clip.keyframes, !claves.isEmpty {
                        Text("\(claves.count)").foregroundStyle(.secondary)
                        Button("Quitar actual") {
                            editor.eliminarKeyframeActual(clip.id)
                        }
                    }
                }
                .controlSize(.small)
                Text("El keyframe usa la posición actual del cabezal")
                    .font(.system(size: 9)).foregroundStyle(.tertiary)
            }

            Section("Sonido") {
                deslizador("Ganancia", clip.ganancia, -60...12, "dB") { valor in
                    editor.fijarGanancia(clip.id, valor)
                }
            }

            if let pistaID = editor.montaje.pistaDe(clip: clip.id), let pista = editor.montaje.pista(pistaID) {
                Section("Mezclador \(pista.nombre)") {
                    deslizador("Volumen", pista.volumenDB, -60...12, "dB") { valor in
                        editor.fijarVolumenPista(pista.id, valor)
                    }
                    if pista.tipo == .audio, pista.duckingActivo {
                        Picker("Sidechain", selection: Binding(
                            get: { pista.fuenteDeDucking ?? editor.montaje.pistasDeAudio.first?.id ?? pista.id },
                            set: { editor.fijarFuenteDeDucking(pista.id, $0) }
                        )) {
                            ForEach(editor.montaje.pistasDeAudio, id: \.id) { fuente in
                                Text(fuente.nombre).tag(fuente.id)
                            }
                        }
                        .help("La pista cuya señal baja el volumen de esta cuando suena")
                    }
                    if pista.tipo == .audio {
                        puertaDeRuido(pista)
                        multibanda(pista)
                        reverbYRetardo(pista)
                    }
                    if pista.tipo == .audio {
                        MedidorEnVivoView(pistaID: pista.id)
                    }
                }
            }

            Section("Fundidos") {
                deslizador("Entrada", Double(clip.entradaFundido) / base.fps, 0...5, "s") { valor in
                    editor.fijarFundidos(clip.id, entrada: Int64(valor * base.fps))
                }
                deslizador("Salida", Double(clip.salidaFundido) / base.fps, 0...5, "s") { valor in
                    editor.fijarFundidos(clip.id, salida: Int64(valor * base.fps))
                }
                Button("Fundido de 1 s en ambos") { editor.fundidoRapido(clip.id) }
                    .controlSize(.small)
            }

            Section("Transiciones") {
                Picker("Tipo", selection: Binding(
                    get: { clip.transicionEntrada?.tipo ?? .disolucion },
                    set: { editor.fijarTransicion(clip.id, tipo: $0) }
                )) {
                    ForEach(TipoDeTransicion.allCases, id: \.self) { Text($0.nombre).tag($0) }
                }
                deslizador("Duración", Double(clip.transicionEntrada?.duracion ?? 0) / base.fps, 0...3, "s") { valor in
                    editor.fijarTransicion(clip.id, tipo: clip.transicionEntrada?.tipo ?? .disolucion, duracion: Int64(valor * base.fps))
                }
                HStack {
                    Button("Aplicar al corte") {
                        editor.fijarTransicion(clip.id, tipo: clip.transicionEntrada?.tipo ?? .disolucion)
                    }
                    if clip.transicionEntrada != nil || clip.transicionSalida != nil {
                        Button("Quitar") { editor.quitarTransiciones(clip.id) }
                    }
                }
                .controlSize(.small)
            }

            Section("Velocidad") {
                deslizador("Velocidad", clip.velocidad * 100, 10...1000, "%") { valor in
                    editor.fijarVelocidad(clip.id, valor / 100)
                }
                if let rampas = clip.rampasDeVelocidad, !rampas.isEmpty {
                    ForEach(rampas, id: \.frame) { rampa in
                        HStack {
                            Text("\(base.timecode(clip.inicio + rampa.frame))").font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                            Text(String(format: "%.0f %%", rampa.velocidad * 100)).font(.system(size: 9))
                            Spacer()
                            Button(role: .destructive) {
                                editor.quitarRampaDeVelocidad(clip.id, en: rampa.frame)
                            } label: { Image(systemName: "xmark.circle").font(.system(size: 9)) }
                            .buttonStyle(.plain)
                            .help("Quitar esta rampa")
                        }
                    }
                    Button("Quitar todas las rampas") { editor.quitarRampasDeVelocidad(clip.id) }
                        .controlSize(.small)
                }
                HStack {
                    Button("Rampa en el cabezal") {
                        editor.anadirRampaDeVelocidad(clip.id, en: editor.cabezal, velocidad: 0.5)
                    }
                    .help("Keyframe de velocidad al 50 % desde el cabezal")
                    Button("Congelar desde el cabezal") {
                        editor.anadirRampaDeVelocidad(clip.id, en: editor.cabezal, velocidad: 0)
                    }
                    .help("Freeze frame: la imagen se queda quieta desde el cabezal")
                }
                .controlSize(.small)
            }

            Section("Organización") {
                Picker("Etiqueta", selection: Binding(
                    get: { clip.etiqueta },
                    set: { editor.etiquetar(clip.id, $0) }
                )) {
                    ForEach(EtiquetaDeColor.allCases, id: \.self) { Text($0.nombre).tag($0) }
                }
                Button("Quitar dejando hueco") { editor.removeSelectedClip() }
                Button("Quitar y cerrar hueco", role: .destructive) { editor.borrarConArrastre() }
            }
            }
        }
        .formStyle(.grouped)
    }

    /// La puerta de ruido de la pista: umbral, profundidad y tiempos. El
    /// interruptor crea o retira el efecto; los mandos solo aparecen activos.
    private func puertaDeRuido(_ pista: Pista) -> some View {
        DisclosureGroup("Puerta de ruido") {
            Toggle("Activa", isOn: Binding(
                get: { pista.puertaDeRuido != nil },
                set: { on in editor.fijarPuertaDeRuido(
                    on ? (pista.puertaDeRuido ?? PuertaDeRuidoDePista()) : nil,
                    enPista: pista.id
                ) }
            ))
            if let puerta = pista.puertaDeRuido {
                deslizador("Umbral", puerta.umbralDB, -70...(-10), "dB") { valor in
                    var nueva = puerta; nueva.umbralDB = valor
                    editor.fijarPuertaDeRuido(nueva, enPista: pista.id)
                }
                deslizador("Profundidad", puerta.profundidad * 100, 0.1...10, "%") { valor in
                    var nueva = puerta; nueva.profundidad = valor / 100
                    editor.fijarPuertaDeRuido(nueva, enPista: pista.id)
                }
            }
        }
        .controlSize(.small)
    }

    /// El compresor multibanda: tres bandas fijas (250 Hz y 4 kHz) con su
    /// umbral y ratio cada una. El «de-esser» y el control de retumbe.
    private func multibanda(_ pista: Pista) -> some View {
        DisclosureGroup("Multibanda") {
            Toggle("Activa", isOn: Binding(
                get: { pista.multibanda != nil },
                set: { on in editor.fijarMultibanda(
                    on ? (pista.multibanda ?? CompresorMultibandaDePista()) : nil,
                    enPista: pista.id
                ) }
            ))
            if let banda = pista.multibanda {
                ForEach([("Graves", banda.graves, { (b: BandaDeMultibanda) in
                    var nueva = banda; nueva.graves = b; editor.fijarMultibanda(nueva, enPista: pista.id)
                }), ("Medios", banda.medios, { (b: BandaDeMultibanda) in
                    var nueva = banda; nueva.medios = b; editor.fijarMultibanda(nueva, enPista: pista.id)
                }), ("Agudos", banda.agudos, { (b: BandaDeMultibanda) in
                    var nueva = banda; nueva.agudos = b; editor.fijarMultibanda(nueva, enPista: pista.id)
                })], id: \.0) { nombre, banda, aplicar in
                    VStack(alignment: .leading, spacing: 1) {
                        Text(nombre).font(.system(size: 10, weight: .medium))
                        deslizador("Umbral", banda.umbralDB, -60...0, "dB") { valor in
                            var nueva = banda; nueva.umbralDB = valor; aplicar(nueva)
                        }
                        deslizador("Ratio", banda.ratio, 1...20, ":1") { valor in
                            var nueva = banda; nueva.ratio = valor; aplicar(nueva)
                        }
                    }
                }
            }
        }
        .controlSize(.small)
    }

    /// Reverb y retardo: los efectos de inserción al final de la cadena.
    private func reverbYRetardo(_ pista: Pista) -> some View {
        Group {
            DisclosureGroup("Reverb") {
                Toggle("Activa", isOn: Binding(
                    get: { pista.reverb != nil },
                    set: { on in editor.fijarReverb(
                        on ? (pista.reverb ?? ReverbDePista()) : nil, enPista: pista.id
                    ) }
                ))
                if let reverb = pista.reverb {
                    deslizador("Tamaño", reverb.tamano * 100, 5...100, "%") { valor in
                        var nueva = reverb; nueva.tamano = valor / 100
                        editor.fijarReverb(nueva, enPista: pista.id)
                    }
                    deslizador("Mezcla", reverb.mezcla * 100, 0...100, "%") { valor in
                        var nueva = reverb; nueva.mezcla = valor / 100
                        editor.fijarReverb(nueva, enPista: pista.id)
                    }
                }
            }
            .controlSize(.small)
            DisclosureGroup("Retardo") {
                Toggle("Activa", isOn: Binding(
                    get: { pista.retardo != nil },
                    set: { on in editor.fijarRetardo(
                        on ? (pista.retardo ?? RetardoDePista()) : nil, enPista: pista.id
                    ) }
                ))
                if let retardo = pista.retardo {
                    deslizador("Tiempo", retardo.tiempoEnSegundos * 1000, 10...2000, "ms") { valor in
                        var nueva = retardo; nueva.tiempoEnSegundos = valor / 1000
                        editor.fijarRetardo(nueva, enPista: pista.id)
                    }
                    deslizador("Realimentación", retardo.realimentacion * 100, 0...90, "%") { valor in
                        var nueva = retardo; nueva.realimentacion = valor / 100
                        editor.fijarRetardo(nueva, enPista: pista.id)
                    }
                    deslizador("Mezcla", retardo.mezcla * 100, 0...100, "%") { valor in
                        var nueva = retardo; nueva.mezcla = valor / 100
                        editor.fijarRetardo(nueva, enPista: pista.id)
                    }
                }
            }
            .controlSize(.small)
        }
    }

    /// Las tres ruedas de color del clip: sombras, medios y altas, con un
    /// deslizador por canal (−100…100). Son los mandos de «color balance» de
    /// Photoshop, a la manera de las ruedas de Resolve.
    private func ruedasDeColor(_ clip: Clip) -> some View {        let ruedas = clip.color.ruedas ?? .neutras
        return VStack(alignment: .leading, spacing: 6) {
            ForEach([("Sombras", ruedas.sombrasRojo, ruedas.sombrasVerde, ruedas.sombrasAzul,
                      { (r: Double, v: Double, a: Double) in editor.fijarRueda(clip.id) { $0.sombrasRojo = r; $0.sombrasVerde = v; $0.sombrasAzul = a } }),
                     ("Medios", ruedas.mediosRojo, ruedas.mediosVerde, ruedas.mediosAzul,
                      { (r: Double, v: Double, a: Double) in editor.fijarRueda(clip.id) { $0.mediosRojo = r; $0.mediosVerde = v; $0.mediosAzul = a } }),
                     ("Altas", ruedas.altasRojo, ruedas.altasVerde, ruedas.altasAzul,
                      { (r: Double, v: Double, a: Double) in editor.fijarRueda(clip.id) { $0.altasRojo = r; $0.altasVerde = v; $0.altasAzul = a } })],
                   id: \.0) { titulo, rojo, verde, azul, aplicar in
                VStack(alignment: .leading, spacing: 1) {
                    Text(titulo).font(.system(size: 10, weight: .medium))
                    canalDeRueda("R", rojo, .red) { aplicar($0, verde, azul) }
                    canalDeRueda("V", verde, .green) { aplicar(rojo, $0, azul) }
                    canalDeRueda("A", azul, .blue) { aplicar(rojo, verde, $0) }
                }
            }
            Button("Restablecer ruedas") { editor.fijarRueda(clip.id) { $0 = .neutras } }
                .controlSize(.small)
        }
        .padding(.vertical, 2)
    }

    /// Un canal de una rueda: deslizador −100…100 con su color.
    private func canalDeRueda(_ inicial: String, _ valor: Double, _ color: Color, _ cambio: @escaping (Double) -> Void) -> some View {
        HStack(spacing: 6) {
            Text(inicial).font(.system(size: 9, design: .monospaced)).foregroundStyle(color).frame(width: 12)
            Slider(value: Binding(get: { valor * 100 }, set: { cambio($0 / 100) }), in: -100...100)
                .tint(color.opacity(0.7))
            Text(String(format: "%+.0f", valor * 100))
                .font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary).frame(width: 30, alignment: .trailing)
        }
    }

    /// Deslizador con valor numérico, que marca su propio punto de deshacer al
    /// empezar el arrastre en vez de anotar uno por fotograma.
    private func deslizador(
        _ etiqueta: String,
        _ valor: Double,
        _ rango: ClosedRange<Double>,
        _ unidad: String,
        _ cambio: @escaping (Double) -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack {
                Text(etiqueta).font(.system(size: 11))
                Spacer()
                Text(String(format: "%.0f %@", valor, unidad))
                    .font(.system(size: 10, design: .monospaced)).foregroundStyle(.secondary)
            }
            Slider(
                value: Binding(get: { min(max(valor, rango.lowerBound), rango.upperBound) }, set: cambio),
                in: rango
            ) { empezando in
                if empezando { editor.beginTrim() } else { editor.endTrim() }
            }
        }
    }

    private var timeline: some View { VistaMontaje(editor: editor) }

    /// El panel de historia: las últimas ediciones, de la más reciente a la
    /// más antigua, con su descripción. Pulsar una deshace hasta ella.
    private var historiaDeEdicion: some View {
        DisclosureGroup(isExpanded: $historiaExpandida) {
            if editor.historiaDeEdicion.isEmpty {
                Text("Sin ediciones todavía").font(.system(size: 9)).foregroundStyle(.tertiary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 1) {
                        ForEach(Array(editor.historiaDeEdicion.enumerated()), id: \.offset) { posicion, descripcion in
                            Button {
                                editor.deshacerHasta(posicion + 1)
                            } label: {
                                HStack(spacing: 6) {
                                    Text("\(editor.historiaDeEdicion.count - posicion)").font(.system(size: 9, design: .monospaced))
                                        .foregroundStyle(.secondary).frame(width: 18, alignment: .trailing)
                                    Text(descripcion).font(.system(size: 10)).lineLimit(1)
                                    Spacer()
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .help("Deshacer hasta aquí")
                        }
                    }
                    .padding(.vertical, 2)
                }
                .frame(maxHeight: 130)
            }
        } label: {
            Label("Historia (\(editor.historiaDeEdicion.count))", systemImage: "clock.arrow.circlepath")
                .font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
        }
        .padding(10)
    }

    private var statusBar: some View {
        HStack(spacing: 8) {
            if editor.isExporting {
                ProgressView(value: editor.exportProgress)
                    .controlSize(.mini).frame(width: 90)
                Button("Cancelar") { editor.cancelarExportacion() }
                    .buttonStyle(.borderless)
                    .font(.system(size: 10, weight: .semibold))
                    .help("Cancelar la exportación actual sin borrar el archivo anterior")
                    .accessibilityLabel("Cancelar exportación")
            }
            Text(editor.status)
                .lineLimit(1)
                .truncationMode(.tail)
                .help(editor.status)
                .foregroundStyle(editor.status.contains("FALLO") || editor.status.contains("error") ? .orange : .primary)
            Spacer()
            Text(editor.timebase.nombre)
                .font(.system(size: 9, design: .monospaced)).foregroundStyle(.tertiary)
            Text("AVFoundation · Apple Silicon").foregroundStyle(.tertiary)
        }
        .font(.system(size: 10)).padding(.horizontal, 12).frame(height: 26)
    }

    private func panelTitle(_ title: String, count: Int?) -> some View {
        HStack {
            Text(title).font(.system(size: 10, weight: .bold)).tracking(0.8).lineLimit(1).truncationMode(.tail)
            if let count { Text("\(count)").foregroundStyle(.tertiary) }
            Spacer()
        }
        .padding(.horizontal, 12).frame(height: 34)
        .background(Color(nsColor: NSColor(calibratedWhite: 0.095, alpha: 1)))
    }
}

extension Double {
    var timecode: String {
        guard isFinite && self >= 0 else { return "00:00:00" }
        let total = Int(self.rounded(.down))
        return String(format: "%02d:%02d:%02d", total / 3600, (total / 60) % 60, total % 60)
    }
}


@main
struct EditorcitoApp: App {

    /// El estado vive en la escena, no en la vista, porque la barra de menús del
    /// sistema tiene que poder llamar a las mismas acciones que los botones.
    @StateObject private var editor = EditorState()

    var body: some Scene {
        WindowGroup("Editorcito") {
            ContentView(editor: editor)
                .sheet(item: Binding(
                    get: { editor.clipConCurvasAbiertas.map { CurvasSheet(id: $0) } },
                    set: { nuevo in
                        if nuevo == nil { editor.cerrarEditorDeCurvas() }
                    }
                )) { hoja in
                    CurvasEditorView(editor: editor, clipID: hoja.id)
                }
        }
        .commands { MenusDeEditorcito(editor: editor) }
    }
}

/// Identidad del sheet de curvas: con `Identifiable` el sheet se abre y se
/// cierra correctamente al cambiar el clip seleccionado.
private struct CurvasSheet: Identifiable {
    let id: UUID
}

/// Barra de nivel en vivo de una pista de audio: lee el pico que el tap de
/// mezcla escribe en el registro compartido y lo dibuja como un medidor de
/// 0 dBFS hacia abajo. Se actualiza con un temporizador de 10 Hz mientras la
/// vista existe.
private struct MedidorEnVivoView: View {
    let pistaID: UUID
    @State private var nivel: Double? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text("Nivel").font(.system(size: 9)).foregroundStyle(.secondary)
                Spacer()
                Text(nivel.map { String(format: "%.1f dBFS", $0) } ?? "—")
                    .font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
            }
            GeometryReader { geometria in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(.white.opacity(0.08))
                    // El medidor va de −60 dBFS (izquierda) a 0 (derecha).
                    if let nivel {
                        let fraccion = min(max((nivel + 60) / 60, 0), 1)
                        RoundedRectangle(cornerRadius: 2)
                            .fill(nivel > -6 ? Color.red : (nivel > -18 ? Color.yellow : Color.green))
                            .frame(width: geometria.size.width * fraccion)
                    }
                }
            }
            .frame(height: 6)
        }
        .onReceive(Timer.publish(every: 0.1, on: .main, in: .common).autoconnect()) { _ in
            nivel = MedidorEnVivo.compartido.picoDe(pistaID.uuidString).map { 20 * log10($0) }
        }
    }
}

/// Editor de curvas RGB: un lienzo por canal donde se arrastran los puntos de
/// control y se ve la curva en vivo en el monitor.
private struct CurvasEditorView: View {
    @ObservedObject var editor: EditorState
    let clipID: UUID
    @State private var canal: CanalDeCurva = .luma

    var body: some View {
        VStack(spacing: 10) {
            Text("Curvas RGB").font(.headline)
            Text(editor.media.first { $0.id == clipID }?.name ?? "Clip")
                .font(.caption).foregroundStyle(.secondary)
            Picker("Canal", selection: $canal) {
                ForEach(CanalDeCurva.allCases) { Text($0.nombre).tag($0) }
            }
            .pickerStyle(.segmented)
            .frame(width: 320)

            CurvaCanvas(
                puntos: Binding(
                    get: { editor.curvaEnEdicion(canal) },
                    set: { editor.fijarPuntoDeCurva(canal, puntos: $0) }
                ),
                color: canal.color
            )
            .frame(width: 320, height: 220)

            HStack {
                Button("Quitar último punto") {
                    let puntos = editor.curvaEnEdicion(canal)
                    if let ultimo = puntos.indices.last, puntos.count > 2 {
                        editor.eliminarPuntoDeCurva(canal, en: ultimo)
                    }
                }
                .disabled(editor.curvaEnEdicion(canal).count <= 2)
                Spacer()
                Button("Listo") { editor.cerrarEditorDeCurvas() }
                    .keyboardShortcut(.defaultAction)
            }
            .frame(width: 320)
        }
        .padding(16)
    }
}

/// Lienzo donde se dibuja la curva y se arrastran sus puntos de control.
private struct CurvaCanvas: View {
    @Binding var puntos: [PuntoDeCurva]
    let color: Color

    var body: some View {
        GeometryReader { geometria in
            let ancho = geometria.size.width
            let alto = geometria.size.height
            ZStack {
                // Cuadrícula de cuartos.
                Path { p in
                    for i in 1...3 {
                        p.move(to: CGPoint(x: ancho * CGFloat(i) / 4, y: 0))
                        p.addLine(to: CGPoint(x: ancho * CGFloat(i) / 4, y: alto))
                        p.move(to: CGPoint(x: 0, y: alto * CGFloat(i) / 4))
                        p.addLine(to: CGPoint(x: ancho, y: alto * CGFloat(i) / 4))
                    }
                }
                .stroke(.white.opacity(0.15), lineWidth: 0.5)

                // La curva interpolada.
                let ordenados = puntos.sorted { $0.x < $1.x }
                Path { p in
                    guard !ordenados.isEmpty else { return }
                    p.move(to: CGPoint(x: CGFloat(ordenados[0].x) * ancho, y: alto - CGFloat(ordenados[0].y) * alto))
                    for punto in ordenados.dropFirst() {
                        p.addLine(to: CGPoint(x: CGFloat(punto.x) * ancho, y: alto - CGFloat(punto.y) * alto))
                    }
                }
                .stroke(color, lineWidth: 2)

                // Puntos de control arrastrables.
                ForEach(Array(puntos.enumerated()), id: \.offset) { indice, punto in
                    Circle()
                        .fill(color)
                        .frame(width: 10, height: 10)
                        .position(x: CGFloat(punto.x) * ancho, y: alto - CGFloat(punto.y) * alto)
                        .gesture(DragGesture(minimumDistance: 0).onChanged { valor in
                            var nuevos = puntos
                            var movido = punto
                            movido.x = min(max(Double(valor.location.x / ancho), 0), 1)
                            movido.y = min(max(Double(1 - valor.location.y / alto), 0), 1)
                            nuevos[indice] = movido
                            puntos = nuevos
                        })
                }
            }
            .background(Color.black.opacity(0.9))
            .clipShape(RoundedRectangle(cornerRadius: 4))
            .gesture(TapGesture(count: 2).onEnded {
                // Doble clic añade un punto donde se hizo clic.
                let posicion = mouseLocation(geometria: geometria)
                var nuevos = puntos
                nuevos.append(PuntoDeCurva(x: posicion.x, y: posicion.y))
                puntos = nuevos.sorted { $0.x < $1.x }
            })
        }
    }

    private func mouseLocation(geometria: GeometryProxy) -> (x: Double, y: Double) {
        let evento = NSApp.currentEvent
        let punto = evento?.locationInWindow ?? .zero
        let local = geometria.frame(in: .global).origin
        return (
            min(max(Double((punto.x - local.x) / geometria.size.width), 0), 1),
            min(max(Double(1 - (punto.y - local.y) / geometria.size.height), 0), 1)
        )
    }
}

/**
 Barra de menús de la aplicación.

 En macOS los atajos viven aquí, no en un menú desplegable de la barra de
 herramientas: SwiftUI solo registra `keyboardShortcut` de forma fiable cuando el
 comando cuelga de la barra del sistema. Tenerlos en un `Menu` suelto los deja
 anunciados en el tooltip y muertos en la práctica.
 */
struct MenusDeEditorcito: Commands {

    @ObservedObject var editor: EditorState
    @AppStorage("ui.escala") private var escalaDeInterfaz = 1.0

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Abrir proyecto…") { editor.openProject() }
                .keyboardShortcut("o", modifiers: .command)
            Button("Guardar proyecto…") { editor.saveProject() }
                .keyboardShortcut("s", modifiers: .command)
            Button("Guardar versión…") { editor.guardarVersion() }
                .keyboardShortcut("s", modifiers: [.command, .option])
            Divider()
            Button("Importar medios…") { editor.importMedia() }
                .keyboardShortcut("i", modifiers: .command)
            Button("Exportar película…") { editor.exportMovie() }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(editor.duracionEnFrames == 0)
            Button("Exportar EDL…") { editor.exportarEDL() }
                .keyboardShortcut("e", modifiers: [.command, .option])
                .disabled(editor.duracionEnFrames == 0)
            Button("Exportar FCPXML…") { editor.exportarFCPXML() }
                .disabled(editor.duracionEnFrames == 0)
            Button("Importar proyecto…") { editor.importarOtroProyecto() }
        }

        CommandGroup(replacing: .undoRedo) {
            Button("Deshacer") { editor.undo() }
                .keyboardShortcut("z", modifiers: .command)
                .disabled(!editor.canUndo)
            Button("Rehacer") { editor.redo() }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!editor.canRedo)
        }

        CommandMenu("Ver") {
            // Se declara con «=» y no con «+»: en un teclado español el «+» del
            // bloque principal exige ⇧, así que anunciar ⌘+ obliga a pulsar ⇧⌘=.
            // macOS acepta las dos formas cuando el atajo se registra sobre «=»,
            // que es lo que hacen Safari y Xcode.
            Button("Ampliar interfaz") { ajustar(0.1) }
                .keyboardShortcut("=", modifiers: .command)
            Button("Reducir interfaz") { ajustar(-0.1) }
                .keyboardShortcut("-", modifiers: .command)
            Button("Tamaño original") { escalaDeInterfaz = 1 }
                .keyboardShortcut("0", modifiers: .command)

            Divider()

            Button(editor.mostrarTranscript ? "Ocultar panel de texto" : "Mostrar panel de texto") {
                editor.mostrarTranscript.toggle()
            }
            .keyboardShortcut("t", modifiers: [.command, .shift])

            Toggle("Transcribir al importar", isOn: $editor.transcribirAlImportar)
            if editor.transcribing || !editor.colaDeTranscripcion.isEmpty {
                Button("Cancelar cola de transcripción") { editor.cancelarColaDeTranscripcion() }
            }

            Button("Quitar silencios") { editor.quitarSilencios() }
                .disabled(editor.midiendoSilencios)

            Divider()

            Button("Ajustar montaje a la ventana") { editor.ajustarMontajeALaVentana() }
                .keyboardShortcut("z", modifiers: .shift)
            Button("Acercar en el montaje") { editor.acercarMontaje(1.3) }
                .keyboardShortcut("]", modifiers: .command)
            Button("Alejar en el montaje") { editor.acercarMontaje(1 / 1.3) }
                .keyboardShortcut("[", modifiers: .command)

            Divider()

            Button("Pistas más altas") { editor.fijarAlturaDePistas(editor.alturaDePistas * 1.25) }
                .keyboardShortcut(.upArrow, modifiers: [.option])
            Button("Pistas más bajas") { editor.fijarAlturaDePistas(editor.alturaDePistas / 1.25) }
                .keyboardShortcut(.downArrow, modifiers: [.option])
            Divider()
            Picker("Instrumento del monitor", selection: Binding(
                get: { editor.instrumentoDeMonitor },
                set: { nuevo in
                    editor.instrumentoDeMonitor = nuevo
                    if nuevo != .ninguno { editor.refrescarFormaDeOnda() }
                }
            )) {
                Text("Ninguno").tag(InstrumentoDeMonitor.ninguno)
                Text("Forma de onda").tag(InstrumentoDeMonitor.formaDeOnda)
                Text("Vectorscopio").tag(InstrumentoDeMonitor.vectorscopio)
                Text("Parade RGB").tag(InstrumentoDeMonitor.paradeRGB)
                Text("Histograma").tag(InstrumentoDeMonitor.histograma)
            }
        }

        CommandMenu("Montaje") {
            Button("J · Reproducir hacia atrás") { editor.teclaJ() }
            Button("K · Pausar") { editor.teclaK() }
            Button("L · Reproducir hacia delante") { editor.teclaL() }
            Divider()
            Button("Dividir en el cabezal") { editor.splitAtPlayhead() }
                .keyboardShortcut("b", modifiers: .command)
            Button("Insertar en el cabezal") { editor.insertarEnCabezal() }
            Button("Superponer en el cabezal") { editor.sobrescribirEnCabezal() }
            Divider()
            Button("Quitar dejando hueco") { editor.removeSelectedClip() }
                .keyboardShortcut(.delete, modifiers: [])
            Button("Quitar y cerrar hueco") { editor.borrarConArrastre() }
                .keyboardShortcut(.delete, modifiers: .shift)
            Button("Cerrar hueco en el cabezal") { editor.cerrarHuecoEnCabezal() }
            Divider()
            Button("Marcador en el cabezal") { editor.anadirMarcador() }
            Button("Entrada de trabajo") { editor.marcarEntradaTrabajo() }
            Button("Salida de trabajo") { editor.marcarSalidaTrabajo() }
            Button("Limpiar rango de trabajo") { editor.limpiarRangoTrabajo() }
            Button("Corte anterior") { editor.irAlCorte(haciaDelante: false) }
                .keyboardShortcut(.upArrow, modifiers: [])
            Button("Corte siguiente") { editor.irAlCorte(haciaDelante: true) }
                .keyboardShortcut(.downArrow, modifiers: [])
            Divider()
            Button("Copiar atributos") { editor.copiarAtributos() }
                .keyboardShortcut("c", modifiers: [.command, .option])
            Button("Pegar atributos") { editor.pegarAtributos() }
                .keyboardShortcut("v", modifiers: [.command, .option])
            Divider()
            Button("Match frame") { editor.matchFrame() }
                .keyboardShortcut("f", modifiers: [])
            Button("Extend edit hasta el cabezal") { editor.extendEdit() }
                .keyboardShortcut("e", modifiers: [])
            Button("Recortar entrada al cabezal") { editor.recortarHastaCabezal(borde: .entrada) }
                .keyboardShortcut("q", modifiers: [])
            Button("Recortar salida al cabezal") { editor.recortarHastaCabezal(borde: .salida) }
                .keyboardShortcut("w", modifiers: [])
            Divider()
            Button("Anidar clips seleccionados") { editor.crearNido() }
                .keyboardShortcut("g", modifiers: [.command])
                .disabled(editor.anidando)
            if let clip = editor.selectedClip, editor.esNido(clip) {
                Button("Desanidar") { editor.desanidar(clipID: clip.id) }
            }
            Divider()
            Button("Añadir título") { editor.anadirTitulo() }
                .keyboardShortcut("t", modifiers: [.command, .option])
            Button("Añadir imagen…") { editor.anadirImagen() }
            Button("Crear subclip") { editor.crearSubclip() }
        }
    }

    private func ajustar(_ delta: Double) {
        escalaDeInterfaz = min(max(escalaDeInterfaz + delta, 0.7), 1.8)
    }
}

private extension Collection {
    subscript(safe index: Index) -> Element? { indices.contains(index) ? self[index] : nil }
}
