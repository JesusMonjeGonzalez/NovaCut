//! Quinta ronda: veinte funciones que Premiere, DaVinci Resolve, Descript o
//! CapCut dan por hechas y que a NovaCut le faltaban. Elegidas para el nicho
//! declarado (entrevistas, podcast y contenido social):
//!
//! - Montaje: cerrar todos los huecos sin desincronizar, acortar pausas por
//!   la transcripción, multicámara automática según quién habla, enlace A/V,
//!   estirar un clip hasta el siguiente y montar al ritmo de los marcadores.
//! - Audio: igualar el volumen de las voces, marcadores en los golpes de la
//!   música, pitido de censura y grabación de voz en off.
//! - Imagen: Ken Burns en fotos, imagen en imagen, difuminar una zona y
//!   guías de encuadre con las zonas seguras de cada red.
//! - Entrega: reemplazar un medio conservando sus efectos, recopilar el
//!   proyecto en una carpeta, capítulos de YouTube, transcripción en texto,
//!   FCPXML para Final Cut/Resolve y subtítulos largos partidos.
//!
//! Todo reutiliza los campos que ya existen en el proyecto: ningún archivo
//! `.ncrough` cambia de formato. La lógica pura está separada de la interfaz
//! y cubierta por las pruebas del final.

use super::*;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Nivel de voz de referencia para podcast y redes (Spotify, Apple Podcasts,
/// YouTube normalizan alrededor de -14/-16 LUFS).
const LUFS_VOZ: f64 = -16.0;
/// Resolución de las envolventes de este módulo: valores RMS por segundo.
const TASA_ENVOLVENTE: f64 = 100.0;
/// Pausa entre palabras a partir de la cual «Acortar pausas» interviene.
const PAUSA_LARGA: f64 = 0.8;
/// Silencio que se conserva al acortar una pausa, repartido a ambos lados.
const PAUSA_CONSERVADA: f64 = 0.3;
/// Caracteres por línea de subtítulo (norma habitual de Netflix y la BBC).
const CARACTERES_POR_LINEA: usize = 42;

/// Cada función nueva, tal como la eligen el menú Herramientas y el centro
/// de comandos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Accion {
    CerrarHuecos,
    AcortarPausas,
    Multicamara,
    EnlaceAv,
    EstirarHastaSiguiente,
    MontarAlRitmo,
    IgualarVoces,
    MarcarGolpes,
    Pitido,
    GrabarVoz,
    KenBurns,
    PipArribaIzquierda,
    PipArribaDerecha,
    PipAbajoIzquierda,
    PipAbajoDerecha,
    DifuminarZona,
    Guias,
    ReemplazarMedio,
    Recopilar,
    CapitulosYoutube,
    ExportarTranscripcion,
    ExportarFcpxml,
    DividirSubtitulos,
    RenderizarRango,
    ExportarStems,
    SuavizarAnimacion,
    MejoraDeVoz,
    VisorMulticamara,
    ImportarFcpxml,
}

impl Accion {
    pub(super) const TODAS: [Accion; 29] = [
        Accion::VisorMulticamara,
        Accion::ImportarFcpxml,
        Accion::SuavizarAnimacion,
        Accion::MejoraDeVoz,
        Accion::RenderizarRango,
        Accion::ExportarStems,
        Accion::CerrarHuecos,
        Accion::AcortarPausas,
        Accion::Multicamara,
        Accion::EnlaceAv,
        Accion::EstirarHastaSiguiente,
        Accion::MontarAlRitmo,
        Accion::IgualarVoces,
        Accion::MarcarGolpes,
        Accion::Pitido,
        Accion::GrabarVoz,
        Accion::KenBurns,
        Accion::PipArribaIzquierda,
        Accion::PipArribaDerecha,
        Accion::PipAbajoIzquierda,
        Accion::PipAbajoDerecha,
        Accion::DifuminarZona,
        Accion::Guias,
        Accion::ReemplazarMedio,
        Accion::Recopilar,
        Accion::CapitulosYoutube,
        Accion::ExportarTranscripcion,
        Accion::ExportarFcpxml,
        Accion::DividirSubtitulos,
    ];

    pub(super) fn titulo(self) -> &'static str {
        match self {
            Accion::CerrarHuecos => "Cerrar todos los huecos",
            Accion::AcortarPausas => "Acortar pausas largas",
            Accion::Multicamara => "Multicámara automática por voz",
            Accion::EnlaceAv => "Enlace audio/vídeo",
            Accion::EstirarHastaSiguiente => "Estirar hasta el siguiente clip",
            Accion::MontarAlRitmo => "Montar al ritmo de los marcadores",
            Accion::IgualarVoces => "Igualar volumen de voces (-16 LUFS)",
            Accion::MarcarGolpes => "Marcadores en los golpes de la música",
            Accion::Pitido => "Pitido de censura en el rango I-O",
            Accion::GrabarVoz => "Grabar voz en off",
            Accion::KenBurns => "Ken Burns en fotos",
            Accion::PipArribaIzquierda => "Imagen en imagen: arriba a la izquierda",
            Accion::PipArribaDerecha => "Imagen en imagen: arriba a la derecha",
            Accion::PipAbajoIzquierda => "Imagen en imagen: abajo a la izquierda",
            Accion::PipAbajoDerecha => "Imagen en imagen: abajo a la derecha",
            Accion::DifuminarZona => "Difuminar una zona (caras, matrículas)",
            Accion::Guias => "Guías de encuadre y zonas seguras",
            Accion::ReemplazarMedio => "Reemplazar medio conservando efectos…",
            Accion::Recopilar => "Recopilar proyecto en una carpeta…",
            Accion::CapitulosYoutube => "Capítulos de YouTube desde marcadores…",
            Accion::ExportarTranscripcion => "Exportar transcripción en texto…",
            Accion::ExportarFcpxml => "Exportar FCPXML (Final Cut, Resolve)…",
            Accion::DividirSubtitulos => "Partir subtítulos largos",
            Accion::RenderizarRango => "Renderizar previsualización (I-O o todo)",
            Accion::ExportarStems => "Exportar stems de audio por pista…",
            Accion::SuavizarAnimacion => "Suavizar animación (entrada y salida lentas)",
            Accion::MejoraDeVoz => "Mejora de voz con IA (RNNoise)",
            Accion::VisorMulticamara => "Visor multicámara",
            Accion::ImportarFcpxml => "Importar FCPXML (Final Cut, Resolve)…",
        }
    }

    pub(super) fn ayuda(self) -> &'static str {
        match self {
            Accion::CerrarHuecos => "Quita el tiempo en que no hay nada en ninguna pista. Mueve todas las pistas a la vez, así que el audio separado no se desincroniza",
            Accion::AcortarPausas => "Como «Shorten word gaps» de Descript: deja en 0,3 s los silencios de más de 0,8 s entre palabras, en todas las pistas",
            Accion::Multicamara => "Selecciona un clip de vídeo por cámara (y su audio si va aparte, en la pista A del mismo número). Crea una pista nueva que corta a la cámara de quien habla",
            Accion::EnlaceAv => "Al arrastrar o borrar un clip, su audio o vídeo separado y sincronizado lo acompaña, como el enlace de Premiere",
            Accion::EstirarHastaSiguiente => "Cambia la velocidad del clip para que llene el hueco hasta el siguiente de su pista (o hasta el cabezal): el «ajustar a relleno» de Premiere",
            Accion::MontarAlRitmo => "Coloca los clips seleccionados uno tras otro, cada uno entre dos marcadores consecutivos: combínalo con los marcadores de los golpes",
            Accion::IgualarVoces => "Mide cada clip seleccionado y ajusta su ganancia para que todas las voces suenen igual de fuerte",
            Accion::MarcarGolpes => "Analiza el clip de música seleccionado y pone un marcador en cada golpe para cortar a ritmo",
            Accion::Pitido => "Silencia el audio entre la entrada y la salida y pone un pitido de 1 kHz encima",
            Accion::GrabarVoz => "Graba el micrófono desde el cabezal mientras suena el montaje; vuelve a pulsar para parar y el audio entra en una pista libre",
            Accion::KenBurns => "Zoom lento y desplazamiento suave en las fotos y fotogramas congelados seleccionados",
            Accion::PipArribaIzquierda | Accion::PipArribaDerecha | Accion::PipAbajoIzquierda | Accion::PipAbajoDerecha => "Reduce el clip seleccionado al 30 % y lo coloca en esa esquina, con margen (solo en exportaciones horizontales)",
            Accion::DifuminarZona => "Capa de ajuste con desenfoque dentro de una elipse sobre el rango I-O (o 5 s desde el cabezal); mueve la máscara en el inspector",
            Accion::Guias => "Dibuja en el monitor el recorte de la exportación (vertical, cuadrado), las zonas seguras y lo que tapa la interfaz de TikTok/Reels",
            Accion::ReemplazarMedio => "Cambia el archivo del clip seleccionado por otro sin perder color, audio, transformación ni posición",
            Accion::Recopilar => "Copia todos los medios usados a una carpeta y guarda allí una copia del proyecto que apunta a ellos: para archivar o pasar a otro equipo",
            Accion::CapitulosYoutube => "Convierte los marcadores en la lista de capítulos que YouTube lee de la descripción; la copia al portapapeles y la guarda",
            Accion::ExportarTranscripcion => "Guarda la transcripción como texto por párrafos con marcas de tiempo, para notas del episodio o un blog",
            Accion::ExportarFcpxml => "Intercambio con Final Cut Pro y DaVinci Resolve: pistas, cortes y medios; avisa de lo que el formato no lleva",
            Accion::DividirSubtitulos => "Parte los subtítulos de más de dos líneas de 42 caracteres en varios, repartiendo el tiempo",
            Accion::RenderizarRango => "Compone en segundo plano el rango I-O (o todo el montaje) para reproducirlo en tiempo real aunque lleve efectos pesados; la barra verde de la regla marca lo renderizado y cualquier cambio la invalida",
            Accion::ExportarStems => "Un WAV por pista de audio, alineados desde el segundo 0, con la ganancia de cada pista y sin máster: para mezclar en Pro Tools, Reaper o Audition",
            Accion::SuavizarAnimacion => "Convierte los keyframes lineales de los clips seleccionados (posición, escala, opacidad, color, efectos y volumen) en curvas que arrancan y frenan con suavidad, como el «Ease In/Ease Out» de Premiere",
            Accion::MejoraDeVoz => "«Limpieza de voz» (Sonido esencial) usa RNNoise, una red muy ligera que va en tiempo real en cualquier equipo, con el modelo incluido para voz grabada; un .rnnn propio en %LOCALAPPDATA%\\NovaCut\\Modelos tiene prioridad",
            Accion::VisorMulticamara => "Ventana con los cuatro ángulos en el cabezal (V1 = ángulo 1, V2 = ángulo 2…); clic en uno, o teclas 1-4, para cortar a él. Usa los proxies si los hay y, al reproducir, se refresca cada dos segundos para no cargar el equipo",
            Accion::ImportarFcpxml => "Abre un montaje de Final Cut Pro o DaVinci Resolve como secuencia nueva: cortes, pistas, entradas y clips desactivados; avisa de lo que no se trae",
        }
    }
}

/// Grupos del menú Herramientas, en el orden en que se usan al montar.
const GRUPOS: &[(&str, &[Accion])] = &[
    ("Reproducción", &[Accion::RenderizarRango]),
    (
        "Montaje",
        &[
            Accion::CerrarHuecos,
            Accion::AcortarPausas,
            Accion::Multicamara,
            Accion::EnlaceAv,
            Accion::VisorMulticamara,
            Accion::EstirarHastaSiguiente,
            Accion::MontarAlRitmo,
        ],
    ),
    (
        "Audio",
        &[
            Accion::MejoraDeVoz,
            Accion::IgualarVoces,
            Accion::MarcarGolpes,
            Accion::Pitido,
            Accion::GrabarVoz,
        ],
    ),
    (
        "Imagen",
        &[
            Accion::SuavizarAnimacion,
            Accion::KenBurns,
            Accion::PipArribaIzquierda,
            Accion::PipArribaDerecha,
            Accion::PipAbajoIzquierda,
            Accion::PipAbajoDerecha,
            Accion::DifuminarZona,
            Accion::Guias,
        ],
    ),
    (
        "Medios y entrega",
        &[
            Accion::ReemplazarMedio,
            Accion::Recopilar,
            Accion::ImportarFcpxml,
            Accion::CapitulosYoutube,
            Accion::ExportarTranscripcion,
            Accion::ExportarFcpxml,
            Accion::ExportarStems,
            Accion::DividirSubtitulos,
        ],
    ),
];

/// Estado de las funciones nuevas que vive en la aplicación, no en el
/// proyecto.
pub(super) struct Estado {
    /// Seleccionar o arrastrar un clip arrastra también su pareja A/V.
    pub(super) enlace_av: bool,
    /// Guías de encuadre sobre el monitor.
    pub(super) guias: bool,
    trabajo: Option<Trabajo>,
    grabacion: Option<Grabacion>,
    /// Texto que se copia al portapapeles en el siguiente fotograma.
    portapapeles: Option<String>,
    /// Último render de previsualización terminado.
    cache: Option<CacheRender>,
    /// Render de previsualización en marcha.
    render: Option<RenderEnCurso>,
    /// Tamaño de exportación con el que se compuso el monitor por última vez.
    ultimo_tamano: Option<(u32, u32)>,
    /// Ajuste de la secuencia al primer clip, pendiente de confirmar.
    propuesta: Option<PropuestaSecuencia>,
    /// Visores extra del monitor (no se guardan en los ajustes).
    pub(super) parade: bool,
    pub(super) histograma: bool,
    /// Visor multicámara abierto y sus fotogramas por ángulo.
    multicam: bool,
    angulos: [Option<egui::TextureHandle>; 4],
    nombres_angulos: [String; 4],
    angulos_clave: Option<(i64, u64)>,
    angulos_rx: Option<Receiver<Vec<Option<PreviewFrame>>>>,
    angulos_pedido: Option<Instant>,
}

/// Lo que haría la secuencia para coincidir con su primer clip, como el
/// aviso de «Clip no coincide» de Premiere.
struct PropuestaSecuencia {
    clip: String,
    tamano: (u32, u32),
    cadencia: Timebase,
}

impl Default for Estado {
    fn default() -> Self {
        Self {
            enlace_av: true,
            guias: false,
            trabajo: None,
            grabacion: None,
            portapapeles: None,
            cache: None,
            render: None,
            ultimo_tamano: None,
            propuesta: None,
            parade: false,
            histograma: false,
            multicam: false,
            angulos: [None, None, None, None],
            nombres_angulos: Default::default(),
            angulos_clave: None,
            angulos_rx: None,
            angulos_pedido: None,
        }
    }
}

/// Lo que invalida un render de previsualización: cualquier edición (la
/// generación del documento), el tamaño del monitor y los subtítulos
/// quemados. La mezcla y las pistas también pasan por la generación.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ClaveRender {
    generacion: u64,
    lienzo: (usize, usize),
    subtitulos: bool,
}

/// Tramo `[inicio, fin)` del montaje ya compuesto en un archivo, como las
/// barras verdes de Premiere.
#[derive(Clone, Debug)]
struct CacheRender {
    archivo: PathBuf,
    inicio: f64,
    fin: f64,
    clave: ClaveRender,
}

struct RenderEnCurso {
    rx: Receiver<Result<(), String>>,
    progreso: Arc<std::sync::Mutex<RenderProgress>>,
    cancelar: Arc<AtomicBool>,
    cache: CacheRender,
}

/// Análisis en segundo plano y la generación del documento en que empezó:
/// si el proyecto cambia mientras tanto, el resultado se descarta.
struct Trabajo {
    rx: Receiver<Resultado>,
    generacion: u64,
}

enum Resultado {
    Multicamara {
        camaras: Vec<usize>,
        desde: f64,
        hasta: f64,
        niveles: Result<Vec<Vec<f32>>, String>,
    },
    Voces(Vec<(usize, Result<f64, String>)>),
    Golpes {
        indice: usize,
        tiempos: Result<Vec<f64>, String>,
    },
    /// Trabajo que no depende del montaje abierto: solo informa.
    Recopilado(Result<String, String>),
}

/// Grabación de voz en off en curso.
struct Grabacion {
    hijo: std::process::Child,
    ruta: PathBuf,
    /// Instante de timeline donde empezó.
    inicio: f64,
    empezada: Instant,
}

impl Drop for Grabacion {
    fn drop(&mut self) {
        // Si la app se cierra grabando, FFmpeg no se queda con el micrófono.
        if let Ok(None) = self.hijo.try_wait() {
            let _ = self.hijo.kill();
            let _ = self.hijo.wait();
        }
    }
}

impl NovaCutWindows {
    /// Menú «Herramientas» de la barra superior.
    pub(super) fn menu_herramientas(&mut self, ui: &mut egui::Ui) {
        let mut elegida: Option<Accion> = None;
        let ocupado = self.mejoras.trabajo.is_some();
        let respuesta = egui::menu::menu_button(
            ui,
            egui::RichText::new(if ocupado {
                "Herramientas (analizando) ▾"
            } else {
                "Herramientas ▾"
            })
            .size(11.0),
            |ui| {
                ui.set_min_width(200.0);
                for (grupo, acciones) in GRUPOS {
                    ui.menu_button(*grupo, |ui| {
                        ui.set_min_width(300.0);
                        for accion in acciones.iter().copied() {
                            let motivo = self.no_disponible(accion);
                            let boton = ui
                                .add_enabled(
                                    motivo.is_none(),
                                    egui::Button::new(self.titulo_en_menu(accion)),
                                )
                                .on_hover_text(accion.ayuda())
                                .on_disabled_hover_text(motivo.unwrap_or(""));
                            if boton.clicked() {
                                elegida = Some(accion);
                                ui.close_menu();
                            }
                        }
                    });
                }
            },
        );
        respuesta.response.on_hover_text(
            "Cerrar huecos, multicámara automática, voces, pitido, voz en off, capítulos, FCPXML y más",
        );
        if let Some(accion) = elegida {
            self.ejecutar_mejora(accion);
        }
    }

    /// Botón de una herramienta dentro de un panel: el mismo aspecto, la
    /// misma ayuda y el mismo motivo de «no disponible» que en el menú, para
    /// que cada función se comporte igual esté donde esté.
    pub(super) fn boton_de_herramienta(&mut self, ui: &mut egui::Ui, accion: Accion, texto: &str) {
        let motivo = self.no_disponible(accion);
        let respuesta = ui
            .add_enabled(motivo.is_none(), egui::Button::new(texto))
            .on_hover_text(accion.ayuda())
            .on_disabled_hover_text(format!(
                "{}\n\nAhora no está disponible: {}",
                accion.ayuda(),
                motivo.unwrap_or("")
            ));
        if respuesta.clicked() {
            self.ejecutar_mejora(accion);
        }
    }

    /// Los conmutadores muestran su estado en el propio texto.
    fn titulo_en_menu(&self, accion: Accion) -> String {
        match accion {
            Accion::EnlaceAv => format!(
                "Enlace audio/vídeo: {}",
                if self.mejoras.enlace_av { "activado" } else { "desactivado" }
            ),
            Accion::Guias => format!(
                "Guías de encuadre: {}",
                if self.mejoras.guias { "visibles" } else { "ocultas" }
            ),
            Accion::GrabarVoz if self.mejoras.grabacion.is_some() => {
                "Parar la grabación de voz".to_owned()
            }
            Accion::RenderizarRango if self.mejoras.render.is_some() => {
                "Cancelar el render de previsualización".to_owned()
            }
            Accion::VisorMulticamara if self.mejoras.multicam => {
                "Cerrar el visor multicámara".to_owned()
            }
            _ => accion.titulo().to_owned(),
        }
    }

    /// Por qué una acción no puede ejecutarse ahora; `None` si puede.
    pub(super) fn no_disponible(&self, accion: Accion) -> Option<&'static str> {
        let ocupado = self.mejoras.trabajo.is_some();
        let seleccion = self.selected_indices();
        let hay_clips = !self.project.clips.is_empty();
        let con_audio = seleccion
            .iter()
            .any(|index| self.project.clips[*index].has_audio);
        match accion {
            Accion::CerrarHuecos | Accion::DifuminarZona | Accion::ExportarFcpxml => {
                (!hay_clips).then_some("El montaje está vacío")
            }
            Accion::AcortarPausas | Accion::ExportarTranscripcion => self
                .project
                .transcript
                .is_empty()
                .then_some("Primero transcribe el montaje (pestaña Transcripción)"),
            Accion::Multicamara if ocupado => Some("Hay otro análisis en marcha"),
            Accion::Multicamara => (seleccion.len() < 2)
                .then_some("Selecciona un clip de vídeo por cámara (de 2 a 4)"),
            Accion::IgualarVoces | Accion::MarcarGolpes if ocupado => {
                Some("Hay otro análisis en marcha")
            }
            Accion::IgualarVoces | Accion::MarcarGolpes => {
                (!con_audio).then_some("Selecciona clips con audio")
            }
            Accion::MontarAlRitmo if self.project.markers.len() < 2 => {
                Some("Hacen falta al menos dos marcadores (M)")
            }
            Accion::MontarAlRitmo
            | Accion::KenBurns
            | Accion::PipArribaIzquierda
            | Accion::PipArribaDerecha
            | Accion::PipAbajoIzquierda
            | Accion::PipAbajoDerecha
            | Accion::EstirarHastaSiguiente
            | Accion::ReemplazarMedio => seleccion.is_empty().then_some("Selecciona un clip"),
            Accion::Pitido => (self.work_in.is_none() || self.work_out.is_none())
                .then_some("Marca entrada (I) y salida (O) sobre lo que hay que tapar"),
            Accion::Recopilar if ocupado => Some("Hay otro trabajo en marcha"),
            Accion::Recopilar => (!hay_clips && self.project.library.is_empty())
                .then_some("El proyecto no usa ningún medio"),
            Accion::CapitulosYoutube => self
                .project
                .markers
                .is_empty()
                .then_some("Añade un marcador (M) donde empieza cada capítulo"),
            Accion::DividirSubtitulos => self
                .project
                .subtitles
                .is_empty()
                .then_some("No hay subtítulos"),
            Accion::GrabarVoz => (!self.ffmpeg_ready).then_some("Hace falta FFmpeg"),
            Accion::EnlaceAv | Accion::Guias => None,
            Accion::RenderizarRango if self.mejoras.render.is_some() => None,
            Accion::RenderizarRango => {
                if !self.ffmpeg_ready {
                    Some("Hace falta FFmpeg")
                } else {
                    (!hay_clips).then_some("El montaje está vacío")
                }
            }
            Accion::SuavizarAnimacion => seleccion.is_empty().then_some("Selecciona clips con keyframes"),
            Accion::MejoraDeVoz | Accion::VisorMulticamara => None,
            Accion::ImportarFcpxml => (!self.ffmpeg_ready).then_some("Hace falta FFmpeg"),
            Accion::ExportarStems if ocupado => Some("Hay otro trabajo en marcha"),
            Accion::ExportarStems => (!self.project.clips.iter().any(|clip| clip.has_audio))
                .then_some("El montaje no tiene audio"),
        }
    }

    pub(super) fn ejecutar_mejora(&mut self, accion: Accion) {
        if let Some(motivo) = self.no_disponible(accion) {
            self.status = motivo.to_owned();
            return;
        }
        match accion {
            Accion::CerrarHuecos => self.cerrar_todos_los_huecos(),
            Accion::AcortarPausas => self.acortar_pausas(),
            Accion::Multicamara => self.empezar_multicamara(),
            Accion::EnlaceAv => {
                self.mejoras.enlace_av = !self.mejoras.enlace_av;
                self.status = if self.mejoras.enlace_av {
                    "Enlace A/V activado: el audio separado acompaña a su vídeo".to_owned()
                } else {
                    "Enlace A/V desactivado: cada clip se selecciona por separado".to_owned()
                };
            }
            Accion::EstirarHastaSiguiente => self.estirar_hasta_siguiente(),
            Accion::MontarAlRitmo => self.montar_al_ritmo(),
            Accion::IgualarVoces => self.empezar_igualar_voces(),
            Accion::MarcarGolpes => self.empezar_marcar_golpes(),
            Accion::Pitido => self.pitido_de_censura(),
            Accion::GrabarVoz => {
                if self.mejoras.grabacion.is_some() {
                    self.parar_grabacion();
                } else {
                    self.empezar_grabacion();
                }
            }
            Accion::KenBurns => self.ken_burns(),
            Accion::PipArribaIzquierda => self.imagen_en_imagen(-1.0, -1.0),
            Accion::PipArribaDerecha => self.imagen_en_imagen(1.0, -1.0),
            Accion::PipAbajoIzquierda => self.imagen_en_imagen(-1.0, 1.0),
            Accion::PipAbajoDerecha => self.imagen_en_imagen(1.0, 1.0),
            Accion::DifuminarZona => self.difuminar_zona(),
            Accion::Guias => {
                self.mejoras.guias = !self.mejoras.guias;
                self.status = if self.mejoras.guias {
                    format!(
                        "Guías visibles para {}×{}",
                        self.export_size.0, self.export_size.1
                    )
                } else {
                    "Guías ocultas".to_owned()
                };
            }
            Accion::ReemplazarMedio => self.reemplazar_medio(),
            Accion::Recopilar => self.recopilar_proyecto(),
            Accion::CapitulosYoutube => self.capitulos_de_youtube(),
            Accion::ExportarTranscripcion => self.exportar_transcripcion(),
            Accion::ExportarFcpxml => self.exportar_fcpxml(),
            Accion::DividirSubtitulos => self.partir_subtitulos(),
            Accion::RenderizarRango => {
                if let Some(render) = &self.mejoras.render {
                    render.cancelar.store(true, Ordering::Relaxed);
                    self.status = "Cancelando el render de previsualización…".to_owned();
                } else {
                    self.empezar_render_previsualizacion();
                }
            }
            Accion::ExportarStems => self.exportar_stems(),
            Accion::SuavizarAnimacion => self.suavizar_animacion(),
            Accion::MejoraDeVoz => self.mejora_de_voz(),
            Accion::VisorMulticamara => {
                self.mejoras.multicam = !self.mejoras.multicam;
                self.mejoras.angulos_clave = None;
            }
            Accion::ImportarFcpxml => self.importar_fcpxml(),
        }
    }

    /// Recoge los análisis en segundo plano, la grabación y el portapapeles.
    pub(super) fn poll_mejoras(&mut self, context: &egui::Context) {
        if let Some(texto) = self.mejoras.portapapeles.take() {
            context.copy_text(texto);
        }
        // El monitor compone con la proporción de la exportación: al cambiar
        // de 16:9 a vertical, el fotograma se vuelve a componer.
        if self.mejoras.ultimo_tamano != Some(self.export_size) {
            let habia = self.mejoras.ultimo_tamano.is_some();
            self.mejoras.ultimo_tamano = Some(self.export_size);
            if habia && self.ffmpeg_ready && self.playback.is_none() && !self.project.clips.is_empty() {
                self.request_preview();
            }
        }
        self.sondear_render(context);
        let grabacion_terminada = self
            .mejoras
            .grabacion
            .as_mut()
            .map(|grabacion| matches!(grabacion.hijo.try_wait(), Ok(Some(_))));
        match grabacion_terminada {
            // FFmpeg se cerró solo: el micrófono se desconectó o no existe.
            Some(true) => {
                if let Some(grabacion) = self.mejoras.grabacion.take() {
                    let (ruta, inicio) = (grabacion.ruta.clone(), grabacion.inicio);
                    drop(grabacion);
                    self.colocar_grabacion(&ruta, inicio);
                }
            }
            Some(false) => {
                if let Some(grabacion) = &self.mejoras.grabacion {
                    self.status = format!(
                        "Grabando voz en off · {} · Herramientas › Audio › Parar la grabación",
                        format_clock(grabacion.empezada.elapsed().as_secs_f64())
                    );
                }
                context.request_repaint_after(Duration::from_millis(250));
            }
            None => {}
        }
        let Some(trabajo) = &self.mejoras.trabajo else {
            return;
        };
        context.request_repaint_after(Duration::from_millis(200));
        let resultado = match trabajo.rx.try_recv() {
            Ok(resultado) => resultado,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.mejoras.trabajo = None;
                self.status = "El análisis terminó sin resultado".to_owned();
                return;
            }
        };
        let vigente = trabajo.generacion == self.document_generation;
        self.mejoras.trabajo = None;
        match resultado {
            Resultado::Recopilado(Ok(mensaje)) => self.status = mensaje,
            Resultado::Recopilado(Err(error)) => self.status = error,
            _ if !vigente => {
                self.status = "Análisis descartado: el montaje cambió mientras tanto".to_owned()
            }
            Resultado::Multicamara {
                camaras,
                desde,
                hasta,
                niveles,
            } => match niveles {
                Ok(niveles) => self.aplicar_multicamara(&camaras, desde, hasta, &niveles),
                Err(error) => self.status = format!("No se pudo escuchar las cámaras: {error}"),
            },
            Resultado::Voces(medidas) => self.aplicar_voces(medidas),
            Resultado::Golpes { indice, tiempos } => match tiempos {
                Ok(tiempos) => self.aplicar_golpes(indice, &tiempos),
                Err(error) => self.status = format!("No se pudo analizar la música: {error}"),
            },
        }
    }

    // ----- Bloque 3: visores, multicámara e intercambio ----------------

    /// Visores que pinta el monitor al reproducir.
    pub(super) fn monitor_scopes(&self) -> MonitorScopes {
        MonitorScopes {
            waveform: self.show_waveform,
            vectorscope: self.show_vectorscope,
            parade: self.mejoras.parade,
            histogram: self.mejoras.histograma,
        }
    }

    /// Ventanas propias de las mejoras; se llama una vez por fotograma.
    pub(super) fn ventanas_mejoras(&mut self, context: &egui::Context) {
        self.mostrar_propuesta_secuencia(context);
        self.visor_multicamara(context);
    }

    /// Ángulo `k` (0…3) en el cabezal: el clip de vídeo de la pista del
    /// plano base más `k`, con el archivo que conviene leer (el proxy si hay)
    /// y el instante de origen.
    fn fuentes_de_angulos(&self) -> Vec<Option<(PathBuf, f64, String)>> {
        let cabezal = self.playhead;
        let visible = |clip: &RoughClip| {
            clip.has_video
                && clip.enabled
                && clip.title.is_none()
                && !clip.is_adjustment
                && clip.nested.is_none()
                && !clip.path.as_os_str().is_empty()
                && cabezal >= clip.timeline_start
                && cabezal < clip.timeline_start + clip.duration()
        };
        let Some(base) = self
            .project
            .clips
            .iter()
            .filter(|clip| visible(*clip))
            .map(|clip| clip.track)
            .min()
        else {
            return vec![None, None, None, None];
        };
        (0..4)
            .map(|angulo| {
                let clip = self
                    .project
                    .clips
                    .iter()
                    .filter(|clip| visible(*clip) && clip.track == base + angulo)
                    .max_by(|a, b| a.timeline_start.total_cmp(&b.timeline_start))?;
                let origen = match clip.freeze_at {
                    Some(congelado) => congelado,
                    None => {
                        clip.in_seconds
                            + (cabezal - clip.timeline_start) * clip.speed.clamp(0.1, 8.0)
                    }
                };
                let ruta = clip
                    .proxy
                    .clone()
                    .filter(|proxy| self.use_proxies && proxy.is_file())
                    .unwrap_or_else(|| clip.path.clone());
                Some((ruta, origen, format!("V{} · {}", clip.track + 1, clip.name())))
            })
            .collect()
    }

    fn visor_multicamara(&mut self, context: &egui::Context) {
        if !self.mejoras.multicam {
            self.mejoras.angulos_rx = None;
            return;
        }
        // Fotogramas que llegan del hilo.
        let llegados = self.mejoras.angulos_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(fotogramas) = llegados {
            self.mejoras.angulos_rx = None;
            for (indice, fotograma) in fotogramas.into_iter().enumerate().take(4) {
                self.mejoras.angulos[indice] = fotograma.map(|fotograma| {
                    let imagen = egui::ColorImage::from_rgba_unmultiplied(
                        [fotograma.width, fotograma.height],
                        &fotograma.pixels,
                    );
                    context.load_texture(
                        format!("angulo-{indice}"),
                        imagen,
                        egui::TextureOptions::LINEAR,
                    )
                });
            }
        }
        // Se piden de nuevo al mover el cabezal o editar; reproduciendo, como
        // mucho cada dos segundos: la reproducción ya ocupa dos procesos de
        // FFmpeg y en un equipo mediano no debe dar tirones. Los fotogramas
        // se sacan de uno en uno, a 320×180 y del proxy si lo hay.
        let clave = (
            self.project.timebase().frames(self.playhead),
            self.document_generation,
        );
        let reproduciendo = self.playback.is_some();
        let toca = self.mejoras.angulos_rx.is_none()
            && self.mejoras.angulos_clave != Some(clave)
            && self
                .mejoras
                .angulos_pedido
                .is_none_or(|pedido| !reproduciendo || pedido.elapsed() >= Duration::from_secs(2));
        if toca {
            let fuentes = self.fuentes_de_angulos();
            for (indice, fuente) in fuentes.iter().enumerate() {
                self.mejoras.nombres_angulos[indice] = fuente
                    .as_ref()
                    .map(|(_, _, nombre)| nombre.clone())
                    .unwrap_or_default();
            }
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let fotogramas: Vec<Option<PreviewFrame>> = fuentes
                    .iter()
                    .map(|fuente| {
                        fuente
                            .as_ref()
                            .and_then(|(ruta, origen, _)| extract_frame(ruta, *origen, 320, 180).ok())
                    })
                    .collect();
                let _ = sender.send(fotogramas);
            });
            self.mejoras.angulos_rx = Some(receiver);
            self.mejoras.angulos_clave = Some(clave);
            self.mejoras.angulos_pedido = Some(Instant::now());
        }
        if self.mejoras.angulos_rx.is_some() || reproduciendo {
            context.request_repaint_after(Duration::from_millis(250));
        }
        let mut abierto = true;
        let mut elegido: Option<usize> = None;
        egui::Window::new("Multicámara")
            .open(&mut abierto)
            .default_width(520.0)
            .resizable(true)
            .show(context, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Clic en un ángulo (o teclas 1-4) para cortar a él en el cabezal. El ángulo 1 es la pista del plano base; 2, 3 y 4, las de encima.",
                    )
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );
                ui.add_space(4.0);
                let ancho = ((ui.available_width() - 6.0) / 2.0).clamp(120.0, 420.0);
                let tamano = egui::vec2(ancho, ancho * 9.0 / 16.0);
                egui::Grid::new("rejilla-multicamara")
                    .num_columns(2)
                    .spacing([6.0, 6.0])
                    .show(ui, |ui| {
                        for indice in 0..4 {
                            let (rect, respuesta) =
                                ui.allocate_exact_size(tamano, egui::Sense::click());
                            let painter = ui.painter();
                            painter.rect_filled(rect, 4.0, egui::Color32::BLACK);
                            match &self.mejoras.angulos[indice] {
                                Some(textura) => {
                                    painter.image(
                                        textura.id(),
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                }
                                None => {
                                    painter.text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Sin ángulo",
                                        egui::FontId::proportional(11.0),
                                        theme::TEXT_FAINT,
                                    );
                                }
                            }
                            let nombre = &self.mejoras.nombres_angulos[indice];
                            painter.text(
                                egui::pos2(rect.left() + 6.0, rect.top() + 4.0),
                                egui::Align2::LEFT_TOP,
                                if nombre.is_empty() {
                                    format!("{}", indice + 1)
                                } else {
                                    format!("{} · {nombre}", indice + 1)
                                },
                                egui::FontId::proportional(11.5),
                                egui::Color32::WHITE,
                            );
                            let borde = if respuesta.hovered() {
                                theme::ACCENT
                            } else {
                                theme::STROKE
                            };
                            painter.rect_stroke(
                                rect,
                                4.0,
                                egui::Stroke::new(1.0_f32, borde),
                                egui::StrokeKind::Inside,
                            );
                            let respuesta = respuesta
                                .on_hover_text(format!("Cortar al ángulo {} ({})", indice + 1, indice + 1));
                            if respuesta.clicked() {
                                elegido = Some(indice + 1);
                            }
                            if indice % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
            });
        if !abierto {
            self.mejoras.multicam = false;
        }
        if let Some(camara) = elegido {
            self.multicam_cut(camara);
        }
    }

    fn importar_fcpxml(&mut self) {
        let Some(ruta) = FileDialog::new()
            .add_filter("Final Cut Pro XML", &["fcpxml"])
            .pick_file()
        else {
            return;
        };
        let texto = match std::fs::read_to_string(&ruta) {
            Ok(texto) => texto,
            Err(error) => {
                self.status = format!("No se pudo leer {}: {error}", ruta.display());
                return;
            }
        };
        let importado = match intercambio::importar_fcpxml(&texto) {
            Ok(importado) => importado,
            Err(error) => {
                self.status = format!("FCPXML no válido: {error}");
                return;
            }
        };
        if importado.clips.is_empty() {
            self.status = "El FCPXML no trae clips de medios que importar".to_owned();
            return;
        }
        let before = self.project.clone();
        let bin = self.project_bin.clone();
        self.stop_playback();
        let id = self.project.new_sequence(&bin);
        let nombre = self.project.unique_sequence_name(&importado.nombre);
        self.project.rename_sequence(id, &nombre);
        if let Some(cadencia) = importado.cadencia {
            self.project.set_timebase(cadencia);
        }
        let offline = importado
            .clips
            .iter()
            .filter(|clip| !clip.path.exists())
            .count();
        self.project.register_media(&importado.clips, &bin);
        self.project.clips = importado.clips;
        self.project.normalize();
        self.after_sequence_switch();
        self.finish_edit(before);
        let mut mensaje = format!(
            "«{nombre}» importada: {} clip(s)",
            self.project.clips.len()
        );
        if offline > 0 {
            mensaje.push_str(&format!(
                " · {offline} offline: usa «Buscar todos en una carpeta…»"
            ));
        }
        if !importado.avisos.is_empty() {
            mensaje.push_str(&format!(" · {}", importado.avisos.join(" · ")));
        }
        self.status = mensaje;
    }

    // ----- Bloque 1: suavizado, voz y secuencia ------------------------

    fn suavizar_animacion(&mut self) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        let before = self.project.clone();
        let mut cambiados = 0;
        for index in indices {
            let clip = &mut self.project.clips[index];
            let mut cambio = false;
            if let Some(claves) = &clip.keyframes {
                let suaves = suavizar_transformacion(claves);
                cambio |= suaves.len() != claves.len();
                clip.keyframes = Some(suaves);
            }
            for claves in clip.anim.values_mut() {
                let suaves = suavizar_claves(claves);
                cambio |= suaves.len() != claves.len();
                *claves = suaves;
            }
            let suaves = suavizar_volumen(&clip.fx.volume_keys);
            cambio |= suaves.len() != clip.fx.volume_keys.len();
            clip.fx.volume_keys = suaves;
            if cambio {
                cambiados += 1;
            }
        }
        if cambiados == 0 {
            self.project = before;
            self.status =
                "No hay animación que suavizar: añade al menos dos keyframes distintos".to_owned();
            return;
        }
        self.finish_edit(before);
        self.status = format!(
            "Animación suavizada en {cambiados} clip(s): arranca y frena con suavidad"
        );
    }

    fn mejora_de_voz(&mut self) {
        self.status = match (modelo_del_usuario(), modelo_incluido()) {
            (Some(propio), _) => format!(
                "Mejora de voz activa con tu modelo {}: sube «Limpieza de voz» en Sonido esencial del clip",
                propio.file_name().unwrap_or_default().to_string_lossy()
            ),
            (None, Some(_)) => "Mejora de voz activa con el modelo incluido: selecciona el clip y sube «Limpieza de voz» en Sonido esencial".to_owned(),
            (None, None) => "No se pudo preparar el modelo de voz (¿disco lleno o sin permisos?): se usa la limpieza clásica".to_owned(),
        };
    }

    /// Tras la primera importación en una secuencia vacía: si el clip no
    /// coincide con la secuencia, se propone ajustarla (sin hacerlo solo).
    pub(super) fn proponer_secuencia(&mut self, clip: &RoughClip) {
        let Some((ancho, alto)) = dimensiones_de_video(&clip.path) else {
            return;
        };
        let tamano = tamano_de_exportacion(ancho, alto);
        let cadencia = clip
            .source_timebase
            .map(cadencia_normalizada)
            .unwrap_or_else(|| self.project.timebase());
        if tamano == self.export_size && cadencia == self.project.timebase() {
            return;
        }
        self.mejoras.propuesta = Some(PropuestaSecuencia {
            clip: clip.name(),
            tamano,
            cadencia,
        });
    }

    pub(super) fn mostrar_propuesta_secuencia(&mut self, context: &egui::Context) {
        let Some(propuesta) = &self.mejoras.propuesta else {
            return;
        };
        let actual = self.project.timebase();
        let texto = format!(
            "«{}» es {}×{} a {}.\nLa secuencia está en {}×{} a {}.",
            propuesta.clip,
            propuesta.tamano.0,
            propuesta.tamano.1,
            texto_de_cadencia(propuesta.cadencia),
            self.export_size.0,
            self.export_size.1,
            texto_de_cadencia(actual)
        );
        let mut ajustar = false;
        let mut mantener = false;
        egui::Window::new("El clip no coincide con la secuencia")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(texto);
                ui.label(
                    egui::RichText::new(
                        "Ajustarla evita bandas negras, reencuadres inesperados y saltos de fotogramas.",
                    )
                    .size(11.5)
                    .color(theme::TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ajustar = theme::accent_button(ui, "Ajustar la secuencia al clip")
                        .on_hover_text("Cambia la cadencia (se puede deshacer) y el tamaño de exportación")
                        .clicked();
                    mantener = ui
                        .button("Mantener la secuencia")
                        .on_hover_text("El clip se adapta a la secuencia actual")
                        .clicked();
                });
            });
        if ajustar {
            let Some(propuesta) = self.mejoras.propuesta.take() else {
                return;
            };
            if propuesta.cadencia != actual {
                let before = self.project.clone();
                self.project.set_timebase(propuesta.cadencia);
                self.finish_edit(before);
            }
            self.export_size = propuesta.tamano;
            self.request_preview();
            self.status = format!(
                "Secuencia ajustada al clip: {}×{} a {}",
                propuesta.tamano.0,
                propuesta.tamano.1,
                texto_de_cadencia(propuesta.cadencia)
            );
        } else if mantener {
            self.mejoras.propuesta = None;
            self.status = "Se mantiene la secuencia; el clip se adapta a ella".to_owned();
        }
    }

    // ----- Render de previsualización -----------------------------------

    fn clave_render(&self) -> ClaveRender {
        ClaveRender {
            generacion: self.document_generation,
            lienzo: monitor_canvas(self.export_size),
            subtitulos: self.burn_subtitles,
        }
    }

    /// El render terminado, si sigue describiendo el montaje tal como está.
    fn cache_vigente(&self) -> Option<&CacheRender> {
        self.mejoras
            .cache
            .as_ref()
            .filter(|cache| cache.clave == self.clave_render() && self.pending_edit.is_none())
    }

    /// Clips que reproduce el monitor: los del montaje, con el tramo ya
    /// renderizado sustituido por su archivo si el render sigue vigente.
    pub(super) fn clips_para_reproducir(&self) -> Vec<RoughClip> {
        let clips = self.effective_clips();
        match self.cache_vigente() {
            Some(cache) if cache.archivo.is_file() => {
                sustituir_por_render(&clips, cache).unwrap_or(clips)
            }
            _ => clips,
        }
    }

    fn empezar_render_previsualizacion(&mut self) {
        let (inicio, fin) = self
            .work_range()
            .unwrap_or((0.0, self.project.duration()));
        if fin - inicio < montaje::MIN_CLIP {
            self.status = "El rango a renderizar está vacío".to_owned();
            return;
        }
        let carpeta = std::env::temp_dir().join("NovaCut Render");
        if let Err(error) = std::fs::create_dir_all(&carpeta) {
            self.status = format!("No se pudo preparar la caché de render: {error}");
            return;
        }
        let archivo = carpeta.join(format!("render-{}-{}.mp4", std::process::id(), marca_de_tiempo()));
        let clave = self.clave_render();
        let (ventana, preroll) = montaje::window(&self.effective_clips(), inicio, fin);
        // Sin máster ni normalización: el monitor los aplica encima al
        // reproducir, como al resto del montaje.
        let trabajo = RenderJob {
            clips: ventana,
            skip: preroll,
            length: Some(fin - inicio),
            fast: true,
            size: (clave.lienzo.0 as u32, clave.lienzo.1 as u32),
            audio_only: false,
            format: ExportFormat::Mp4Video,
            master_gain_db: 0.0,
            normalize_loudness: false,
            measured_loudness: None,
            hw: None,
            ..self.render_job(archivo.clone())
        };
        let progreso = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        let cancelar = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let (progreso_hilo, cancelar_hilo) = (Arc::clone(&progreso), Arc::clone(&cancelar));
        std::thread::spawn(move || {
            let _ = sender.send(run_export(&trabajo, &cancelar_hilo, &progreso_hilo));
        });
        self.mejoras.render = Some(RenderEnCurso {
            rx: receiver,
            progreso,
            cancelar,
            cache: CacheRender {
                archivo,
                inicio,
                fin,
                clave,
            },
        });
        self.status = format!("Renderizando previsualización de {}…", format_clock(fin - inicio));
    }

    fn sondear_render(&mut self, context: &egui::Context) {
        let estado = match &self.mejoras.render {
            None => return,
            Some(render) => (
                render.rx.try_recv(),
                render.progreso.lock().map(|progreso| progreso.pct).unwrap_or(0.0),
            ),
        };
        match estado {
            (Err(mpsc::TryRecvError::Empty), pct) => {
                self.status = format!("Renderizando previsualización · {:.0} %", pct * 100.0);
                context.request_repaint_after(Duration::from_millis(250));
            }
            (Ok(Ok(())), _) => {
                let Some(render) = self.mejoras.render.take() else {
                    return;
                };
                let vigente = render.cache.clave == self.clave_render();
                if let Some(anterior) = self.mejoras.cache.replace(render.cache) {
                    let _ = std::fs::remove_file(anterior.archivo);
                }
                self.status = if vigente {
                    "Previsualización renderizada: ese tramo se reproduce en tiempo real (barra verde)"
                        .to_owned()
                } else {
                    "Render terminado, pero el montaje cambió mientras tanto: vuelve a renderizar"
                        .to_owned()
                };
            }
            (Ok(Err(error)), _) => {
                self.mejoras.render = None;
                self.status = if error == "Exportación cancelada" {
                    "Render de previsualización cancelado".to_owned()
                } else {
                    format!("Fallo el render de previsualización: {error}")
                };
            }
            (Err(mpsc::TryRecvError::Disconnected), _) => {
                self.mejoras.render = None;
                self.status = "El render de previsualización se interrumpió".to_owned();
            }
        }
    }

    /// Franja inferior de la regla: verde lo renderizado y vigente, naranja
    /// lo que se está renderizando.
    pub(super) fn dibujar_barra_de_render(
        &self,
        painter: &egui::Painter,
        regla: egui::Rect,
        pistas: egui::Rect,
        to_x: &dyn Fn(f64) -> f32,
    ) {
        let franja = |inicio: f64, fin: f64, color: egui::Color32| {
            let izquierda = to_x(inicio).max(pistas.left());
            let derecha = to_x(fin).min(pistas.right());
            if derecha > izquierda {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(izquierda, regla.bottom() - 3.0),
                        egui::pos2(derecha, regla.bottom()),
                    ),
                    0.0,
                    color,
                );
            }
        };
        if let Some(render) = &self.mejoras.render {
            franja(render.cache.inicio, render.cache.fin, theme::WARN);
        } else if let Some(cache) = self.cache_vigente() {
            franja(cache.inicio, cache.fin, theme::OK);
        }
    }

    fn exportar_stems(&mut self) {
        let Some(carpeta) = FileDialog::new()
            .set_title("Carpeta para los stems de audio")
            .pick_folder()
        else {
            return;
        };
        let clips = self.effective_clips();
        let total = self.project.duration();
        let trabajos: Vec<(usize, RenderJob)> = pistas_de_audio(&clips)
            .into_iter()
            .map(|pista| {
                let ruta = carpeta.join(format!(
                    "{} - A{}.wav",
                    nombre_de_archivo(&self.project.name),
                    pista + 1
                ));
                let trabajo = RenderJob {
                    clips: stem_de_pista(&clips, pista),
                    skip: 0.0,
                    length: Some(total),
                    fast: false,
                    audio_only: true,
                    format: ExportFormat::WavAudio,
                    master_gain_db: 0.0,
                    normalize_loudness: false,
                    measured_loudness: None,
                    hw: None,
                    ..self.render_job(ruta)
                };
                (pista, trabajo)
            })
            .collect();
        if trabajos.is_empty() {
            self.status = "No hay audio audible que exportar (revisa silencios y solos)".to_owned();
            return;
        }
        let cuantos = trabajos.len();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut fallos = Vec::new();
            for (pista, trabajo) in &trabajos {
                let progreso = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
                if let Err(error) = run_export(trabajo, &AtomicBool::new(false), &progreso) {
                    fallos.push(format!("A{}: {error}", pista + 1));
                }
            }
            let resultado = if fallos.is_empty() {
                Ok(format!(
                    "{} stem(s) de audio exportados en {}",
                    trabajos.len(),
                    carpeta.display()
                ))
            } else {
                Err(format!("Fallaron stems de audio: {}", fallos.join(" · ")))
            };
            let _ = sender.send(Resultado::Recopilado(resultado));
        });
        self.mejoras.trabajo = Some(Trabajo {
            rx: receiver,
            generacion: self.document_generation,
        });
        self.status = format!("Exportando {cuantos} stem(s) de audio…");
    }

    // ----- Montaje -----------------------------------------------------

    fn cerrar_todos_los_huecos(&mut self) {
        let intervalos = self
            .project
            .clips
            .iter()
            .map(|clip| (clip.timeline_start, clip.timeline_start + clip.duration()))
            .collect();
        let huecos = huecos_globales(intervalos, self.frame_duration() * 0.99);
        if huecos.is_empty() {
            self.status = "No hay huecos: en todo momento hay algo en alguna pista".to_owned();
            return;
        }
        self.quitar_tramos(huecos, "Huecos cerrados");
    }

    fn acortar_pausas(&mut self) {
        let tramos = pausas_largas(&self.project.transcript, PAUSA_LARGA, PAUSA_CONSERVADA);
        if tramos.is_empty() {
            self.status = "No hay pausas de más de 0,8 s entre palabras".to_owned();
            return;
        }
        self.quitar_tramos(tramos, "Pausas acortadas");
    }

    /// Quita `tramos` de todas las pistas y cierra el hueco, con
    /// transcripción, subtítulos y marcadores detrás. Mismo camino que la
    /// edición por texto, así que un solo paso de deshacer.
    fn quitar_tramos(&mut self, tramos: Vec<(f64, f64)>, que: &str) {
        let tramos = fusionar_tramos(tramos);
        let Some(primero) = tramos.first().copied() else {
            return;
        };
        if self.project.clips.iter().any(|clip| {
            self.project.clip_locked(clip) && clip.timeline_start + clip.duration() > primero.0
        }) {
            self.status =
                "Hay pistas bloqueadas con material después del primer corte: desbloquéalas"
                    .to_owned();
            return;
        }
        let clips = match transcripcion::extract_ranges(&self.project.clips, &tramos) {
            Ok(clips) => clips,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let before = self.project.clone();
        let quitado: f64 = tramos.iter().map(|(inicio, fin)| fin - inicio).sum();
        self.project.clips = clips;
        self.project.transcript = transcripcion::remap_words(&self.project.transcript, &tramos);
        self.project.subtitles = transcripcion::remap_subtitles(&self.project.subtitles, &tramos);
        self.project.markers = self
            .project
            .markers
            .iter()
            .filter_map(|marker| {
                Some(Marker {
                    time: transcripcion::remap_time(marker.time, &tramos)?,
                    name: marker.name.clone(),
                })
            })
            .collect();
        self.refresh_caption_clips();
        self.playhead = transcripcion::remap_time(self.playhead, &tramos).unwrap_or(primero.0);
        self.transcript_selection = None;
        self.clear_selection();
        self.finish_edit(before);
        self.status = format!(
            "{que}: {} tramo(s), {} menos de montaje",
            tramos.len(),
            format_clock(quitado)
        );
    }

    fn empezar_multicamara(&mut self) {
        let seleccion = self.selected_indices();
        let camaras: Vec<usize> = seleccion
            .iter()
            .copied()
            .filter(|index| {
                let clip = &self.project.clips[*index];
                clip.has_video
                    && clip.title.is_none()
                    && !clip.is_adjustment
                    && !montaje::is_complex(clip)
                    && !clip.path.as_os_str().is_empty()
            })
            .collect();
        if !(2..=4).contains(&camaras.len()) {
            self.status = "Selecciona de 2 a 4 clips de vídeo, uno por cámara".to_owned();
            return;
        }
        let mut fuentes = Vec::with_capacity(camaras.len());
        for &camara in &camaras {
            let clip = &self.project.clips[camara];
            let audio = if clip.has_audio {
                Some(camara)
            } else {
                seleccion.iter().copied().find(|other| {
                    let other = &self.project.clips[*other];
                    !other.has_video && other.has_audio && other.track == clip.track
                })
            };
            let Some(audio) = audio else {
                self.status = format!(
                    "La cámara de V{} no tiene sonido: selecciona también su audio en A{}",
                    clip.track + 1,
                    clip.track + 1
                );
                return;
            };
            fuentes.push(audio);
        }
        let implicados: Vec<usize> = camaras.iter().chain(fuentes.iter()).copied().collect();
        if implicados.iter().any(|index| {
            let clip = &self.project.clips[*index];
            (clip.speed - 1.0).abs() > 1e-6 || clip.fx.reverse
        }) {
            self.status = "Las cámaras deben ir a velocidad normal y sin invertir".to_owned();
            return;
        }
        let desde = implicados
            .iter()
            .map(|index| self.project.clips[*index].timeline_start)
            .fold(f64::NEG_INFINITY, f64::max);
        let hasta = implicados
            .iter()
            .map(|index| {
                let clip = &self.project.clips[*index];
                clip.timeline_start + clip.duration()
            })
            .fold(f64::INFINITY, f64::min);
        if hasta - desde < 2.0 {
            self.status =
                "Las cámaras apenas coinciden en el tiempo: sincronízalas primero".to_owned();
            return;
        }
        let tareas: Vec<(PathBuf, f64)> = fuentes
            .iter()
            .map(|index| {
                let clip = &self.project.clips[*index];
                (
                    clip.path.clone(),
                    clip.in_seconds + (desde - clip.timeline_start),
                )
            })
            .collect();
        let duracion = hasta - desde;
        let camaras_hilo = camaras.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let niveles = tareas
                .iter()
                .map(|(ruta, inicio)| envolvente(ruta, *inicio, duracion))
                .collect::<Result<Vec<Vec<f32>>, String>>();
            let _ = sender.send(Resultado::Multicamara {
                camaras: camaras_hilo,
                desde,
                hasta,
                niveles,
            });
        });
        self.mejoras.trabajo = Some(Trabajo {
            rx: receiver,
            generacion: self.document_generation,
        });
        self.status = format!(
            "Escuchando {} cámaras para decidir quién habla ({})…",
            camaras.len(),
            format_clock(duracion)
        );
    }

    fn aplicar_multicamara(
        &mut self,
        camaras: &[usize],
        desde: f64,
        hasta: f64,
        niveles: &[Vec<f32>],
    ) {
        // Ventanas de medio segundo; un plano dura al menos dos.
        let ventana = 50;
        let eleccion = elegir_camaras(niveles, ventana, 4);
        let tramos = tramos_de_eleccion(&eleccion, desde, hasta, ventana as f64 / TASA_ENVOLVENTE);
        if tramos.is_empty() {
            self.status = "No se oyó a nadie en las cámaras seleccionadas".to_owned();
            return;
        }
        let pista = self
            .project
            .clips
            .iter()
            .filter(|clip| clip.has_video)
            .map(|clip| clip.track + 1)
            .max()
            .unwrap_or(0);
        if pista > 15 || self.project.lane_locked(pista, true) {
            self.status = "No queda una pista de vídeo libre encima de las cámaras".to_owned();
            return;
        }
        let mut nuevos = Vec::with_capacity(tramos.len());
        for (inicio, fin, camara) in &tramos {
            let Some(&indice) = camaras.get(*camara) else {
                continue;
            };
            let clip = &self.project.clips[indice];
            if let Some(mut parte) = montaje::clip_portion(
                clip,
                inicio - clip.timeline_start,
                fin - clip.timeline_start,
            ) {
                parte.has_audio = false;
                parte.track = pista;
                parte.timeline_start = *inicio;
                parte.transition = None;
                parte.fade_in_seconds = 0.0;
                parte.fade_out_seconds = 0.0;
                nuevos.push(parte);
            }
        }
        let cambios = tramos.len().saturating_sub(1);
        let before = self.project.clone();
        let primero = self.project.clips.len();
        self.project.clips.extend(nuevos);
        self.selection = (primero..self.project.clips.len()).collect();
        self.selected = self.selection.iter().next().copied();
        self.finish_edit(before);
        self.status = format!(
            "Multicámara automática en V{}: {} planos, {cambios} cortes. Ajusta cualquiera con Rodar (N)",
            pista + 1,
            tramos.len()
        );
    }

    fn estirar_hasta_siguiente(&mut self) {
        let Some(index) = self.selected.filter(|index| *index < self.project.clips.len()) else {
            return;
        };
        let clip = self.project.clips[index].clone();
        if self.project.clip_locked(&clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if montaje::is_complex(&clip) || clip.title.is_some() || clip.is_adjustment {
            self.status = "Solo se estiran clips de medio sin rampa ni anidado".to_owned();
            return;
        }
        let fin = clip.timeline_start + clip.duration();
        let siguiente = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(other, candidate)| {
                *other != index
                    && candidate.track == clip.track
                    && candidate.has_video == clip.has_video
                    && candidate.timeline_start >= fin - 0.001
            })
            .map(|(_, candidate)| candidate.timeline_start)
            .fold(f64::INFINITY, f64::min);
        let objetivo = if siguiente.is_finite() && siguiente > fin + 0.001 {
            siguiente
        } else if self.playhead > clip.timeline_start + montaje::MIN_CLIP
            && (self.playhead - fin).abs() > 0.001
        {
            self.playhead
        } else {
            self.status =
                "No hay hueco detrás del clip: deja espacio o coloca el cabezal donde debe acabar"
                    .to_owned();
            return;
        };
        match estirado(&clip, objetivo - clip.timeline_start) {
            Ok(nuevo) => {
                let velocidad = nuevo.speed;
                let before = self.project.clone();
                self.project.clips[index] = nuevo;
                self.finish_edit(before);
                self.status = format!(
                    "Clip estirado a {} con velocidad {:.0} %",
                    format_clock(objetivo - clip.timeline_start),
                    velocidad * 100.0
                );
            }
            Err(error) => self.status = error,
        }
    }

    fn montar_al_ritmo(&mut self) {
        let mut seleccion: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| {
                let clip = &self.project.clips[*index];
                clip.has_video
                    && clip.title.is_none()
                    && !clip.is_adjustment
                    && !montaje::is_complex(clip)
                    && !self.project.clip_locked(clip)
            })
            .collect();
        if seleccion.is_empty() {
            self.status = "Selecciona clips de vídeo o fotos en pistas desbloqueadas".to_owned();
            return;
        }
        seleccion.sort_by(|left, right| {
            self.project.clips[*left]
                .timeline_start
                .total_cmp(&self.project.clips[*right].timeline_start)
        });
        let primero = self.project.clips[seleccion[0]].timeline_start;
        let mut marcas: Vec<f64> = self
            .project
            .markers
            .iter()
            .map(|marker| marker.time)
            .filter(|time| *time >= primero - self.frame_duration() / 2.0)
            .collect();
        marcas.sort_by(f64::total_cmp);
        marcas.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        if marcas.len() < 2 {
            self.status =
                "Hacen falta dos marcadores a partir del primer clip seleccionado".to_owned();
            return;
        }
        let pista = self.project.clips[seleccion[0]].track;
        let originales: Vec<RoughClip> = seleccion
            .iter()
            .map(|index| self.project.clips[*index].clone())
            .collect();
        let colocados = colocar_al_ritmo(&originales, &marcas, pista);
        let Some(fin) = colocados
            .iter()
            .map(|clip| clip.timeline_start + clip.duration())
            .reduce(f64::max)
        else {
            return;
        };
        // Se quitan de mayor a menor índice para no desplazar los pendientes.
        let mut por_indice = seleccion.clone();
        por_indice.sort_unstable();
        let mut restantes = self.project.clips.clone();
        for index in por_indice.iter().rev() {
            restantes.remove(*index);
        }
        if span_hits_complex_clip(&restantes, pista, true, marcas[0], fin, &[]) {
            self.status =
                "Entre los marcadores hay una rampa o secuencia anidada en esa pista".to_owned();
            return;
        }
        let before = self.project.clone();
        clear_track_span(&mut restantes, pista, true, marcas[0], fin, &[]);
        let primero_nuevo = restantes.len();
        let cuenta = colocados.len();
        restantes.extend(colocados);
        self.project.clips = restantes;
        self.selection = (primero_nuevo..self.project.clips.len()).collect();
        self.selected = self.selection.iter().next().copied();
        self.finish_edit(before);
        self.status = if cuenta < seleccion.len() {
            format!(
                "{cuenta} clip(s) al ritmo; {} no cupieron: faltan marcadores",
                seleccion.len() - cuenta
            )
        } else {
            format!("{cuenta} clip(s) montados al ritmo de los marcadores")
        };
    }

    // ----- Audio -------------------------------------------------------

    fn empezar_igualar_voces(&mut self) {
        let tareas: Vec<(usize, PathBuf, f64, f64)> = self
            .selected_indices()
            .into_iter()
            .filter_map(|index| {
                let clip = &self.project.clips[index];
                (clip.has_audio
                    && clip.title.is_none()
                    && clip.nested.is_none()
                    && !clip.path.as_os_str().is_empty()
                    && !self.project.clip_locked(clip))
                .then(|| {
                    (
                        index,
                        clip.path.clone(),
                        clip.in_seconds,
                        clip.source_duration(),
                    )
                })
            })
            .collect();
        if tareas.is_empty() {
            self.status = "Selecciona clips con audio en pistas desbloqueadas".to_owned();
            return;
        }
        let cuantos = tareas.len();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let medidas = tareas
                .into_iter()
                .map(|(index, ruta, inicio, duracion)| (index, medir_lufs(&ruta, inicio, duracion)))
                .collect();
            let _ = sender.send(Resultado::Voces(medidas));
        });
        self.mejoras.trabajo = Some(Trabajo {
            rx: receiver,
            generacion: self.document_generation,
        });
        self.status = format!("Midiendo el volumen de {cuantos} clip(s)…");
    }

    fn aplicar_voces(&mut self, medidas: Vec<(usize, Result<f64, String>)>) {
        let before = self.project.clone();
        let mut ajustados = Vec::new();
        let mut fallos = Vec::new();
        for (index, medida) in medidas {
            match (medida, self.project.clips.get_mut(index)) {
                (Ok(lufs), Some(clip)) => {
                    clip.gain_db = ganancia_para(lufs);
                    ajustados.push(format!("{} {:+.1} dB", clip.name(), clip.gain_db));
                }
                (Err(error), Some(clip)) => fallos.push(format!("{}: {error}", clip.name())),
                _ => {}
            }
        }
        if ajustados.is_empty() {
            self.status = format!("No se pudo medir ninguna voz: {}", fallos.join(" · "));
            return;
        }
        self.finish_edit(before);
        let mut mensaje = format!(
            "Voces igualadas a {:.0} LUFS: {}",
            LUFS_VOZ,
            ajustados.into_iter().take(4).collect::<Vec<_>>().join(", ")
        );
        if !fallos.is_empty() {
            mensaje.push_str(&format!(" · sin medir: {}", fallos.join(", ")));
        }
        self.status = mensaje;
    }

    fn empezar_marcar_golpes(&mut self) {
        let Some(index) = self
            .selected
            .filter(|index| self.project.clips.get(*index).is_some_and(|clip| clip.has_audio))
            .or_else(|| {
                self.selected_indices()
                    .into_iter()
                    .find(|index| self.project.clips[*index].has_audio)
            })
        else {
            return;
        };
        let clip = &self.project.clips[index];
        if montaje::is_complex(clip) || clip.fx.reverse || clip.path.as_os_str().is_empty() {
            self.status = "Elige un clip de audio normal (sin rampa, anidado ni invertido)".to_owned();
            return;
        }
        let (ruta, inicio, duracion) = (clip.path.clone(), clip.in_seconds, clip.source_duration());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let tiempos = envolvente(&ruta, inicio, duracion).map(|niveles| detectar_golpes(&niveles));
            let _ = sender.send(Resultado::Golpes { indice: index, tiempos });
        });
        self.mejoras.trabajo = Some(Trabajo {
            rx: receiver,
            generacion: self.document_generation,
        });
        self.status = "Buscando los golpes de la música…".to_owned();
    }

    fn aplicar_golpes(&mut self, indice: usize, tiempos: &[f64]) {
        let Some(clip) = self.project.clips.get(indice) else {
            return;
        };
        let velocidad = clip.speed.clamp(0.1, 8.0);
        let inicio = clip.timeline_start;
        let nuevos: Vec<f64> = tiempos
            .iter()
            .map(|origen| inicio + origen / velocidad)
            .filter(|time| {
                !self
                    .project
                    .markers
                    .iter()
                    .any(|marker| (marker.time - time).abs() < 0.05)
            })
            .take(500)
            .collect();
        if nuevos.is_empty() {
            self.status = "No se encontraron golpes claros en ese audio".to_owned();
            return;
        }
        let before = self.project.clone();
        let base = self.project.markers.len();
        for (numero, time) in nuevos.iter().enumerate() {
            self.project.markers.push(Marker {
                time: *time,
                name: format!("Golpe {}", base + numero + 1),
            });
        }
        self.project
            .markers
            .sort_by(|left, right| left.time.total_cmp(&right.time));
        self.finish_edit(before);
        self.status = format!(
            "{} marcadores en los golpes; selecciona clips y usa «Montar al ritmo»",
            nuevos.len()
        );
    }

    fn pitido_de_censura(&mut self) {
        let Some((inicio, fin)) = self.work_range() else {
            self.status = "Marca entrada (I) y salida (O) sobre lo que hay que tapar".to_owned();
            return;
        };
        let silenciado = match silenciar_tramo(&self.project.clips, inicio, fin, |clip| {
            self.project.clip_locked(clip)
        }) {
            Ok(clips) => clips,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let carpeta = self.carpeta_de_trabajo("NovaCut Pitidos");
        let ruta = carpeta.join(format!("pitido-{}.wav", marca_de_tiempo()));
        let duracion = fin - inicio;
        if let Err(error) = std::fs::create_dir_all(&carpeta)
            .and_then(|()| std::fs::write(&ruta, wav_pitido(duracion)))
        {
            self.status = format!("No se pudo escribir el pitido: {error}");
            return;
        }
        let pista = free_track(&silenciado, false, 0, inicio, fin);
        let before = self.project.clone();
        self.project.clips = silenciado;
        self.project.clips.push(RoughClip {
            path: ruta,
            in_seconds: 0.0,
            out_seconds: duracion,
            source_duration_seconds: Some(duracion),
            has_video: false,
            has_audio: true,
            timeline_start: inicio,
            track: pista,
            gain_db: -6.0,
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = format!(
            "Pitido de {} en A{}; el audio de debajo queda silenciado",
            format_clock(duracion),
            pista + 1
        );
    }

    fn empezar_grabacion(&mut self) {
        let carpeta = self.carpeta_de_trabajo("NovaCut Voz");
        if let Err(error) = std::fs::create_dir_all(&carpeta) {
            self.status = format!("No se pudo crear la carpeta de grabaciones: {error}");
            return;
        }
        let entrada = match entrada_de_microfono() {
            Ok(entrada) => entrada,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let ruta = carpeta.join(format!("voz-{}.wav", marca_de_tiempo()));
        let hijo = Command::new(tool_path("ffmpeg.exe"))
            .args(["-y", "-v", "error"])
            .args(&entrada)
            .args(["-ac", "1", "-ar", "48000", "-c:a", "pcm_s16le"])
            .arg(&ruta)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let hijo = match hijo {
            Ok(hijo) => hijo,
            Err(error) => {
                self.status = format!("No se pudo abrir el micrófono: {error}");
                return;
            }
        };
        let inicio = self.playhead;
        // El montaje suena mientras se graba, como la grabación de voz en
        // off de Premiere: mejor con auriculares.
        if !self.project.clips.is_empty() {
            self.start_playback(1);
        }
        self.mejoras.grabacion = Some(Grabacion {
            hijo,
            ruta,
            inicio,
            empezada: Instant::now(),
        });
        self.status = "Grabando voz en off…".to_owned();
    }

    fn parar_grabacion(&mut self) {
        let Some(mut grabacion) = self.mejoras.grabacion.take() else {
            return;
        };
        self.stop_playback();
        // «q» es la forma limpia de parar FFmpeg: cierra la cabecera del WAV.
        if let Some(entrada) = grabacion.hijo.stdin.as_mut() {
            let _ = entrada.write_all(b"q");
            let _ = entrada.flush();
        }
        let limite = Instant::now() + Duration::from_secs(4);
        loop {
            match grabacion.hijo.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < limite => {
                    std::thread::sleep(Duration::from_millis(40));
                }
                _ => {
                    let _ = grabacion.hijo.kill();
                    let _ = grabacion.hijo.wait();
                    break;
                }
            }
        }
        let (ruta, inicio) = (grabacion.ruta.clone(), grabacion.inicio);
        drop(grabacion);
        self.colocar_grabacion(&ruta, inicio);
    }

    fn colocar_grabacion(&mut self, ruta: &Path, inicio: f64) {
        let sonda = match probe_media(ruta) {
            Ok(sonda) if sonda.has_audio && sonda.duration > 0.2 => sonda,
            _ => {
                self.status =
                    "No se grabó sonido: revisa que el micrófono esté conectado y permitido"
                        .to_owned();
                return;
            }
        };
        let fin = inicio + sonda.duration;
        let pista = free_track(&self.project.clips, false, 0, inicio, fin);
        if self.project.lane_locked(pista, false) {
            self.status = format!(
                "Grabación guardada en {}, pero A{} está bloqueada",
                ruta.display(),
                pista + 1
            );
            return;
        }
        let before = self.project.clone();
        self.project.clips.push(RoughClip {
            path: ruta.to_path_buf(),
            in_seconds: 0.0,
            out_seconds: sonda.duration,
            source_duration_seconds: Some(sonda.duration),
            has_video: false,
            has_audio: true,
            timeline_start: inicio,
            track: pista,
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = format!(
            "Voz en off de {} colocada en A{}",
            format_clock(sonda.duration),
            pista + 1
        );
    }

    // ----- Imagen ------------------------------------------------------

    fn ken_burns(&mut self) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| {
                let clip = &self.project.clips[*index];
                (is_image_file(&clip.path) || clip.freeze_at.is_some())
                    && !self.project.clip_locked(clip)
            })
            .collect();
        if indices.is_empty() {
            self.status = "Selecciona fotos o fotogramas congelados".to_owned();
            return;
        }
        let before = self.project.clone();
        for (orden, index) in indices.iter().enumerate() {
            let sentido = if orden % 2 == 0 { 1.0 } else { -1.0 };
            let clip = &mut self.project.clips[*index];
            // Con entrada y salida lentas: el zoom no arranca de golpe.
            clip.keyframes = Some(suavizar_transformacion(&ken_burns_de(clip, sentido)));
        }
        self.finish_edit(before);
        self.status = if self.export_size.1 >= self.export_size.0 {
            format!(
                "Ken Burns en {} foto(s). Ojo: en vertical y cuadrado el reencuadre ignora el zoom",
                indices.len()
            )
        } else {
            format!("Ken Burns en {} foto(s)", indices.len())
        };
    }

    fn imagen_en_imagen(&mut self, horizontal: f64, vertical: f64) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| {
                let clip = &self.project.clips[*index];
                clip.has_video
                    && clip.title.is_none()
                    && !clip.is_adjustment
                    && !self.project.clip_locked(clip)
            })
            .collect();
        if indices.is_empty() {
            self.status = "Selecciona un clip de vídeo en una pista desbloqueada".to_owned();
            return;
        }
        let (x, y) = posicion_pip(horizontal, vertical, ESCALA_PIP);
        let before = self.project.clone();
        for index in &indices {
            let clip = &mut self.project.clips[*index];
            clip.scale_percent = ESCALA_PIP;
            clip.position_x = x;
            clip.position_y = y;
            // Con keyframes la transformación estática no se usaría.
            clip.keyframes = None;
        }
        self.finish_edit(before);
        self.status = if self.export_size.1 >= self.export_size.0 {
            "Imagen en imagen aplicada; en vertical y cuadrado el reencuadre llena el lienzo y no se verá"
                .to_owned()
        } else {
            "Imagen en imagen aplicada; debe estar en una pista por encima del plano principal"
                .to_owned()
        };
    }

    fn difuminar_zona(&mut self) {
        let (inicio, fin) = self
            .work_range()
            .unwrap_or((self.playhead, self.playhead + 5.0));
        let pista = self.project.video_track_count().min(15);
        if self.project.lane_locked(pista, true) {
            self.status = format!("V{} está bloqueada", pista + 1);
            return;
        }
        let before = self.project.clone();
        self.project.clips.push(RoughClip {
            out_seconds: (fin - inicio).max(montaje::MIN_CLIP),
            timeline_start: inicio,
            track: pista,
            has_audio: false,
            is_adjustment: true,
            blur: 0.12,
            mask: Some(Mask {
                shape: MaskShape::Ellipse,
                position_x: 0.5,
                position_y: 0.42,
                size_x: 0.22,
                size_y: 0.36,
                feather: 0.2,
                inverted: false,
            }),
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.bottom_tab = BottomTab::Inspector;
        self.finish_edit(before);
        self.status =
            "Zona difuminada; mueve y ajusta la elipse en Inspector › Máscara".to_owned();
    }

    /// Guías sobre la imagen del monitor (que siempre es 16:9).
    pub(super) fn dibujar_guias(&self, painter: &egui::Painter, rect: egui::Rect) {
        if !self.mejoras.guias {
            return;
        }
        let (ancho, alto) = (self.export_size.0.max(1) as f32, self.export_size.1.max(1) as f32);
        let area = area_exportada(rect, ancho / alto);
        let sombra = egui::Color32::from_black_alpha(150);
        // Lo que el reencuadre recorta.
        if area.left() > rect.left() + 0.5 {
            painter.rect_filled(
                egui::Rect::from_min_max(rect.left_top(), egui::pos2(area.left(), rect.bottom())),
                0.0,
                sombra,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(area.right(), rect.top()), rect.right_bottom()),
                0.0,
                sombra,
            );
        }
        let trazo = |color: egui::Color32| egui::Stroke::new(1.0_f32, color);
        painter.rect_stroke(area, 0.0, trazo(theme::ACCENT), egui::StrokeKind::Inside);
        // Zona segura de acción (93 %) y de títulos (90 %).
        let accion = area.shrink2(area.size() * 0.035);
        let titulos = area.shrink2(area.size() * 0.05);
        painter.rect_stroke(
            accion,
            0.0,
            trazo(egui::Color32::from_white_alpha(70)),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            titulos,
            0.0,
            trazo(egui::Color32::from_rgba_unmultiplied(246, 140, 40, 150)),
            egui::StrokeKind::Inside,
        );
        // Tercios.
        let tenue = trazo(egui::Color32::from_white_alpha(35));
        for fraccion in [1.0 / 3.0, 2.0 / 3.0] {
            let x = area.left() + area.width() * fraccion;
            let y = area.top() + area.height() * fraccion;
            painter.line_segment([egui::pos2(x, area.top()), egui::pos2(x, area.bottom())], tenue);
            painter.line_segment([egui::pos2(area.left(), y), egui::pos2(area.right(), y)], tenue);
        }
        // En vertical, lo que tapan los botones y el texto de TikTok/Reels.
        if alto > ancho {
            let peligro = egui::Color32::from_rgba_unmultiplied(246, 83, 83, 55);
            let abajo = egui::Rect::from_min_max(
                egui::pos2(area.left(), area.bottom() - area.height() * 0.2),
                area.right_bottom(),
            );
            let derecha = egui::Rect::from_min_max(
                egui::pos2(area.right() - area.width() * 0.15, area.top() + area.height() * 0.35),
                egui::pos2(area.right(), abajo.top()),
            );
            painter.rect_filled(abajo, 0.0, peligro);
            painter.rect_filled(derecha, 0.0, peligro);
            painter.text(
                abajo.center(),
                egui::Align2::CENTER_CENTER,
                "Interfaz de la app",
                egui::FontId::proportional(11.0),
                theme::TEXT,
            );
        }
        painter.text(
            egui::pos2(area.left() + 4.0, area.top() + 3.0),
            egui::Align2::LEFT_TOP,
            format!("{}×{}", self.export_size.0, self.export_size.1),
            egui::FontId::monospace(11.0),
            theme::ACCENT,
        );
    }

    // ----- Medios y entrega --------------------------------------------

    fn reemplazar_medio(&mut self) {
        let Some(index) = self.selected.filter(|index| *index < self.project.clips.len()) else {
            return;
        };
        let clip = self.project.clips[index].clone();
        if self.project.clip_locked(&clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if clip.title.is_some() || clip.is_adjustment || clip.nested.is_some() {
            self.status = "Solo se reemplazan clips que vienen de un archivo".to_owned();
            return;
        }
        let mut dialogo = FileDialog::new()
            .set_title("Reemplazar el medio del clip")
            .add_filter(
                "Vídeo, imagen o audio",
                &[
                    "mp4", "mov", "mkv", "avi", "webm", "m4v", "jpg", "jpeg", "png", "bmp",
                    "webp", "gif", "tif", "tiff", "wav", "mp3", "m4a", "aac", "flac", "ogg",
                ],
            );
        if let Some(carpeta) = clip.path.parent().filter(|carpeta| carpeta.is_dir()) {
            dialogo = dialogo.set_directory(carpeta);
        }
        let Some(nuevo) = dialogo.pick_file() else {
            return;
        };
        let sonda = if is_image_file(&nuevo) {
            Ok(MediaProbe {
                duration: DEFAULT_IMAGE_DURATION,
                has_video: true,
                has_audio: false,
                frame_rate: None,
                variable_frame_rate: false,
                source_pts: None,
            })
        } else {
            probe_media(&nuevo)
        };
        let sonda = match sonda {
            Ok(sonda) => sonda,
            Err(error) => {
                self.status = format!("No se pudo analizar el medio nuevo: {error}");
                return;
            }
        };
        match reemplazado(&clip, &nuevo, &sonda, is_image_file(&nuevo)) {
            Ok(reemplazo) => {
                let before = self.project.clone();
                self.project.clips[index] = reemplazo;
                let bin = self.project_bin.clone();
                let registro = self.project.clips[index].clone();
                self.project.register_media(&[registro], &bin);
                self.finish_edit(before);
                self.status = format!(
                    "Medio reemplazado por {}; se conservan efectos y posición",
                    nuevo.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            Err(error) => self.status = error,
        }
    }

    fn recopilar_proyecto(&mut self) {
        let Some(destino) = FileDialog::new()
            .set_title("Carpeta donde recopilar el proyecto")
            .pick_folder()
        else {
            return;
        };
        let carpeta_medios = destino.join("Medios");
        let mut mapa: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut nombres: HashSet<String> = HashSet::new();
        let mut faltan = Vec::new();
        for ruta in rutas_del_proyecto(&self.project) {
            if !ruta.is_file() {
                faltan.push(ruta.file_name().unwrap_or_default().to_string_lossy().into_owned());
                continue;
            }
            let nombre = nombre_libre(&ruta, &mut nombres);
            mapa.insert(ruta, carpeta_medios.join(nombre));
        }
        let mut proyecto = self.project.clone();
        for clip in proyecto.all_clips_mut() {
            remapear_clip(clip, &mapa);
        }
        let ruta_proyecto = destino.join(format!("{}.ncrough", nombre_de_archivo(&self.project.name)));
        let copias: Vec<(PathBuf, PathBuf)> = mapa.into_iter().collect();
        let total = copias.len();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let resultado = (|| -> Result<String, String> {
                std::fs::create_dir_all(&carpeta_medios)
                    .map_err(|error| format!("no se pudo crear {}: {error}", carpeta_medios.display()))?;
                for (origen, destino) in &copias {
                    let igual = match (std::fs::metadata(origen), std::fs::metadata(destino)) {
                        (Ok(a), Ok(b)) => a.len() == b.len(),
                        _ => false,
                    };
                    if !igual {
                        std::fs::copy(origen, destino).map_err(|error| {
                            format!("no se pudo copiar {}: {error}", origen.display())
                        })?;
                    }
                }
                let almacenado = project_for_storage(&proyecto, &ruta_proyecto);
                let json = serde_json::to_string_pretty(&almacenado).map_err(|error| error.to_string())?;
                write_text_atomically(&ruta_proyecto, &json)?;
                let mut mensaje = format!(
                    "Proyecto recopilado en {} con {total} medio(s)",
                    ruta_proyecto.display()
                );
                if !faltan.is_empty() {
                    mensaje.push_str(&format!(
                        " · {} offline sin copiar: {}",
                        faltan.len(),
                        faltan.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
                Ok(mensaje)
            })()
            .map_err(|error| format!("No se pudo recopilar el proyecto: {error}"));
            let _ = sender.send(Resultado::Recopilado(resultado));
        });
        self.mejoras.trabajo = Some(Trabajo {
            rx: receiver,
            generacion: self.document_generation,
        });
        self.status = format!("Copiando {total} medio(s) a {}…", destino.display());
    }

    fn capitulos_de_youtube(&mut self) {
        let marcas: Vec<(f64, String)> = self
            .project
            .markers
            .iter()
            .map(|marker| (marker.time, marker.name.clone()))
            .collect();
        let (texto, avisos) = capitulos_youtube(&marcas, self.project.duration());
        let Some(ruta) = FileDialog::new()
            .add_filter("Texto", &["txt"])
            .set_file_name(format!("{} - capítulos.txt", nombre_de_archivo(&self.project.name)))
            .save_file()
        else {
            self.mejoras.portapapeles = Some(texto);
            self.status = "Capítulos copiados al portapapeles".to_owned();
            return;
        };
        self.mejoras.portapapeles = Some(texto.clone());
        self.status = match std::fs::write(&ruta, texto) {
            Ok(()) if avisos.is_empty() => format!(
                "Capítulos guardados en {} y copiados: pégalos en la descripción de YouTube",
                ruta.display()
            ),
            Ok(()) => format!("Capítulos guardados y copiados · {}", avisos.join(" · ")),
            Err(error) => format!("No se pudo guardar: {error} (sí están en el portapapeles)"),
        };
    }

    fn exportar_transcripcion(&mut self) {
        let texto = transcripcion_en_texto(&self.project.transcript);
        let Some(ruta) = FileDialog::new()
            .add_filter("Texto", &["txt"])
            .set_file_name(format!("{} - transcripción.txt", nombre_de_archivo(&self.project.name)))
            .save_file()
        else {
            return;
        };
        self.status = match std::fs::write(&ruta, texto) {
            Ok(()) => format!("Transcripción guardada en {}", ruta.display()),
            Err(error) => format!("No se pudo guardar la transcripción: {error}"),
        };
    }

    fn exportar_fcpxml(&mut self) {
        let clips = flatten_nested(&self.project.clips);
        let (xml, omitidos) = fcpxml(
            &self.project.name,
            &clips,
            self.project.timebase(),
            self.export_size,
        );
        let Some(ruta) = FileDialog::new()
            .add_filter("Final Cut Pro XML", &["fcpxml"])
            .set_file_name(format!("{}.fcpxml", nombre_de_archivo(&self.project.name)))
            .save_file()
        else {
            return;
        };
        self.status = match std::fs::write(&ruta, xml) {
            Ok(()) if omitidos.is_empty() => format!("FCPXML guardado: {}", ruta.display()),
            Ok(()) => format!(
                "FCPXML guardado · {} sin representar: {}",
                omitidos.len(),
                omitidos.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            ),
            Err(error) => format!("No se pudo guardar el FCPXML: {error}"),
        };
    }

    fn partir_subtitulos(&mut self) {
        let (nuevos, cambiados) = dividir_subtitulos(&self.project.subtitles, CARACTERES_POR_LINEA);
        if cambiados == 0 {
            self.status = "Todos los subtítulos caben ya en dos líneas de 42 caracteres".to_owned();
            return;
        }
        let before = self.project.clone();
        self.project.subtitles = nuevos;
        self.finish_edit(before);
        self.status = format!(
            "{cambiados} subtítulo(s) partidos o recolocados; ahora hay {}",
            self.project.subtitles.len()
        );
    }

    /// Carpeta para archivos que genera NovaCut: junto al proyecto guardado,
    /// si no junto al primer medio y, como último recurso, en temporales.
    fn carpeta_de_trabajo(&self, nombre: &str) -> PathBuf {
        self.project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| {
                self.project
                    .clips
                    .iter()
                    .find(|clip| clip.path.is_file())
                    .and_then(|clip| clip.path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(nombre)
    }
}

// ----- Lógica pura -------------------------------------------------------

/// Tramos en que no hay nada en ninguna pista, entre el primer clip y el
/// último. Quitarlos de todas las pistas a la vez no desincroniza nada.
fn huecos_globales(mut intervalos: Vec<(f64, f64)>, minimo: f64) -> Vec<(f64, f64)> {
    intervalos.retain(|(inicio, fin)| fin > inicio && inicio.is_finite() && fin.is_finite());
    intervalos.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut huecos = Vec::new();
    let Some(&(_, primer_fin)) = intervalos.first() else {
        return huecos;
    };
    let mut fin = primer_fin;
    for &(inicio, final_) in intervalos.iter().skip(1) {
        if inicio > fin + minimo {
            huecos.push((fin, inicio));
        }
        fin = fin.max(final_);
    }
    huecos
}

/// Tramos ordenados y sin solaparse, que es lo que esperan las funciones de
/// reasignación de la transcripción.
fn fusionar_tramos(mut tramos: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    tramos.retain(|(inicio, fin)| fin - inicio > 0.001);
    tramos.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut fusionados: Vec<(f64, f64)> = Vec::new();
    for (inicio, fin) in tramos {
        match fusionados.last_mut() {
            Some(ultimo) if inicio <= ultimo.1 + 0.001 => ultimo.1 = ultimo.1.max(fin),
            _ => fusionados.push((inicio, fin)),
        }
    }
    fusionados
}

/// Silencios entre palabras de más de `umbral` segundos: se quita el centro
/// y quedan `conservar` segundos repartidos a ambos lados.
fn pausas_largas(palabras: &[transcripcion::Word], umbral: f64, conservar: f64) -> Vec<(f64, f64)> {
    palabras
        .windows(2)
        .filter_map(|par| {
            let hueco = par[1].start - par[0].end;
            (hueco > umbral && hueco > conservar)
                .then(|| (par[0].end + conservar / 2.0, par[1].start - conservar / 2.0))
        })
        .collect()
}

/// Cámara elegida en cada ventana: la que más suena, con histéresis. Solo se
/// cambia si la nueva suena claramente más (×1,4, unos 3 dB), por encima
/// del ruido, y el plano actual ya duró `sosten` ventanas. Antes de que
/// hable nadie se usa la primera cámara que habla.
fn elegir_camaras(niveles: &[Vec<f32>], ventana: usize, sosten: usize) -> Vec<usize> {
    let largo = niveles.iter().map(Vec::len).min().unwrap_or(0);
    if niveles.is_empty() || largo == 0 || ventana == 0 {
        return Vec::new();
    }
    let ventanas = largo.div_ceil(ventana);
    let medias: Vec<Vec<f32>> = (0..ventanas)
        .map(|numero| {
            let desde = numero * ventana;
            let hasta = (desde + ventana).min(largo);
            niveles
                .iter()
                .map(|nivel| nivel[desde..hasta].iter().sum::<f32>() / (hasta - desde) as f32)
                .collect()
        })
        .collect();
    let pico = medias.iter().flatten().copied().fold(0.0_f32, f32::max);
    let ruido = pico * 0.1;
    let mut actual: Option<usize> = None;
    let mut duracion = 0usize;
    let mut salida: Vec<Option<usize>> = Vec::with_capacity(ventanas);
    for media in &medias {
        let (ganador, nivel) = media
            .iter()
            .copied()
            .enumerate()
            .fold((0usize, f32::MIN), |mejor, (camara, valor)| {
                if valor > mejor.1 {
                    (camara, valor)
                } else {
                    mejor
                }
            });
        match actual {
            None if nivel > ruido => {
                actual = Some(ganador);
                duracion = 0;
            }
            Some(camara) => {
                duracion += 1;
                if ganador != camara
                    && nivel > ruido
                    && nivel > media[camara] * 1.4
                    && duracion >= sosten
                {
                    actual = Some(ganador);
                    duracion = 0;
                }
            }
            None => {}
        }
        salida.push(actual);
    }
    let primera = salida.iter().flatten().next().copied().unwrap_or(0);
    salida
        .into_iter()
        .map(|camara| camara.unwrap_or(primera))
        .collect()
}

/// Ventanas consecutivas de la misma cámara como tramos de timeline.
fn tramos_de_eleccion(eleccion: &[usize], desde: f64, hasta: f64, paso: f64) -> Vec<(f64, f64, usize)> {
    let mut tramos: Vec<(f64, f64, usize)> = Vec::new();
    for (numero, camara) in eleccion.iter().copied().enumerate() {
        let inicio = (desde + numero as f64 * paso).min(hasta);
        let fin = (desde + (numero + 1) as f64 * paso).min(hasta);
        if fin - inicio < 1e-6 {
            continue;
        }
        match tramos.last_mut() {
            Some(ultimo) if ultimo.2 == camara => ultimo.1 = fin,
            _ => tramos.push((inicio, fin, camara)),
        }
    }
    if let Some(ultimo) = tramos.last_mut() {
        ultimo.1 = hasta;
    }
    tramos
}

/// Clips sincronizados con el `indice`: el mismo archivo, uno con imagen y
/// otro sin ella, a la misma velocidad y con el mismo ancla (el instante de
/// timeline donde caería el segundo 0 del medio).
pub(super) fn enlazados(clips: &[RoughClip], indice: usize) -> Vec<usize> {
    let Some(base) = clips.get(indice) else {
        return Vec::new();
    };
    if base.path.as_os_str().is_empty()
        || base.title.is_some()
        || base.is_adjustment
        || base.nested.is_some()
        || base.fx.reverse
    {
        return Vec::new();
    }
    let ancla = |clip: &RoughClip| clip.timeline_start - clip.in_seconds / clip.speed.clamp(0.1, 8.0);
    let base_fin = base.timeline_start + base.duration();
    clips
        .iter()
        .enumerate()
        .filter(|(otro, clip)| {
            *otro != indice
                && clip.path == base.path
                && clip.has_video != base.has_video
                && !clip.fx.reverse
                && clip.nested.is_none()
                && (clip.speed - base.speed).abs() < 1e-6
                && (ancla(*clip) - ancla(base)).abs() < 0.02
                && clip.timeline_start < base_fin - 0.001
                && base.timeline_start < clip.timeline_start + clip.duration() - 0.001
        })
        .map(|(otro, _)| otro)
        .collect()
}

/// Recorta las parejas enlazadas del clip `indice` igual que a él:
/// `(cabeza, cola)` como en `montaje::retime`. Con ripple, la pareja
/// conserva su inicio y lo que viene detrás en su carril se desplaza lo
/// mismo que en el carril del clip. `antes` es el montaje al empezar el
/// gesto; `bloqueados` dice, por índice, qué clips no se pueden tocar.
pub(super) fn recortar_parejas(
    actuales: &mut [RoughClip],
    antes: &[RoughClip],
    indice: usize,
    (cabeza, cola): (f64, f64),
    ripple: bool,
    bloqueados: &[bool],
) {
    if actuales.len() != antes.len() {
        return;
    }
    let parejas = enlazados(antes, indice);
    for &pareja in &parejas {
        if bloqueados.get(pareja).copied().unwrap_or(true) {
            continue;
        }
        let base = &antes[pareja];
        let Some(mut recortado) = montaje::retime(base, cabeza, cola) else {
            continue;
        };
        if ripple {
            recortado.timeline_start = base.timeline_start;
        }
        actuales[pareja] = recortado;
        if !ripple {
            continue;
        }
        let fin = base.timeline_start + base.duration();
        let desplazamiento = cola - cabeza;
        for (otro, previo) in antes.iter().enumerate() {
            if otro != pareja
                && otro != indice
                && !parejas.contains(&otro)
                && previo.track == base.track
                && previo.has_video == base.has_video
                && previo.timeline_start >= fin - 0.001
            {
                actuales[otro].timeline_start = (previo.timeline_start + desplazamiento).max(0.0);
            }
        }
    }
}

/// El montaje con `[inicio, fin)` sustituido por su render: lo de debajo se
/// levanta y el archivo renderizado ocupa ese tramo en la pista más alta.
/// `None` si el tramo corta una rampa o una secuencia anidada.
fn sustituir_por_render(clips: &[RoughClip], cache: &CacheRender) -> Option<Vec<RoughClip>> {
    let mut resto = montaje::lift(clips, cache.inicio, cache.fin, |_| true).ok()?;
    let duracion = cache.fin - cache.inicio;
    resto.push(RoughClip {
        path: cache.archivo.clone(),
        in_seconds: 0.0,
        out_seconds: duracion,
        source_duration_seconds: Some(duracion),
        has_video: true,
        has_audio: true,
        timeline_start: cache.inicio,
        track: 15,
        ..Default::default()
    });
    Some(resto)
}

/// Pistas (buses) con audio audible, en orden.
fn pistas_de_audio(clips: &[RoughClip]) -> Vec<usize> {
    let mut pistas: Vec<usize> = clips
        .iter()
        .filter(|clip| clip.has_audio && !clip.muted && !clip.is_adjustment && clip.title.is_none())
        .map(|clip| clip.track)
        .collect();
    pistas.sort_unstable();
    pistas.dedup();
    pistas
}

/// Solo el audio de una pista, listo para exportarlo como stem.
fn stem_de_pista(clips: &[RoughClip], pista: usize) -> Vec<RoughClip> {
    clips
        .iter()
        .filter(|clip| {
            clip.has_audio
                && !clip.muted
                && !clip.is_adjustment
                && clip.title.is_none()
                && clip.track == pista
        })
        .cloned()
        .map(|mut clip| {
            clip.has_video = false;
            clip
        })
        .collect()
}

/// Herramientas que tienen sentido para un clip concreto, en el orden del
/// submenú de su menú contextual: solo las aplicables, para no enterrar lo
/// útil en una lista larga.
pub(super) fn herramientas_de_clip(clip: &RoughClip) -> Vec<Accion> {
    let de_archivo = clip.title.is_none()
        && !clip.is_adjustment
        && clip.nested.is_none()
        && !clip.path.as_os_str().is_empty();
    let animado = clip.keyframes.as_ref().is_some_and(|claves| claves.len() > 1)
        || clip.anim.values().any(|claves| claves.len() > 1)
        || clip.fx.volume_keys.len() > 1;
    let mut herramientas = Vec::new();
    if animado {
        herramientas.push(Accion::SuavizarAnimacion);
    }
    if de_archivo && (is_image_file(&clip.path) || clip.freeze_at.is_some()) {
        herramientas.push(Accion::KenBurns);
    }
    if clip.has_video && de_archivo {
        herramientas.extend([
            Accion::PipArribaIzquierda,
            Accion::PipArribaDerecha,
            Accion::PipAbajoIzquierda,
            Accion::PipAbajoDerecha,
        ]);
    }
    if de_archivo && !montaje::is_complex(clip) {
        herramientas.push(Accion::EstirarHastaSiguiente);
    }
    if clip.has_audio && de_archivo {
        herramientas.push(Accion::IgualarVoces);
        herramientas.push(Accion::MarcarGolpes);
    }
    if de_archivo {
        herramientas.push(Accion::ReemplazarMedio);
    }
    herramientas
}

/// Separación máxima entre los keyframes intermedios de una curva suave.
const PASO_SUAVIZADO: f64 = 0.2;
/// Tramos más cortos no se suavizan. Es mayor que `PASO_SUAVIZADO`, así que
/// suavizar dos veces no cambia nada.
const TRAMO_MINIMO_SUAVIZADO: f64 = 0.25;

/// Curva de entrada y salida lentas (smoothstep): 0 → 0, 1 → 1, pendiente
/// nula en los extremos.
fn facilidad(fraccion: f64) -> f64 {
    let f = fraccion.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

/// Puntos intermedios `(tiempo, fracción ya suavizada)` de un tramo `[a, b]`,
/// sin los extremos. Vacío si el tramo es corto.
fn puntos_suaves(a: f64, b: f64) -> Vec<(f64, f64)> {
    let duracion = b - a;
    if duracion < TRAMO_MINIMO_SUAVIZADO {
        return Vec::new();
    }
    let pasos = (duracion / PASO_SUAVIZADO).ceil().max(2.0) as usize;
    (1..pasos)
        .map(|paso| {
            let fraccion = paso as f64 / pasos as f64;
            (a + duracion * fraccion, facilidad(fraccion))
        })
        .collect()
}

/// Keyframes de un parámetro con curvas suaves entre cada par. Los tramos
/// sin cambio de valor no ganan puntos.
fn suavizar_claves(claves: &[animacion::Key]) -> Vec<animacion::Key> {
    let mut ordenadas = claves.to_vec();
    ordenadas.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut salida = Vec::with_capacity(ordenadas.len());
    for (orden, clave) in ordenadas.iter().enumerate() {
        if let Some(previa) = orden.checked_sub(1).map(|anterior| ordenadas[anterior]) {
            if (clave.v - previa.v).abs() > 1e-9 {
                for (t, f) in puntos_suaves(previa.t, clave.t) {
                    salida.push(animacion::Key {
                        t,
                        v: previa.v + (clave.v - previa.v) * f,
                    });
                }
            }
        }
        salida.push(*clave);
    }
    salida
}

/// Igual para los keyframes de transformación: todas las propiedades
/// siguen la misma curva.
fn suavizar_transformacion(claves: &[TransformKeyframe]) -> Vec<TransformKeyframe> {
    let mut ordenadas = claves.to_vec();
    ordenadas.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut salida = Vec::with_capacity(ordenadas.len());
    for (orden, clave) in ordenadas.iter().enumerate() {
        if let Some(previa) = orden.checked_sub(1).map(|anterior| ordenadas[anterior]) {
            let cambia = (clave.x - previa.x).abs() > 1e-9
                || (clave.y - previa.y).abs() > 1e-9
                || (clave.scale - previa.scale).abs() > 1e-9
                || (clave.opacity - previa.opacity).abs() > 1e-9;
            if cambia {
                let mezcla = |a: f64, b: f64, f: f64| a + (b - a) * f;
                for (t, f) in puntos_suaves(previa.t, clave.t) {
                    salida.push(TransformKeyframe {
                        t,
                        x: mezcla(previa.x, clave.x, f),
                        y: mezcla(previa.y, clave.y, f),
                        scale: mezcla(previa.scale, clave.scale, f),
                        opacity: mezcla(previa.opacity, clave.opacity, f),
                    });
                }
            }
        }
        salida.push(*clave);
    }
    salida
}

/// Igual para la banda elástica de volumen.
fn suavizar_volumen(claves: &[efectos::VolumeKey]) -> Vec<efectos::VolumeKey> {
    let como_claves: Vec<animacion::Key> = claves
        .iter()
        .map(|clave| animacion::Key {
            t: clave.t,
            v: clave.db,
        })
        .collect();
    suavizar_claves(&como_claves)
        .into_iter()
        .map(|clave| efectos::VolumeKey {
            t: clave.t,
            db: clave.v,
        })
        .collect()
}

/// Carpeta donde el usuario deja los modelos RNNoise.
fn carpeta_de_modelos() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("NovaCut").join("Modelos"))
}

/// Modelo RNNoise que viaja dentro del ejecutable: `beguiling-drafter`
/// (voz humana con ruido de grabación) de GregorR/rnnoise-models, cuyos
/// modelos no están sujetos a derechos de autor. SHA-256
/// ae3f7411e1e6a884f839a4a145c394408398f09854dbc1216ee02faafc98a17b.
const MODELO_VOZ: &[u8] = include_bytes!("../../assets/modelos/voz.rnnn");

/// El modelo incluido, escrito una vez en la carpeta de modelos (o en
/// temporales en el host de desarrollo) para que `arnndn` lo lea.
fn modelo_incluido() -> Option<PathBuf> {
    let carpeta = carpeta_de_modelos()
        .unwrap_or_else(|| std::env::temp_dir().join("NovaCut Modelos"));
    let ruta = carpeta.join("novacut-voz.rnnn");
    let intacto = std::fs::metadata(&ruta).is_ok_and(|datos| datos.len() == MODELO_VOZ.len() as u64);
    if !intacto {
        std::fs::create_dir_all(&carpeta).ok()?;
        let temporal = carpeta.join(format!(".novacut-voz-{}.tmp", std::process::id()));
        std::fs::write(&temporal, MODELO_VOZ).ok()?;
        let _ = std::fs::remove_file(&ruta);
        std::fs::rename(&temporal, &ruta).ok()?;
    }
    Some(ruta)
}

/// Modelo RNNoise (`.rnnn`) que se usa: el indicado en
/// NOVACUT_RNNOISE_MODEL, uno que el usuario haya puesto junto a la
/// aplicación (`modelos/`) o en su carpeta de modelos, y si no, el incluido.
fn modelo_rnnoise() -> Option<PathBuf> {
    modelo_del_usuario().or_else(modelo_incluido)
}

fn modelo_del_usuario() -> Option<PathBuf> {
    if let Some(ruta) = std::env::var_os("NOVACUT_RNNOISE_MODEL").map(PathBuf::from) {
        if ruta.is_file() {
            return Some(ruta);
        }
    }
    let mut carpetas = Vec::new();
    if let Some(junto) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|carpeta| carpeta.join("modelos")))
    {
        carpetas.push(junto);
    }
    carpetas.extend(carpeta_de_modelos());
    for carpeta in carpetas {
        let Ok(entradas) = std::fs::read_dir(&carpeta) else {
            continue;
        };
        let mut modelos: Vec<PathBuf> = entradas
            .flatten()
            .map(|entrada| entrada.path())
            .filter(|ruta| {
                ruta.is_file()
                    && ruta
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("rnnn"))
                    // La copia del modelo incluido no cuenta como del usuario.
                    && ruta.file_name().is_some_and(|nombre| nombre != "novacut-voz.rnnn")
            })
            .collect();
        modelos.sort();
        if let Some(modelo) = modelos.into_iter().next() {
            return Some(modelo);
        }
    }
    None
}

/// Cadena de «Sonido esencial» del clip, con RNNoise en lugar de la
/// limpieza clásica cuando hay modelo.
pub(super) fn cadena_de_voz(fx: &efectos::ClipFx) -> String {
    if fx.voice_cleanup <= 0.001 {
        return fx.audio_chain();
    }
    cadena_de_voz_con(fx, modelo_rnnoise().as_deref())
}

fn cadena_de_voz_con(fx: &efectos::ClipFx, modelo: Option<&Path>) -> String {
    let cadena = fx.audio_chain();
    let Some(modelo) = modelo else {
        return cadena;
    };
    let limpieza = fx.voice_cleanup.clamp(0.0, 1.0);
    // Mismo texto que genera `ClipFx::audio_chain` para la limpieza clásica.
    let clasica = format!(
        ",highpass=f=80,afftdn=nr={:.1}:nf=-40:tn=1",
        6.0 + limpieza * 24.0
    );
    let neuronal = format!(
        ",highpass=f=80,arnndn=m={}:mix={:.3}",
        escape_lut_path(modelo),
        limpieza
    );
    cadena.replacen(&clasica, &neuronal, 1)
}

/// Ancho y alto de la imagen tal como se ve (los móviles graban en
/// horizontal y marcan el giro).
fn dimensiones_de_video(ruta: &Path) -> Option<(u32, u32)> {
    let salida = Command::new(tool_path("ffprobe.exe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:stream_tags=rotate:stream_side_data=rotation",
            "-of",
            "json",
        ])
        .arg(ruta)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !salida.status.success() {
        return None;
    }
    parse_dimensiones(&String::from_utf8_lossy(&salida.stdout))
}

fn parse_dimensiones(json: &str) -> Option<(u32, u32)> {
    let documento: serde_json::Value = serde_json::from_str(json).ok()?;
    let flujo = documento.get("streams")?.get(0)?;
    let ancho = u32::try_from(flujo.get("width")?.as_u64()?).ok()?;
    let alto = u32::try_from(flujo.get("height")?.as_u64()?).ok()?;
    let giro_lateral = flujo
        .get("side_data_list")
        .and_then(|lista| lista.as_array())
        .into_iter()
        .flatten()
        .filter_map(|dato| dato.get("rotation").and_then(|giro| giro.as_f64()))
        .chain(
            flujo
                .get("tags")
                .and_then(|etiquetas| etiquetas.get("rotate"))
                .and_then(|giro| giro.as_str())
                .and_then(|giro| giro.trim().parse::<f64>().ok()),
        )
        .next()
        .unwrap_or(0.0);
    let cuarto = (giro_lateral.round() as i64).rem_euclid(180) == 90;
    (ancho > 0 && alto > 0).then_some(if cuarto { (alto, ancho) } else { (ancho, alto) })
}

/// Tamaño de exportación que coincide con la imagen del clip (par, como
/// exigen los códecs).
fn tamano_de_exportacion(ancho: u32, alto: u32) -> (u32, u32) {
    let par = |valor: u32| (valor.clamp(2, 8192) / 2) * 2;
    (par(ancho), par(alto))
}

/// Cadencia del medio llevada a la profesional más cercana (29,97, 25,
/// 59,94…) si difiere menos de un 0,5 %: los móviles declaran 29,98 o 30,02.
fn cadencia_normalizada(cadencia: Timebase) -> Timebase {
    let fps = cadencia.fps();
    if !(1.0..=240.0).contains(&fps) {
        return Timebase::default();
    }
    [
        Timebase::P23_976,
        Timebase::P24,
        Timebase::P25,
        Timebase::NTSC30,
        Timebase::P30,
        Timebase::P50,
        Timebase::NTSC60,
        Timebase::P60,
    ]
    .into_iter()
    .min_by(|a, b| (a.fps() - fps).abs().total_cmp(&(b.fps() - fps).abs()))
    .filter(|conocida| (conocida.fps() - fps).abs() / fps < 0.005)
    .unwrap_or_else(|| Timebase::from_fps(fps))
}

fn texto_de_cadencia(cadencia: Timebase) -> String {
    let fps = cadencia.fps();
    if (fps - fps.round()).abs() < 0.001 {
        format!("{} fps", fps.round() as u32)
    } else {
        format!("{fps:.3} fps")
    }
}

/// Material sin límite de duración: fotos y fotogramas congelados.
fn sin_limite(clip: &RoughClip) -> bool {
    is_image_file(&clip.path) || clip.freeze_at.is_some()
}

/// El clip con la velocidad justa para durar `duracion` en el montaje.
/// Keyframes y banda de volumen se estiran con él.
fn estirado(clip: &RoughClip, duracion: f64) -> Result<RoughClip, String> {
    if duracion < montaje::MIN_CLIP {
        return Err("El destino está demasiado cerca del inicio del clip".to_owned());
    }
    let origen = clip.source_duration();
    let velocidad = origen / duracion;
    if !(0.1..=8.0).contains(&velocidad) {
        return Err(format!(
            "Haría falta velocidad {:.0} %: el límite es de 10 % a 800 %",
            velocidad * 100.0
        ));
    }
    let factor = duracion / clip.duration().max(1e-9);
    let mut nuevo = clip.clone();
    nuevo.speed = velocidad;
    if let Some(keyframes) = &mut nuevo.keyframes {
        for keyframe in keyframes.iter_mut() {
            keyframe.t *= factor;
        }
    }
    for clave in &mut nuevo.fx.volume_keys {
        clave.t *= factor;
    }
    let (entrada, salida) = nuevo.effective_fades();
    nuevo.fade_in_seconds = entrada;
    nuevo.fade_out_seconds = salida;
    Ok(nuevo)
}

/// Coloca cada clip entre dos marcadores consecutivos, en `pista`. Si un
/// vídeo no tiene material para todo el intervalo, se queda más corto.
fn colocar_al_ritmo(clips: &[RoughClip], marcas: &[f64], pista: usize) -> Vec<RoughClip> {
    let mut colocados = Vec::new();
    for (orden, clip) in clips.iter().enumerate() {
        let (Some(&inicio), Some(&fin)) = (marcas.get(orden), marcas.get(orden + 1)) else {
            break;
        };
        let duracion = fin - inicio;
        if duracion < montaje::MIN_CLIP {
            continue;
        }
        let velocidad = clip.speed.clamp(0.1, 8.0);
        let mut nuevo = clip.clone();
        nuevo.out_seconds = clip.in_seconds + duracion * velocidad;
        if sin_limite(clip) {
            let techo = clip.source_duration_seconds.unwrap_or(0.0).max(nuevo.out_seconds);
            nuevo.source_duration_seconds = Some(techo);
        } else if let Some(techo) = clip.source_duration_seconds {
            nuevo.out_seconds = nuevo
                .out_seconds
                .min(techo)
                .max(clip.in_seconds + montaje::MIN_CLIP);
        }
        nuevo.timeline_start = inicio;
        nuevo.track = pista;
        nuevo.transition = None;
        nuevo.fade_in_seconds = 0.0;
        nuevo.fade_out_seconds = 0.0;
        colocados.push(nuevo);
    }
    colocados
}

/// Ganancia que lleva una voz medida en `lufs` al nivel de referencia.
fn ganancia_para(lufs: f64) -> f64 {
    ((LUFS_VOZ - lufs) * 10.0).round().clamp(-240.0, 240.0) / 10.0
}

/// Sonoridad integrada de un tramo de un medio.
fn medir_lufs(ruta: &Path, inicio: f64, duracion: f64) -> Result<f64, String> {
    let salida = Command::new(tool_path("ffmpeg.exe"))
        .args(["-v", "info", "-ss", &format_seconds(inicio), "-t"])
        .arg(format_seconds(duracion.max(0.5)))
        .arg("-i")
        .arg(ruta)
        .args([
            "-vn",
            "-af",
            "loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json",
            "-f",
            "null",
            "-",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFmpeg no está disponible: {error}"))?;
    let stderr = String::from_utf8_lossy(&salida.stderr);
    if !salida.status.success() {
        return Err(stderr.lines().last().unwrap_or("error de FFmpeg").trim().to_owned());
    }
    let informe = parse_loudness(&stderr)?;
    if informe.integrated_lufs.is_finite() && informe.integrated_lufs > -70.0 {
        Ok(informe.integrated_lufs)
    } else {
        Err("está en silencio".to_owned())
    }
}

/// Envolvente RMS a 100 valores por segundo, leída por tramos para no
/// cargar en memoria una hora de audio.
fn envolvente(ruta: &Path, inicio: f64, duracion: f64) -> Result<Vec<f32>, String> {
    let mut hijo = Command::new(tool_path("ffmpeg.exe"))
        .args(["-v", "error", "-ss", &format_seconds(inicio), "-t"])
        .arg(format_seconds(duracion.max(0.1)))
        .arg("-i")
        .arg(ruta)
        .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("FFmpeg no está disponible: {error}"))?;
    let Some(mut lector) = hijo.stdout.take() else {
        let _ = hijo.kill();
        let _ = hijo.wait();
        return Err("FFmpeg no devolvió audio".to_owned());
    };
    // 80 muestras de 16 bits a 8 kHz = una centésima de segundo.
    let mut bloque = [0u8; 160];
    let mut niveles = Vec::new();
    while lector.read_exact(&mut bloque).is_ok() {
        let energia: f32 = bloque
            .chunks_exact(2)
            .map(|par| {
                let muestra = i16::from_le_bytes([par[0], par[1]]) as f32 / 32768.0;
                muestra * muestra
            })
            .sum();
        niveles.push((energia / 80.0).sqrt());
    }
    let estado = hijo.wait().map_err(|error| error.to_string())?;
    if niveles.is_empty() {
        return Err(if estado.success() {
            "el medio no tiene audio en ese tramo".to_owned()
        } else {
            format!("no se pudo leer el audio de {}", ruta.display())
        });
    }
    Ok(niveles)
}

/// Golpes de la música (segundos desde el inicio de la envolvente): subidas
/// bruscas de energía por encima de la media local, separadas al menos
/// 0,25 s.
fn detectar_golpes(niveles: &[f32]) -> Vec<f64> {
    let largo = niveles.len();
    if largo < 10 {
        return Vec::new();
    }
    let logaritmo: Vec<f32> = niveles.iter().map(|nivel| (nivel + 1e-4).ln()).collect();
    let flujo: Vec<f32> = (0..largo)
        .map(|indice| {
            if indice == 0 {
                return 0.0;
            }
            let desde = indice.saturating_sub(4);
            let previos = &logaritmo[desde..indice];
            let media = previos.iter().sum::<f32>() / previos.len() as f32;
            (logaritmo[indice] - media).max(0.0)
        })
        .collect();
    // Sumas acumuladas para la media y la varianza locales (±1 s).
    let mut suma = vec![0.0_f64; largo + 1];
    let mut cuadrados = vec![0.0_f64; largo + 1];
    for (indice, valor) in flujo.iter().enumerate() {
        suma[indice + 1] = suma[indice] + *valor as f64;
        cuadrados[indice + 1] = cuadrados[indice] + (*valor as f64) * (*valor as f64);
    }
    let separacion = 25;
    let mut golpes = Vec::new();
    let mut ultimo: Option<usize> = None;
    for indice in 1..largo {
        let valor = flujo[indice] as f64;
        let desde = indice.saturating_sub(50);
        let hasta = (indice + 51).min(largo);
        let n = (hasta - desde) as f64;
        let media = (suma[hasta] - suma[desde]) / n;
        let varianza = ((cuadrados[hasta] - cuadrados[desde]) / n - media * media).max(0.0);
        let umbral = (media + 1.5 * varianza.sqrt()).max(0.5);
        let maximo_local = valor >= flujo[indice - 1] as f64
            && flujo.get(indice + 1).is_none_or(|siguiente| valor >= *siguiente as f64);
        let separado = ultimo.is_none_or(|previo| indice - previo >= separacion);
        if valor > umbral && maximo_local && separado && niveles[indice] > 0.01 {
            golpes.push(indice as f64 / TASA_ENVOLVENTE);
            ultimo = Some(indice);
        }
    }
    golpes
}

/// Silencia el audio de `[inicio, fin)` partiendo los clips que lo cruzan.
/// Las pistas bloqueadas no se tocan; una rampa o un anidado cancela todo.
fn silenciar_tramo(
    clips: &[RoughClip],
    inicio: f64,
    fin: f64,
    bloqueado: impl Fn(&RoughClip) -> bool,
) -> Result<Vec<RoughClip>, String> {
    let mut resultado = Vec::with_capacity(clips.len() + 4);
    for clip in clips {
        let clip_inicio = clip.timeline_start;
        let clip_fin = clip_inicio + clip.duration();
        let cruza = clip_inicio < fin - 0.001 && clip_fin > inicio + 0.001;
        if !clip.has_audio || clip.muted || !cruza || bloqueado(clip) {
            resultado.push(clip.clone());
            continue;
        }
        if montaje::is_complex(clip) {
            return Err(format!(
                "«{}» tiene rampa o es una secuencia anidada: no se puede silenciar por dentro",
                clip.name()
            ));
        }
        let desde = (inicio - clip_inicio).max(0.0);
        let hasta = (fin - clip_inicio).min(clip.duration());
        if desde > 0.001 {
            resultado.extend(montaje::clip_portion(clip, 0.0, desde));
        }
        if let Some(mut medio) = montaje::clip_portion(clip, desde, hasta) {
            medio.muted = true;
            resultado.push(medio);
        }
        if hasta < clip.duration() - 0.001 {
            resultado.extend(montaje::clip_portion(clip, hasta, clip.duration()));
        }
    }
    Ok(resultado)
}

/// WAV mono de 48 kHz y 16 bits con un tono de 1 kHz a -12 dBFS y rampas de
/// 5 ms para que no chasquee.
fn wav_pitido(segundos: f64) -> Vec<u8> {
    const TASA: u32 = 48_000;
    let muestras = (segundos.max(0.01) * TASA as f64).round() as usize;
    let rampa = (0.005 * TASA as f64) as usize;
    let datos = (muestras * 2) as u32;
    let mut wav = Vec::with_capacity(44 + muestras * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + datos).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&TASA.to_le_bytes());
    wav.extend_from_slice(&(TASA * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&datos.to_le_bytes());
    for indice in 0..muestras {
        let t = indice as f64 / TASA as f64;
        let envolvente = (indice.min(muestras - 1 - indice) as f64 / rampa.max(1) as f64).min(1.0);
        let valor = 0.25 * envolvente * (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
        wav.extend_from_slice(&((valor * 32767.0).round() as i16).to_le_bytes());
    }
    wav
}

/// Dispositivos de audio que lista `ffmpeg -list_devices true -f dshow`,
/// tanto en el formato nuevo («"Nombre" (audio)») como en el antiguo (con
/// cabeceras de sección).
#[cfg_attr(not(windows), allow(dead_code))]
fn microfonos_dshow(salida: &str) -> Vec<String> {
    let mut en_audio = false;
    let mut nombres = Vec::new();
    for linea in salida.lines() {
        if linea.contains("DirectShow audio devices") {
            en_audio = true;
            continue;
        }
        if linea.contains("DirectShow video devices") {
            en_audio = false;
            continue;
        }
        if linea.contains("Alternative name") {
            continue;
        }
        let Some(apertura) = linea.find('"') else {
            continue;
        };
        let resto = &linea[apertura + 1..];
        let Some(cierre) = resto.find('"') else {
            continue;
        };
        let nombre = &resto[..cierre];
        if !nombre.is_empty() && (en_audio || linea.trim_end().ends_with("(audio)")) {
            nombres.push(nombre.to_owned());
        }
    }
    nombres
}

#[cfg(windows)]
fn entrada_de_microfono() -> Result<Vec<String>, String> {
    let salida = Command::new(tool_path("ffmpeg.exe"))
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFmpeg no está disponible: {error}"))?;
    let nombre = microfonos_dshow(&String::from_utf8_lossy(&salida.stderr))
        .into_iter()
        .next()
        .ok_or("No se encontró ningún micrófono: conéctalo o revisa la privacidad de Windows")?;
    Ok(vec![
        "-f".to_owned(),
        "dshow".to_owned(),
        "-i".to_owned(),
        format!("audio={nombre}"),
    ])
}

#[cfg(target_os = "macos")]
fn entrada_de_microfono() -> Result<Vec<String>, String> {
    Ok(vec![
        "-f".to_owned(),
        "avfoundation".to_owned(),
        "-i".to_owned(),
        ":0".to_owned(),
    ])
}

#[cfg(not(any(windows, target_os = "macos")))]
fn entrada_de_microfono() -> Result<Vec<String>, String> {
    Ok(vec![
        "-f".to_owned(),
        "pulse".to_owned(),
        "-i".to_owned(),
        "default".to_owned(),
    ])
}

/// Keyframes de Ken Burns: del encuadre actual a un 15 % más cerca, con un
/// desplazamiento lateral cuyo sentido alterna entre fotos consecutivas.
fn ken_burns_de(clip: &RoughClip, sentido: f64) -> [TransformKeyframe; 2] {
    let duracion = clip.duration().max(montaje::MIN_CLIP);
    [
        TransformKeyframe {
            t: 0.0,
            x: clip.position_x,
            y: clip.position_y,
            scale: clip.scale_percent,
            opacity: clip.opacity,
        },
        TransformKeyframe {
            t: duracion,
            x: clip.position_x + 40.0 * sentido,
            y: clip.position_y - 20.0,
            scale: (clip.scale_percent * 1.15).clamp(1.0, 800.0),
            opacity: clip.opacity,
        },
    ]
}

/// Escala de la imagen en imagen.
const ESCALA_PIP: f64 = 30.0;

/// Posición (px sobre 1920×1080, desde el centro) de una capa al `escala` %
/// pegada a la esquina indicada con 48 px de margen.
fn posicion_pip(horizontal: f64, vertical: f64, escala: f64) -> (f64, f64) {
    let margen = 48.0;
    let x = 960.0 - 1920.0 * escala / 200.0 - margen;
    let y = 540.0 - 1080.0 * escala / 200.0 - margen;
    (horizontal.signum() * x, vertical.signum() * y)
}

/// Rectángulo del monitor (16:9) que sobrevive al reencuadre de una
/// exportación de proporción `aspecto`. Vertical y cuadrado se recortan al
/// centro; los horizontales más anchos se ajustan con bandas arriba y abajo.
fn area_exportada(monitor: egui::Rect, aspecto: f32) -> egui::Rect {
    let propio = monitor.width() / monitor.height().max(1.0);
    if aspecto < propio {
        let ancho = monitor.height() * aspecto;
        egui::Rect::from_center_size(monitor.center(), egui::vec2(ancho, monitor.height()))
    } else {
        let alto = monitor.width() / aspecto.max(0.01);
        egui::Rect::from_center_size(monitor.center(), egui::vec2(monitor.width(), alto))
    }
}

/// El clip apuntando al archivo `nuevo`, con sus efectos, su posición y su
/// duración; la entrada se conserva si el medio nuevo la tiene.
fn reemplazado(
    clip: &RoughClip,
    nuevo: &Path,
    sonda: &MediaProbe,
    imagen: bool,
) -> Result<RoughClip, String> {
    if clip.has_video && !sonda.has_video {
        return Err("El clip está en una pista de vídeo y el archivo nuevo no tiene imagen".to_owned());
    }
    if !clip.has_video && !sonda.has_audio {
        return Err("El clip está en una pista de audio y el archivo nuevo no tiene sonido".to_owned());
    }
    let mut reemplazo = clip.clone();
    let necesita = clip.source_duration();
    reemplazo.path = nuevo.to_path_buf();
    reemplazo.proxy = None;
    reemplazo.freeze_at = None;
    reemplazo.source_timebase = sonda.frame_rate;
    reemplazo.source_vfr = sonda.variable_frame_rate;
    reemplazo.source_pts = sonda.source_pts.clone();
    reemplazo.has_audio = clip.has_audio && sonda.has_audio;
    if imagen {
        reemplazo.in_seconds = 0.0;
        reemplazo.out_seconds = necesita;
        reemplazo.source_duration_seconds = Some(necesita.max(sonda.duration));
    } else {
        let entrada = if clip.in_seconds + necesita <= sonda.duration + 0.001 {
            clip.in_seconds
        } else {
            0.0
        };
        reemplazo.in_seconds = entrada;
        reemplazo.out_seconds = (entrada + necesita).min(sonda.duration);
        reemplazo.source_duration_seconds = Some(sonda.duration);
    }
    if reemplazo.source_duration() < montaje::MIN_CLIP {
        return Err("El archivo nuevo es demasiado corto".to_owned());
    }
    Ok(reemplazo)
}

/// Todos los archivos que usa el proyecto (secuencias, biblioteca, anidados
/// y LUT), sin repetir.
fn rutas_del_proyecto(proyecto: &RoughProject) -> Vec<PathBuf> {
    fn recoger(clips: &[RoughClip], rutas: &mut Vec<PathBuf>) {
        for clip in clips {
            let de_archivo = clip.title.is_none()
                && !clip.is_adjustment
                && clip.nested.is_none()
                && !clip.path.as_os_str().is_empty();
            if de_archivo && !rutas.contains(&clip.path) {
                rutas.push(clip.path.clone());
            }
            if let Some(lut) = &clip.lut {
                if !rutas.contains(lut) {
                    rutas.push(lut.clone());
                }
            }
            if let Some(hijos) = &clip.nested {
                recoger(hijos, rutas);
            }
        }
    }
    let mut rutas = Vec::new();
    recoger(&proyecto.clips, &mut rutas);
    for secuencia in &proyecto.sequences {
        recoger(&secuencia.data.clips, &mut rutas);
    }
    let biblioteca: Vec<RoughClip> = proyecto.library.iter().map(|item| item.clip.clone()).collect();
    recoger(&biblioteca, &mut rutas);
    rutas
}

/// Apunta el clip (y sus anidados) a las copias; los proxies se descartan
/// porque se regeneran.
fn remapear_clip(clip: &mut RoughClip, mapa: &HashMap<PathBuf, PathBuf>) {
    if let Some(copia) = mapa.get(&clip.path) {
        clip.path = copia.clone();
        clip.proxy = None;
    }
    if let Some(copia) = clip.lut.as_ref().and_then(|lut| mapa.get(lut)) {
        clip.lut = Some(copia.clone());
    }
    for hijo in clip.nested.iter_mut().flatten() {
        remapear_clip(hijo, mapa);
    }
}

/// Nombre de archivo que no choca con otro ya usado en la carpeta de
/// destino (sin distinguir mayúsculas, como Windows).
fn nombre_libre(ruta: &Path, usados: &mut HashSet<String>) -> String {
    let nombre = ruta
        .file_name()
        .map(|nombre| nombre.to_string_lossy().into_owned())
        .unwrap_or_else(|| "medio".to_owned());
    let tallo = ruta
        .file_stem()
        .map(|tallo| tallo.to_string_lossy().into_owned())
        .unwrap_or_else(|| "medio".to_owned());
    let extension = ruta
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut candidato = nombre;
    let mut numero = 2;
    while !usados.insert(candidato.to_lowercase()) {
        candidato = format!("{tallo}-{numero}{extension}");
        numero += 1;
    }
    candidato
}

/// Nombre de proyecto apto como nombre de archivo en Windows.
fn nombre_de_archivo(nombre: &str) -> String {
    let limpio: String = nombre
        .chars()
        .map(|caracter| {
            if r#"\/:*?"<>|"#.contains(caracter) || caracter.is_control() {
                '_'
            } else {
                caracter
            }
        })
        .collect();
    let limpio = limpio.trim().trim_end_matches('.').to_owned();
    if limpio.is_empty() {
        "NovaCut".to_owned()
    } else {
        limpio
    }
}

fn marca_de_tiempo() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duracion| duracion.as_millis())
        .unwrap_or(0)
}

/// Marca de capítulo tal como la lee YouTube: `M:SS` o `H:MM:SS`.
fn marca_youtube(segundos: f64, con_horas: bool) -> String {
    let total = segundos.max(0.0).floor() as u64;
    if con_horas {
        format!("{}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

/// Lista de capítulos para la descripción de YouTube y los avisos de lo
/// que YouTube rechazaría (el primero en 0:00, al menos tres, de 10 s o más).
fn capitulos_youtube(marcas: &[(f64, String)], total: f64) -> (String, Vec<String>) {
    let mut lista: Vec<(f64, String)> = marcas
        .iter()
        .filter(|(tiempo, _)| tiempo.is_finite() && *tiempo >= 0.0 && *tiempo <= total.max(0.0))
        .cloned()
        .collect();
    lista.sort_by(|left, right| left.0.total_cmp(&right.0));
    lista.dedup_by(|siguiente, anterior| (siguiente.0 - anterior.0).abs() < 1.0);
    let mut avisos = Vec::new();
    let empieza_en_cero = lista.first().is_some_and(|primero| primero.0 < 1.0);
    if empieza_en_cero {
        lista[0].0 = 0.0;
    } else {
        lista.insert(0, (0.0, "Introducción".to_owned()));
        avisos.push("se añadió «Introducción» en 0:00, que YouTube exige".to_owned());
    }
    if lista.len() < 3 {
        avisos.push("YouTube pide al menos tres capítulos".to_owned());
    }
    let cortos = lista
        .iter()
        .enumerate()
        .filter(|(orden, (tiempo, _))| {
            let fin = lista.get(orden + 1).map_or(total, |siguiente| siguiente.0);
            fin - tiempo < 10.0
        })
        .count();
    if cortos > 0 {
        avisos.push(format!(
            "{cortos} capítulo(s) duran menos de 10 s y YouTube los ignora"
        ));
    }
    let con_horas = total >= 3600.0;
    let texto = lista
        .iter()
        .enumerate()
        .map(|(orden, (tiempo, nombre))| {
            let nombre = nombre.trim();
            let nombre = if nombre.is_empty() {
                format!("Capítulo {}", orden + 1)
            } else {
                nombre.to_owned()
            };
            format!("{} {nombre}", marca_youtube(*tiempo, con_horas))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (texto + "\n", avisos)
}

/// Transcripción en párrafos con la hora de inicio de cada uno. Un párrafo
/// acaba en una pausa larga o en un final de frase seguido de pausa.
fn transcripcion_en_texto(palabras: &[transcripcion::Word]) -> String {
    let reloj = |segundos: f64| {
        let total = segundos.max(0.0).floor() as u64;
        format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
    };
    let mut texto = String::new();
    let mut parrafo: Vec<&str> = Vec::new();
    let mut inicio = 0.0;
    for (indice, palabra) in palabras.iter().enumerate() {
        if parrafo.is_empty() {
            inicio = palabra.start;
        }
        parrafo.push(palabra.text.trim());
        let cierra = match palabras.get(indice + 1) {
            None => true,
            Some(siguiente) => {
                let pausa = siguiente.start - palabra.end;
                pausa > 1.5 || (palabra.text.trim_end().ends_with(['.', '?', '!']) && pausa > 0.6)
            }
        };
        if cierra {
            texto.push_str(&format!("[{}] {}\n\n", reloj(inicio), parrafo.join(" ")));
            parrafo.clear();
        }
    }
    texto
}

/// Parte los subtítulos que no caben en dos líneas de `por_linea`
/// caracteres, repartiendo su tiempo según la longitud de cada trozo, y
/// coloca el salto de línea donde las dos líneas quedan más parejas.
/// Devuelve los subtítulos y cuántos cambiaron.
fn dividir_subtitulos(subtitulos: &[Subtitle], por_linea: usize) -> (Vec<Subtitle>, usize) {
    let por_linea = por_linea.max(8);
    let mut salida = Vec::with_capacity(subtitulos.len());
    let mut cambiados = 0;
    for subtitulo in subtitulos {
        let palabras: Vec<&str> = subtitulo.text.split_whitespace().collect();
        let largo = palabras.join(" ").chars().count();
        if largo <= por_linea || palabras.len() < 2 {
            salida.push(subtitulo.clone());
            continue;
        }
        // Líneas que caben, de dos en dos: cada subtítulo nuevo tiene como
        // mucho dos líneas y ninguna pasa del límite (salvo una palabra
        // suelta más larga que la línea entera).
        let lineas = agrupar_palabras(&palabras, por_linea);
        let grupos: Vec<Vec<&str>> = lineas.chunks(2).map(|par| par.concat()).collect();
        let caracteres: Vec<usize> = grupos
            .iter()
            .map(|grupo| grupo.join(" ").chars().count().max(1))
            .collect();
        let total: usize = caracteres.iter().sum();
        let duracion = (subtitulo.end - subtitulo.start).max(0.0);
        let mut tiempo = subtitulo.start;
        let mut trozos = Vec::with_capacity(grupos.len());
        for (orden, grupo) in grupos.iter().enumerate() {
            let fin = if orden + 1 == grupos.len() {
                subtitulo.end
            } else {
                tiempo + duracion * caracteres[orden] as f64 / total as f64
            };
            trozos.push(Subtitle {
                start: tiempo,
                end: fin,
                text: en_dos_lineas(grupo, por_linea),
            });
            tiempo = fin;
        }
        if trozos.len() != 1 || trozos[0].text != subtitulo.text {
            cambiados += 1;
        }
        salida.extend(trozos);
    }
    (salida, cambiados)
}

/// Palabras en grupos de como mucho `limite` caracteres (una palabra más
/// larga que el límite va sola).
fn agrupar_palabras<'a>(palabras: &[&'a str], limite: usize) -> Vec<Vec<&'a str>> {
    let mut grupos: Vec<Vec<&'a str>> = Vec::new();
    let mut actual: Vec<&'a str> = Vec::new();
    let mut largo = 0;
    for &palabra in palabras {
        let extra = palabra.chars().count() + usize::from(!actual.is_empty());
        if !actual.is_empty() && largo + extra > limite {
            grupos.push(std::mem::take(&mut actual));
            largo = 0;
        }
        largo += palabra.chars().count() + usize::from(!actual.is_empty());
        actual.push(palabra);
    }
    if !actual.is_empty() {
        grupos.push(actual);
    }
    grupos
}

/// Una o dos líneas, con el salto donde quedan más parejas.
fn en_dos_lineas(palabras: &[&str], por_linea: usize) -> String {
    let una = palabras.join(" ");
    if una.chars().count() <= por_linea || palabras.len() < 2 {
        return una;
    }
    let corte = (1..palabras.len())
        .min_by_key(|corte| {
            let izquierda = palabras[..*corte].join(" ").chars().count();
            let derecha = palabras[*corte..].join(" ").chars().count();
            izquierda.max(derecha)
        })
        .unwrap_or(1);
    format!("{}\n{}", palabras[..corte].join(" "), palabras[corte..].join(" "))
}

/// Escapa texto para atributos XML.
fn xml(texto: &str) -> String {
    texto
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// URL `file://` de una ruta local, con lo que no es seguro codificado.
fn url_de_archivo(ruta: &Path) -> String {
    let texto = ruta.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !texto.starts_with('/') {
        url.push('/');
    }
    for byte in texto.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// Tiempo racional de FCPXML en fotogramas exactos de la cadencia.
fn tiempo_fcpxml(fotogramas: i64, cadencia: Timebase) -> String {
    if fotogramas <= 0 {
        "0s".to_owned()
    } else {
        format!(
            "{}/{}s",
            fotogramas * cadencia.denominator as i64,
            cadencia.numerator
        )
    }
}

/// FCPXML 1.9 que leen Final Cut Pro y DaVinci Resolve: un hueco en la
/// historia principal y cada clip conectado en su carril (vídeo arriba,
/// audio abajo). Devuelve también lo que no se representa.
fn fcpxml(
    nombre: &str,
    clips: &[RoughClip],
    cadencia: Timebase,
    tamano: (u32, u32),
) -> (String, Vec<String>) {
    let mut omitidos = Vec::new();
    let mut activos: Vec<&RoughClip> = Vec::new();
    for clip in clips {
        if clip.title.is_some() || clip.is_adjustment || clip.path.as_os_str().is_empty() {
            omitidos.push(format!("{} (título o capa de ajuste)", clip.name()));
            continue;
        }
        if (clip.speed - 1.0).abs() > 1e-6
            || clip.fx.reverse
            || clip.speed_ramp.as_ref().is_some_and(|puntos| !puntos.is_empty())
        {
            omitidos.push(format!("{} (va sin su cambio de velocidad)", clip.name()));
        }
        activos.push(clip);
    }
    let mut medios: Vec<(PathBuf, f64, bool, bool)> = Vec::new();
    for clip in &activos {
        let duracion = clip.source_duration_seconds.unwrap_or(clip.out_seconds);
        match medios.iter_mut().find(|medio| medio.0 == clip.path) {
            Some(medio) => {
                medio.1 = medio.1.max(duracion);
                medio.2 |= clip.has_video;
                medio.3 |= clip.has_audio;
            }
            None => medios.push((clip.path.clone(), duracion, clip.has_video, clip.has_audio)),
        }
    }
    let total = activos
        .iter()
        .map(|clip| cadencia.frames(clip.timeline_start + clip.duration()))
        .max()
        .unwrap_or(0);
    let mut texto = String::new();
    texto.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n");
    texto.push_str("<fcpxml version=\"1.9\">\n  <resources>\n");
    texto.push_str(&format!(
        "    <format id=\"r1\" frameDuration=\"{}/{}s\" width=\"{}\" height=\"{}\"/>\n",
        cadencia.denominator, cadencia.numerator, tamano.0, tamano.1
    ));
    for (orden, (ruta, duracion, video, audio)) in medios.iter().enumerate() {
        let nombre_medio = ruta
            .file_name()
            .map(|nombre| nombre.to_string_lossy().into_owned())
            .unwrap_or_else(|| "medio".to_owned());
        texto.push_str(&format!(
            "    <asset id=\"r{}\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\" format=\"r1\"{}>\n      <media-rep kind=\"original-media\" src=\"{}\"/>\n    </asset>\n",
            orden + 2,
            xml(&nombre_medio),
            tiempo_fcpxml(cadencia.frames(*duracion).max(1), cadencia),
            u8::from(*video),
            u8::from(*audio),
            if *audio { " audioSources=\"1\" audioChannels=\"2\"" } else { "" },
            xml(&url_de_archivo(ruta))
        ));
    }
    texto.push_str("  </resources>\n  <library>\n    <event name=\"NovaCut\">\n");
    texto.push_str(&format!("      <project name=\"{}\">\n", xml(nombre)));
    texto.push_str(&format!(
        "        <sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"{}\" audioLayout=\"stereo\" audioRate=\"48k\">\n          <spine>\n",
        tiempo_fcpxml(total, cadencia),
        if cadencia.drop_frame { "DF" } else { "NDF" }
    ));
    texto.push_str(&format!(
        "            <gap name=\"Hueco\" offset=\"0s\" start=\"0s\" duration=\"{}\">\n",
        tiempo_fcpxml(total, cadencia)
    ));
    for clip in &activos {
        let Some(orden) = medios.iter().position(|medio| medio.0 == clip.path) else {
            continue;
        };
        let carril = if clip.has_video {
            clip.track as i64 + 1
        } else {
            -(clip.track as i64 + 1)
        };
        let fuente = match (clip.has_video, clip.has_audio) {
            (true, false) => " srcEnable=\"video\"",
            (false, true) => " srcEnable=\"audio\"",
            _ => "",
        };
        texto.push_str(&format!(
            "              <asset-clip ref=\"r{}\" lane=\"{carril}\" offset=\"{}\" start=\"{}\" duration=\"{}\" name=\"{}\"{fuente}{}/>\n",
            orden + 2,
            tiempo_fcpxml(cadencia.frames(clip.timeline_start), cadencia),
            tiempo_fcpxml(cadencia.frames(clip.in_seconds), cadencia),
            tiempo_fcpxml(cadencia.frames(clip.duration()).max(1), cadencia),
            xml(&clip.name()),
            if clip.enabled { "" } else { " enabled=\"0\"" }
        ));
    }
    texto.push_str("            </gap>\n          </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n");
    (texto, omitidos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(ruta: &str, inicio: f64, duracion: f64) -> RoughClip {
        RoughClip {
            path: PathBuf::from(ruta),
            in_seconds: 0.0,
            out_seconds: duracion,
            source_duration_seconds: Some(duracion),
            timeline_start: inicio,
            ..Default::default()
        }
    }

    fn palabra(texto: &str, inicio: f64, fin: f64) -> transcripcion::Word {
        transcripcion::Word {
            start: inicio,
            end: fin,
            text: texto.to_owned(),
        }
    }

    #[test]
    fn los_huecos_son_los_de_todas_las_pistas_a_la_vez() {
        let huecos = huecos_globales(vec![(0.0, 2.0), (3.0, 5.0), (4.0, 6.0), (8.0, 9.0)], 0.01);
        assert_eq!(huecos, vec![(2.0, 3.0), (6.0, 8.0)]);
        // Un hueco en una pista tapado por otra pista no es hueco.
        assert!(huecos_globales(vec![(0.0, 2.0), (3.0, 5.0), (1.0, 4.0)], 0.01).is_empty());
        assert!(huecos_globales(Vec::new(), 0.01).is_empty());
    }

    #[test]
    fn cerrar_huecos_mantiene_juntos_el_video_y_su_audio_separado() {
        let mut video = clip("a.mp4", 0.0, 2.0);
        video.has_audio = false;
        let mut audio = clip("a.mp4", 0.0, 2.0);
        audio.has_video = false;
        let mut segundo = clip("b.mp4", 5.0, 2.0);
        segundo.has_audio = false;
        let mut segundo_audio = clip("b.mp4", 5.0, 2.0);
        segundo_audio.has_video = false;
        let clips = vec![video, audio, segundo, segundo_audio];
        let intervalos = clips
            .iter()
            .map(|clip| (clip.timeline_start, clip.timeline_start + clip.duration()))
            .collect();
        let huecos = huecos_globales(intervalos, 0.01);
        let cerrados = transcripcion::extract_ranges(&clips, &huecos).unwrap();
        assert!((cerrados[2].timeline_start - 2.0).abs() < 1e-9);
        assert!((cerrados[3].timeline_start - 2.0).abs() < 1e-9);
    }

    #[test]
    fn las_pausas_largas_se_acortan_dejando_aire() {
        let palabras = vec![
            palabra("hola", 0.0, 0.5),
            palabra("qué", 0.6, 0.9),
            palabra("tal", 2.9, 3.2),
        ];
        let tramos = pausas_largas(&palabras, 0.8, 0.3);
        assert_eq!(tramos.len(), 1);
        assert!((tramos[0].0 - 1.05).abs() < 1e-9);
        assert!((tramos[0].1 - 2.75).abs() < 1e-9);
        let restante = (palabras[2].start - palabras[1].end) - (tramos[0].1 - tramos[0].0);
        assert!((restante - 0.3).abs() < 1e-9);
    }

    #[test]
    fn la_multicamara_sigue_a_quien_habla_sin_saltos_nerviosos() {
        // Cámara 0 habla 3 s, luego la 1 otros 3 s; la otra se oye poco.
        let mut uno = vec![0.05_f32; 600];
        let mut dos = vec![0.05_f32; 600];
        for nivel in &mut uno[..300] {
            *nivel = 0.5;
        }
        for nivel in &mut dos[300..] {
            *nivel = 0.5;
        }
        let eleccion = elegir_camaras(&[uno.clone(), dos.clone()], 50, 4);
        assert_eq!(eleccion, vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1]);
        let tramos = tramos_de_eleccion(&eleccion, 10.0, 16.0, 0.5);
        assert_eq!(tramos, vec![(10.0, 13.0, 0), (13.0, 16.0, 1)]);

        // Una interrupción de medio segundo no provoca un corte.
        let mut uno = vec![0.5_f32; 600];
        let mut dos = vec![0.05_f32; 600];
        for indice in 100..150 {
            uno[indice] = 0.05;
            dos[indice] = 0.5;
        }
        let eleccion = elegir_camaras(&[uno, dos], 50, 4);
        assert!(eleccion.iter().all(|camara| *camara == 0), "{eleccion:?}");
    }

    #[test]
    fn antes_de_que_hable_nadie_se_usa_la_primera_camara_que_habla() {
        let mut uno = vec![0.0_f32; 400];
        let mut dos = vec![0.0_f32; 400];
        for nivel in &mut dos[200..] {
            *nivel = 0.4;
        }
        for nivel in &mut uno[..10] {
            *nivel = 0.001;
        }
        let eleccion = elegir_camaras(&[uno, dos], 50, 4);
        assert!(eleccion.iter().all(|camara| *camara == 1), "{eleccion:?}");
    }

    #[test]
    fn el_enlace_encuentra_el_audio_sincronizado_y_no_otro() {
        let mut video = clip("entrevista.mov", 10.0, 5.0);
        video.in_seconds = 2.0;
        video.out_seconds = 7.0;
        video.source_duration_seconds = Some(60.0);
        video.has_audio = false;
        let mut audio = video.clone();
        audio.has_video = false;
        audio.has_audio = true;
        let mut desplazado = audio.clone();
        desplazado.timeline_start = 10.5;
        let otro_archivo = RoughClip {
            path: PathBuf::from("otro.wav"),
            ..audio.clone()
        };
        let clips = vec![video, audio, desplazado, otro_archivo];
        assert_eq!(enlazados(&clips, 0), vec![1]);
        assert_eq!(enlazados(&clips, 1), vec![0]);
        assert!(enlazados(&clips, 3).is_empty());
    }

    #[test]
    fn estirar_ajusta_la_velocidad_y_los_keyframes() {
        let mut original = clip("a.mp4", 0.0, 4.0);
        original.keyframes = Some(vec![TransformKeyframe {
            t: 4.0,
            x: 0.0,
            y: 0.0,
            scale: 120.0,
            opacity: 100.0,
        }]);
        let nuevo = estirado(&original, 8.0).unwrap();
        assert!((nuevo.speed - 0.5).abs() < 1e-9);
        assert!((nuevo.duration() - 8.0).abs() < 1e-9);
        assert!((nuevo.keyframes.unwrap()[0].t - 8.0).abs() < 1e-9);
        assert!(estirado(&original, 0.2).is_err(), "haría falta 2000 %");
    }

    #[test]
    fn montar_al_ritmo_pone_cada_clip_entre_dos_marcadores() {
        let mut foto = clip("foto.jpg", 20.0, 5.0);
        foto.has_audio = false;
        let video = clip("plano.mp4", 30.0, 10.0);
        let corto = clip("corto.mp4", 40.0, 0.5);
        let colocados = colocar_al_ritmo(&[foto, video, corto], &[1.0, 3.0, 4.5, 6.0], 2);
        assert_eq!(colocados.len(), 3);
        assert!((colocados[0].timeline_start - 1.0).abs() < 1e-9);
        assert!((colocados[0].duration() - 2.0).abs() < 1e-9);
        assert!((colocados[1].timeline_start - 3.0).abs() < 1e-9);
        assert!((colocados[1].duration() - 1.5).abs() < 1e-9);
        // Sin material para 1,5 s, el plano corto se queda con lo que tiene.
        assert!((colocados[2].duration() - 0.5).abs() < 1e-9);
        assert!(colocados.iter().all(|clip| clip.track == 2));
        // Una foto larga amplía su duración de origen: `normalize` no la recorta.
        let foto_larga = colocar_al_ritmo(&[clip("foto.png", 0.0, 5.0)], &[0.0, 9.0], 0);
        assert!(foto_larga[0].source_duration_seconds.unwrap() >= 9.0);
    }

    #[test]
    fn la_ganancia_lleva_cada_voz_a_menos_16_lufs() {
        assert!((ganancia_para(-23.0) - 7.0).abs() < 1e-9);
        assert!((ganancia_para(-10.0) + 6.0).abs() < 1e-9);
        assert!((ganancia_para(-90.0) - 24.0).abs() < 1e-9);
    }

    #[test]
    fn los_golpes_se_detectan_donde_sube_la_energia() {
        let mut niveles = vec![0.02_f32; 400];
        let golpes = [20usize, 70, 120, 170, 220, 270, 320];
        for &golpe in &golpes {
            for (paso, nivel) in [0.6_f32, 0.4, 0.25, 0.1].iter().enumerate() {
                niveles[golpe + paso] = *nivel;
            }
        }
        let detectados = detectar_golpes(&niveles);
        assert_eq!(detectados.len(), golpes.len(), "{detectados:?}");
        for (detectado, esperado) in detectados.iter().zip(golpes) {
            assert!((detectado - esperado as f64 / 100.0).abs() < 0.021, "{detectado} vs {esperado}");
        }
        assert!(detectar_golpes(&vec![0.3_f32; 300]).is_empty(), "sin golpes en un tono plano");
    }

    #[test]
    fn el_pitido_silencia_solo_el_tramo_marcado() {
        let entrevista = clip("voz.wav", 0.0, 10.0);
        let mut musica = clip("musica.wav", 0.0, 3.0);
        musica.track = 1;
        let resultado = silenciar_tramo(&[entrevista, musica], 2.0, 4.0, |clip| clip.track == 1).unwrap();
        let voz: Vec<&RoughClip> = resultado
            .iter()
            .filter(|clip| clip.path == Path::new("voz.wav"))
            .collect();
        assert_eq!(voz.len(), 3);
        assert!(!voz[0].muted && voz[1].muted && !voz[2].muted);
        assert!((voz[1].timeline_start - 2.0).abs() < 1e-9);
        assert!((voz[1].duration() - 2.0).abs() < 1e-9);
        assert!((voz[2].in_seconds - 4.0).abs() < 1e-9);
        // La pista bloqueada no se toca.
        assert_eq!(resultado.iter().filter(|clip| clip.track == 1).count(), 1);
    }

    #[test]
    fn el_wav_del_pitido_es_valido() {
        let wav = wav_pitido(1.0);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 48_000 * 2);
        let datos = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(datos as usize, 48_000 * 2);
        let pico = wav[44..]
            .chunks_exact(2)
            .map(|par| i16::from_le_bytes([par[0], par[1]]).unsigned_abs())
            .max()
            .unwrap();
        assert!(pico > 7_000 && pico < 9_000, "-12 dBFS ≈ 8191: {pico}");
    }

    #[test]
    fn se_leen_los_microfonos_de_directshow() {
        let nuevo = "[dshow @ 0000] \"HD Webcam\" (video)\n\
[dshow @ 0000]   Alternative name \"@device_pnp_x\"\n\
[dshow @ 0000] \"Micrófono (Realtek(R) Audio)\" (audio)\n\
[dshow @ 0000]   Alternative name \"@device_cm_y\"\n";
        assert_eq!(microfonos_dshow(nuevo), vec!["Micrófono (Realtek(R) Audio)"]);
        let antiguo = "[dshow @ 0] DirectShow video devices\n\
[dshow @ 0]  \"Cam\"\n\
[dshow @ 0] DirectShow audio devices\n\
[dshow @ 0]  \"Blue Yeti\"\n";
        assert_eq!(microfonos_dshow(antiguo), vec!["Blue Yeti"]);
    }

    #[test]
    fn ken_burns_acerca_y_desplaza_desde_el_encuadre_actual() {
        let foto = clip("foto.jpg", 0.0, 6.0);
        let [inicio, fin] = ken_burns_de(&foto, -1.0);
        assert_eq!(inicio.t, 0.0);
        assert!((fin.t - 6.0).abs() < 1e-9);
        assert!((fin.scale - 115.0).abs() < 1e-9);
        assert!((fin.x + 40.0).abs() < 1e-9);
        let mut con_clave = foto.clone();
        con_clave.keyframes = Some(vec![inicio, fin]);
        let (_, _, escala, _) = con_clave.evaluate_transform(3.0);
        assert!((escala - 107.5).abs() < 1e-9);
    }

    #[test]
    fn la_imagen_en_imagen_queda_dentro_del_lienzo() {
        let (x, y) = posicion_pip(1.0, -1.0, ESCALA_PIP);
        assert!(x > 0.0 && y < 0.0);
        // El borde de la capa queda a 48 px del borde del lienzo 1920×1080.
        assert!((960.0 - (x + 1920.0 * 0.3 / 2.0) - 48.0).abs() < 1e-9);
        assert!((540.0 - (-y + 1080.0 * 0.3 / 2.0) - 48.0).abs() < 1e-9);
    }

    #[test]
    fn el_area_exportada_recorta_vertical_y_cuadrado_al_centro() {
        let monitor = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(640.0, 360.0));
        let vertical = area_exportada(monitor, 1080.0 / 1920.0);
        assert!((vertical.height() - 360.0).abs() < 0.01);
        assert!((vertical.width() - 202.5).abs() < 0.01);
        assert!((vertical.center().x - 320.0).abs() < 0.01);
        let horizontal = area_exportada(monitor, 16.0 / 9.0);
        assert!((horizontal.width() - 640.0).abs() < 0.01);
        assert!((horizontal.height() - 360.0).abs() < 0.01);
    }

    #[test]
    fn reemplazar_conserva_efectos_y_entrada() {
        let mut original = clip("viejo.mp4", 12.0, 4.0);
        original.in_seconds = 3.0;
        original.out_seconds = 7.0;
        original.exposure = 0.4;
        original.gain_db = -3.0;
        original.proxy = Some(PathBuf::from("proxy.mp4"));
        let sonda = MediaProbe {
            duration: 30.0,
            has_video: true,
            has_audio: true,
            frame_rate: None,
            variable_frame_rate: false,
            source_pts: None,
        };
        let nuevo = reemplazado(&original, Path::new("nuevo.mp4"), &sonda, false).unwrap();
        assert_eq!(nuevo.path, PathBuf::from("nuevo.mp4"));
        assert_eq!(nuevo.in_seconds, 3.0);
        assert!((nuevo.duration() - 4.0).abs() < 1e-9);
        assert_eq!(nuevo.exposure, 0.4);
        assert_eq!(nuevo.gain_db, -3.0);
        assert_eq!(nuevo.timeline_start, 12.0);
        assert!(nuevo.proxy.is_none(), "el proxy era del medio viejo");
        // Un medio más corto que la entrada empieza desde el principio.
        let corto = MediaProbe { duration: 5.0, ..sonda };
        let nuevo = reemplazado(&original, Path::new("corto.mp4"), &corto, false).unwrap();
        assert_eq!(nuevo.in_seconds, 0.0);
        assert!((nuevo.duration() - 4.0).abs() < 1e-9);
        let sin_imagen = MediaProbe {
            has_video: false,
            ..corto
        };
        assert!(reemplazado(&original, Path::new("voz.wav"), &sin_imagen, false).is_err());
    }

    #[test]
    fn recopilar_no_pisa_archivos_con_el_mismo_nombre() {
        let mut usados = HashSet::new();
        assert_eq!(nombre_libre(Path::new("C:/a/plano.mp4"), &mut usados), "plano.mp4");
        assert_eq!(nombre_libre(Path::new("D:/b/PLANO.mp4"), &mut usados), "PLANO-2.mp4");
        assert_eq!(nombre_libre(Path::new("E:/c/plano.mp4"), &mut usados), "plano-3.mp4");
        let mut anidado = clip("C:/a/plano.mp4", 0.0, 1.0);
        anidado.lut = Some(PathBuf::from("C:/luts/calido.cube"));
        let contenedor = RoughClip {
            nested: Some(vec![anidado]),
            ..Default::default()
        };
        let proyecto = RoughProject {
            clips: vec![contenedor, clip("C:/a/plano.mp4", 3.0, 1.0)],
            ..RoughProject::default()
        };
        let rutas = rutas_del_proyecto(&proyecto);
        assert_eq!(
            rutas,
            vec![PathBuf::from("C:/a/plano.mp4"), PathBuf::from("C:/luts/calido.cube")]
        );
        let mut mapa = HashMap::new();
        mapa.insert(PathBuf::from("C:/a/plano.mp4"), PathBuf::from("X:/Medios/plano.mp4"));
        let mut copia = proyecto.clips[0].clone();
        remapear_clip(&mut copia, &mapa);
        assert_eq!(
            copia.nested.unwrap()[0].path,
            PathBuf::from("X:/Medios/plano.mp4")
        );
        assert_eq!(nombre_de_archivo("Episodio 3: ¿qué?"), "Episodio 3_ ¿qué_");
    }

    #[test]
    fn los_capitulos_siguen_las_reglas_de_youtube() {
        let (texto, avisos) = capitulos_youtube(
            &[
                (0.2, "Intro".to_owned()),
                (65.0, "El tema".to_owned()),
                (3700.0, "Despedida".to_owned()),
            ],
            3800.0,
        );
        assert_eq!(texto, "0:00:00 Intro\n0:01:05 El tema\n1:01:40 Despedida\n");
        assert!(avisos.is_empty(), "{avisos:?}");
        let (texto, avisos) = capitulos_youtube(&[(30.0, "Uno".to_owned())], 40.0);
        assert_eq!(texto, "0:00 Introducción\n0:30 Uno\n");
        assert_eq!(avisos.len(), 2, "{avisos:?}");
    }

    #[test]
    fn la_transcripcion_sale_en_parrafos_con_hora() {
        let palabras = vec![
            palabra("Hola.", 0.0, 0.4),
            palabra("Bienvenidos", 1.2, 1.8),
            palabra("todos", 1.9, 2.3),
            palabra("hoy", 5.0, 5.3),
        ];
        assert_eq!(
            transcripcion_en_texto(&palabras),
            "[00:00:00] Hola.\n\n[00:00:01] Bienvenidos todos\n\n[00:00:05] hoy\n\n"
        );
    }

    #[test]
    fn los_subtitulos_largos_se_parten_en_dos_lineas_como_mucho() {
        let texto = "Esta es una frase bastante larga que no cabe de ninguna manera en una sola línea de subtítulo y necesita partirse en varias";
        let (nuevos, cambiados) = dividir_subtitulos(
            &[Subtitle {
                start: 10.0,
                end: 16.0,
                text: texto.to_owned(),
            }],
            42,
        );
        assert_eq!(cambiados, 1);
        assert!(nuevos.len() >= 2);
        assert_eq!(nuevos[0].start, 10.0);
        assert_eq!(nuevos.last().unwrap().end, 16.0);
        for par in nuevos.windows(2) {
            assert!((par[0].end - par[1].start).abs() < 1e-9);
        }
        for subtitulo in &nuevos {
            let lineas: Vec<&str> = subtitulo.text.lines().collect();
            assert!(lineas.len() <= 2, "{:?}", subtitulo.text);
            assert!(lineas.iter().all(|linea| linea.chars().count() <= 42), "{:?}", subtitulo.text);
        }
        let unidos: Vec<&str> = nuevos
            .iter()
            .flat_map(|subtitulo| subtitulo.text.split_whitespace())
            .collect();
        assert_eq!(unidos.join(" "), texto);
        let corto = Subtitle {
            start: 0.0,
            end: 1.0,
            text: "Hola".to_owned(),
        };
        assert_eq!(dividir_subtitulos(&[corto], 42).1, 0);
    }

    #[test]
    fn el_fcpxml_lleva_medios_carriles_y_tiempos_exactos() {
        let mut video = clip("C:\\Medios\\Entrevista 1.mov", 2.0, 4.0);
        video.in_seconds = 1.0;
        video.out_seconds = 5.0;
        video.source_duration_seconds = Some(60.0);
        let mut musica = clip("C:\\Medios\\Música & ritmo.wav", 0.0, 6.0);
        musica.has_video = false;
        musica.track = 1;
        let titulo = RoughClip {
            title: Some(Titulo::default()),
            ..Default::default()
        };
        let (xml, omitidos) = fcpxml("Episodio <1>", &[video, musica, titulo], Timebase::P25, (1920, 1080));
        assert_eq!(omitidos.len(), 1);
        assert_eq!(xml.matches("<asset id=").count(), 2);
        assert_eq!(xml.matches("<asset-clip").count(), 2);
        assert!(xml.contains("frameDuration=\"1/25s\""));
        assert!(xml.contains("lane=\"1\" offset=\"50/25s\" start=\"25/25s\" duration=\"100/25s\""));
        assert!(xml.contains("lane=\"-2\" offset=\"0s\""));
        assert!(xml.contains("srcEnable=\"audio\""));
        assert!(xml.contains("src=\"file:///C:/Medios/Entrevista%201.mov\""));
        assert!(xml.contains("M%C3%BAsica%20%26%20ritmo.wav"));
        assert!(xml.contains("name=\"Episodio &lt;1&gt;\""));
        assert!(xml.contains("<gap name=\"Hueco\" offset=\"0s\" start=\"0s\" duration=\"150/25s\">"));
        assert_eq!(xml.matches("<asset ").count(), xml.matches("</asset>").count());
    }

    fn grafo(clips: &[RoughClip], tamano: (u32, u32)) -> String {
        let mut orden = Command::new("ffmpeg");
        let (indices, titulos) = push_render_inputs(&mut orden, clips, tamano, false, Timebase::P25);
        build_render_filters(
            clips,
            &indices,
            &titulos,
            tamano,
            true,
            false,
            &[],
            0.0,
            false,
            Timebase::P25,
            None,
        )
        .unwrap()
        .join(";")
    }

    #[test]
    fn la_posicion_exportada_es_proporcional_al_tamano_de_salida() {
        let capa = RoughClip {
            path: PathBuf::from("capa.mp4"),
            out_seconds: 2.0,
            position_x: 300.0,
            position_y: -108.0,
            ..Default::default()
        };
        // En 1080p, píxeles tal cual (como antes).
        assert!(grafo(&[capa.clone()], (1920, 1080)).contains("x=(W-w)/2+300.000:y=(H-h)/2+-108.000"));
        // En 720p, dos tercios: donde la ve el monitor.
        assert!(grafo(&[capa.clone()], (1280, 720)).contains("x=(W-w)/2+200.000:y=(H-h)/2+-72.000"));
        // En 4K, el doble.
        assert!(grafo(&[capa], (3840, 2160)).contains("x=(W-w)/2+600.000:y=(H-h)/2+-216.000"));
    }

    #[test]
    fn en_vertical_la_escala_del_clip_se_respeta() {
        let plano = |escala: f64| RoughClip {
            path: PathBuf::from("plano.mp4"),
            out_seconds: 2.0,
            scale_percent: escala,
            ..Default::default()
        };
        // Al 100 %, el mismo relleno de siempre.
        assert!(grafo(&[plano(100.0)], (1080, 1920))
            .contains("scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920"));
        // Al 50 %, la mitad del encuadre de relleno: antes se ignoraba.
        assert!(grafo(&[plano(50.0)], (1080, 1920))
            .contains("scale=540:960:force_original_aspect_ratio=increase,crop=540:960"));
        // En horizontal no cambia nada: ajustar sin recortar.
        assert!(grafo(&[plano(50.0)], (1920, 1080))
            .contains("scale=960:540:force_original_aspect_ratio=decrease,setsar=1"));
    }

    #[test]
    fn el_zoom_animado_tambien_funciona_en_vertical() {
        let mut foto = clip("foto.jpg", 0.0, 4.0);
        foto.keyframes = Some(ken_burns_de(&foto, 1.0).to_vec());
        let texto = grafo(&[foto], (1080, 1920));
        assert!(texto.contains("eval=frame:force_original_aspect_ratio=increase"), "{texto}");
    }

    #[test]
    fn el_monitor_toma_la_proporcion_de_la_exportacion() {
        assert_eq!(monitor_canvas((1920, 1080)), (640, 360));
        assert_eq!(monitor_canvas((3840, 2160)), (640, 360));
        assert_eq!(monitor_canvas((1080, 1920)), (202, 360));
        assert_eq!(monitor_canvas((1080, 1080)), (360, 360));
        assert_eq!(monitor_canvas((1080, 1350)), (288, 360));
        assert_eq!(monitor_canvas((3840, 1600)), (640, 266));
        assert_eq!(fit_to_monitor("a", "b", (640, 360)), "[a]null[b]");
        assert!(fit_to_monitor("a", "b", (202, 360)).contains("pad=640:360:(ow-iw)/2:(oh-ih)/2"));
        let mut filtros = vec!["color=c=black:s=202x360[vout]".to_owned()];
        assert_eq!(
            append_monitor_view(&mut filtros, (202, 360), MonitorScopes::default()),
            "vmonitor"
        );
        let dos = MonitorScopes {
            waveform: true,
            vectorscope: true,
            ..MonitorScopes::default()
        };
        assert_eq!(append_monitor_view(&mut filtros, (640, 360), dos), "vscoped");
        assert!(filtros.iter().any(|filtro| filtro.starts_with("[vmonitor]split=3")));
    }

    #[test]
    fn los_cuatro_visores_van_cada_uno_a_su_esquina() {
        let todos = MonitorScopes {
            waveform: true,
            vectorscope: true,
            parade: true,
            histogram: true,
        };
        let mut filtros = Vec::new();
        assert_eq!(append_monitor_scopes_from(&mut filtros, "vmonitor", todos), "vscoped");
        let grafo = filtros.join(";");
        assert!(grafo.starts_with("[vmonitor]split=5[scopebase][scopein0][scopein1][scopein2][scopein3]"));
        assert!(grafo.contains("display=parade"));
        assert!(grafo.contains("histogram=display_mode=overlay"));
        for esquina in ["overlay=0:H-h", "overlay=W-w:H-h", "overlay=0:0", "overlay=W-w:0"] {
            assert!(grafo.contains(esquina), "falta {esquina}");
        }
        // Cada etiqueta se produce una vez y se consume una vez.
        assert_eq!(grafo.matches("[vscoped]").count(), 1);
        let solo_parade = MonitorScopes {
            parade: true,
            ..MonitorScopes::default()
        };
        let mut filtros = Vec::new();
        append_monitor_scopes_from(&mut filtros, "vmonitor", solo_parade);
        assert_eq!(filtros.len(), 3);
        assert!(filtros[2].ends_with("overlay=0:0[vscoped]"));
    }

    #[test]
    fn un_montaje_sobrevive_a_la_ida_y_vuelta_por_fcpxml() {
        let mut entrevista = clip("C:/Medios/Entrevista 1.mov", 2.0, 4.0);
        entrevista.in_seconds = 1.0;
        entrevista.out_seconds = 5.0;
        entrevista.source_duration_seconds = Some(60.0);
        let mut recurso = clip("C:/Medios/Recurso.mov", 3.0, 2.0);
        recurso.track = 1;
        recurso.has_audio = false;
        recurso.enabled = false;
        let mut musica = clip("C:/Medios/Música & ritmo.wav", 0.0, 6.0);
        musica.has_video = false;
        musica.track = 1;
        let originales = vec![entrevista, recurso, musica];
        let (xml, omitidos) = fcpxml("Episodio 3", &originales, Timebase::P25, (1920, 1080));
        assert!(omitidos.is_empty());
        let importado = intercambio::importar_fcpxml(&xml).unwrap();
        assert_eq!(importado.nombre, "Episodio 3");
        assert_eq!(importado.cadencia, Some(Timebase::P25));
        assert!(importado.avisos.is_empty(), "{:?}", importado.avisos);
        assert_eq!(importado.clips.len(), originales.len());
        for (antes, despues) in originales.iter().zip(&importado.clips) {
            assert_eq!(despues.path, antes.path);
            assert_eq!(despues.track, antes.track, "{:?}", antes.path);
            assert_eq!(despues.has_video, antes.has_video);
            assert_eq!(despues.has_audio, antes.has_audio);
            assert_eq!(despues.enabled, antes.enabled);
            assert!((despues.timeline_start - antes.timeline_start).abs() < 1e-9);
            assert!((despues.in_seconds - antes.in_seconds).abs() < 1e-9);
            assert!((despues.duration() - antes.duration()).abs() < 1e-9);
        }
    }

    #[test]
    fn el_render_sustituye_su_tramo_y_nada_mas() {
        let fondo = clip("fondo.mp4", 0.0, 10.0);
        let mut encima = clip("encima.mp4", 3.0, 2.0);
        encima.track = 1;
        let cache = CacheRender {
            archivo: PathBuf::from("render.mp4"),
            inicio: 2.0,
            fin: 6.0,
            clave: ClaveRender {
                generacion: 0,
                lienzo: (640, 360),
                subtitulos: false,
            },
        };
        let resultado = sustituir_por_render(&[fondo, encima], &cache).unwrap();
        let render: Vec<&RoughClip> = resultado
            .iter()
            .filter(|clip| clip.path == Path::new("render.mp4"))
            .collect();
        assert_eq!(render.len(), 1);
        assert_eq!(render[0].timeline_start, 2.0);
        assert!((render[0].duration() - 4.0).abs() < 1e-9);
        // Nada más ocupa el tramo renderizado.
        for clip in resultado.iter().filter(|clip| clip.path != Path::new("render.mp4")) {
            let fin = clip.timeline_start + clip.duration();
            assert!(fin <= 2.0 + 1e-6 || clip.timeline_start >= 6.0 - 1e-6, "{:?}", clip.path);
        }
        // El fondo sigue antes y después del tramo.
        let fondo_total: f64 = resultado
            .iter()
            .filter(|clip| clip.path == Path::new("fondo.mp4"))
            .map(RoughClip::duration)
            .sum();
        assert!((fondo_total - 6.0).abs() < 1e-9);
        // Con una rampa cortada por el tramo, no se sustituye.
        let mut rampa = clip("rampa.mp4", 0.0, 4.0);
        rampa.speed_ramp = Some(vec![SpeedPoint {
            source_t: 0.0,
            speed: 1.0,
        }]);
        assert!(sustituir_por_render(&[rampa], &cache).is_none());
    }

    #[test]
    fn recortar_un_plano_recorta_su_audio_enlazado() {
        let mut video = clip("entrevista.mov", 0.0, 10.0);
        video.has_audio = false;
        let mut audio = video.clone();
        audio.has_video = false;
        audio.has_audio = true;
        let mut detras = clip("musica.wav", 10.0, 5.0);
        detras.has_video = false;
        let antes = vec![video, audio, detras];
        // Entrada del vídeo 1 s más tarde, sin ripple.
        let mut actuales = antes.clone();
        actuales[0] = montaje::retime(&antes[0], 1.0, 0.0).unwrap();
        recortar_parejas(&mut actuales, &antes, 0, (1.0, 0.0), false, &[false; 3]);
        assert!((actuales[1].timeline_start - 1.0).abs() < 1e-9);
        assert!((actuales[1].in_seconds - 1.0).abs() < 1e-9);
        assert_eq!(actuales[2].timeline_start, 10.0);
        // Con ripple, la pareja conserva su inicio y lo de detrás se acerca.
        let mut actuales = antes.clone();
        recortar_parejas(&mut actuales, &antes, 0, (1.0, 0.0), true, &[false; 3]);
        assert_eq!(actuales[1].timeline_start, 0.0);
        assert!((actuales[1].duration() - 9.0).abs() < 1e-9);
        assert!((actuales[2].timeline_start - 9.0).abs() < 1e-9);
        // Salida 2 s antes.
        let mut actuales = antes.clone();
        recortar_parejas(&mut actuales, &antes, 0, (0.0, -2.0), false, &[false; 3]);
        assert!((actuales[1].duration() - 8.0).abs() < 1e-9);
        // Una pareja en pista bloqueada no se toca.
        let mut actuales = antes.clone();
        recortar_parejas(&mut actuales, &antes, 0, (1.0, 0.0), false, &[false, true, false]);
        assert_eq!(actuales[1].timeline_start, 0.0);
    }

    #[test]
    fn cada_stem_lleva_solo_el_audio_audible_de_su_pista() {
        let mut voz = clip("voz.wav", 0.0, 10.0);
        voz.has_video = false;
        let camara = clip("camara.mp4", 0.0, 10.0);
        let mut musica = clip("musica.wav", 2.0, 5.0);
        musica.has_video = false;
        musica.track = 1;
        let mut muda = clip("muda.wav", 0.0, 3.0);
        muda.has_video = false;
        muda.track = 2;
        muda.muted = true;
        let titulo = RoughClip {
            title: Some(Titulo::default()),
            ..Default::default()
        };
        let clips = vec![voz, camara, musica, muda, titulo];
        assert_eq!(pistas_de_audio(&clips), vec![0, 1]);
        let pista_cero = stem_de_pista(&clips, 0);
        assert_eq!(pista_cero.len(), 2);
        assert!(pista_cero.iter().all(|clip| !clip.has_video && clip.has_audio));
        assert_eq!(stem_de_pista(&clips, 1).len(), 1);
        assert!(stem_de_pista(&clips, 2).is_empty());
    }

    #[test]
    fn la_animacion_suavizada_arranca_y_frena_despacio() {
        let lineal = vec![
            animacion::Key { t: 0.0, v: 0.0 },
            animacion::Key { t: 1.0, v: 10.0 },
            animacion::Key { t: 3.0, v: 10.0 },
        ];
        let suave = suavizar_claves(&lineal);
        // Los extremos no se mueven y el tramo plano no gana puntos.
        assert_eq!(suave.first().unwrap().v, 0.0);
        assert_eq!(suave.last().unwrap().t, 3.0);
        assert!(suave.iter().all(|clave| clave.t <= 1.0 || clave.t == 3.0));
        let valor = |t: f64| animacion::value_at(&suave, t).unwrap();
        // Simétrica: en el centro, igual que la lineal.
        assert!((valor(0.5) - 5.0).abs() < 1e-9);
        // Al principio va más despacio que la lineal y al final frena.
        assert!(valor(0.2) < 2.0 * 0.6, "{}", valor(0.2));
        assert!(valor(0.8) > 8.0, "{}", valor(0.8));
        // Monótona, sin rebotes.
        assert!(suave.windows(2).all(|par| par[1].v >= par[0].v));
        // Suavizar dos veces no cambia nada.
        assert_eq!(suavizar_claves(&suave), suave);
        // Tramos muy cortos se dejan como están.
        let corto = vec![animacion::Key { t: 0.0, v: 0.0 }, animacion::Key { t: 0.1, v: 1.0 }];
        assert_eq!(suavizar_claves(&corto), corto);
    }

    #[test]
    fn transformacion_y_volumen_se_suavizan_con_la_misma_curva() {
        let foto = clip("foto.jpg", 0.0, 4.0);
        let suave = suavizar_transformacion(&ken_burns_de(&foto, 1.0));
        assert!(suave.len() > 2);
        let mut con_claves = foto.clone();
        con_claves.keyframes = Some(suave.clone());
        let (x, _, escala, _) = con_claves.evaluate_transform(2.0);
        assert!((escala - 107.5).abs() < 1e-6, "{escala}");
        assert!((x - 20.0).abs() < 1e-6, "{x}");
        let (_, _, pronto, _) = con_claves.evaluate_transform(0.4);
        assert!(pronto < 100.0 + 15.0 * 0.1, "arranca despacio: {pronto}");
        assert_eq!(suavizar_transformacion(&suave).len(), suave.len());
        let volumen = suavizar_volumen(&[
            efectos::VolumeKey { t: 0.0, db: -30.0 },
            efectos::VolumeKey { t: 2.0, db: 0.0 },
        ]);
        assert!(volumen.len() > 2);
        assert_eq!(volumen.last().unwrap().db, 0.0);
    }

    #[test]
    fn la_limpieza_de_voz_usa_rnnoise_si_hay_modelo() {
        let fx = efectos::ClipFx {
            voice_cleanup: 0.5,
            compressor: true,
            ..Default::default()
        };
        // Sin modelo, exactamente la cadena de siempre.
        assert_eq!(cadena_de_voz_con(&fx, None), fx.audio_chain());
        let cadena = cadena_de_voz_con(&fx, Some(Path::new("C:/NovaCut/Modelos/voz.rnnn")));
        assert!(cadena.contains("arnndn=m=C\\\\:/NovaCut/Modelos/voz.rnnn:mix=0.500"), "{cadena}");
        assert!(!cadena.contains("afftdn"), "{cadena}");
        assert!(cadena.contains("highpass=f=80"));
        assert!(cadena.contains("acompressor"), "el resto de la cadena se conserva");
        // Sin limpieza pedida, el modelo no cambia nada.
        let sin = efectos::ClipFx::default();
        assert_eq!(cadena_de_voz_con(&sin, Some(Path::new("voz.rnnn"))), sin.audio_chain());
    }

    #[test]
    fn la_secuencia_se_propone_con_la_imagen_y_la_cadencia_del_clip() {
        let horizontal = r#"{"streams":[{"width":3840,"height":2160}]}"#;
        assert_eq!(parse_dimensiones(horizontal), Some((3840, 2160)));
        // Un móvil graba 1920×1080 girado -90°: se ve en vertical.
        let movil = r#"{"streams":[{"width":1920,"height":1080,"side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}]}"#;
        assert_eq!(parse_dimensiones(movil), Some((1080, 1920)));
        let antiguo = r#"{"streams":[{"width":1280,"height":720,"tags":{"rotate":"270"}}]}"#;
        assert_eq!(parse_dimensiones(antiguo), Some((720, 1280)));
        let volteado = r#"{"streams":[{"width":1280,"height":720,"tags":{"rotate":"180"}}]}"#;
        assert_eq!(parse_dimensiones(volteado), Some((1280, 720)));
        assert_eq!(parse_dimensiones(r#"{"streams":[]}"#), None);
        assert_eq!(tamano_de_exportacion(1081, 1921), (1080, 1920));
        // Los móviles declaran cadencias casi profesionales.
        let casi = |fps: f64| cadencia_normalizada(Timebase::from_fps(fps));
        assert_eq!(casi(29.98), Timebase::NTSC30);
        assert_eq!(casi(30.02), Timebase::P30);
        assert_eq!(casi(59.9), Timebase::NTSC60);
        assert_eq!(casi(25.0), Timebase::P25);
        assert_eq!(texto_de_cadencia(Timebase::P25), "25 fps");
        assert_eq!(texto_de_cadencia(Timebase::NTSC30), "29.970 fps");
    }

    /// FFmpeg real o salto, salvo con NOVACUT_REQUIRE_REAL=1, como en
    /// `render_real_tests`.
    fn ffmpeg_real() -> bool {
        let disponible = Command::new(tool_path("ffmpeg.exe"))
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|estado| estado.success());
        if !disponible && std::env::var_os("NOVACUT_REQUIRE_REAL").is_some() {
            panic!("prueba real saltada: sin FFmpeg");
        }
        disponible
    }

    #[test]
    fn la_envolvente_y_la_sonoridad_se_leen_con_ffmpeg_real() {
        if !ffmpeg_real() {
            return;
        }
        let ruta = std::env::temp_dir().join(format!("novacut-pitido-{}.wav", std::process::id()));
        std::fs::write(&ruta, wav_pitido(2.0)).unwrap();
        let niveles = envolvente(&ruta, 0.0, 2.0).unwrap();
        assert!((niveles.len() as i64 - 200).abs() <= 3, "{} valores", niveles.len());
        // RMS de un seno de amplitud 0,25.
        let esperado = 0.25 / 2.0_f32.sqrt();
        assert!((niveles[100] - esperado).abs() < 0.02, "{}", niveles[100]);
        // Un seno de 1 kHz a -12 dBFS de pico suena a unos -15 LUFS.
        let lufs = medir_lufs(&ruta, 0.0, 2.0).unwrap();
        assert!(lufs > -18.0 && lufs < -12.0, "{lufs} LUFS");
        let _ = std::fs::remove_file(&ruta);
    }

    #[test]
    fn los_golpes_de_un_audio_real_caen_donde_suenan() {
        if !ffmpeg_real() {
            return;
        }
        // Ráfagas de 30 ms cada medio segundo sobre silencio.
        let mut wav = wav_pitido(4.0);
        let golpes: Vec<f64> = (0..8).map(|numero| 0.25 + numero as f64 * 0.5).collect();
        for (indice, muestra) in wav[44..].chunks_exact_mut(2).enumerate() {
            let t = indice as f64 / 48_000.0;
            if !golpes.iter().any(|golpe| t >= *golpe && t < golpe + 0.03) {
                muestra.copy_from_slice(&0i16.to_le_bytes());
            }
        }
        let ruta = std::env::temp_dir().join(format!("novacut-golpes-{}.wav", std::process::id()));
        std::fs::write(&ruta, wav).unwrap();
        let detectados = detectar_golpes(&envolvente(&ruta, 0.0, 4.0).unwrap());
        let _ = std::fs::remove_file(&ruta);
        assert_eq!(detectados.len(), golpes.len(), "{detectados:?}");
        for (detectado, golpe) in detectados.iter().zip(&golpes) {
            assert!((detectado - golpe).abs() < 0.03, "{detectado} vs {golpe}");
        }
    }

    /// Vídeo de prueba de 2 s con una fuente lavfi (y tono de 440 Hz).
    fn medio_de_prueba(nombre: &str, fuente: &str) -> PathBuf {
        let carpeta = std::env::temp_dir().join(format!("novacut-mejoras-{}", std::process::id()));
        std::fs::create_dir_all(&carpeta).unwrap();
        let ruta = carpeta.join(nombre);
        let estado = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", fuente])
            .args(["-f", "lavfi", "-i", "sine=f=440:d=2:sample_rate=48000"])
            .args(["-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&ruta)
            .status()
            .unwrap();
        assert!(estado.success());
        ruta
    }

    fn pixel(fotograma: &PreviewFrame, x: usize, y: usize) -> [u8; 3] {
        let inicio = (y * fotograma.width + x) * 4;
        [fotograma.pixels[inicio], fotograma.pixels[inicio + 1], fotograma.pixels[inicio + 2]]
    }

    fn blanco(color: [u8; 3]) -> bool {
        color.iter().all(|canal| *canal > 200)
    }

    fn negro(color: [u8; 3]) -> bool {
        color.iter().all(|canal| *canal < 30)
    }

    #[test]
    fn el_monitor_en_vertical_muestra_lo_que_saldra() {
        if !ffmpeg_real() {
            return;
        }
        let video = medio_de_prueba("blanco-vertical.mp4", "color=c=white:s=640x360:r=25:d=2");
        let mut plano = RoughClip {
            path: video,
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            ..Default::default()
        };
        let lienzo = monitor_canvas((1080, 1920));
        let fotograma = render_preview_frame_at(&[(plano.clone(), 1.0)], Timebase::P25, lienzo).unwrap();
        assert_eq!((fotograma.width, fotograma.height), (640, 360));
        // Relleno: la franja vertical del centro es imagen; los lados, bandas.
        assert!(blanco(pixel(&fotograma, 320, 180)));
        assert!(blanco(pixel(&fotograma, 320, 5)), "{:?}", [0usize, 5, 40, 90, 150, 180, 250, 300, 355].map(|y| (y, pixel(&fotograma, 320, y))));
        assert!(negro(pixel(&fotograma, 20, 180)), "{:?}", pixel(&fotograma, 20, 180));
        assert!(negro(pixel(&fotograma, 620, 180)));
        // Al 50 %, la mitad del encuadre de relleno: arriba ya no hay imagen.
        plano.scale_percent = 50.0;
        let fotograma = render_preview_frame_at(&[(plano, 1.0)], Timebase::P25, lienzo).unwrap();
        assert!(blanco(pixel(&fotograma, 320, 180)));
        assert!(negro(pixel(&fotograma, 320, 30)), "{:?}", pixel(&fotograma, 320, 30));
    }

    fn exportar_a(clips: &[RoughClip], salida: &Path, tamano: (u32, u32)) {
        let trabajo = RenderJob {
            clips: clips.to_vec(),
            output: salida.to_path_buf(),
            fast: true,
            size: tamano,
            audio_only: false,
            format: ExportFormat::Mp4Video,
            track_gains: Vec::new(),
            master_gain_db: 0.0,
            normalize_loudness: false,
            timebase: Timebase::P25,
            measured_loudness: None,
            hw: None,
            skip: 0.0,
            length: None,
            encode: exportacion::EncodeSettings::default(),
        };
        let progreso = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        run_export(&trabajo, &AtomicBool::new(false), &progreso).unwrap();
    }

    #[test]
    fn la_exportacion_coloca_y_escala_como_el_monitor() {
        if !ffmpeg_real() {
            return;
        }
        let fondo = medio_de_prueba("negro.mp4", "color=c=black:s=640x360:r=25:d=2");
        let capa = medio_de_prueba("blanco.mp4", "color=c=white:s=640x360:r=25:d=2");
        let base = RoughClip {
            path: fondo,
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            ..Default::default()
        };
        // Imagen en imagen arriba a la derecha.
        let (x, y) = posicion_pip(1.0, -1.0, ESCALA_PIP);
        let esquina = RoughClip {
            path: capa.clone(),
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            track: 1,
            has_audio: false,
            scale_percent: ESCALA_PIP,
            position_x: x,
            position_y: y,
            ..Default::default()
        };
        let carpeta = capa.parent().unwrap().to_path_buf();
        // En 720p, como en 1080p: la capa cae en la esquina, a 1/40 del borde.
        let salida = carpeta.join("pip-720.mp4");
        exportar_a(&[base.clone(), esquina.clone()], &salida, (1280, 720));
        let fotograma = extract_frame(&salida, 1.0, 1280, 720).unwrap();
        assert!(blanco(pixel(&fotograma, 1280 - 32 - 60, 32 + 40)), "la capa no está en su esquina");
        assert!(negro(pixel(&fotograma, 640, 360)));
        assert!(negro(pixel(&fotograma, 1280 - 10, 10)), "el margen debe verse");
        // Mismo sitio que en el monitor.
        let monitor = render_preview_frame(&[(base.clone(), 1.0), (esquina, 1.0)], Timebase::P25).unwrap();
        assert!(blanco(pixel(&monitor, 640 - 16 - 30, 16 + 20)));
        // Vertical al 50 %: el centro es imagen y arriba del todo, no.
        let mitad = RoughClip {
            path: capa,
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            track: 1,
            has_audio: false,
            scale_percent: 50.0,
            ..Default::default()
        };
        let salida = carpeta.join("vertical-50.mp4");
        exportar_a(&[base, mitad], &salida, (360, 640));
        let fotograma = extract_frame(&salida, 1.0, 360, 640).unwrap();
        assert!(blanco(pixel(&fotograma, 180, 320)));
        assert!(negro(pixel(&fotograma, 180, 40)), "{:?}", pixel(&fotograma, 180, 40));
    }

    /// Fallo anterior a estas rondas: `rotate` calculaba su lienzo con
    /// `rotw(iw)`, es decir, para un giro de `iw` radianes. En 720p el
    /// lienzo salía de 959 px y la imagen perdía 160 px por cada lado.
    #[test]
    fn un_plano_a_pantalla_completa_llega_a_los_bordes_en_cualquier_tamano() {
        if !ffmpeg_real() {
            return;
        }
        let blanco_total = medio_de_prueba("borde.mp4", "color=c=white:s=640x360:r=25:d=2");
        let plano = RoughClip {
            path: blanco_total.clone(),
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            ..Default::default()
        };
        for (ancho, alto) in [(1280, 720), (854, 480), (1080, 1920)] {
            let salida = blanco_total.with_file_name(format!("borde-{ancho}x{alto}.mp4"));
            exportar_a(&[plano.clone()], &salida, (ancho, alto));
            let fotograma = extract_frame(&salida, 1.0, ancho as usize, alto as usize).unwrap();
            for (x, y) in [(4, alto as usize / 2), (ancho as usize - 5, alto as usize / 2), (ancho as usize / 2, 4), (ancho as usize / 2, alto as usize - 5)] {
                assert!(
                    blanco(pixel(&fotograma, x, y)),
                    "{ancho}x{alto}: ({x},{y}) = {:?}",
                    pixel(&fotograma, x, y)
                );
            }
        }
    }

    #[test]
    fn el_grafo_de_reproduccion_con_los_cuatro_visores_funciona() {
        if !ffmpeg_real() {
            return;
        }
        let video = medio_de_prueba("carta.mp4", "testsrc2=s=640x360:r=25:d=2");
        let clips = vec![RoughClip {
            path: video,
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            ..Default::default()
        }];
        for tamano in [(1920, 1080), (1080, 1920), (1080, 1080)] {
            let lienzo = monitor_canvas(tamano);
            let mut orden = Command::new(tool_path("ffmpeg.exe"));
            orden.args(["-v", "error"]);
            let (indices, titulos) =
                push_render_inputs(&mut orden, &clips, (lienzo.0 as u32, lienzo.1 as u32), false, Timebase::P25);
            let mut filtros = build_render_filters(
                &clips,
                &indices,
                &titulos,
                (lienzo.0 as u32, lienzo.1 as u32),
                true,
                false,
                &[],
                0.0,
                false,
                Timebase::P25,
                None,
            )
            .unwrap();
            let todos = MonitorScopes {
                waveform: true,
                vectorscope: true,
                parade: true,
                histogram: true,
            };
            let etiqueta = append_monitor_view(&mut filtros, lienzo, todos);
            let _grafo = attach_graph(&mut orden, &filtros).unwrap();
            let salida = orden
                .args(["-map", &format!("[{etiqueta}]")])
                .args(["-frames:v", "3", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
                .output()
                .unwrap();
            assert!(
                salida.status.success(),
                "{tamano:?}: {:?} {:?} {}", salida.status, filtros,
                String::from_utf8_lossy(&salida.stderr)
            );
            assert_eq!(salida.stdout.len(), 640 * 360 * 4 * 3, "{tamano:?}");
        }
    }

    #[test]
    fn el_render_de_previsualizacion_sustituye_su_tramo_y_se_reproduce() {
        if !ffmpeg_real() {
            return;
        }
        let video = medio_de_prueba("tramo.mp4", "testsrc2=s=640x360:r=25:d=2");
        let clips = vec![RoughClip {
            path: video.clone(),
            out_seconds: 2.0,
            source_duration_seconds: Some(2.0),
            ..Default::default()
        }];
        let archivo = video.with_file_name("render-cache.mp4");
        let trabajo = RenderJob {
            clips: clips.clone(),
            output: archivo.clone(),
            fast: true,
            size: (640, 360),
            audio_only: false,
            format: ExportFormat::Mp4Video,
            track_gains: Vec::new(),
            master_gain_db: 0.0,
            normalize_loudness: false,
            timebase: Timebase::P25,
            measured_loudness: None,
            hw: None,
            skip: 0.5,
            length: Some(1.0),
            encode: exportacion::EncodeSettings::default(),
        };
        let progreso = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        run_export(&trabajo, &AtomicBool::new(false), &progreso).unwrap();
        let cache = CacheRender {
            archivo,
            inicio: 0.5,
            fin: 1.5,
            clave: ClaveRender {
                generacion: 0,
                lienzo: (640, 360),
                subtitulos: false,
            },
        };
        let sustituidos = sustituir_por_render(&clips, &cache).unwrap();
        assert_eq!(sustituidos.len(), 3, "cabeza, cola y el render");
        // El montaje con el render dentro se exporta entero y dura lo mismo.
        let salida = video.with_file_name("con-render.mp4");
        exportar_a(&sustituidos, &salida, (640, 360));
        let duracion = probe_media(&salida).unwrap().duration;
        assert!((duracion - 2.0).abs() < 0.15, "{duracion}");
        // En mitad del tramo se ve lo mismo que sin render.
        let directo = extract_frame(&salida, 1.0, 160, 90).unwrap();
        let original = extract_frame(&video, 1.0, 160, 90).unwrap();
        let diferencia: f64 = directo
            .pixels
            .iter()
            .zip(&original.pixels)
            .map(|(a, b)| (*a as f64 - *b as f64).abs())
            .sum::<f64>()
            / directo.pixels.len() as f64;
        assert!(diferencia < 12.0, "diferencia media {diferencia}");
    }

    /// Nivel RMS en dB de todo el audio de un archivo.
    fn nivel_db(ruta: &Path) -> f64 {
        let crudo = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-i"])
            .arg(ruta)
            .args(["-ac", "1", "-f", "f32le", "-"])
            .output()
            .unwrap()
            .stdout;
        let muestras: Vec<f64> = crudo
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)
            .collect();
        let potencia = muestras.iter().map(|m| m * m).sum::<f64>() / muestras.len().max(1) as f64;
        10.0 * potencia.max(1e-12).log10()
    }

    #[test]
    fn la_mejora_de_voz_usa_el_modelo_incluido_y_quita_ruido_de_verdad() {
        // El modelo viaja dentro del ejecutable: siempre hay RNNoise.
        let fx = efectos::ClipFx {
            voice_cleanup: 1.0,
            ..Default::default()
        };
        assert!(cadena_de_voz(&fx).contains("arnndn=m="), "{}", cadena_de_voz(&fx));
        let modelo = modelo_incluido().unwrap();
        assert_eq!(std::fs::read(&modelo).unwrap(), MODELO_VOZ);
        if !ffmpeg_real() {
            return;
        }
        let carpeta = std::env::temp_dir().join(format!("novacut-voz-{}", std::process::id()));
        std::fs::create_dir_all(&carpeta).unwrap();
        let ruido = carpeta.join("ruido.wav");
        let estado = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "anoisesrc=c=pink:a=0.2:d=3:r=48000"])
            .arg(&ruido)
            .status()
            .unwrap();
        assert!(estado.success());
        let exportar = |limpieza: f64, nombre: &str| {
            let clip = RoughClip {
                path: ruido.clone(),
                out_seconds: 3.0,
                source_duration_seconds: Some(3.0),
                has_video: false,
                fx: efectos::ClipFx {
                    voice_cleanup: limpieza,
                    ..Default::default()
                },
                ..Default::default()
            };
            let salida = carpeta.join(nombre);
            let trabajo = RenderJob {
                clips: vec![clip],
                output: salida.clone(),
                fast: false,
                size: (640, 360),
                audio_only: true,
                format: ExportFormat::WavAudio,
                track_gains: Vec::new(),
                master_gain_db: 0.0,
                normalize_loudness: false,
                timebase: Timebase::P25,
                measured_loudness: None,
                hw: None,
                skip: 0.0,
                length: None,
                encode: exportacion::EncodeSettings::default(),
            };
            let progreso = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
            run_export(&trabajo, &AtomicBool::new(false), &progreso).unwrap();
            salida
        };
        let sucio = nivel_db(&exportar(0.0, "sucio.wav"));
        let limpio = nivel_db(&exportar(1.0, "limpio.wav"));
        assert!(limpio < sucio - 12.0, "RNNoise solo bajó el ruido de {sucio:.1} a {limpio:.1} dB");
        let _ = std::fs::remove_dir_all(&carpeta);
    }

    #[test]
    fn las_acciones_tienen_texto_y_estan_en_un_grupo() {
        for accion in Accion::TODAS {
            assert!(!accion.titulo().is_empty());
            assert!(accion.ayuda().len() > 20);
            assert!(
                GRUPOS.iter().any(|(_, acciones)| acciones.contains(&accion)),
                "{accion:?} no está en el menú"
            );
        }
    }
}
