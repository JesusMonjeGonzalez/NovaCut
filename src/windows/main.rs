#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use editorcito::edl;
use editorcito::proxy_cache;
use editorcito::relink;
#[cfg(test)]
use editorcito::subtitles::srt_timestamp;
use editorcito::subtitles::{build_srt, parse_srt, search_subtitles, shift_subtitles, Subtitle};
use editorcito::Timebase;
use eframe::egui;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

mod aceleracion;
mod batch;
mod command_center;
mod efectos;
mod navigation;
mod subtitulos_animados;
mod transcripcion;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Fuera de Windows no hay consola que ocultar: el host de desarrollo en
/// macOS/Linux acepta la llamada y la ignora.
#[cfg(not(windows))]
trait CommandExt {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}

#[cfg(not(windows))]
impl CommandExt for Command {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self {
        self
    }
}

mod media_browser;

/// Resumen pequeño del reloj de presentación que viaja con el clip. No se
/// persisten todos los PTS, solo la evidencia suficiente para diagnosticar la
/// fuente y decidir si el filtro CFR debe tratarla como VFR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SourcePtsSummary {
    frame_count: u64,
    first_seconds: f64,
    last_seconds: f64,
    median_frame_duration_seconds: f64,
    max_frame_duration_seconds: f64,
    variable_delta_count: u64,
    gap_count: u64,
}

impl SourcePtsSummary {
    fn is_variable(&self) -> bool {
        let deltas = self.frame_count.saturating_sub(1).max(1) as f64;
        self.gap_count as f64 / deltas > 0.02 || self.variable_delta_count as f64 / deltas > 0.05
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct RoughClip {
    path: PathBuf,
    in_seconds: f64,
    out_seconds: f64,
    /// Duración física del archivo fuente. Permite recuperar handles sin
    /// fabricar tiempo inexistente al extender un clip recortado.
    #[serde(default)]
    source_duration_seconds: Option<f64>,
    /// Cadencia declarada por el medio. El montaje puede conformarla a su
    /// propio timebase, pero no debe perder este dato al guardar.
    #[serde(default)]
    source_timebase: Option<Timebase>,
    /// El scan de PTS encontro saltos por encima de la cadencia esperada.
    #[serde(default)]
    source_vfr: bool,
    /// Evidencia persistida del scan de PTS para diagnóstico y conformado.
    #[serde(default)]
    source_pts: Option<SourcePtsSummary>,
    #[serde(default = "enabled_by_default")]
    has_video: bool,
    has_audio: bool,
    #[serde(default = "normal_speed")]
    speed: f64,
    #[serde(default)]
    timeline_start: f64,
    #[serde(default)]
    track: usize,
    #[serde(default)]
    gain_db: f64,
    #[serde(default)]
    muted: bool,
    /// Balance estéreo: -1.0 = solo izquierda, 0.0 = centrado, 1.0 = solo derecha.
    #[serde(default)]
    pan: f64,
    #[serde(default)]
    position_x: f64,
    #[serde(default)]
    position_y: f64,
    #[serde(default = "normal_scale")]
    scale_percent: f64,
    #[serde(default)]
    rotation: f64,
    #[serde(default = "full_opacity")]
    opacity: f64,
    #[serde(default)]
    fade_in_seconds: f64,
    #[serde(default)]
    fade_out_seconds: f64,
    /// Titulo dibujado sobre la imagen. El clip no usa su medio.
    #[serde(default)]
    title: Option<Titulo>,
    /// Capa de ajuste: sin medio propio, aplica sus efectos de color/blur/
    /// máscara/LUT a todo lo compuesto por debajo en las pistas inferiores,
    /// como en Premiere o DaVinci.
    #[serde(default)]
    is_adjustment: bool,
    #[serde(default = "zero_color")]
    exposure: f64,
    #[serde(default = "zero_color")]
    contrast: f64,
    #[serde(default = "zero_color")]
    saturation: f64,
    /// Oscurecimiento de bordes; 0 apagado, 1 máximo.
    #[serde(default = "zero_color")]
    vignette: f64,
    /// Transición por negro con el clip anterior de la misma pista.
    #[serde(default)]
    transition: Option<String>,
    #[serde(default = "default_transition_duration")]
    transition_duration: f64,
    /// Etiqueta de color 0-6 para organizar el montaje.
    #[serde(default)]
    label: u8,
    /// Desenfoque gaussiano como fracción del lado corto (0 = nítido).
    #[serde(default = "zero_color")]
    blur: f64,
    /// Ruedas de color sombras/medios/altas por canal (−1…1).
    #[serde(default)]
    wheels: Option<Wheels>,
    /// Chroma key activo del clip (pantalla verde/azul).
    #[serde(default)]
    chroma: Option<Chroma>,
    /// Curvas RGB + luminancia (puntos x/y en 0…1).
    #[serde(default)]
    curves: Option<Curves>,
    /// Keyframes de transformación (t local en s del clip → x, y, escala, opacidad).
    #[serde(default)]
    keyframes: Option<Vec<TransformKeyframe>>,
    /// Modo de fusión con lo que hay debajo (normal = overlay clásico).
    #[serde(default)]
    fusion: Fusion,
    /// Máscara rectangular/elítmica con pluma.
    #[serde(default)]
    mask: Option<Mask>,
    /// LUT 3D (.cube) aplicado al clip.
    #[serde(default)]
    lut: Option<PathBuf>,
    /// Proxy de baja resolución para edición fluida (el export usa el original).
    #[serde(default)]
    proxy: Option<PathBuf>,
    /// Curva de velocidad sobre tiempo de origen local (segundos).
    #[serde(default)]
    speed_ramp: Option<Vec<SpeedPoint>>,
    /// Efectos de vídeo y audio (Lumetri básico, recorte, EQ, ducking…).
    #[serde(default)]
    fx: efectos::ClipFx,
    /// Estado de transiciones derivado para el render en curso; nunca se
    /// guarda, lo recalcula `resolve_render_clips`.
    #[serde(skip)]
    runtime: efectos::TransitionRuntime,
    /// Secuencia compuesta; sus clips usan tiempos relativos al contenedor.
    #[serde(default)]
    nested: Option<Vec<RoughClip>>,
    /// Instante de origen congelado: el clip muestra un único fotograma.
    #[serde(default)]
    freeze_at: Option<f64>,
    /// Clip activo. Uno desactivado permanece en el montaje pero no se compone
    /// ni se exporta, como el "enable/disable" de cualquier montador.
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

/// Modos de fusión; los nombres viajan igual que en macOS.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
enum Fusion {
    #[default]
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "multiplicar")]
    Multiply,
    #[serde(rename = "pantalla")]
    Screen,
    #[serde(rename = "superponer")]
    Overlay,
    #[serde(rename = "aclarar")]
    Lighten,
    #[serde(rename = "oscurecer")]
    Darken,
    #[serde(rename = "colorDodge")]
    ColorDodge,
    #[serde(rename = "colorBurn")]
    ColorBurn,
    #[serde(rename = "luzFuerte")]
    HardLight,
    #[serde(rename = "luzSuave")]
    SoftLight,
    #[serde(rename = "diferencia")]
    Difference,
    #[serde(rename = "exclusion")]
    Exclusion,
    /// Sin equivalente directo en FFmpeg; se renderiza como normal.
    #[serde(rename = "color")]
    Color,
    /// Sin equivalente directo en FFmpeg; se renderiza como normal.
    #[serde(rename = "luminosidad")]
    Luminosity,
}

impl Fusion {
    const ALL: [Self; 14] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Lighten,
        Self::Darken,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Color,
        Self::Luminosity,
    ];

    fn blend_mode(self) -> Option<&'static str> {
        match self {
            Self::Normal | Self::Color | Self::Luminosity => None,
            Self::Multiply => Some("multiply"),
            Self::Screen => Some("screen"),
            Self::Overlay => Some("overlay"),
            Self::Lighten => Some("lighten"),
            Self::Darken => Some("darken"),
            Self::ColorDodge => Some("dodge"),
            Self::ColorBurn => Some("burn"),
            Self::HardLight => Some("hardlight"),
            Self::SoftLight => Some("softlight"),
            Self::Difference => Some("difference"),
            Self::Exclusion => Some("exclusion"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiplicar",
            Self::Screen => "Pantalla",
            Self::Overlay => "Superponer",
            Self::Lighten => "Aclarar",
            Self::Darken => "Oscurecer",
            Self::ColorDodge => "Dodge",
            Self::ColorBurn => "Burn",
            Self::HardLight => "Luz fuerte",
            Self::SoftLight => "Luz suave",
            Self::Difference => "Diferencia",
            Self::Exclusion => "Exclusión",
            Self::Color => "Color (≈normal)",
            Self::Luminosity => "Luminosidad (≈normal)",
        }
    }
}

/// Máscara rectangular o elíptica con pluma, en fracciones del lienzo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
enum MaskShape {
    #[default]
    #[serde(rename = "rectangulo")]
    Rectangle,
    #[serde(rename = "elipse")]
    Ellipse,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Mask {
    #[serde(default, rename = "forma")]
    shape: MaskShape,
    #[serde(default = "half_center", rename = "posicionX")]
    position_x: f64,
    #[serde(default = "half_center", rename = "posicionY")]
    position_y: f64,
    #[serde(default = "half_center", rename = "tamanoX")]
    size_x: f64,
    #[serde(default = "half_center", rename = "tamanoY")]
    size_y: f64,
    #[serde(default = "default_feather", rename = "pluma")]
    feather: f64,
    #[serde(default, rename = "invertida")]
    inverted: bool,
}

fn default_feather() -> f64 {
    0.1
}

impl Default for Mask {
    fn default() -> Self {
        Self {
            shape: MaskShape::Rectangle,
            position_x: half_center(),
            position_y: half_center(),
            size_x: half_center(),
            size_y: half_center(),
            feather: default_feather(),
            inverted: false,
        }
    }
}

impl Mask {
    /// Expresión `geq` del factor de alfa sobre una capa de tamaño W×H.
    fn alpha_expression(&self, width: f64, height: f64) -> String {
        let cx = width * self.position_x.clamp(0.0, 1.0);
        let cy = height * self.position_y.clamp(0.0, 1.0);
        let hx = (width * self.size_x.clamp(0.0, 1.0) / 2.0).max(1.0);
        let hy = (height * self.size_y.clamp(0.0, 1.0) / 2.0).max(1.0);
        let feather_px = (self.feather.clamp(0.0, 1.0) * hx.min(hy)).max(0.5);
        let factor = if self.shape == MaskShape::Ellipse {
            let dx = format!("(X-{cx:.2})/{hx:.2}");
            let dy = format!("(Y-{cy:.2})/{hy:.2}");
            let d = format!("sqrt(({dx})*({dx})+({dy})*({dy}))");
            format!("clip((1-{d})/{:.4},0,1)", self.feather.clamp(0.01, 1.0))
        } else {
            format!(
                "clip(min(({hx:.2}-abs(X-{cx:.2}))/{feather_px:.2},({hy:.2}-abs(Y-{cy:.2}))/{feather_px:.2}),0,1)"
            )
        };
        let factor = if self.inverted {
            format!("(1-({factor}))")
        } else {
            factor
        };
        format!("alpha(X,Y)*({factor})")
    }
}

/// Estilo de los subtítulos quemados.
#[derive(Clone, Serialize, Deserialize)]
struct SubtitleStyle {
    #[serde(default = "subtitle_size")]
    size: f64,
    #[serde(default = "subtitle_position_y")]
    position_y: f64,
    #[serde(default = "full_channel")]
    red: f64,
    #[serde(default = "full_channel")]
    green: f64,
    #[serde(default = "full_channel")]
    blue: f64,
}

fn subtitle_size() -> f64 {
    54.0
}

fn subtitle_position_y() -> f64 {
    0.92
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            size: subtitle_size(),
            position_y: subtitle_position_y(),
            red: full_channel(),
            green: full_channel(),
            blue: full_channel(),
        }
    }
}

/// Un keyframe de transformación: t local (0 = inicio del clip) y el estado
/// completo de transformación en ese instante.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct TransformKeyframe {
    t: f64,
    x: f64,
    y: f64,
    scale: f64,
    opacity: f64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct SpeedPoint {
    source_t: f64,
    speed: f64,
}

impl RoughClip {
    fn speed_at_source_time(&self, source_t: f64) -> f64 {
        let Some(points) = &self.speed_ramp else {
            return self.speed.clamp(0.1, 8.0);
        };
        if points.is_empty() {
            return self.speed.clamp(0.1, 8.0);
        }
        let mut ordered = points.clone();
        ordered.sort_by(|left, right| left.source_t.total_cmp(&right.source_t));
        if source_t <= ordered[0].source_t {
            return ordered[0].speed.clamp(0.1, 8.0);
        }
        for pair in ordered.windows(2) {
            if source_t <= pair[1].source_t {
                let span = (pair[1].source_t - pair[0].source_t).max(1e-9);
                let mix = ((source_t - pair[0].source_t) / span).clamp(0.0, 1.0);
                return (pair[0].speed + (pair[1].speed - pair[0].speed) * mix).clamp(0.1, 8.0);
            }
        }
        ordered.last().unwrap().speed.clamp(0.1, 8.0)
    }

    fn speed_ramp_boundaries(&self) -> Vec<f64> {
        let duration = self.source_duration();
        let mut boundaries = vec![0.0, duration];
        if let Some(points) = &self.speed_ramp {
            for point in points {
                boundaries.push(point.source_t.clamp(0.0, duration));
            }
            // Subdivisión máxima de 0,25 s para aproximar una rampa suave.
            let steps = (duration / 0.25).ceil().clamp(1.0, 240.0) as usize;
            for step in 1..steps {
                boundaries.push(duration * step as f64 / steps as f64);
            }
        }
        boundaries.sort_by(f64::total_cmp);
        boundaries.dedup_by(|left, right| (*left - *right).abs() < 0.001);
        boundaries
    }

    fn ramped_duration(&self) -> f64 {
        self.speed_ramp_boundaries()
            .windows(2)
            .map(|window| {
                let midpoint = (window[0] + window[1]) / 2.0;
                (window[1] - window[0]) / self.speed_at_source_time(midpoint)
            })
            .sum()
    }

    /// Transformación evaluada en `local_t` (s de timeline dentro del clip):
    /// interpolación lineal entre keyframes; fuera de rango, el más cercano.
    fn evaluate_transform(&self, local_t: f64) -> (f64, f64, f64, f64) {
        let Some(keyframes) = &self.keyframes else {
            return (
                self.position_x,
                self.position_y,
                self.scale_percent,
                self.opacity,
            );
        };
        if keyframes.is_empty() {
            return (
                self.position_x,
                self.position_y,
                self.scale_percent,
                self.opacity,
            );
        }
        let sample = |k: &TransformKeyframe| (k.x, k.y, k.scale, k.opacity);
        if local_t <= keyframes[0].t {
            return sample(&keyframes[0]);
        }
        if local_t >= keyframes[keyframes.len() - 1].t {
            return sample(&keyframes[keyframes.len() - 1]);
        }
        for window in keyframes.windows(2) {
            let (left, right) = (window[0], window[1]);
            if local_t >= left.t && local_t <= right.t {
                let span = (right.t - left.t).max(1e-9);
                let mix = (local_t - left.t) / span;
                let mix_value = |a: f64, b: f64| a + (b - a) * mix;
                return (
                    mix_value(left.x, right.x),
                    mix_value(left.y, right.y),
                    mix_value(left.scale, right.scale),
                    mix_value(left.opacity, right.opacity),
                );
            }
        }
        sample(&keyframes[keyframes.len() - 1])
    }
}

/// Curvas de color: maestra (luma) más una curva por canal.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Curves {
    #[serde(default)]
    luma: Vec<CurvePoint>,
    #[serde(default, rename = "r")]
    red: Vec<CurvePoint>,
    #[serde(default, rename = "g")]
    green: Vec<CurvePoint>,
    #[serde(default, rename = "b")]
    blue: Vec<CurvePoint>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct CurvePoint {
    x: f64,
    y: f64,
}

impl Default for CurvePoint {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

impl Curves {
    /// Curva identidad: solo los puntos (0,0) y (1,1) en orden.
    fn channel_is_identity(points: &[CurvePoint]) -> bool {
        points.len() == 2
            && points[0].x.abs() < 1e-9
            && points[0].y.abs() < 1e-9
            && (points[1].x - 1.0).abs() < 1e-9
            && (points[1].y - 1.0).abs() < 1e-9
    }

    fn is_identity(&self) -> bool {
        Self::channel_is_identity(&self.luma)
            && Self::channel_is_identity(&self.red)
            && Self::channel_is_identity(&self.green)
            && Self::channel_is_identity(&self.blue)
    }
}

fn identity_channel() -> Vec<CurvePoint> {
    vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }]
}

/// Chroma key: color de pantalla, tolerancia, suavizado y supresión de derrame.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Chroma {
    #[serde(default)]
    red: f64,
    #[serde(default = "full_channel")]
    green: f64,
    #[serde(default)]
    blue: f64,
    #[serde(default = "default_tolerance")]
    tolerance: f64,
    #[serde(default = "default_smooth")]
    smooth: f64,
    #[serde(default = "default_spill")]
    spill: f64,
}

fn default_tolerance() -> f64 {
    0.4
}

fn default_smooth() -> f64 {
    0.15
}

fn default_spill() -> f64 {
    0.5
}

/// Ruedas de color: desplazamiento por canal en cada rango tonal.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Wheels {
    #[serde(default, rename = "sr")]
    shadows_r: f64,
    #[serde(default, rename = "sg")]
    shadows_g: f64,
    #[serde(default, rename = "sb")]
    shadows_b: f64,
    #[serde(default, rename = "mr")]
    mid_r: f64,
    #[serde(default, rename = "mg")]
    mid_g: f64,
    #[serde(default, rename = "mb")]
    mid_b: f64,
    #[serde(default, rename = "hr")]
    high_r: f64,
    #[serde(default, rename = "hg")]
    high_g: f64,
    #[serde(default, rename = "hb")]
    high_b: f64,
}

impl Wheels {
    fn is_neutral(&self) -> bool {
        [
            self.shadows_r,
            self.shadows_g,
            self.shadows_b,
            self.mid_r,
            self.mid_g,
            self.mid_b,
            self.high_r,
            self.high_g,
            self.high_b,
        ]
        .iter()
        .all(|value| value.abs() < 0.001)
    }
}

fn default_transition_duration() -> f64 {
    0.5
}

/// Qué gesto de timeline está en curso para el clip activo.
#[derive(Clone, Copy, PartialEq)]
enum DragKind {
    Move,
    TrimStart,
    TrimEnd,
    RippleTrimStart,
    RippleTrimEnd,
}

/// Herramienta activa del montaje. Cambia tanto el cursor como el significado
/// de los gestos sobre clips, regla y fondo de pistas.
#[derive(Clone, Copy, PartialEq)]
enum EditTool {
    Select,
    TrackSelect,
    Blade,
    Trim,
    RippleTrim,
    Hand,
    Zoom,
    Magic,
}

impl EditTool {
    const ALL: [Self; 8] = [
        Self::Select,
        Self::TrackSelect,
        Self::Blade,
        Self::Trim,
        Self::RippleTrim,
        Self::Hand,
        Self::Zoom,
        Self::Magic,
    ];

    fn icon(self) -> &'static str {
        match self {
            Self::Select => "↖",
            Self::TrackSelect => "⇥",
            Self::Blade => "✂",
            Self::Trim => "↔",
            Self::RippleTrim => "⇤",
            Self::Hand => "✋",
            Self::Zoom => "🔍",
            Self::Magic => "✨",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Select => "Selección",
            Self::TrackSelect => "Pista",
            Self::Blade => "Tijeras",
            Self::Trim => "Recortar",
            Self::RippleTrim => "Ripple",
            Self::Hand => "Mano",
            Self::Zoom => "Zoom",
            Self::Magic => "Varita",
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            Self::Select => "A",
            Self::TrackSelect => "U",
            Self::Blade => "C",
            Self::Trim => "R",
            Self::RippleTrim => "T",
            Self::Hand => "H",
            Self::Zoom => "Z",
            Self::Magic => "G",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Select => "Seleccionar, mover y ajustar bordes",
            Self::TrackSelect => "Seleccionar desde el clip hasta el final de su pista",
            Self::Blade => "Partir el clip exactamente donde pulses",
            Self::Trim => "Arrastrar cualquier mitad del clip para recortar ese borde",
            Self::RippleTrim => "Recortar y cerrar o abrir el montaje automáticamente",
            Self::Hand => "Arrastrar la timeline horizontalmente",
            Self::Zoom => "Clic para acercar; Mayús+clic para alejar",
            Self::Magic => "Detectar escenas en vídeo o silencios en audio",
        }
    }
}

enum TimelineToolAction {
    Split(usize, f64),
    Magic(usize),
    SelectTrack(usize, bool),
    Zoom(f64, f32),
}

/// Paleta de etiquetas: 0 = sin etiqueta.
fn label_color(label: u8) -> Option<egui::Color32> {
    match label {
        1 => Some(egui::Color32::from_rgb(246, 83, 83)),
        2 => Some(egui::Color32::from_rgb(246, 150, 30)),
        3 => Some(egui::Color32::from_rgb(240, 220, 40)),
        4 => Some(egui::Color32::from_rgb(90, 200, 110)),
        5 => Some(egui::Color32::from_rgb(80, 160, 250)),
        6 => Some(egui::Color32::from_rgb(190, 100, 250)),
        _ => None,
    }
}

const THUMB_WIDTH: usize = 160;
const THUMB_HEIGHT: usize = 90;

const MONITOR_WIDTH: usize = 640;
const MONITOR_HEIGHT: usize = 360;
/// Duración por defecto de una imagen fija al importarla.
const DEFAULT_IMAGE_DURATION: f64 = 5.0;

/// ¿El medio es una imagen fija? Estas se importan con duración fija y se
/// renderizan con un input en bucle de FFmpeg.
fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "bmp" | "webp" | "gif" | "tif" | "tiff")
    )
}

/// Reproducción en curso del monitor: proceso FFmpeg + reloj local.
struct Playback {
    child: std::process::Child,
    audio_child: std::process::Child,
    rx: Receiver<Option<PreviewFrame>>,
    start_playhead: f64,
    last_consumed: u64,
    timebase: Timebase,
    /// RMS de audio por canal, actualizado por el hilo lector.
    meter: Arc<std::sync::Mutex<(f32, f32)>>,
    /// Mantiene vivo el dispositivo de audio mientras se reproduce.
    _stream: rodio::OutputStream,
    sink: Arc<rodio::Sink>,
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.sink.stop();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.audio_child.kill();
        let _ = self.audio_child.wait();
    }
}

/// Extrae un fotograma escalado de un medio para previsualizaciones.
fn extract_frame(
    path: &Path,
    time: f64,
    width: usize,
    height: usize,
) -> Result<PreviewFrame, String> {
    let mut command = Command::new(tool_path("ffmpeg.exe"));
    command.args(["-v", "error"]);
    if is_image_file(path) {
        command.arg("-i");
    } else {
        command.args(["-ss", &format_seconds(time), "-i"]);
    }
    let result = command
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,format=rgba"
            ),
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFmpeg no esta disponible: {error}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    if result.stdout.len() != width * height * 4 {
        return Err("Fotograma incompleto".to_owned());
    }
    Ok(PreviewFrame {
        pixels: result.stdout,
        width,
        height,
    })
}

/// Extrae un fotograma pequeño del medio para la miniatura de la timeline.
fn generate_thumbnail(path: &Path, time: f64) -> Result<PreviewFrame, String> {
    extract_frame(path, time, THUMB_WIDTH, THUMB_HEIGHT)
}

/// Evento de gesto de timeline emitido por el bucle de pintado.
enum TimelineDragEvent {
    Move(usize, f64, usize),
    TrimStart(usize, f64),
    TrimEnd(usize, f64),
    Commit(usize),
    Select(usize, SelectionMode),
}

/// Conmutadores de cabecera de pista. Se recogen mientras se dibuja y se
/// aplican de golpe, para no clonar el proyecto en cada fotograma.
#[derive(Clone, Copy)]
enum TrackToggle {
    Mute(usize),
    Solo(usize),
    Hide(usize),
    LockAudio(usize),
    LockVideo(usize),
}

/// Cómo afecta un clic a la selección, según los modificadores pulsados.
#[derive(Clone, Copy, PartialEq)]
enum SelectionMode {
    /// Clic normal: solo ese clip.
    Replace,
    /// Ctrl+clic: añade o quita.
    Toggle,
    /// Mayús+clic: extiende desde el clip principal.
    Extend,
}

impl SelectionMode {
    fn from_modifiers(modifiers: egui::Modifiers) -> Self {
        if modifiers.shift {
            Self::Extend
        } else if modifiers.ctrl || modifiers.command {
            Self::Toggle
        } else {
            Self::Replace
        }
    }
}

/// Ajuste magnético: devuelve el valor más cercano entre los bordes de los
/// demás clips, el cabezal y los marcadores si está a menos de `tolerance`
/// segundos; si no, el valor original. El clip que se está moviendo queda
/// excluido para no anclarse a sí mismo.
fn snap_time(
    value: f64,
    clips: &[RoughClip],
    moving: Option<usize>,
    markers: &[Marker],
    playhead: f64,
    tolerance: f64,
) -> f64 {
    let mut candidates: Vec<f64> = clips
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != moving)
        .flat_map(|(_, clip)| [clip.timeline_start, clip.timeline_start + clip.duration()])
        .chain(markers.iter().map(|marker| marker.time))
        .chain(std::iter::once(playhead))
        .filter(|time| (*time - value).abs() <= tolerance)
        .collect();
    candidates.sort_by(f64::total_cmp);
    match candidates
        .into_iter()
        .min_by(|left, right| (left - value).abs().total_cmp(&(right - value).abs()))
    {
        Some(snapped) => snapped,
        None => value,
    }
}

fn zero_color() -> f64 {
    0.0
}

/// Escapa el texto para el valor entrecomillado de `drawtext`.
fn escape_drawtext(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(':', "\\:")
}

/// Ruta de fuente en sintaxis segura para filtergraph: barras y dos puntos
/// del drive escapados (ffmpeg acepta `/` también en Windows).
fn escape_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
}

fn hex_color(red: f64, green: f64, blue: f64) -> String {
    let channel = |value: f64| ((value.clamp(0.0, 1.0)) * 255.0).round() as u8;
    format!(
        "{:02X}{:02X}{:02X}",
        channel(red),
        channel(green),
        channel(blue)
    )
}

/// Primera fuente TTF del sistema disponible.
fn find_font() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.to_path_buf()))
    {
        for name in ["segoeui.ttf", "arial.ttf"] {
            candidates.push(directory.join("fonts").join(name));
        }
    }
    if let Some(windir) = std::env::var_os("WINDIR") {
        let fonts = PathBuf::from(windir).join("Fonts");
        for name in [
            "segoeui.ttf",
            "arial.ttf",
            "calibri.ttf",
            "times.ttf",
            "consola.ttf",
        ] {
            candidates.push(fonts.join(name));
        }
    }
    // Host de desarrollo en macOS/Linux.
    for path in [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        candidates.push(PathBuf::from(path));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Filtros de una capa de título: texto con estilo o, si el título lleva
/// subtítulos animados, sus palabras maquetadas. El mate de fondo, si lo
/// hay, va debajo en ambos casos. `preview_time` es el tiempo local para el
/// monitor (un solo fotograma); en exportación va `None`.
fn title_layer_filters(
    title: &Titulo,
    font: &Path,
    fontsize: f64,
    canvas: (u32, u32),
    preview_time: Option<f64>,
) -> String {
    let fontcolor = hex_color(title.red, title.green, title.blue);
    let escaped_font = escape_filter_path(font);
    let Some(captions) = title.style.captions.as_ref() else {
        return efectos::title_drawing(
            &title.style,
            &escape_drawtext(&title.text),
            &escaped_font,
            fontsize,
            &fontcolor,
            (title.position_x, title.position_y),
            canvas.1 as f64 / 1080.0,
        );
    };
    let words = subtitulos_animados::caption_filters(
        captions,
        font,
        &escaped_font,
        &escape_drawtext,
        fontsize,
        &fontcolor,
        canvas,
        title.position_y,
        preview_time,
    );
    match title.style.matte {
        Some(_) => {
            let matte_only = efectos::TitleStyle {
                matte: title.style.matte,
                ..Default::default()
            };
            let matte = efectos::title_drawing(&matte_only, "", "", 0.0, "", (0.0, 0.0), 1.0);
            format!("{matte},{words}")
        }
        None => words,
    }
}

/// Viñeta como filtro explícito; amount 0 devuelve cadena vacía.
fn vignette_filter(amount: f64) -> String {
    if amount.abs() < 0.001 {
        return String::new();
    }
    let angle = std::f64::consts::PI / 4.0 * amount.clamp(-1.0, 1.0);
    format!(",vignette=angle={angle:.5}")
}

/// Desenfoque gaussiano: fracción del lado corto → sigma del gblur.
fn blur_filter(blur: f64, short_side_px: f64) -> String {
    if blur < 0.001 {
        return String::new();
    }
    let sigma = (blur.clamp(0.0, 1.0) * short_side_px.max(1.0) / 4.0).clamp(0.1, 250.0);
    format!(",gblur=sigma={sigma:.2}")
}

/// Ruedas de color como `colorbalance`; vacío si las nueve están neutras.
fn wheels_filter(wheels: Option<&Wheels>) -> String {
    let Some(wheels) = wheels else {
        return String::new();
    };
    if wheels.is_neutral() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut push = |key: &str, value: f64| {
        if value.abs() >= 0.001 {
            parts.push(format!("{key}={:.4}", value.clamp(-1.0, 1.0)));
        }
    };
    push("rs", wheels.shadows_r);
    push("gs", wheels.shadows_g);
    push("bs", wheels.shadows_b);
    push("rm", wheels.mid_r);
    push("gm", wheels.mid_g);
    push("bm", wheels.mid_b);
    push("rh", wheels.high_r);
    push("gh", wheels.high_g);
    push("bh", wheels.high_b);
    if parts.is_empty() {
        String::new()
    } else {
        format!(",colorbalance={}", parts.join(":"))
    }
}

/// Chroma key como `chromakey` + `despill` para el derrame; vacío si no hay.
fn chroma_filter(chroma: Option<&Chroma>) -> String {
    let Some(chroma) = chroma else {
        return String::new();
    };
    let hex = hex_color(chroma.red, chroma.green, chroma.blue);
    let mut out = format!(
        ",chromakey=color=0x{hex}:similarity={:.4}:blend={:.4}",
        chroma.tolerance.clamp(0.0, 1.0),
        chroma.smooth.clamp(0.0, 1.0)
    );
    if chroma.spill >= 0.001 {
        let kind = if chroma.blue > chroma.green {
            "blue"
        } else {
            "green"
        };
        out.push_str(&format!(
            ",despill=type={kind}:mix={:.4}",
            chroma.spill.clamp(0.0, 1.0)
        ));
    }
    out
}

/// Curvas RGB/luma como filtro `curves` de FFmpeg; vacío si es identidad.
fn curves_filter(curves: Option<&Curves>) -> String {
    let Some(curves) = curves else {
        return String::new();
    };
    if curves.is_identity() {
        return String::new();
    }
    let channel_points = |points: &[CurvePoint]| -> Option<String> {
        if Curves::channel_is_identity(points) || points.is_empty() {
            return None;
        }
        let mut sorted: Vec<CurvePoint> = points.to_vec();
        sorted.sort_by(|left, right| left.x.total_cmp(&right.x));
        sorted.dedup_by(|left, right| (left.x - right.x).abs() < 1e-6);
        Some(
            sorted
                .iter()
                .map(|point| {
                    format!(
                        "{:.4}/{:.4}",
                        point.x.clamp(0.0, 1.0),
                        point.y.clamp(0.0, 1.0)
                    )
                })
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    let mut parts = Vec::new();
    if let Some(master) = channel_points(&curves.luma) {
        parts.push(format!("master='{master}'"));
    }
    if let Some(red) = channel_points(&curves.red) {
        parts.push(format!("r='{red}'"));
    }
    if let Some(green) = channel_points(&curves.green) {
        parts.push(format!("g='{green}'"));
    }
    if let Some(blue) = channel_points(&curves.blue) {
        parts.push(format!("b='{blue}'"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(",curves={}", parts.join(":"))
    }
}

fn lut_filter(path: Option<&Path>) -> String {
    path.filter(|path| path.is_file())
        .map(|path| format!(",lut3d=file='{}'", escape_filter_path(path)))
        .unwrap_or_default()
}

fn mask_filter(mask: Option<&Mask>, width: f64, height: f64) -> String {
    let Some(mask) = mask else {
        return String::new();
    };
    let alpha = mask.alpha_expression(width, height).replace(',', "\\,");
    ",geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='".to_owned() + &alpha + "'"
}

/// Ajusta a un segmento lo que depende de la posición dentro del clip
/// original: la banda de volumen (tiempo local), los fundidos de audio de
/// las transiciones y, en clips invertidos, qué tramo del medio se lee.
fn fit_segment_effects(
    segment: &mut RoughClip,
    clip: &RoughClip,
    local_start: f64,
    first: bool,
    last: bool,
) {
    for key in &mut segment.fx.volume_keys {
        key.t -= local_start;
    }
    if !first {
        segment.runtime.audio_fade_in = 0.0;
        segment.runtime.white_in = false;
    }
    if !last {
        segment.runtime.audio_fade_out = 0.0;
        segment.runtime.white_out = false;
    }
    if clip.fx.reverse {
        let from = segment.in_seconds - clip.in_seconds;
        let to = segment.out_seconds - clip.in_seconds;
        segment.in_seconds = clip.out_seconds - to;
        segment.out_seconds = clip.out_seconds - from;
    }
}

fn expand_speed_ramps(clips: Vec<RoughClip>) -> Vec<RoughClip> {
    let mut out = Vec::new();
    for clip in clips {
        if clip.nested.is_some()
            || clip
                .speed_ramp
                .as_ref()
                .is_none_or(|points| points.is_empty())
        {
            out.push(clip);
            continue;
        }
        let mut timeline_offset = 0.0;
        let boundaries = clip.speed_ramp_boundaries();
        for window in boundaries.windows(2) {
            let source_start = window[0];
            let source_end = window[1];
            if source_end - source_start < 0.004 {
                continue;
            }
            let speed = clip.speed_at_source_time((source_start + source_end) / 2.0);
            let timeline_duration = (source_end - source_start) / speed;
            let mut segment = clip.clone();
            let (x, y, scale, opacity) =
                clip.evaluate_transform(timeline_offset + timeline_duration / 2.0);
            segment.in_seconds = clip.in_seconds + source_start;
            segment.out_seconds = clip.in_seconds + source_end;
            segment.timeline_start = clip.timeline_start + timeline_offset;
            segment.speed = speed;
            segment.speed_ramp = None;
            segment.keyframes = None;
            segment.position_x = x;
            segment.position_y = y;
            segment.scale_percent = scale;
            segment.opacity = opacity;
            segment.fade_in_seconds = if timeline_offset < 0.001 {
                clip.fade_in_seconds.min(timeline_duration)
            } else {
                0.0
            };
            let first = timeline_offset < 0.001;
            let last = source_end >= clip.source_duration() - 0.001;
            fit_segment_effects(&mut segment, &clip, timeline_offset, first, last);
            timeline_offset += timeline_duration;
            segment.fade_out_seconds = if last {
                clip.fade_out_seconds.min(timeline_duration)
            } else {
                0.0
            };
            out.push(segment);
        }
    }
    out
}

/// Expande los clips con keyframes en segmentos constantes por tramo:
/// cada segmento lleva la transformación evaluada en su punto medio.
fn expand_keyframes(clips: Vec<RoughClip>) -> Vec<RoughClip> {
    let mut out = Vec::with_capacity(clips.len());
    for clip in clips {
        let Some(keyframes) = &clip.keyframes else {
            out.push(clip);
            continue;
        };
        if !clip.has_video || keyframes.len() < 2 {
            out.push(clip);
            continue;
        }
        let duration = clip.duration();
        let speed = clip.speed.clamp(0.1, 8.0);
        let (fade_in, fade_out) = clip.effective_fades();
        let mut boundaries: Vec<f64> = vec![0.0];
        for keyframe in keyframes {
            let t = keyframe.t.clamp(0.0, duration);
            if t > *boundaries.last().unwrap() + 0.01 && t < duration - 0.01 {
                boundaries.push(t);
            }
        }
        boundaries.push(duration);
        let segments = boundaries.windows(2);
        let segment_count = boundaries.len() - 1;
        for (segment_index, window) in segments.enumerate() {
            let (seg_start, seg_end) = (window[0], window[1]);
            let seg_dur = seg_end - seg_start;
            if seg_dur < 0.02 {
                continue;
            }
            let mid = (seg_start + seg_end) / 2.0;
            let (x, y, scale, opacity) = clip.evaluate_transform(mid);
            let mut segment = clip.clone();
            segment.keyframes = None;
            segment.position_x = x;
            segment.position_y = y;
            segment.scale_percent = scale.clamp(1.0, 800.0);
            segment.opacity = opacity.clamp(0.0, 100.0);
            segment.in_seconds = clip.in_seconds + seg_start * speed;
            segment.out_seconds = clip.in_seconds + seg_end * speed;
            segment.timeline_start = clip.timeline_start + seg_start;
            segment.fade_in_seconds = if segment_index == 0 {
                fade_in.min(seg_dur)
            } else {
                0.0
            };
            segment.fade_out_seconds = if segment_index + 1 == segment_count {
                fade_out.min(seg_dur)
            } else {
                0.0
            };
            fit_segment_effects(
                &mut segment,
                &clip,
                seg_start,
                segment_index == 0,
                segment_index + 1 == segment_count,
            );
            out.push(segment);
        }
    }
    out
}

/// Pipeline compartido de preparación para render: transiciones + keyframes.
fn prepare_render_clips(clips: &[RoughClip]) -> Vec<RoughClip> {
    expand_keyframes(expand_speed_ramps(resolve_render_clips(&flatten_nested(
        clips,
    ))))
}

fn flatten_nested(clips: &[RoughClip]) -> Vec<RoughClip> {
    let mut out = Vec::new();
    for clip in clips {
        let Some(children) = &clip.nested else {
            out.push(clip.clone());
            continue;
        };
        for child in flatten_nested(children) {
            let mut child = child;
            child.timeline_start += clip.timeline_start;
            child.track = clip.track.saturating_add(child.track).min(15);
            out.push(child);
        }
    }
    out
}

/// ¿Hay suficiente material de reserva para alargar el clip `seconds`?
fn clip_can_extend_by(clip: &RoughClip, seconds: f64) -> bool {
    match clip.source_duration_seconds {
        Some(source) => clip.out_seconds + seconds * clip.speed.clamp(0.1, 8.0) <= source + 0.001,
        None => false,
    }
}

/// Materializa las transiciones sobre copias de los clips.
///
/// `negro`: al clip anterior de la misma pista se le fija el fundido de
/// salida y al actual el de entrada, ambos iguales a la duración pedida
/// (paso por negro).
///
/// `dissolve`: fundido cruzado real. El clip anterior conserva el alfa
/// completo y se alarga `d` segundos usando la reserva del medio, de modo
/// que el clip actual entra fundiéndose ENCIMA de él (y su audio, encima
/// del audio saliente). Sin reserva en el medio, cae a paso por negro.
///
/// `blanco`: como `negro`, pero los fundidos van a blanco.
///
/// Deslizar/empujar: el saliente se alarga igual que en la disolvencia y el
/// entrante llega en movimiento por encima; al empujar, el saliente también
/// se desplaza. El audio se cruza con fundidos propios. Sin reserva, el
/// entrante desliza sobre lo que haya debajo.
fn resolve_render_clips(clips: &[RoughClip]) -> Vec<RoughClip> {
    let mut out = clips.to_vec();
    for clip in &mut out {
        clip.runtime = efectos::TransitionRuntime::default();
    }
    for i in 0..out.len() {
        let transition = out[i].transition.clone();
        let Some(transition) =
            transition.filter(|name| efectos::TRANSITIONS.iter().any(|(id, _)| id == name))
        else {
            continue;
        };
        if !out[i].has_video {
            continue;
        }
        let start_i = out[i].timeline_start;
        let mut best: Option<usize> = None;
        for j in 0..i {
            if !out[j].has_video || out[j].track != out[i].track {
                continue;
            }
            let end_j = out[j].timeline_start + out[j].duration();
            if (end_j - start_i).abs() < 0.06 {
                match best {
                    Some(b) => {
                        if out[j].timeline_start > out[b].timeline_start {
                            best = Some(j);
                        }
                    }
                    None => best = Some(j),
                }
            }
        }
        if let Some(j) = best {
            let d = out[i]
                .transition_duration
                .max(0.04)
                .min(out[j].duration() / 2.0)
                .min(out[i].duration() / 2.0);
            let extendable = clip_can_extend_by(&out[j], d);
            if let Some((dx, dy, push)) = efectos::motion_of(&transition) {
                out[i].runtime.slide_in = Some((dx, dy, start_i, d));
                out[i].runtime.audio_fade_in = d;
                if extendable {
                    let speed = out[j].speed.clamp(0.1, 8.0);
                    out[j].out_seconds += d * speed;
                    out[j].runtime.audio_fade_out = d;
                    if push {
                        out[j].runtime.slide_out = Some((dx, dy, start_i, d));
                    }
                }
            } else if transition == "dissolve" && extendable {
                let speed = out[j].speed.clamp(0.1, 8.0);
                out[j].out_seconds += d * speed;
                out[j].fade_out_seconds = 0.0;
                out[i].fade_in_seconds = d;
            } else {
                out[j].fade_out_seconds = d;
                out[i].fade_in_seconds = d;
                if transition == "blanco" {
                    out[j].runtime.white_out = true;
                    out[i].runtime.white_in = true;
                }
            }
        }
    }
    out
}

/// Cadena `eq` si algún ajuste está activo; vacío cuando todo es neutro.
/// Deja libre el tramo `[start, end)` de una pista recortando, partiendo o
/// eliminando lo que hubiera allí, como hace la superposición de cualquier
/// montador. Devuelve cuántos clips se vieron afectados.
///
/// Los clips con rampa de velocidad o secuencias anidadas no se parten por
/// dentro: su tiempo de origen no es lineal, así que se dejan intactos.
fn clear_track_span(
    clips: &mut Vec<RoughClip>,
    track: usize,
    is_video: bool,
    start: f64,
    end: f64,
    protect: &[usize],
) -> usize {
    if end - start <= 0.001 {
        return 0;
    }
    let mut result: Vec<RoughClip> = Vec::with_capacity(clips.len() + 2);
    let mut touched = 0;
    for (index, clip) in clips.iter().enumerate() {
        let same_lane = clip.track == track && clip.has_video == is_video;
        let clip_start = clip.timeline_start;
        let clip_end = clip.timeline_start + clip.duration();
        let overlaps = clip_start < end - 0.001 && clip_end > start + 0.001;
        if !same_lane || !overlaps || protect.contains(&index) {
            result.push(clip.clone());
            continue;
        }
        let complex = clip.nested.is_some()
            || clip
                .speed_ramp
                .as_ref()
                .is_some_and(|points| !points.is_empty());
        if complex {
            result.push(clip.clone());
            continue;
        }
        touched += 1;
        let speed = clip.speed.clamp(0.1, 8.0);
        // Trozo que sobrevive por delante del hueco.
        if clip_start < start - 0.001 {
            let mut head = clip.clone();
            head.out_seconds = clip.in_seconds + (start - clip_start) * speed;
            head.fade_out_seconds = 0.0;
            if head.out_seconds - head.in_seconds > 0.001 {
                result.push(head);
            }
        }
        // Trozo que sobrevive por detrás.
        if clip_end > end + 0.001 {
            let mut tail = clip.clone();
            tail.timeline_start = end;
            tail.in_seconds = clip.in_seconds + (end - clip_start) * speed;
            tail.fade_in_seconds = 0.0;
            tail.transition = None;
            if tail.out_seconds - tail.in_seconds > 0.001 {
                result.push(tail);
            }
        }
    }
    *clips = result;
    touched
}

fn span_hits_complex_clip(
    clips: &[RoughClip],
    track: usize,
    is_video: bool,
    start: f64,
    end: f64,
    exclude: &[usize],
) -> bool {
    clips.iter().enumerate().any(|(index, clip)| {
        !exclude.contains(&index)
            && clip.track == track
            && clip.has_video == is_video
            && clip.timeline_start < end - 0.001
            && clip.timeline_start + clip.duration() > start + 0.001
            && (clip.nested.is_some()
                || clip
                    .speed_ramp
                    .as_ref()
                    .is_some_and(|points| !points.is_empty()))
    })
}

/// Abre un hueco en una pista partiendo el clip lineal que cruce el punto y
/// desplazando todo lo que quede a la derecha. La operación es atómica: si el
/// corte cae dentro de una rampa o secuencia anidada, no modifica nada.
fn ripple_track_at(
    clips: &mut Vec<RoughClip>,
    track: usize,
    is_video: bool,
    at: f64,
    span: f64,
) -> bool {
    if span <= 0.001 {
        return true;
    }
    let crosses = |clip: &RoughClip| {
        clip.track == track
            && clip.has_video == is_video
            && clip.timeline_start < at - 0.001
            && clip.timeline_start + clip.duration() > at + 0.001
    };
    if clips.iter().any(|clip| {
        crosses(clip)
            && (clip.nested.is_some()
                || clip
                    .speed_ramp
                    .as_ref()
                    .is_some_and(|points| !points.is_empty()))
    }) {
        return false;
    }

    let mut result = Vec::with_capacity(clips.len() + 1);
    for clip in clips.iter() {
        let same_lane = clip.track == track && clip.has_video == is_video;
        if !same_lane {
            result.push(clip.clone());
            continue;
        }
        if crosses(clip) {
            let speed = clip.speed.clamp(0.1, 8.0);
            let source_cut = clip.in_seconds + (at - clip.timeline_start) * speed;
            let mut head = clip.clone();
            head.out_seconds = source_cut;
            head.fade_out_seconds = 0.0;
            let mut tail = clip.clone();
            tail.timeline_start = at + span;
            tail.in_seconds = source_cut;
            tail.fade_in_seconds = 0.0;
            tail.transition = None;
            result.push(head);
            result.push(tail);
        } else {
            let mut shifted = clip.clone();
            if shifted.timeline_start >= at - 0.001 {
                shifted.timeline_start += span;
            }
            result.push(shifted);
        }
    }
    *clips = result;
    true
}

/// Recorta el montaje al rango [start, end] y lo desplaza al origen. Los clips
/// con rampa o secuencia anidada no se recortan por dentro: se conservan
/// enteros si tocan el rango, porque su tiempo de origen no es lineal.
fn trim_clips_to_range(clips: &[RoughClip], start: f64, end: f64) -> Vec<RoughClip> {
    let mut result = Vec::new();
    for clip in clips {
        let clip_start = clip.timeline_start;
        let clip_end = clip.timeline_start + clip.duration();
        if clip_end <= start + 0.001 || clip_start >= end - 0.001 {
            continue;
        }
        let mut trimmed = clip.clone();
        let complex = clip.nested.is_some()
            || clip
                .speed_ramp
                .as_ref()
                .is_some_and(|points| !points.is_empty());
        if complex {
            trimmed.timeline_start = clip_start - start;
            result.push(trimmed);
            continue;
        }
        let speed = clip.speed.clamp(0.1, 8.0);
        let head = (start - clip_start).max(0.0);
        let tail = (clip_end - end).max(0.0);
        trimmed.timeline_start = (clip_start + head) - start;
        trimmed.in_seconds = clip.in_seconds + head * speed;
        trimmed.out_seconds = clip.out_seconds - tail * speed;
        if trimmed.out_seconds - trimmed.in_seconds <= 0.001 {
            continue;
        }
        let half = trimmed.duration() / 2.0;
        trimmed.fade_in_seconds = trimmed.fade_in_seconds.min(half).max(0.0);
        trimmed.fade_out_seconds = trimmed.fade_out_seconds.min(half).max(0.0);
        result.push(trimmed);
    }
    result
}

fn color_eq_filter(exposure: f64, contrast: f64, saturation: f64) -> String {
    if exposure.abs() < 0.001 && contrast.abs() < 0.001 && saturation.abs() < 0.001 {
        return String::new();
    }
    let brightness = exposure.clamp(-1.0, 1.0) / 2.0;
    let contrast = 1.0 + contrast.clamp(-1.0, 1.0);
    let saturation = (1.0 + saturation.clamp(-1.0, 1.0)).max(0.0);
    format!(",eq=brightness={brightness:.4}:contrast={contrast:.4}:saturation={saturation:.4}")
}

/// Texto con posicion en fracciones de lienzo (0…1) y tamano relativo a 1080p,
/// igual que el `TituloDeClip` de macOS.
#[derive(Clone, Serialize, Deserialize)]
struct Titulo {
    text: String,
    #[serde(default = "half_center")]
    position_x: f64,
    #[serde(default = "half_center")]
    position_y: f64,
    #[serde(default = "title_size")]
    size: f64,
    #[serde(default = "full_channel")]
    red: f64,
    #[serde(default = "full_channel")]
    green: f64,
    #[serde(default = "full_channel")]
    blue: f64,
    /// Caja, contorno, sombra y mate de «Gráficos esenciales».
    #[serde(default)]
    style: efectos::TitleStyle,
}

fn half_center() -> f64 {
    0.5
}

fn title_size() -> f64 {
    96.0
}

fn full_channel() -> f64 {
    1.0
}

impl Default for Titulo {
    fn default() -> Self {
        Self {
            text: String::new(),
            position_x: half_center(),
            position_y: half_center(),
            size: title_size(),
            red: full_channel(),
            green: full_channel(),
            blue: full_channel(),
            style: efectos::TitleStyle::default(),
        }
    }
}

impl Default for RoughClip {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            in_seconds: 0.0,
            out_seconds: 1.0,
            source_duration_seconds: None,
            source_timebase: None,
            source_vfr: false,
            source_pts: None,
            has_video: true,
            has_audio: true,
            speed: 1.0,
            timeline_start: 0.0,
            track: 0,
            gain_db: 0.0,
            muted: false,
            pan: 0.0,
            position_x: 0.0,
            position_y: 0.0,
            scale_percent: 100.0,
            rotation: 0.0,
            opacity: 100.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            title: None,
            is_adjustment: false,
            exposure: zero_color(),
            contrast: zero_color(),
            saturation: zero_color(),
            vignette: zero_color(),
            transition: None,
            transition_duration: default_transition_duration(),
            label: 0,
            blur: zero_color(),
            wheels: None,
            chroma: None,
            curves: None,
            keyframes: None,
            fusion: Fusion::Normal,
            mask: None,
            lut: None,
            proxy: None,
            speed_ramp: None,
            fx: efectos::ClipFx::default(),
            runtime: efectos::TransitionRuntime::default(),
            nested: None,
            freeze_at: None,
            enabled: true,
        }
    }
}

impl RoughClip {
    fn name(&self) -> String {
        if self.nested.is_some() {
            return "Secuencia anidada".to_owned();
        }
        if self.is_adjustment {
            return "Capa de ajuste".to_owned();
        }
        if let Some(title) = &self.title {
            return if title.text.trim().is_empty() {
                "Titulo".to_owned()
            } else {
                title.text.trim().chars().take(28).collect()
            };
        }
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Medio")
            .to_owned()
    }

    fn duration(&self) -> f64 {
        if let Some(children) = &self.nested {
            return children
                .iter()
                .map(|child| child.timeline_start + child.duration())
                .fold(0.0, f64::max);
        }
        if self
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
        {
            self.ramped_duration()
        } else {
            self.source_duration() / self.speed.clamp(0.1, 8.0)
        }
    }

    fn source_duration(&self) -> f64 {
        (self.out_seconds - self.in_seconds).max(0.0)
    }

    /// Fundidos limitados a la mitad del clip cada uno para que nunca se crucen.
    fn effective_fades(&self) -> (f64, f64) {
        let half = self.duration() / 2.0;
        (
            self.fade_in_seconds.max(0.0).min(half),
            self.fade_out_seconds.max(0.0).min(half),
        )
    }
}

/// Marcador con nombre anclado a un instante de la timeline.
#[derive(Clone, Serialize, Deserialize)]
struct Marker {
    time: f64,
    name: String,
}

#[derive(Clone)]
struct LoudnessReport {
    integrated_lufs: f64,
    true_peak_db: f64,
    range_lu: f64,
    /// Campos "measured_*" que loudnorm necesita en un segundo paso para
    /// normalizar con precisión en vez de procesar dinámicamente a ciegas.
    threshold_db: f64,
    target_offset_db: f64,
}

impl LoudnessReport {
    /// Argumentos del segundo paso de loudnorm con los valores ya medidos.
    fn two_pass_args(&self) -> String {
        format!(
            "measured_I={:.2}:measured_TP={:.2}:measured_LRA={:.2}:measured_thresh={:.2}:offset={:.2}:linear=true",
            self.integrated_lufs,
            self.true_peak_db,
            self.range_lu,
            self.threshold_db,
            self.target_offset_db
        )
    }
}

fn parse_loudness(stderr: &str) -> Result<LoudnessReport, String> {
    #[derive(Deserialize)]
    struct RawLoudness {
        input_i: String,
        input_tp: String,
        input_lra: String,
        input_thresh: String,
        target_offset: String,
    }
    let start = stderr
        .rfind('{')
        .ok_or_else(|| "FFmpeg no devolvió medición LUFS".to_owned())?;
    let end = stderr[start..]
        .find('}')
        .map(|offset| start + offset + 1)
        .ok_or_else(|| "Medición LUFS incompleta".to_owned())?;
    let raw: RawLoudness = serde_json::from_str(&stderr[start..end])
        .map_err(|error| format!("Medición LUFS no válida: {error}"))?;
    let number = |value: &str| {
        value
            .parse::<f64>()
            .map_err(|_| format!("Valor LUFS no válido: {value}"))
    };
    Ok(LoudnessReport {
        integrated_lufs: number(&raw.input_i)?,
        true_peak_db: number(&raw.input_tp)?,
        range_lu: number(&raw.input_lra)?,
        threshold_db: number(&raw.input_thresh)?,
        target_offset_db: number(&raw.target_offset)?,
    })
}

fn parse_silences(stderr: &str) -> Vec<(f64, f64)> {
    let mut ranges = Vec::new();
    let mut start = None;
    for line in stderr.lines() {
        if let Some(value) = line.split("silence_start:").nth(1) {
            start = value.split_whitespace().next().and_then(|v| v.parse().ok());
        }
        if let (Some(from), Some(value)) = (start, line.split("silence_end:").nth(1)) {
            if let Some(to) = value.split_whitespace().next().and_then(|v| v.parse().ok()) {
                if to > from {
                    ranges.push((from, to));
                }
                start = None;
            }
        }
    }
    ranges
}

fn without_silences(clip: &RoughClip, silences: &[(f64, f64)]) -> Vec<RoughClip> {
    let source_duration = clip.source_duration();
    let mut audible = Vec::new();
    let mut cursor = 0.0;
    for &(start, end) in silences {
        let start = start.clamp(cursor, source_duration);
        let end = end.clamp(start, source_duration);
        if start - cursor >= 0.04 {
            audible.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if source_duration - cursor >= 0.04 {
        audible.push((cursor, source_duration));
    }
    let mut timeline = clip.timeline_start;
    audible
        .into_iter()
        .map(|(start, end)| {
            let mut segment = clip.clone();
            segment.in_seconds = clip.in_seconds + start;
            segment.out_seconds = clip.in_seconds + end;
            segment.timeline_start = timeline;
            segment.fade_in_seconds = 0.0;
            segment.fade_out_seconds = 0.0;
            segment.keyframes = None;
            timeline += segment.duration();
            segment
        })
        .collect()
}

/// Instantes de origen (segundos desde el inicio del clip analizado) donde
/// el filtro `scdet` detectó un cambio de escena.
fn parse_scene_cuts(stderr: &str) -> Vec<f64> {
    let mut cuts = Vec::new();
    for line in stderr.lines() {
        if let Some(value) = line.split("lavfi.scd.time:").nth(1) {
            if let Some(time) = value
                .trim()
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .and_then(|v| v.parse::<f64>().ok())
            {
                cuts.push(time);
            }
        }
    }
    cuts
}

/// Parte un clip en los instantes de origen indicados sin quitar nada de
/// metraje (a diferencia de `without_silences`, la duración total no
/// cambia, así que no hace falta desplazar los clips posteriores).
fn split_by_scene_cuts(clip: &RoughClip, cut_source_times: &[f64]) -> Vec<RoughClip> {
    let source_duration = clip.source_duration();
    let mut boundaries: Vec<f64> = std::iter::once(0.0)
        .chain(
            cut_source_times
                .iter()
                .copied()
                .filter(|time| *time > 0.0 && *time < source_duration),
        )
        .chain(std::iter::once(source_duration))
        .collect();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup_by(|a, b| (*a - *b).abs() < 0.08);
    let mut timeline = clip.timeline_start;
    boundaries
        .windows(2)
        .filter(|window| window[1] - window[0] >= 0.04)
        .map(|window| {
            let mut segment = clip.clone();
            segment.in_seconds = clip.in_seconds + window[0];
            segment.out_seconds = clip.in_seconds + window[1];
            segment.timeline_start = timeline;
            segment.fade_in_seconds = if window[0] <= 0.001 {
                clip.fade_in_seconds
            } else {
                0.0
            };
            segment.fade_out_seconds = if window[1] >= source_duration - 0.001 {
                clip.fade_out_seconds
            } else {
                0.0
            };
            segment.keyframes = None;
            timeline += segment.duration();
            segment
        })
        .collect()
}

/// Mezcla el montaje a WAV 16 kHz y lo transcribe palabra a palabra con
/// Whisper. Los tiempos quedan en segundos de timeline.
fn transcribe_mix(
    prepared: &[RoughClip],
    track_gains: &[f64],
    master_gain_db: f64,
    timebase: Timebase,
    whisper: &Path,
    model: &Path,
) -> Result<Vec<transcripcion::Word>, String> {
    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUN.fetch_add(1, Ordering::Relaxed);
    let prefix = std::env::temp_dir().join(format!("novacut-whisper-{}-{run}", std::process::id()));
    let wav = prefix.with_extension("wav");
    let json = prefix.with_extension("json");
    let _ = std::fs::remove_file(&wav);
    let _ = std::fs::remove_file(&json);
    let mut ffmpeg = Command::new(tool_path("ffmpeg.exe"));
    ffmpeg.args(["-y", "-v", "error"]);
    let (indices, titles) = push_render_inputs(&mut ffmpeg, prepared, (640, 360), false, timebase);
    let result = build_render_filters(
        prepared,
        &indices,
        &titles,
        (640, 360),
        false,
        true,
        track_gains,
        master_gain_db,
        false,
        timebase,
        None,
    )
    .and_then(|filters| {
        let output = ffmpeg
            .args(["-filter_complex", &filters.join(";")])
            .args([
                "-map",
                "[aout]",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&wav)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("No se pudo preparar audio: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let output = Command::new(whisper)
            .arg("-m")
            .arg(model)
            .arg("-f")
            .arg(&wav)
            // Un segmento por palabra: la base de la edición por
            // texto y de los subtítulos animados.
            .args(["-ml", "1", "-sow", "-oj", "-of"])
            .arg(&prefix)
            .args(["-l", "auto"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("No se pudo iniciar Whisper: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let content = std::fs::read_to_string(&json)
            .map_err(|error| format!("Whisper no generó JSON: {error}"))?;
        transcripcion::parse_whisper_words(&content)
    });
    let _ = std::fs::remove_file(wav);
    let _ = std::fs::remove_file(json);
    result
}

fn whisper_files() -> Option<(PathBuf, PathBuf)> {
    let mut roots = Vec::new();
    if let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        roots.push(root.join("whisper"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("NovaCut").join("Whisper"));
    }
    // Carpeta explícita (útil en desarrollo y en instalaciones portables).
    if let Some(root) = std::env::var_os("NOVACUT_WHISPER_DIR") {
        roots.insert(0, PathBuf::from(root));
    }
    for root in roots {
        let executable = ["whisper-cli.exe", "main.exe", "whisper-cli"]
            .into_iter()
            .map(|name| root.join(name))
            .find(|path| path.is_file());
        let Some(executable) = executable else {
            continue;
        };
        if let Some(model) = find_whisper_model(&root) {
            return Some((executable, model));
        }
    }
    None
}

/// Busca cualquier modelo `ggml-*.bin` en la carpeta, sin exigir un nombre
/// exacto (Whisper distribuye tiny/base/small/medium/large, con o sin
/// sufijos de idioma o cuantización). Prefiere modelos más pequeños porque
/// son más rápidos; el usuario puede sustituir el archivo si quiere más
/// precisión.
fn find_whisper_model(root: &Path) -> Option<PathBuf> {
    const PREFERENCE: [&str; 5] = ["tiny", "base", "small", "medium", "large"];
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ggml-") && name.ends_with(".bin"))
        })
        .collect();
    candidates.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        PREFERENCE
            .iter()
            .position(|size| name.contains(size))
            .unwrap_or(PREFERENCE.len())
    });
    candidates.into_iter().next()
}

/// Convierte los subtítulos del proyecto en clips de título que cubren la
/// pista superior, así el monitor y la exportación los dibujan solos.
fn clips_with_subtitles(
    clips: &[RoughClip],
    subtitles: &[Subtitle],
    style: Option<&SubtitleStyle>,
) -> Vec<RoughClip> {
    let mut out = clips.to_vec();
    if subtitles.is_empty() {
        return out;
    }
    let top_track = clips
        .iter()
        .filter(|clip| clip.has_video)
        .map(|clip| clip.track)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .min(15);
    for subtitle in subtitles {
        let style = style.cloned().unwrap_or_default();
        let duration = (subtitle.end - subtitle.start).max(0.2);
        out.push(RoughClip {
            out_seconds: duration,
            timeline_start: subtitle.start,
            track: top_track,
            has_audio: false,
            title: Some(Titulo {
                text: subtitle.text.clone(),
                position_x: 0.5,
                position_y: style.position_y.clamp(0.0, 1.0),
                size: style.size.clamp(8.0, 400.0),
                red: style.red.clamp(0.0, 1.0),
                green: style.green.clamp(0.0, 1.0),
                blue: style.blue.clamp(0.0, 1.0),
                style: efectos::TitleStyle::default(),
            }),
            ..Default::default()
        });
    }
    out
}

/// Traduce clips del proyecto a cortes de EDL. Devuelve también lo que se
/// queda fuera, que en un EDL es bastante: títulos, capas de ajuste, anidados
/// y rampas de velocidad. Se cuenta, no se silencia.
fn project_to_edl_clips(
    clips: &[RoughClip],
    timebase: Timebase,
) -> (Vec<edl::EdlClip>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for clip in clips {
        if clip.title.is_some() || clip.is_adjustment || clip.nested.is_some() {
            skipped.push(clip.name());
            continue;
        }
        if clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
        {
            skipped.push(format!("{} (rampa de velocidad)", clip.name()));
        }
        let duration = timebase.frames(clip.duration());
        if duration <= 0 {
            continue;
        }
        let channel = match (clip.has_video, clip.has_audio) {
            (true, true) => edl::Channel::Both,
            (true, false) => edl::Channel::Video,
            (false, true) => edl::Channel::Audio(clip.track + 1),
            (false, false) => continue,
        };
        out.push(edl::EdlClip {
            source: clip.path.to_string_lossy().into_owned(),
            name: clip.name(),
            channel,
            source_in: timebase.frames(clip.in_seconds),
            record_in: timebase.frames(clip.timeline_start),
            duration,
            speed: clip.speed,
        });
    }
    (out, skipped)
}

/// Convierte los cortes leídos de un EDL en clips del proyecto, colocados en
/// pistas donde no se pisen. `existing` son los clips que ya hay en el montaje.
fn edl_clips_to_project(
    sources: &[edl::EdlClip],
    timebase: Timebase,
    existing: &[RoughClip],
) -> Vec<RoughClip> {
    let mut placed: Vec<RoughClip> = existing.to_vec();
    let mut added = Vec::new();
    for source in sources {
        // Un evento `B` deja dos clips, el vídeo y su audio, como el plano que
        // eran. Los demás canales dejan uno solo.
        let parts: Vec<(bool, bool, usize)> = match source.channel {
            edl::Channel::Video => vec![(true, false, 0)],
            edl::Channel::Audio(index) => vec![(false, true, index.saturating_sub(1).min(15))],
            edl::Channel::Both => vec![(true, false, 0), (false, true, 0)],
        };
        let start = timebase.seconds(source.record_in);
        let duration = timebase.seconds(source.duration);
        for (has_video, has_audio, preferred) in parts {
            let speed = if source.speed.is_finite() && source.speed > 0.0 {
                source.speed.clamp(0.1, 8.0)
            } else {
                1.0
            };
            let source_in = timebase.seconds(source.source_in);
            let clip = RoughClip {
                path: PathBuf::from(&source.source),
                in_seconds: source_in,
                // El tramo consumido del medio se estira con la velocidad: dos
                // segundos de montaje al doble gastan cuatro del original.
                out_seconds: source_in + duration * speed,
                has_video,
                has_audio,
                speed,
                timeline_start: start,
                // Dos clips pisándose en la misma pista no describen ningún
                // montaje: se busca la primera pista libre en ese tramo.
                track: free_track(&placed, has_video, preferred, start, start + duration),
                ..Default::default()
            };
            placed.push(clip.clone());
            added.push(clip);
        }
    }
    added
}

/// Primera pista del mismo tipo, desde la preferida hacia arriba, donde el
/// tramo pedido no pisa a nadie. El tope de 16 es el del modelo.
fn free_track(clips: &[RoughClip], video: bool, preferred: usize, start: f64, end: f64) -> usize {
    (preferred..16)
        .find(|track| {
            !clips.iter().any(|clip| {
                clip.track == *track
                    && clip.has_video == video
                    && clip.timeline_start < end
                    && start < clip.timeline_start + clip.duration()
            })
        })
        .unwrap_or(15)
}

#[derive(Clone, Serialize, Deserialize)]
struct RoughProject {
    version: u32,
    name: String,
    clips: Vec<RoughClip>,
    #[serde(default)]
    markers: Vec<Marker>,
    #[serde(default)]
    subtitles: Vec<Subtitle>,
    /// Estilo de los subtítulos quemados.
    #[serde(default)]
    subtitle_style: Option<SubtitleStyle>,
    /// Ganancia por pista de audio en dB (índice = pista A).
    #[serde(default)]
    track_gains: Vec<f64>,
    /// Ganancia de mezcla master en dB.
    #[serde(default)]
    master_gain_db: f64,
    /// Normaliza el bus final a -14 LUFS mediante FFmpeg.
    #[serde(default)]
    normalize_loudness: bool,
    /// Silencio por pista de audio (índice = pista A).
    #[serde(default)]
    track_mutes: Vec<bool>,
    /// Solo por pista de audio; si hay alguno activo, el resto enmudece.
    #[serde(default)]
    track_solos: Vec<bool>,
    /// Pistas de vídeo ocultas: no se componen ni se exportan.
    #[serde(default)]
    video_hidden: Vec<bool>,
    /// Pistas de vídeo bloqueadas: no admiten arrastre ni recorte.
    #[serde(default)]
    video_locked: Vec<bool>,
    /// Pistas de audio bloqueadas.
    #[serde(default)]
    audio_locked: Vec<bool>,
    /// Fotogramas por segundo del montaje; base del timecode y del paso a paso.
    #[serde(default = "default_fps")]
    fps: f64,
    /// Fuente canonica de la cadencia. `fps` se conserva para proyectos viejos
    /// y para consumidores que aun no conocen este campo.
    #[serde(default)]
    timebase: Option<Timebase>,
    /// Número mínimo de pistas de vídeo, para poder crear pistas vacías y
    /// soltar material en ellas antes de que contengan nada.
    #[serde(default)]
    min_video_tracks: usize,
    /// Número mínimo de pistas de audio.
    #[serde(default)]
    min_audio_tracks: usize,
    /// Transcripción palabra a palabra en tiempo de timeline (Whisper). Es
    /// la superficie de la edición por texto.
    #[serde(default)]
    transcript: Vec<transcripcion::Word>,
}

impl RoughProject {
    fn normalize(&mut self) {
        if self.version < 2 {
            let mut cursor = 0.0;
            for clip in &mut self.clips {
                clip.timeline_start = cursor;
                clip.track = 0;
                cursor += clip.duration();
            }
            self.version = 2;
        }
        let timebase = self
            .timebase
            .and_then(|rate| Timebase::new(rate.numerator, rate.denominator, rate.drop_frame).ok())
            .unwrap_or_else(|| Timebase::from_fps(self.fps));
        self.timebase = Some(timebase);
        self.fps = timebase.fps();
        let inferred_source_durations: HashMap<PathBuf, f64> = self
            .clips
            .iter()
            .filter(|clip| !clip.path.as_os_str().is_empty())
            .fold(HashMap::new(), |mut durations, clip| {
                durations
                    .entry(clip.path.clone())
                    .and_modify(|duration| {
                        *duration =
                            duration.max(clip.source_duration_seconds.unwrap_or(clip.out_seconds))
                    })
                    .or_insert_with(|| clip.source_duration_seconds.unwrap_or(clip.out_seconds));
                durations
            });
        for clip in &mut self.clips {
            if clip.source_duration_seconds.is_none() && !clip.path.as_os_str().is_empty() {
                clip.source_duration_seconds = inferred_source_durations.get(&clip.path).copied();
            }
            if let Some(source_duration) = clip.source_duration_seconds.filter(|value| *value > 0.0)
            {
                clip.out_seconds = clip.out_seconds.clamp(0.001, source_duration);
                clip.in_seconds = clip.in_seconds.clamp(0.0, clip.out_seconds - 0.001);
            }
            clip.timeline_start = clip.timeline_start.max(0.0);
            clip.speed = clip.speed.clamp(0.1, 8.0);
            clip.scale_percent = clip.scale_percent.clamp(1.0, 800.0);
            clip.opacity = clip.opacity.clamp(0.0, 100.0);
            let half = clip.duration() / 2.0;
            clip.fade_in_seconds = clip.fade_in_seconds.max(0.0).min(half);
            clip.fade_out_seconds = clip.fade_out_seconds.max(0.0).min(half);
        }
    }

    fn duration(&self) -> f64 {
        self.clips
            .iter()
            .map(|clip| clip.timeline_start + clip.duration())
            .fold(0.0, f64::max)
    }

    fn timebase(&self) -> Timebase {
        self.timebase
            .unwrap_or_else(|| Timebase::from_fps(self.fps))
    }

    fn set_timebase(&mut self, timebase: Timebase) {
        self.timebase = Some(timebase);
        self.fps = timebase.fps();
    }

    fn video_track_count(&self) -> usize {
        let used = self
            .clips
            .iter()
            .filter(|clip| clip.has_video)
            .map(|clip| clip.track + 1)
            .max()
            .unwrap_or(1);
        used.max(self.min_video_tracks).clamp(1, 16)
    }

    fn audio_track_count(&self) -> usize {
        let used = self
            .clips
            .iter()
            .filter(|clip| clip.has_audio)
            .map(|clip| clip.track + 1)
            .max()
            .unwrap_or(1);
        used.max(self.min_audio_tracks).clamp(1, 16)
    }

    /// Ajusta la longitud de los vectores por pista al montaje actual, para que
    /// la interfaz pueda indexarlos sin comprobar límites en cada dibujo.
    fn sync_track_state(&mut self) {
        let video = self.video_track_count();
        let audio = self.audio_track_count();
        self.track_mutes.resize(audio, false);
        self.track_solos.resize(audio, false);
        self.track_gains.resize(audio, 0.0);
        self.audio_locked.resize(audio, false);
        self.video_hidden.resize(video, false);
        self.video_locked.resize(video, false);
        if self.fps < 1.0 {
            self.fps = default_fps();
        }
    }

    fn any_solo(&self) -> bool {
        self.track_solos.iter().any(|solo| *solo)
    }

    /// Un clip suena si su pista no está enmudecida y, cuando hay solos activos,
    /// solo si su pista es una de las que están en solo.
    fn track_audible(&self, track: usize) -> bool {
        if self.any_solo() {
            return self.track_solos.get(track).copied().unwrap_or(false);
        }
        !self.track_mutes.get(track).copied().unwrap_or(false)
    }

    fn track_visible(&self, track: usize) -> bool {
        !self.video_hidden.get(track).copied().unwrap_or(false)
    }

    fn clip_locked(&self, clip: &RoughClip) -> bool {
        self.lane_locked(clip.track, clip.has_video)
    }

    fn lane_locked(&self, track: usize, is_video: bool) -> bool {
        if is_video {
            self.video_locked.get(track).copied().unwrap_or(false)
        } else {
            self.audio_locked.get(track).copied().unwrap_or(false)
        }
    }

    /// Instantes de corte del montaje (inicios y finales de clip), ordenados y
    /// sin duplicados, para saltar de corte en corte con las flechas.
    fn cut_points(&self) -> Vec<f64> {
        let mut points: Vec<f64> = vec![0.0];
        for clip in &self.clips {
            points.push(clip.timeline_start);
            points.push(clip.timeline_start + clip.duration());
        }
        points.sort_by(f64::total_cmp);
        points.dedup_by(|left, right| (*left - *right).abs() < 0.001);
        points
    }
}

struct PreviewFrame {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}

#[derive(Deserialize)]
struct MacProject {
    nombre: Option<String>,
    medios: Vec<MacMedia>,
    montaje: MacTimeline,
}

#[derive(Deserialize)]
struct MacMedia {
    id: String,
    ruta: String,
    #[serde(default, rename = "rutaRelativa")]
    ruta_relativa: Option<String>,
    nombre: String,
}

#[derive(Deserialize)]
struct MacTimeline {
    timebase: MacTimebase,
    pistas: Vec<MacTrack>,
}

#[derive(Deserialize)]
struct MacTimebase {
    numerador: i32,
    denominador: i32,
    #[serde(default, rename = "dropFrame")]
    drop_frame: bool,
}

#[derive(Deserialize)]
struct MacTrack {
    tipo: String,
    clips: Vec<MacClip>,
}

#[derive(Deserialize)]
struct MacClip {
    #[serde(rename = "mediaID")]
    media_id: String,
    inicio: i64,
    duracion: i64,
    #[serde(rename = "entradaEnOrigen")]
    source_in: i64,
    #[serde(default = "normal_speed")]
    velocidad: f64,
    #[serde(default)]
    ganancia: f64,
    #[serde(default)]
    transformacion: MacTransform,
    #[serde(default, rename = "entradaFundido")]
    fade_in_frames: i64,
    #[serde(default, rename = "salidaFundido")]
    fade_out_frames: i64,
    #[serde(default = "enabled_by_default")]
    habilitado: bool,
    #[serde(default, rename = "esAjuste")]
    es_ajuste: bool,
    #[serde(default, rename = "esTitulo")]
    es_titulo: bool,
    #[serde(default)]
    titulo: Option<MacTitulo>,
    #[serde(default)]
    color: MacColor,
    #[serde(default, rename = "modoDeFusion")]
    fusion: Fusion,
    #[serde(default, rename = "mascara")]
    mask: Option<Mask>,
}

#[derive(Deserialize)]
struct MacTitulo {
    #[serde(default)]
    texto: String,
    #[serde(default = "half_center", rename = "posicionX")]
    position_x: f64,
    #[serde(default = "half_center", rename = "posicionY")]
    position_y: f64,
    #[serde(default = "title_size")]
    tamano: f64,
    #[serde(default = "full_channel")]
    rojo: f64,
    #[serde(default = "full_channel")]
    verde: f64,
    #[serde(default = "full_channel")]
    azul: f64,
    #[serde(default)]
    forma: Option<String>,
}

/// Solo los primarios y la viñeta se portan; curvas, ruedas y desenfoque
/// siguen pendientes de mapear a filtros FFmpeg equivalentes.
#[derive(Default, Deserialize)]
struct MacColor {
    #[serde(default, rename = "exposicion")]
    exposure: f64,
    #[serde(default, rename = "contraste")]
    contrast: f64,
    #[serde(default, rename = "saturacion")]
    saturation: f64,
    #[serde(default)]
    vignette: f64,
    #[serde(default, rename = "desenfoque")]
    blur: f64,
    #[serde(default)]
    ruedas: Option<MacRuedas>,
    #[serde(default)]
    croma: Option<MacCroma>,
    #[serde(default)]
    curvas: Option<MacCurvas>,
}

#[derive(Deserialize)]
struct MacPunto {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct MacCurvas {
    #[serde(default)]
    luma: Vec<MacPunto>,
    #[serde(default)]
    rojo: Vec<MacPunto>,
    #[serde(default)]
    verde: Vec<MacPunto>,
    #[serde(default)]
    azul: Vec<MacPunto>,
}

#[derive(Deserialize)]
struct MacCroma {
    #[serde(default)]
    rojo: f64,
    #[serde(default)]
    verde: f64,
    #[serde(default)]
    azul: f64,
    #[serde(default, rename = "tolerancia")]
    tolerance: f64,
    #[serde(default, rename = "suavizado")]
    smooth: f64,
    #[serde(default, rename = "suprimirDerrame")]
    spill: f64,
}

#[derive(Deserialize)]
struct MacRuedas {
    #[serde(default, rename = "sombrasRojo")]
    shadows_r: f64,
    #[serde(default, rename = "sombrasVerde")]
    shadows_g: f64,
    #[serde(default, rename = "sombrasAzul")]
    shadows_b: f64,
    #[serde(default, rename = "mediosRojo")]
    mid_r: f64,
    #[serde(default, rename = "mediosVerde")]
    mid_g: f64,
    #[serde(default, rename = "mediosAzul")]
    mid_b: f64,
    #[serde(default, rename = "altasRojo")]
    high_r: f64,
    #[serde(default, rename = "altasVerde")]
    high_g: f64,
    #[serde(default, rename = "altasAzul")]
    high_b: f64,
}

#[derive(Default, Deserialize)]
struct MacTransform {
    #[serde(default, rename = "posicionX")]
    position_x: f64,
    #[serde(default, rename = "posicionY")]
    position_y: f64,
    #[serde(default = "normal_scale", rename = "escala")]
    scale_percent: f64,
    #[serde(default, rename = "rotacion")]
    rotation: f64,
    #[serde(default = "full_opacity", rename = "opacidad")]
    opacity: f64,
}

impl Default for RoughProject {
    fn default() -> Self {
        Self {
            version: 2,
            name: "Montaje sin título".to_owned(),
            clips: Vec::new(),
            markers: Vec::new(),
            subtitles: Vec::new(),
            subtitle_style: None,
            track_gains: Vec::new(),
            master_gain_db: 0.0,
            normalize_loudness: false,
            track_mutes: Vec::new(),
            track_solos: Vec::new(),
            video_hidden: Vec::new(),
            video_locked: Vec::new(),
            audio_locked: Vec::new(),
            fps: default_fps(),
            timebase: Some(Timebase::default()),
            min_video_tracks: 0,
            min_audio_tracks: 0,
            transcript: Vec::new(),
        }
    }
}

struct NovaCutWindows {
    project: RoughProject,
    project_path: Option<PathBuf>,
    selected: Option<usize>,
    status: String,
    export_result: Option<Receiver<Result<PathBuf, String>>>,
    setup_result: Option<Receiver<Result<(), String>>>,
    ffmpeg_ready: bool,
    playhead: f64,
    /// Ajuste magnético de arrastres al cabezal, marcadores y bordes.
    snap_enabled: bool,
    preview_result: Option<Receiver<Result<PreviewFrame, String>>>,
    preview_texture: Option<egui::TextureHandle>,
    preview_refresh_pending: bool,
    undo_stack: Vec<RoughProject>,
    redo_stack: Vec<RoughProject>,
    dirty: bool,
    /// Serialización del último estado guardado, para que undo/redo puedan
    /// recuperar correctamente el estado limpio del documento.
    clean_project_json: Option<String>,
    drag_edit: Option<(usize, DragKind, RoughProject)>,
    export_cancel: Option<Arc<AtomicBool>>,
    montage_render: Option<Receiver<Result<PathBuf, String>>>,
    /// Zoom de timeline: 1.0 ajusta el montaje al ancho disponible.
    zoom: f32,
    /// Desplazamiento horizontal de la timeline, en segundos.
    hscroll: f64,
    thumbnails: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    thumb_inflight: Option<(PathBuf, Receiver<Result<PreviewFrame, String>>)>,
    /// Preset de salida elegido: (ancho, alto).
    export_size: (u32, u32),
    /// Formato de la exportación: vídeo o solo audio.
    export_format: ExportFormat,
    /// Reproducción fluida en curso del montaje en el monitor.
    playback: Option<Playback>,
    /// Nivel de audio suavizado para el medidor del monitor.
    meter_display: (f32, f32),
    /// Volumen del monitor de reproducción (0…1).
    monitor_volume: f32,
    /// Resultado pendiente de "Exportar fotograma".
    frame_result: Option<Receiver<Result<PathBuf, String>>>,
    /// Quemar los subtítulos en el vídeo exportado y en el monitor.
    burn_subtitles: bool,
    /// Usa proxies disponibles en monitor y previsualización, nunca al exportar.
    use_proxies: bool,
    /// Presupuesto de disco para la carpeta de proxies, en gigabytes. Cero
    /// desactiva el recorte automático.
    proxy_limit_gb: f32,
    /// Última comprobación de que el proxy enlazado describe al medio actual.
    /// Se cachea porque mirar el disco en cada repintado castiga a quien monta
    /// desde un NAS, y basta con revisarlo un par de veces por segundo.
    proxy_freshness: Option<(PathBuf, PathBuf, bool, std::time::Instant)>,
    /// Resultado de generación de proxy: índice del clip y ruta creada.
    proxy_result: Option<Receiver<ProxyResult>>,
    loudness_result: Option<Receiver<LoudnessJobResult>>,
    loudness_report: Option<LoudnessReport>,
    /// Generación del proyecto en la que se midió `loudness_report`; si el
    /// proyecto cambia después, la medición deja de ser válida para el
    /// segundo paso de loudnorm y la exportación cae a un paso aproximado.
    loudness_report_generation: Option<u64>,
    silence_result: Option<Receiver<SilenceResult>>,
    transcription_result: Option<Receiver<TranscriptionResult>>,
    /// Selección en la transcripción: (ancla, extremo), índices de palabra.
    transcript_selection: Option<(usize, usize)>,
    /// Muestra las muletillas resaltadas en la transcripción.
    transcript_show_fillers: bool,
    /// GPUs capaces de codificar, en orden de preferencia (detectadas al
    /// arrancar en segundo plano).
    hw_backends: Vec<aceleracion::HwBackend>,
    hw_detect: Option<Receiver<Vec<aceleracion::HwBackend>>>,
    hardware_encoding: bool,
    ui_scale: f32,
    /// Falso en el primer arranque: se elige un tamaño según la pantalla.
    ui_scale_chosen: bool,
    pending_silence_cut: Option<(ClipJobKey, Vec<(f64, f64)>)>,
    scene_cut_result: Option<Receiver<SceneCutResult>>,
    pending_scene_cut: Option<(ClipJobKey, Vec<f64>)>,
    show_waveform: bool,
    show_vectorscope: bool,
    pending_document_action: Option<DocumentAction>,
    /// Recuperacion pendiente: se ofrece, nunca se carga silenciosamente.
    pending_recovery: Option<RoughProject>,
    document_generation: u64,
    /// Edición en vivo (texto/arrastre) aún sin comprometer a undo/disco:
    /// (estado previo al gesto, instante del último cambio).
    pending_edit: Option<(RoughProject, std::time::Instant)>,
    /// Progreso del render compartido con el hilo de FFmpeg.
    render_progress: Arc<std::sync::Mutex<RenderProgress>>,
    /// Rango de trabajo (entrada/salida) para exportar solo un tramo.
    work_in: Option<f64>,
    work_out: Option<f64>,
    /// Limita exportación y reproducción al rango de trabajo.
    export_range_only: bool,
    /// Clip copiado con Ctrl+C, listo para pegar en el cabezal.
    clip_clipboard: Option<RoughClip>,
    /// Atributos copiados (color, transformación, audio) para pegarlos en otro clip.
    attribute_clipboard: Option<Box<RoughClip>>,
    batch_state: batch::State,
    /// Altura de cada pista del montaje en píxeles.
    track_height: f32,
    /// Envolventes de audio cacheadas por medio, para dibujar la onda.
    waveforms: HashMap<PathBuf, Arc<Vec<f32>>>,
    waveform_inflight: Option<WaveformJob>,
    /// Filtro de texto del panel de medios.
    media_filter: String,
    media_options: media_browser::Options,
    media_file_status: media_browser::FileStatus,
    /// Pestaña activa del panel inferior de herramientas.
    bottom_tab: BottomTab,
    subtitle_query: String,
    subtitle_offset_ms: f64,
    /// Panel inferior desplegado.
    bottom_open: bool,
    /// Hoja de atajos de teclado abierta.
    show_shortcuts: bool,
    command_center: command_center::State,
    /// Clip cuyo menú contextual se está mostrando, para resaltarlo.
    context_menu_clip: Option<usize>,
    /// Píxeles por segundo de la última timeline dibujada; base del imán.
    timeline_pps: f64,
    /// Selección múltiple. `selected` es el clip principal y siempre pertenece
    /// a este conjunto cuando hay algo seleccionado.
    selection: std::collections::BTreeSet<usize>,
    /// Caja de selección en curso: esquina inicial en coordenadas de pantalla.
    marquee_origin: Option<egui::Pos2>,
    /// Al soltar un clip encima de otro se sobrescribe lo que haya debajo,
    /// como en los montadores habituales. Se puede desactivar.
    overwrite_on_drop: bool,
    /// Portapapeles de varios clips, con tiempos relativos al primero.
    clip_clipboard_group: Vec<RoughClip>,
    /// Modo de interacción elegido en la barra de herramientas del montaje.
    edit_tool: EditTool,
    /// Sondeo de medios fuera del hilo de UI.
    import_result: Option<Receiver<ImportJobResult>>,
    /// Drop recibido antes de conocer el rectángulo exacto de la timeline.
    pending_drop_paths: Vec<PathBuf>,
    pending_drop_position: Option<egui::Pos2>,
    /// Diálogo "Ir a timecode" (Ctrl+G).
    show_goto: bool,
    goto_text: String,
    /// Repetir la reproducción al llegar al final (montaje o rango).
    loop_playback: bool,
    /// Monitor a pantalla completa sin paneles (Esc para salir).
    monitor_fullscreen: bool,
    /// Proyectos abiertos o guardados recientemente.
    recent_projects: Vec<PathBuf>,
    /// Monitor de fuente y su último fotograma renderizado.
    source_monitor: Option<SourceMonitor>,
    source_frame: Option<egui::TextureHandle>,
    source_frame_key: Option<(PathBuf, f64)>,
    source_frame_inflight: Option<SourceFrameJob>,
    /// JSON de preferencias ya escrito en disco; base del guardado diferido.
    saved_settings_json: Option<String>,
    settings_dirty_at: Option<std::time::Instant>,
    /// Último auto-guardado de recuperación (cada 30 s con cambios).
    last_autosave: Option<std::time::Instant>,
}

/// Pestañas del panel inferior, equivalentes a los paneles de la app macOS.
#[derive(Clone, Copy, PartialEq)]
enum BottomTab {
    Mixer,
    Subtitles,
    Markers,
    Clips,
    Transcript,
    Inspector,
}

impl BottomTab {
    /// Orden persistido en los ajustes: solo se añade al final.
    const ALL: [BottomTab; 6] = [
        BottomTab::Mixer,
        BottomTab::Subtitles,
        BottomTab::Markers,
        BottomTab::Clips,
        BottomTab::Transcript,
        BottomTab::Inspector,
    ];

    /// Orden en la columna derecha, del más usado al menos.
    const SHOWN: [BottomTab; 6] = [
        BottomTab::Inspector,
        BottomTab::Transcript,
        BottomTab::Subtitles,
        BottomTab::Markers,
        BottomTab::Mixer,
        BottomTab::Clips,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Mixer => "Mezclador",
            Self::Subtitles => "Subtítulos",
            Self::Markers => "Marcadores",
            Self::Clips => "Planos",
            Self::Transcript => "Transcripción",
            Self::Inspector => "Inspector",
        }
    }
}

/// Órdenes que nacen del menú contextual del montaje y se aplican tras dibujar,
/// cuando ya no hay préstamos vivos sobre los clips.
#[derive(Clone, Copy)]
enum ClipCommand {
    Split,
    Duplicate,
    Copy,
    CopyAttributes,
    PasteAttributes,
    RemoveLeavingGap,
    RemoveClosingGap,
    TrimStartToPlayhead,
    TrimEndToPlayhead,
    CloseGap,
    Nest,
    Unnest,
    Relink,
    QuickFade,
    ToggleEnabled,
    SplitAllTracks,
    Overwrite,
    Insert,
    DetachAudio,
    FreezeFrame,
}

#[derive(Clone)]
struct ClipJobKey {
    generation: u64,
    index: usize,
    path: PathBuf,
    in_seconds: f64,
    out_seconds: f64,
}

/// Envolvente de audio en curso: medio de origen y canal de resultado.
type WaveformJob = (PathBuf, Receiver<Result<Vec<f32>, String>>);

type ProxyResult = (ClipJobKey, Result<PathBuf, String>);
type SilenceResult = (ClipJobKey, Result<Vec<(f64, f64)>, String>);
type SceneCutResult = (ClipJobKey, Result<Vec<f64>, String>);
type LoudnessJobResult = (u64, Result<LoudnessReport, String>);
type TranscriptionResult = (u64, Result<Vec<transcripcion::Word>, String>);

#[derive(Clone, Copy)]
struct ImportTarget {
    timeline_start: f64,
    track: usize,
    is_video: bool,
}

struct MediaProbe {
    duration: f64,
    has_video: bool,
    has_audio: bool,
    frame_rate: Option<Timebase>,
    variable_frame_rate: bool,
    source_pts: Option<SourcePtsSummary>,
}

type MediaProbeResult = (PathBuf, Result<MediaProbe, String>);
type ImportJobResult = (u64, Option<ImportTarget>, Vec<MediaProbeResult>);

/// Tamaño del fotograma del monitor de fuente.
const SOURCE_WIDTH: usize = 480;
const SOURCE_HEIGHT: usize = 270;

type SourceFrameJob = (PathBuf, f64, Receiver<Result<PreviewFrame, String>>);

/// Estado del monitor de fuente: medio original con puntos de entrada y
/// salida antes de montar.
#[derive(Clone)]
struct SourceMonitor {
    path: PathBuf,
    template: RoughClip,
    duration: f64,
    time: f64,
    source_in: f64,
    source_out: Option<f64>,
    playing: bool,
    play_anchor: (f64, std::time::Instant),
}

#[derive(Clone)]
enum DocumentAction {
    New,
    Open,
    OpenPath(PathBuf),
}

#[derive(Default)]
struct RenderProgress {
    pct: f64,
    eta_secs: f64,
    /// Aviso para el usuario al terminar (p. ej. la GPU falló y se usó CPU).
    note: Option<String>,
}

/// Preferencias de interfaz que sobreviven al cierre de la aplicación.
#[derive(Clone, Serialize, Deserialize)]
struct UiSettings {
    zoom: f32,
    track_height: f32,
    bottom_tab: u8,
    bottom_open: bool,
    show_waveform: bool,
    show_vectorscope: bool,
    burn_subtitles: bool,
    use_proxies: bool,
    overwrite_on_drop: bool,
    snap_enabled: bool,
    monitor_volume: f32,
    export_size: (u32, u32),
    export_format: u8,
    #[serde(default)]
    loop_playback: bool,
    #[serde(default = "default_proxy_limit_gb")]
    proxy_limit_gb: f32,
    /// Usa la GPU para H.264/HEVC cuando hay una disponible.
    #[serde(default = "enabled_by_default")]
    hardware_encoding: bool,
    /// Tamaño de la interfaz (1.0 = 100 %). Accesibilidad y pantallas
    /// grandes; también responde a Ctrl+= / Ctrl+- / Ctrl+0.
    /// `None` hasta que el usuario (o el primer arranque) lo fija.
    #[serde(default)]
    ui_scale: Option<f32>,
}

/// Tamaños de interfaz ofrecidos en el menú «Aa».
const UI_SCALES: [f32; 6] = [0.9, 1.0, 1.1, 1.25, 1.4, 1.5];

fn default_proxy_limit_gb() -> f32 {
    20.0
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("settings.json"))
}

fn recents_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("recents.json"))
}

/// Analiza `HH:MM:SS:FF`, `HH:MM:SS`, `MM:SS` o segundos decimales.
fn parse_timecode(text: &str, timebase: Timebase) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return Some(seconds.max(0.0));
    }
    if text.split([':', ';']).count() == 4 {
        return timebase
            .frames_from_timecode(&text)
            .map(|frames| timebase.seconds(frames));
    }
    let parts: Vec<f64> = text
        .split([':', ';'])
        .map(|part| part.trim().parse::<f64>().ok())
        .collect::<Option<Vec<f64>>>()?;
    if parts.iter().any(|part| *part < 0.0) {
        return None;
    }
    match parts.len() {
        3 => Some(parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]),
        2 => Some(parts[0] * 60.0 + parts[1]),
        _ => None,
    }
}

/// Qué produce la exportación: un vídeo con su códec o solo audio.
#[derive(Clone, Copy, PartialEq)]
enum ExportFormat {
    Mp4Video,
    WavAudio,
    Mp3Audio,
    Mp4Hevc,
    MovProRes,
    WebmVp9,
    Gif,
}

impl ExportFormat {
    /// Orden del selector; el índice es también el valor persistido en los
    /// ajustes, así que los formatos nuevos se añaden siempre al final.
    const ALL: [ExportFormat; 7] = [
        Self::Mp4Video,
        Self::WavAudio,
        Self::Mp3Audio,
        Self::Mp4Hevc,
        Self::MovProRes,
        Self::WebmVp9,
        Self::Gif,
    ];

    fn code(self) -> u8 {
        Self::ALL
            .iter()
            .position(|format| *format == self)
            .unwrap_or(0) as u8
    }

    fn from_code(code: u8) -> Self {
        Self::ALL
            .get(code as usize)
            .copied()
            .unwrap_or(Self::Mp4Video)
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Mp4Video | Self::Mp4Hevc => "mp4",
            Self::WavAudio => "wav",
            Self::Mp3Audio => "mp3",
            Self::MovProRes => "mov",
            Self::WebmVp9 => "webm",
            Self::Gif => "gif",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mp4Video => "MP4 H.264 (vídeo)",
            Self::WavAudio => "WAV (solo audio)",
            Self::Mp3Audio => "MP3 (solo audio)",
            Self::Mp4Hevc => "MP4 HEVC/H.265",
            Self::MovProRes => "MOV ProRes 422 (máster)",
            Self::WebmVp9 => "WebM VP9 (web)",
            Self::Gif => "GIF animado",
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            Self::Mp4Video => "H.264",
            Self::WavAudio => "WAV",
            Self::Mp3Audio => "MP3",
            Self::Mp4Hevc => "HEVC",
            Self::MovProRes => "ProRes",
            Self::WebmVp9 => "WebM",
            Self::Gif => "GIF",
        }
    }

    fn is_audio_only(self) -> bool {
        matches!(self, Self::WavAudio | Self::Mp3Audio)
    }

    fn has_audio_track(self) -> bool {
        self != Self::Gif
    }

    fn default_file_name(self) -> String {
        if self.is_audio_only() {
            format!("NovaCut Audio.{}", self.extension())
        } else {
            format!("NovaCut Export.{}", self.extension())
        }
    }

    /// Argumentos de códec con GPU opcional. Solo H.264 y HEVC se aceleran;
    /// ProRes, WebM y GIF siguen en CPU.
    fn export_args(self, fast: bool, hw: Option<aceleracion::HwBackend>) -> Vec<String> {
        let hevc = match self {
            Self::Mp4Video => false,
            Self::Mp4Hevc => true,
            _ => {
                return self
                    .codec_args(fast)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            }
        };
        let Some(backend) = hw else {
            return self
                .codec_args(fast)
                .into_iter()
                .map(str::to_owned)
                .collect();
        };
        let mut args = backend.video_args(hevc, fast);
        args.extend(
            [
                "-fps_mode",
                "cfr",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ]
            .map(str::to_owned),
        );
        args
    }

    /// Argumentos de códec. `fast` solo se usa con H.264 (renders internos
    /// de previsualización).
    fn codec_args(self, fast: bool) -> Vec<&'static str> {
        match self {
            Self::Mp4Video => vec![
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                if fast { "ultrafast" } else { "medium" },
                "-crf",
                if fast { "28" } else { "18" },
                "-fps_mode",
                "cfr",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ],
            Self::Mp4Hevc => vec![
                "-c:v",
                "libx265",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "medium",
                "-crf",
                "22",
                "-tag:v",
                "hvc1",
                "-fps_mode",
                "cfr",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ],
            Self::MovProRes => vec![
                "-c:v",
                "prores_ks",
                "-profile:v",
                "2",
                "-pix_fmt",
                "yuv422p10le",
                "-vendor",
                "apl0",
                "-fps_mode",
                "cfr",
                "-c:a",
                "pcm_s16le",
            ],
            Self::WebmVp9 => vec![
                "-c:v",
                "libvpx-vp9",
                "-pix_fmt",
                "yuv420p",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-fps_mode",
                "cfr",
                "-c:a",
                "libopus",
                "-b:a",
                "160k",
            ],
            Self::Gif => vec!["-loop", "0"],
            Self::WavAudio => vec!["-c:a", "pcm_s16le"],
            Self::Mp3Audio => vec!["-c:a", "libmp3lame", "-b:a", "192k"],
        }
    }
}

/// Paleta óptima en el mismo grafo: un GIF sin paleta propia sale con
/// bandas. 15 fps y 720 px de ancho como máximo, como el exportador de GIF
/// de Media Encoder.
fn gif_palette_filters(input: &str, output: &str) -> Vec<String> {
    vec![
        format!("[{input}]fps=15,scale='min(iw,720)':-2:flags=lanczos,split=2[gifa][gifb]"),
        "[gifa]palettegen=stats_mode=diff[gifpal]".to_owned(),
        format!(
            "[gifb][gifpal]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle[{output}]"
        ),
    ]
}

impl NovaCutWindows {
    fn clip_job_key(&self, index: usize) -> ClipJobKey {
        let clip = &self.project.clips[index];
        ClipJobKey {
            generation: self.document_generation,
            index,
            path: clip.path.clone(),
            in_seconds: clip.in_seconds,
            out_seconds: clip.out_seconds,
        }
    }

    fn clip_job_is_current(&self, key: &ClipJobKey) -> bool {
        self.document_generation == key.generation
            && self.project.clips.get(key.index).is_some_and(|clip| {
                clip.path == key.path
                    && clip.in_seconds == key.in_seconds
                    && clip.out_seconds == key.out_seconds
            })
    }

    fn reset_document(&mut self) {
        self.cancel_document_jobs();
        self.project = RoughProject::default();
        self.project_path = None;
        self.clear_selection();
        self.playhead = 0.0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.preview_texture = None;
        self.preview_result = None;
        self.preview_refresh_pending = false;
        self.dirty = false;
        self.clean_project_json = serde_json::to_string(&self.project).ok();
        self.pending_edit = None;
        self.pending_recovery = None;
        clear_recovery();
        self.work_in = None;
        self.work_out = None;
        self.export_range_only = false;
        self.waveforms.clear();
        self.waveform_inflight = None;
        self.media_filter.clear();
        self.document_generation = self.document_generation.wrapping_add(1);
        self.status = "Proyecto nuevo".to_owned();
    }

    fn cancel_document_jobs(&mut self) {
        self.stop_playback();
        if let Some(cancel) = self.export_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.export_result = None;
        self.montage_render = None;
        self.frame_result = None;
        self.proxy_result = None;
        self.loudness_result = None;
        self.loudness_report = None;
        self.loudness_report_generation = None;
        self.silence_result = None;
        self.pending_silence_cut = None;
        self.scene_cut_result = None;
        self.pending_scene_cut = None;
        self.transcription_result = None;
        self.drag_edit = None;
        self.import_result = None;
        self.pending_drop_paths.clear();
        self.pending_drop_position = None;
    }

    fn refresh_dirty_from_saved(&mut self) {
        self.dirty = match (
            &self.clean_project_json,
            serde_json::to_string(&self.project),
        ) {
            (Some(saved), Ok(current)) => saved != &current,
            _ => true,
        };
    }

    fn current_ui_settings(&self) -> UiSettings {
        UiSettings {
            zoom: self.zoom,
            track_height: self.track_height,
            bottom_tab: BottomTab::ALL
                .iter()
                .position(|tab| *tab == self.bottom_tab)
                .unwrap_or(0) as u8,
            bottom_open: self.bottom_open,
            show_waveform: self.show_waveform,
            show_vectorscope: self.show_vectorscope,
            burn_subtitles: self.burn_subtitles,
            use_proxies: self.use_proxies,
            overwrite_on_drop: self.overwrite_on_drop,
            snap_enabled: self.snap_enabled,
            monitor_volume: self.monitor_volume,
            export_size: self.export_size,
            export_format: self.export_format.code(),
            loop_playback: self.loop_playback,
            proxy_limit_gb: self.proxy_limit_gb,
            hardware_encoding: self.hardware_encoding,
            ui_scale: Some(self.ui_scale),
        }
    }

    /// Registra un proyecto al principio de la lista de recientes (máx. 10).
    fn push_recent(&mut self, path: &Path) {
        self.recent_projects.retain(|recent| recent != path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(10);
        if let Some(target) = recents_path() {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&self.recent_projects) {
                let _ = std::fs::write(&target, json);
            }
        }
    }

    fn load_recents(&mut self) {
        let Some(path) = recents_path() else {
            return;
        };
        if let Ok(json) = std::fs::read_to_string(&path) {
            self.recent_projects = serde_json::from_str::<Vec<PathBuf>>(&json).unwrap_or_default();
            self.recent_projects.retain(|path| path.exists());
        }
    }

    fn load_ui_settings(&mut self) {
        let Some(path) = settings_path() else {
            return;
        };
        let Ok(json) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(settings) = serde_json::from_str::<UiSettings>(&json) else {
            return;
        };
        self.zoom = settings.zoom.clamp(1.0, 400.0);
        self.track_height = settings.track_height.clamp(28.0, 180.0);
        if let Some(tab) = BottomTab::ALL.get(settings.bottom_tab as usize) {
            self.bottom_tab = *tab;
        }
        self.bottom_open = settings.bottom_open;
        self.show_waveform = settings.show_waveform;
        self.show_vectorscope = settings.show_vectorscope;
        self.burn_subtitles = settings.burn_subtitles;
        self.use_proxies = settings.use_proxies;
        self.overwrite_on_drop = settings.overwrite_on_drop;
        self.snap_enabled = settings.snap_enabled;
        self.monitor_volume = settings.monitor_volume.clamp(0.0, 1.0);
        if settings.export_size.0 > 0 && settings.export_size.1 > 0 {
            self.export_size = settings.export_size;
        }
        self.export_format = ExportFormat::from_code(settings.export_format);
        self.loop_playback = settings.loop_playback;
        self.proxy_limit_gb = settings.proxy_limit_gb.clamp(0.0, 1000.0);
        self.hardware_encoding = settings.hardware_encoding;
        if let Some(scale) = settings.ui_scale {
            self.ui_scale = scale.clamp(0.75, 2.0);
            self.ui_scale_chosen = true;
        }
        self.saved_settings_json = serde_json::to_string(&settings).ok();
    }

    /// Escribe las preferencias en disco solo cuando llevan 1.5 s sin cambiar,
    /// para no tocar el disco en cada píxel de un slider.
    fn poll_settings_save(&mut self) {
        let Some(json) = serde_json::to_string(&self.current_ui_settings()).ok() else {
            return;
        };
        if Some(&json) == self.saved_settings_json.as_ref() {
            self.settings_dirty_at = None;
            return;
        }
        let since = *self
            .settings_dirty_at
            .get_or_insert(std::time::Instant::now());
        if since.elapsed() < std::time::Duration::from_millis(1500) {
            return;
        }
        if let Some(path) = settings_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&path, &json).is_ok() {
                self.saved_settings_json = Some(json);
            }
        }
        self.settings_dirty_at = None;
    }

    /// Guarda la sesión de recuperación cada 30 s mientras haya cambios, para
    /// no perder trabajo si la aplicación se cierra de forma abrupta.
    fn poll_autosave(&mut self) {
        const AUTOSAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        if !self.has_unsaved_changes() {
            self.last_autosave = None;
            return;
        }
        let Some(last) = self.last_autosave else {
            self.last_autosave = Some(std::time::Instant::now());
            return;
        };
        if last.elapsed() >= AUTOSAVE_INTERVAL {
            save_recovery(&self.project);
            self.last_autosave = Some(std::time::Instant::now());
        }
    }

    /// Encola una edición en vivo (texto/arrastre) sin escribir undo ni disco
    /// todavía. Conserva el estado previo a la primera modificación de este
    /// gesto y renueva el temporizador en cada cambio; `poll_pending_edit`
    /// confirma la edición como una única entrada de undo cuando el usuario
    /// deja de interactuar, evitando una entrada por tecla o píxel de arrastre.
    fn queue_edit(&mut self, baseline: RoughProject) {
        match &mut self.pending_edit {
            Some((_, since)) => *since = std::time::Instant::now(),
            None => self.pending_edit = Some((baseline, std::time::Instant::now())),
        }
    }

    fn flush_pending_edit(&mut self) {
        if let Some((baseline, _)) = self.pending_edit.take() {
            self.finish_edit(baseline);
        }
    }

    fn poll_pending_edit(&mut self, context: &egui::Context) {
        const COMMIT_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);
        let Some((_, since)) = &self.pending_edit else {
            return;
        };
        let elapsed = since.elapsed();
        let dragging = context.input(|input| input.pointer.any_down());
        if elapsed >= COMMIT_DEBOUNCE && !dragging {
            self.flush_pending_edit();
        } else {
            context.request_repaint_after(COMMIT_DEBOUNCE.saturating_sub(elapsed));
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        self.dirty || self.pending_edit.is_some()
    }

    fn execute_document_action(&mut self, action: DocumentAction) {
        match action {
            DocumentAction::New => self.reset_document(),
            DocumentAction::Open => self.open_project(),
            DocumentAction::OpenPath(path) => self.load_project_from(path),
        }
    }

    fn request_document_action(&mut self, action: DocumentAction) {
        if self.has_unsaved_changes() {
            self.pending_document_action = Some(action);
        } else {
            self.execute_document_action(action);
        }
    }

    fn show_unsaved_dialog(&mut self, context: &egui::Context) {
        let Some(action) = self.pending_document_action.clone() else {
            return;
        };
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        egui::Window::new("Cambios sin guardar")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label("El proyecto actual tiene cambios sin guardar.");
                ui.label("¿Quieres guardarlos antes de continuar?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    save = ui
                        .button("Guardar")
                        .on_hover_text("Guarda el proyecto y continúa")
                        .clicked();
                    discard = ui
                        .button("Descartar")
                        .on_hover_text("Continúa sin guardar: los cambios se pierden")
                        .clicked();
                    cancel = ui
                        .button("Cancelar")
                        .on_hover_text("Vuelve sin hacer nada")
                        .clicked();
                });
            });
        if save {
            self.save_project(false);
            if !self.has_unsaved_changes() {
                self.pending_document_action = None;
                self.execute_document_action(action);
            }
        } else if discard {
            self.pending_document_action = None;
            self.execute_document_action(action);
        } else if cancel {
            self.pending_document_action = None;
        }
    }

    fn show_recovery_dialog(&mut self, context: &egui::Context) {
        let Some(project) = self.pending_recovery.as_ref() else {
            return;
        };
        let project_name = project.name.clone();
        let mut recover = false;
        let mut discard = false;
        egui::Window::new("Recuperacion disponible")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(format!(
                    "NovaCut encontro cambios no guardados de «{project_name}»."
                ));
                ui.label("La sesion no se carga hasta que lo confirmes.");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    recover = ui
                        .button("Recuperar")
                        .on_hover_text("Abre la copia de la sesión anterior, que no se cerró bien")
                        .clicked();
                    discard = ui
                        .button("Descartar")
                        .on_hover_text("Empieza sin la copia recuperada")
                        .clicked();
                });
            });
        if recover {
            let Some(project) = self.pending_recovery.take() else {
                return;
            };
            self.cancel_document_jobs();
            self.project = project;
            self.project.normalize();
            self.project_path = None;
            self.clear_selection();
            self.playhead = 0.0;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.preview_texture = None;
            self.preview_result = None;
            self.preview_refresh_pending = false;
            self.dirty = true;
            self.clean_project_json = None;
            self.pending_edit = None;
            self.work_in = None;
            self.work_out = None;
            self.export_range_only = false;
            self.document_generation = self.document_generation.wrapping_add(1);
            self.status = "Sesion anterior recuperada".to_owned();
            if self.ffmpeg_ready {
                self.request_preview();
            }
        } else if discard {
            self.pending_recovery = None;
            clear_recovery();
            self.status = "Recuperacion descartada".to_owned();
        }
    }

    /// Clips con medio ausente en disco (los títulos nunca cuentan).
    fn missing_media_indices(&self) -> Vec<usize> {
        self.project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.title.is_none() && !clip.path.as_os_str().is_empty() && !clip.path.exists()
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Pide al usuario el nuevo archivo para el medio del clip seleccionado.
    /// Clips cuyo medio ya no está en disco, con el nombre que hay que buscar.
    fn offline_clips(&self) -> Vec<relink::MissingMedia> {
        self.project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                !clip.path.as_os_str().is_empty() && clip.nested.is_none() && !clip.path.is_file()
            })
            .filter_map(|(index, clip)| {
                Some(relink::MissingMedia {
                    key: index,
                    name: clip.path.file_name()?.to_str()?.to_owned(),
                    // El tamaño original no se guarda en el proyecto; el nombre
                    // y la profundidad son lo que hay para desempatar.
                    bytes: 0,
                })
            })
            .collect()
    }

    /// Localiza de golpe todos los medios offline dentro de una carpeta.
    fn relink_all_from_folder(&mut self) {
        let wanted = self.offline_clips();
        if wanted.is_empty() {
            self.status = "No hay medios offline que revincular".to_owned();
            return;
        }
        let start = self
            .project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let Some(root) = rfd::FileDialog::new().set_directory(start).pick_folder() else {
            return;
        };
        let outcome = relink::find_missing_media(&wanted, &root, 200_000);
        if outcome.found.is_empty() {
            self.status = relink::summarize(&outcome);
            return;
        }
        let before = self.project.clone();
        let mut unreadable = Vec::new();
        let mut relinked = 0;
        for media in &wanted {
            let Some(path) = outcome.found.get(&media.key) else {
                continue;
            };
            if !self.apply_relink(media.key, path) {
                unreadable.push(media.name.clone());
                continue;
            }
            relinked += 1;
        }
        self.project.normalize();
        self.finish_edit(before);
        let mut text = relink::summarize(&outcome);
        if !unreadable.is_empty() {
            // Encontrado por nombre pero ilegible: archivo corrupto o un códec
            // que estas herramientas no abren. Decirlo, no contarlo como éxito.
            text = text.replacen(
                &format!("{} medio", outcome.found.len()),
                &format!("{relinked} medio"),
                1,
            );
            text.push_str(&format!(
                " · {} no se pudieron analizar: {}",
                unreadable.len(),
                unreadable
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        self.status = text;
    }

    /// Apunta un clip a otro archivo conservando el montaje. `false` si el medio
    /// no se pudo analizar; en ese caso no se conserva la duración del anterior,
    /// que describiría a otro vídeo.
    fn apply_relink(&mut self, index: usize, new_path: &Path) -> bool {
        let metadata = if is_image_file(new_path) {
            Some(MediaProbe {
                duration: DEFAULT_IMAGE_DURATION,
                has_video: true,
                has_audio: false,
                frame_rate: None,
                variable_frame_rate: false,
                source_pts: None,
            })
        } else {
            probe_media(new_path).ok()
        };
        let ok = metadata.is_some();
        let clip = &mut self.project.clips[index];
        clip.path = new_path.to_path_buf();
        clip.source_duration_seconds = metadata.as_ref().map(|probe| probe.duration);
        clip.source_timebase = metadata.as_ref().and_then(|probe| probe.frame_rate);
        clip.source_vfr = metadata
            .as_ref()
            .is_some_and(|probe| probe.variable_frame_rate);
        clip.source_pts = metadata.and_then(|probe| probe.source_pts);
        ok
    }

    fn relink_selected(&mut self) {
        let Some(index) = self.selected else {
            self.status = "Selecciona un clip offline primero".to_owned();
            return;
        };
        if self.project.clip_locked(&self.project.clips[index]) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let Some(current) = self.project.clips[index]
            .path
            .parent()
            .map(|p| p.to_path_buf())
        else {
            return;
        };
        let Some(new_path) = rfd::FileDialog::new().set_directory(&current).pick_file() else {
            return;
        };
        let before = self.project.clone();
        let metadata_ok = self.apply_relink(index, &new_path);
        self.project.normalize();
        self.finish_edit(before);
        self.status = if metadata_ok {
            format!("Medio revinculado y analizado: {}", new_path.display())
        } else {
            format!(
                "Medio revinculado; no se pudo analizar: {}",
                new_path.display()
            )
        };
    }

    fn new(context: &eframe::CreationContext<'_>) -> Self {
        // El tema lo aplica `main` vía `theme::apply` antes de crear la app.
        let mut app = Self {
            project: RoughProject::default(),
            project_path: None,
            selected: None,
            status: "Importa clips para empezar.".to_owned(),
            export_result: None,
            setup_result: None,
            ffmpeg_ready: multimedia_tools_available(),
            playhead: 0.0,
            snap_enabled: true,
            preview_result: None,
            preview_texture: None,
            preview_refresh_pending: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            dirty: false,
            clean_project_json: serde_json::to_string(&RoughProject::default()).ok(),
            drag_edit: None,
            export_cancel: None,
            montage_render: None,
            zoom: 1.0,
            hscroll: 0.0,
            thumbnails: std::collections::HashMap::new(),
            thumb_inflight: None,
            export_size: (1920, 1080),
            export_format: ExportFormat::Mp4Video,
            playback: None,
            meter_display: (0.0, 0.0),
            monitor_volume: 1.0,
            frame_result: None,
            burn_subtitles: false,
            use_proxies: true,
            proxy_limit_gb: 20.0,
            proxy_freshness: None,
            proxy_result: None,
            loudness_result: None,
            loudness_report: None,
            loudness_report_generation: None,
            silence_result: None,
            transcription_result: None,
            transcript_selection: None,
            transcript_show_fillers: true,
            hw_backends: Vec::new(),
            hw_detect: None,
            hardware_encoding: true,
            ui_scale: 1.0,
            ui_scale_chosen: false,
            pending_silence_cut: None,
            scene_cut_result: None,
            pending_scene_cut: None,
            show_waveform: false,
            show_vectorscope: false,
            pending_document_action: None,
            pending_recovery: None,
            document_generation: 0,
            pending_edit: None,
            render_progress: Arc::new(std::sync::Mutex::new(RenderProgress::default())),
            work_in: None,
            work_out: None,
            export_range_only: false,
            clip_clipboard: None,
            attribute_clipboard: None,
            batch_state: batch::State::default(),
            track_height: 54.0,
            waveforms: HashMap::new(),
            waveform_inflight: None,
            media_filter: String::new(),
            media_options: media_browser::Options::default(),
            media_file_status: media_browser::FileStatus::default(),
            bottom_tab: BottomTab::Mixer,
            subtitle_query: String::new(),
            subtitle_offset_ms: 0.0,
            bottom_open: false,
            show_shortcuts: false,
            command_center: command_center::State::default(),
            context_menu_clip: None,
            timeline_pps: 0.0,
            selection: std::collections::BTreeSet::new(),
            marquee_origin: None,
            overwrite_on_drop: true,
            clip_clipboard_group: Vec::new(),
            edit_tool: EditTool::Select,
            import_result: None,
            pending_drop_paths: Vec::new(),
            pending_drop_position: None,
            show_goto: false,
            goto_text: String::new(),
            loop_playback: false,
            monitor_fullscreen: false,
            recent_projects: Vec::new(),
            source_monitor: None,
            source_frame: None,
            source_frame_key: None,
            source_frame_inflight: None,
            saved_settings_json: None,
            settings_dirty_at: None,
            last_autosave: None,
        };
        app.load_ui_settings();
        app.load_recents();
        app.start_hw_detection();
        // Revisión de interfaz: NOVACUT_UI_ZOOM emula pantallas mayores que
        // la real (p. ej. 0.9 en 1728 px = 1920 px lógicos).
        if let Some(zoom) = std::env::var("NOVACUT_UI_ZOOM")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
        {
            app.ui_scale = zoom.clamp(0.3, 2.0);
            app.ui_scale_chosen = true;
        }
        context.egui_ctx.set_zoom_factor(app.ui_scale);
        if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if extension == "ncrough" || extension == "editorcito" {
                app.load_project_from(path);
            } else if app.ffmpeg_ready {
                app.import_paths(vec![path]);
            }
        } else if let Some(mut project) = load_recovery() {
            project.normalize();
            app.pending_recovery = Some(project);
            app.status = "Hay una recuperacion pendiente de confirmacion".to_owned();
        } else if !app.ffmpeg_ready {
            app.status = "Falta el motor multimedia. Pulsa Instalar FFmpeg.".to_owned();
        }
        app
    }

    /// Abre el monitor de fuente con el medio del clip indicado.
    fn open_source_monitor(&mut self, index: usize) {
        let Some(clip) = self.project.clips.get(index).cloned() else {
            return;
        };
        if clip.title.is_some() || clip.nested.is_some() || clip.path.as_os_str().is_empty() {
            self.status = "El monitor de fuente necesita un medio de archivo".to_owned();
            return;
        }
        if !clip.path.exists() {
            self.status = "El medio está offline".to_owned();
            return;
        }
        let duration = clip
            .source_duration_seconds
            .unwrap_or(clip.out_seconds)
            .max(clip.out_seconds);
        self.source_monitor = Some(SourceMonitor {
            path: clip.path.clone(),
            template: clip,
            duration,
            time: 0.0,
            source_in: 0.0,
            source_out: None,
            playing: false,
            play_anchor: (0.0, std::time::Instant::now()),
        });
        self.source_frame = None;
        self.source_frame_key = None;
        self.source_frame_inflight = None;
    }

    /// Pide el fotograma del monitor de fuente cuando cambia el instante.
    fn poll_source_frame(&mut self, context: &egui::Context) {
        if let Some(monitor) = &mut self.source_monitor {
            if monitor.playing {
                let (start_time, started) = monitor.play_anchor;
                monitor.time = (start_time + started.elapsed().as_secs_f64()).min(monitor.duration);
                if monitor.time >= monitor.duration {
                    monitor.playing = false;
                }
            }
        }
        let Some(monitor) = &self.source_monitor else {
            return;
        };
        let wanted = monitor.time;
        let path = monitor.path.clone();
        if let Some((inflight_path, inflight_time, receiver)) = &mut self.source_frame_inflight {
            if let Ok(result) = receiver.try_recv() {
                if let Ok(frame) = result {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.pixels,
                    );
                    self.source_frame = Some(context.load_texture(
                        "source-monitor",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                    self.source_frame_key = Some((inflight_path.clone(), *inflight_time));
                }
                self.source_frame_inflight = None;
            }
        }
        if self.source_frame_inflight.is_none() {
            let stale = self
                .source_frame_key
                .as_ref()
                .is_none_or(|(cached_path, cached_time)| {
                    *cached_path != path || (*cached_time - wanted).abs() > 0.04
                });
            if stale {
                let (sender, receiver) = mpsc::channel();
                let thread_path = path.clone();
                std::thread::spawn(move || {
                    let _ = sender.send(extract_frame(
                        &thread_path,
                        wanted,
                        SOURCE_WIDTH,
                        SOURCE_HEIGHT,
                    ));
                });
                self.source_frame_inflight = Some((path, wanted, receiver));
            }
        }
        let playing = self
            .source_monitor
            .as_ref()
            .is_some_and(|monitor| monitor.playing);
        if self.source_frame_inflight.is_some() || playing {
            context.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    /// Ventana del monitor de fuente con In/Out e inserción en el montaje.
    fn show_source_monitor(&mut self, context: &egui::Context) {
        if self.source_monitor.is_none() {
            return;
        }
        let duration = self.source_monitor.as_ref().unwrap().duration;
        let media_name = self
            .source_monitor
            .as_ref()
            .unwrap()
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "medio".to_owned());
        let mut open = true;
        let mut scrub: Option<f64> = None;
        let mut toggle_play = false;
        let mut set_in = false;
        let mut set_out = false;
        let mut insert = false;
        let mut jump = false;
        let mut time = self.source_monitor.as_ref().unwrap().time;
        let source_in = self.source_monitor.as_ref().unwrap().source_in;
        let mut source_out = self.source_monitor.as_ref().unwrap().source_out;
        let fps = self.project.fps;
        egui::Window::new(format!("Fuente · {media_name}"))
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(context, |ui| {
                if let Some(texture) = &self.source_frame {
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::vec2(ui.available_width(), ui.available_width() * 9.0 / 16.0),
                    )));
                } else {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Cargando fotograma…").color(theme::TEXT_FAINT),
                        );
                    });
                    ui.add_space(80.0);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("⏮").on_hover_text("Al inicio").clicked() {
                        jump = true;
                    }
                    if ui
                        .button(if time > source_in + 0.001 && source_out.is_none() {
                            "⏸"
                        } else {
                            "▶"
                        })
                        .on_hover_text("Reproducir hasta el final del medio")
                        .clicked()
                    {
                        toggle_play = true;
                    }
                    let slider = ui.add(
                        egui::Slider::new(&mut time, 0.0..=duration.max(0.04)).show_value(false),
                    );
                    if slider.changed() {
                        scrub = Some(time);
                    }
                    ui.monospace(timecode(time, fps));
                });
                ui.horizontal(|ui| {
                    if ui
                        .button("Marcar entrada")
                        .on_hover_text("Entrada del recorte en el visor de origen (I)")
                        .clicked()
                    {
                        set_in = true;
                    }
                    if ui
                        .button("Marcar salida")
                        .on_hover_text("Salida del recorte en el visor de origen (O)")
                        .clicked()
                    {
                        set_out = true;
                    }
                    if ui
                        .button("Quitar salida")
                        .on_hover_text("Usa el medio hasta su final")
                        .clicked()
                    {
                        source_out = None;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Insertar en el cabezal").strong(),
                            ))
                            .on_hover_text("Inserta el recorte en el cabezal del montaje")
                            .clicked()
                        {
                            insert = true;
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(format!(
                        "Entrada {} · Salida {} · seleccionado {:.2} s",
                        timecode(source_in, fps),
                        source_out
                            .filter(|out| *out > source_in + 0.04)
                            .map(|out| timecode(out, fps))
                            .unwrap_or_else(|| "—".to_owned()),
                        source_out
                            .filter(|out| *out > source_in + 0.04)
                            .map_or(duration - source_in, |out| out - source_in)
                    ))
                    .size(11.5)
                    .color(theme::TEXT_FAINT),
                );
            });
        let Some(monitor) = &mut self.source_monitor else {
            return;
        };
        if !open {
            self.source_monitor = None;
            return;
        }
        monitor.source_in = source_in.clamp(0.0, (duration - 0.04).max(0.0));
        monitor.source_out = source_out.map(|out| out.clamp(monitor.source_in + 0.04, duration));
        if set_in {
            monitor.source_in = time.clamp(0.0, (duration - 0.04).max(0.0));
            if monitor
                .source_out
                .is_some_and(|out| out <= monitor.source_in + 0.04)
            {
                monitor.source_out = None;
            }
        }
        if set_out {
            monitor.source_out = Some(time.clamp(monitor.source_in + 0.04, duration));
        }
        if jump {
            monitor.time = monitor.source_in;
            monitor.playing = false;
        }
        if toggle_play {
            monitor.playing = !monitor.playing;
            monitor.play_anchor = (monitor.time, std::time::Instant::now());
        }
        if let Some(scrub) = scrub {
            monitor.time = scrub.clamp(0.0, duration);
            monitor.playing = false;
        }
        if insert {
            let Some(monitor) = self.source_monitor.clone() else {
                return;
            };
            let out = monitor
                .source_out
                .unwrap_or(monitor.duration)
                .max(monitor.source_in + 0.04);
            let mut clip = monitor.template;
            clip.in_seconds = monitor.source_in;
            clip.out_seconds = out;
            clip.speed = 1.0;
            clip.timeline_start = self.playhead.max(0.0);
            clip.track = 0;
            clip.freeze_at = None;
            clip.fade_in_seconds = 0.0;
            clip.fade_out_seconds = 0.0;
            clip.transition = None;
            let span_end = clip.timeline_start + clip.duration();
            let before = self.project.clone();
            clear_track_span(
                &mut self.project.clips,
                clip.track,
                clip.has_video,
                clip.timeline_start,
                span_end,
                &[],
            );
            self.project.clips.push(clip);
            self.select_only(self.project.clips.len() - 1);
            self.finish_edit(before);
            self.status = "Fuente insertada en el cabezal".to_owned();
        }
    }

    /// Diálogo modal para saltar a un timecode exacto (Ctrl+G).
    fn show_goto_dialog(&mut self, context: &egui::Context) {
        if !self.show_goto {
            return;
        }
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Ir a timecode")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label("HH:MM:SS:FF · MM:SS · segundos");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.goto_text)
                        .hint_text("00:01:23:12")
                        .desired_width(170.0)
                        .font(egui::TextStyle::Monospace),
                );
                field.request_focus();
                confirm |=
                    field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    confirm |= ui
                        .button("Ir")
                        .on_hover_text("Lleva el cabezal a ese timecode")
                        .clicked();
                    cancel |= ui
                        .button("Cancelar")
                        .on_hover_text("Cierra sin moverse")
                        .clicked();
                });
            });
        if confirm {
            match parse_timecode(&self.goto_text, self.project.timebase()) {
                Some(time) => {
                    self.stop_playback();
                    self.seek(time.min(self.timeline_extent()));
                    self.show_goto = false;
                }
                None => self.status = "Timecode no válido".to_owned(),
            }
        }
        if cancel {
            self.show_goto = false;
        }
    }

    /// Monitor del programa ocupando toda la ventana; Esc o el botón salen.
    fn show_fullscreen_monitor(&mut self, context: &egui::Context) {
        let mut exit =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::BLACK))
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("Salir (Esc)")
                        .on_hover_text("Sale de la pantalla completa (Esc)")
                        .clicked()
                    {
                        exit = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(timecode(self.playhead, self.project.fps));
                    });
                });
                ui.centered_and_justified(|ui| {
                    if let Some(texture) = &self.preview_texture {
                        let size = texture.size_vec2();
                        let available = ui.available_size();
                        let scale = (available.x / size.x).min(available.y / size.y).min(1.0);
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(
                            texture.id(),
                            size * scale,
                        )));
                    } else {
                        ui.label(
                            egui::RichText::new("Sin imagen · pulsa Espacio para reproducir")
                                .color(theme::TEXT_FAINT),
                        );
                    }
                });
            });
        if exit {
            self.monitor_fullscreen = false;
        }
    }

    fn import_media(&mut self) {
        let Some(paths) = FileDialog::new()
            .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm", "m4v"])
            .add_filter(
                "Imagen",
                &["jpg", "jpeg", "png", "bmp", "webp", "gif", "tif", "tiff"],
            )
            .add_filter("Audio", &["wav", "mp3", "m4a", "aac", "flac", "ogg"])
            .pick_files()
        else {
            return;
        };

        self.import_paths(paths);
    }

    /// Carpeta donde viven los proxies: junto al proyecto si está guardado, si
    /// no junto al medio de referencia. Debe coincidir con la que usa la
    /// generación, o el presupuesto miraría a un sitio donde no hay nada.
    fn proxy_root(&self, source: Option<&Path>) -> PathBuf {
        self.project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| source.and_then(Path::parent).map(Path::to_path_buf))
            .or_else(|| {
                self.project
                    .clips
                    .iter()
                    .find(|clip| !clip.path.as_os_str().is_empty())
                    .and_then(|clip| clip.path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(std::env::temp_dir)
            .join("NovaCut Proxies")
    }

    /// Proxies que el proyecto abierto tiene enlazados: nunca se desalojan.
    fn linked_proxies(&self) -> HashSet<PathBuf> {
        self.project
            .clips
            .iter()
            .filter_map(|clip| clip.proxy.clone())
            .collect()
    }

    /// ¿El proxy enlazado por el clip seleccionado sigue describiendo su medio?
    /// `None` cuando no hay proxy que juzgar.
    fn selected_proxy_is_current(&mut self) -> Option<bool> {
        let clip = self.project.clips.get(self.selected?)?;
        let (proxy, source) = (clip.proxy.clone()?, clip.path.clone());
        if let Some((cached_proxy, cached_source, fresh, at)) = &self.proxy_freshness {
            if *cached_proxy == proxy
                && *cached_source == source
                && at.elapsed() < std::time::Duration::from_millis(500)
            {
                return Some(*fresh);
            }
        }
        let fresh = proxy_cache::proxy_matches_source(&proxy, &source);
        self.proxy_freshness = Some((proxy, source, fresh, std::time::Instant::now()));
        Some(fresh)
    }

    fn proxy_cache_summary(&self) -> String {
        let usage = proxy_cache::usage(&self.proxy_root(None), &self.linked_proxies());
        if usage.files == 0 {
            return "Caché de proxies vacía".to_owned();
        }
        format!(
            "{} archivo{} · {} · {} desalojables",
            usage.files,
            if usage.files == 1 { "" } else { "s" },
            proxy_cache::format_size(usage.total_bytes),
            proxy_cache::format_size(usage.evictable_bytes())
        )
    }

    /// Recorta la caché al presupuesto. Devuelve el aviso cuando hay algo que
    /// contar, para que quien llame decida si pisa el estado o lo encadena.
    fn enforce_proxy_budget(&mut self) -> Option<String> {
        let limit = (self.proxy_limit_gb.max(0.0) as f64 * 1_000_000_000.0) as u64;
        if limit == 0 {
            return None;
        }
        let eviction =
            proxy_cache::enforce_budget(&self.proxy_root(None), limit, &self.linked_proxies());
        if eviction.over_budget {
            return Some(format!(
                "Los proxies enlazados ocupan {} y el límite es {}. No se ha borrado ninguno en uso.",
                proxy_cache::format_size(eviction.remaining_bytes),
                proxy_cache::format_size(limit)
            ));
        }
        if eviction.removed == 0 {
            return None;
        }
        Some(format!(
            "caché recortada · {} antiguo{} desalojado{} · {} liberados",
            eviction.removed,
            if eviction.removed == 1 { "" } else { "s" },
            if eviction.removed == 1 { "" } else { "s" },
            proxy_cache::format_size(eviction.freed_bytes)
        ))
    }

    fn create_proxy_for_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if self.proxy_result.is_some() {
            self.status = "Ya se está generando un proxy".to_owned();
            return;
        }
        let key = self.clip_job_key(index);
        let source = key.path.clone();
        if !source.is_file() {
            self.status = "El medio original no está disponible".to_owned();
            return;
        }
        let root = self.proxy_root(Some(&source));
        // El nombre depende del medio y de su huella, no del índice del clip:
        // reordenar o borrar clips ya no reasigna proxies a otro vídeo.
        let Some(target) = proxy_cache::proxy_target(&root, &source) else {
            self.status = "No se pudo leer el medio original".to_owned();
            return;
        };
        // Otro clip del mismo archivo ya lo generó: enlazarlo y no recodificar.
        if target.is_file() {
            let before = self.project.clone();
            self.project.clips[index].proxy = Some(target.clone());
            self.finish_edit(before);
            proxy_cache::touch(&target);
            self.status = format!("Proxy reutilizado: {}", target.display());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.proxy_result = Some(receiver);
        self.status = format!(
            "Generando proxy para {}...",
            self.project.clips[index].name()
        );
        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&root)
                .map_err(|error| format!("No se pudo crear la carpeta de proxies: {error}"))
                .and_then(|()| {
                    let output = Command::new(tool_path("ffmpeg.exe"))
                        .args(["-y", "-v", "error", "-i"])
                        .arg(&source)
                        .args([
                            "-vf",
                            "scale=-2:540",
                            "-c:v",
                            "libx264",
                            "-preset",
                            "veryfast",
                            "-crf",
                            "28",
                            "-c:a",
                            "aac",
                            "-b:a",
                            "128k",
                        ])
                        .arg(&target)
                        .creation_flags(CREATE_NO_WINDOW)
                        .output()
                        .map_err(|error| format!("No se pudo iniciar FFmpeg: {error}"))?;
                    if output.status.success() {
                        Ok(target)
                    } else {
                        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
                    }
                });
            let _ = sender.send((key, result));
        });
    }

    fn poll_proxy(&mut self) {
        let Some(receiver) = &self.proxy_result else {
            return;
        };
        let Ok((key, result)) = receiver.try_recv() else {
            return;
        };
        let current = self.clip_job_is_current(&key);
        match result {
            Ok(path) if current => {
                let before = self.project.clone();
                self.project.clips[key.index].proxy = Some(path.clone());
                self.finish_edit(before);
                self.status = format!("Proxy creado: {}", path.display());
                if let Some(trim) = self.enforce_proxy_budget() {
                    self.status = format!("{} · {trim}", self.status);
                }
            }
            Ok(_) => {
                self.status =
                    "Proxy terminado, pero el clip cambió; resultado descartado".to_owned()
            }
            Err(error) => self.status = format!("Fallo al crear proxy: {error}"),
        }
        self.proxy_result = None;
    }

    fn analyze_loudness(&mut self) {
        if self.loudness_result.is_some() || self.project.clips.is_empty() {
            return;
        }
        let prepared = prepare_render_clips(&self.effective_clips());
        let track_gains = self.project.track_gains.clone();
        let master_gain_db = self.project.master_gain_db;
        let timebase = self.project.timebase();
        let generation = self.document_generation;
        let (sender, receiver) = mpsc::channel();
        self.loudness_result = Some(receiver);
        self.status = "Analizando sonoridad del montaje...".to_owned();
        std::thread::spawn(move || {
            let mut command = Command::new(tool_path("ffmpeg.exe"));
            command.args(["-v", "info"]);
            let (indices, titles) =
                push_render_inputs(&mut command, &prepared, (640, 360), false, timebase);
            let result = build_render_filters(
                &prepared,
                &indices,
                &titles,
                (640, 360),
                false,
                true,
                &track_gains,
                master_gain_db,
                false,
                timebase,
                None,
            )
            .and_then(|mut filters| {
                filters.push(
                    "[aout]loudnorm=I=-14:TP=-1:LRA=11:print_format=json[analysis]".to_owned(),
                );
                let output = command
                    .args(["-filter_complex", &filters.join(";")])
                    .args(["-map", "[analysis]", "-f", "null", "NUL"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .map_err(|error| format!("No se pudo iniciar FFmpeg: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    parse_loudness(&stderr)
                } else {
                    Err(stderr.trim().to_owned())
                }
            });
            let _ = sender.send((generation, result));
        });
    }

    fn poll_loudness(&mut self) {
        let Some(receiver) = &self.loudness_result else {
            return;
        };
        let Ok((generation, result)) = receiver.try_recv() else {
            return;
        };
        if generation != self.document_generation {
            self.loudness_result = None;
            self.status = "Medición LUFS descartada porque el proyecto cambió".to_owned();
            return;
        }
        match result {
            Ok(report) => {
                self.status = format!(
                    "Sonoridad: {:.1} LUFS, pico {:.1} dBTP, rango {:.1} LU",
                    report.integrated_lufs, report.true_peak_db, report.range_lu
                );
                self.loudness_report = Some(report);
                self.loudness_report_generation = Some(generation);
            }
            Err(error) => self.status = format!("Fallo al analizar LUFS: {error}"),
        }
        self.loudness_result = None;
    }

    /// Medición LUFS válida para el proyecto tal como está ahora mismo, o
    /// `None` si nunca se midió o el proyecto cambió desde entonces.
    fn current_loudness_measurement(&self) -> Option<&LoudnessReport> {
        if self.loudness_report_generation == Some(self.document_generation) {
            self.loudness_report.as_ref()
        } else {
            None
        }
    }

    fn cut_silences_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if self.silence_result.is_some() {
            return;
        }
        let clip = self.project.clips[index].clone();
        if !clip.has_audio || !clip.path.is_file() {
            self.status = "Selecciona un clip con audio disponible".to_owned();
            return;
        }
        if clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
        {
            self.status = "Quita la rampa de velocidad antes de cortar silencios".to_owned();
            return;
        }
        let key = self.clip_job_key(index);
        let (sender, receiver) = mpsc::channel();
        self.silence_result = Some(receiver);
        self.status = "Detectando silencios...".to_owned();
        std::thread::spawn(move || {
            let output = Command::new(tool_path("ffmpeg.exe"))
                .args(["-v", "info", "-ss", &format_seconds(clip.in_seconds), "-t"])
                .arg(format_seconds(clip.source_duration()))
                .args(["-i"])
                .arg(&clip.path)
                .args([
                    "-af",
                    "silencedetect=noise=-35dB:d=0.4",
                    "-f",
                    "null",
                    "NUL",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|error| format!("No se pudo iniciar FFmpeg: {error}"))
                .and_then(|output| {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if output.status.success() {
                        Ok(parse_silences(&stderr))
                    } else {
                        Err(stderr.trim().to_owned())
                    }
                });
            let _ = sender.send((key, output));
        });
    }

    fn poll_silences(&mut self) {
        let Some(receiver) = &self.silence_result else {
            return;
        };
        let Ok((key, result)) = receiver.try_recv() else {
            return;
        };
        let current = self.clip_job_is_current(&key);
        match result {
            Ok(ranges) if ranges.is_empty() => {
                self.status = "No se detectaron silencios de al menos 0,4 s".to_owned();
            }
            Ok(ranges) if current => {
                self.status = format!(
                    "Detectados {} silencios; revisa y confirma el corte",
                    ranges.len()
                );
                self.pending_silence_cut = Some((key, ranges));
            }
            Ok(_) => {
                self.status = "Análisis descartado porque el clip cambió".to_owned();
            }
            Err(error) => self.status = format!("Fallo al detectar silencios: {error}"),
        }
        self.silence_result = None;
    }

    fn show_silence_review(&mut self, context: &egui::Context) {
        let Some((key, ranges)) = self.pending_silence_cut.clone() else {
            return;
        };
        if !self.clip_job_is_current(&key) {
            self.pending_silence_cut = None;
            return;
        }
        let removed: f64 = ranges.iter().map(|(start, end)| end - start).sum();
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Revisar silencios")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 90.0))
            .show(context, |ui| {
                ui.label(format!(
                    "{} tramos silenciosos, {:.2} s en total.",
                    ranges.len(),
                    removed
                ));
                ui.label("El corte compactará el clip y puede deshacerse con Ctrl+Z.");
                ui.horizontal(|ui| {
                    apply = ui
                        .button("Aplicar corte")
                        .on_hover_text("Quita los silencios propuestos (se puede deshacer)")
                        .clicked();
                    cancel = ui
                        .button("Cancelar")
                        .on_hover_text("Descarta la propuesta")
                        .clicked();
                });
            });
        if apply {
            let before = self.project.clone();
            let segments = without_silences(&self.project.clips[key.index], &ranges);
            let old_duration = self.project.clips[key.index].duration();
            let new_duration: f64 = segments.iter().map(RoughClip::duration).sum();
            let removed_duration = (old_duration - new_duration).max(0.0);
            let removed_clip = self.project.clips[key.index].clone();
            self.project.clips.splice(key.index..=key.index, segments);
            for clip in &mut self.project.clips {
                if clip.track == removed_clip.track
                    && clip.has_video == removed_clip.has_video
                    && clip.timeline_start >= removed_clip.timeline_start + old_duration - 0.001
                {
                    clip.timeline_start = (clip.timeline_start - removed_duration).max(0.0);
                }
            }
            if key.index < self.project.clips.len() {
                self.select_only(key.index);
            } else {
                self.clear_selection();
            }
            self.pending_silence_cut = None;
            self.finish_edit(before);
            self.status = format!("Cortados {} silencios", ranges.len());
        } else if cancel {
            self.pending_silence_cut = None;
            self.status = "Corte de silencios cancelado".to_owned();
        }
    }

    fn detect_scene_cuts_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if self.scene_cut_result.is_some() {
            return;
        }
        let clip = self.project.clips[index].clone();
        if !clip.has_video || clip.title.is_some() || !clip.path.is_file() {
            self.status = "Selecciona un clip de vídeo disponible".to_owned();
            return;
        }
        if clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
        {
            self.status = "Quita la rampa de velocidad antes de detectar escenas".to_owned();
            return;
        }
        let key = self.clip_job_key(index);
        let (sender, receiver) = mpsc::channel();
        self.scene_cut_result = Some(receiver);
        self.status = "Detectando cortes de escena...".to_owned();
        std::thread::spawn(move || {
            let output = Command::new(tool_path("ffmpeg.exe"))
                .args(["-v", "info", "-ss", &format_seconds(clip.in_seconds), "-t"])
                .arg(format_seconds(clip.source_duration()))
                .args(["-i"])
                .arg(&clip.path)
                .args(["-vf", "scdet=threshold=10", "-f", "null", "NUL"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|error| format!("No se pudo iniciar FFmpeg: {error}"))
                .and_then(|output| {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if output.status.success() {
                        Ok(parse_scene_cuts(&stderr))
                    } else {
                        Err(stderr.trim().to_owned())
                    }
                });
            let _ = sender.send((key, output));
        });
    }

    fn poll_scene_cuts(&mut self) {
        let Some(receiver) = &self.scene_cut_result else {
            return;
        };
        let Ok((key, result)) = receiver.try_recv() else {
            return;
        };
        let current = self.clip_job_is_current(&key);
        match result {
            Ok(cuts) if cuts.is_empty() => {
                self.status = "No se detectaron cambios de escena".to_owned();
            }
            Ok(cuts) if current => {
                self.status = format!(
                    "Detectados {} cortes de escena; revisa y confirma la partición",
                    cuts.len()
                );
                self.pending_scene_cut = Some((key, cuts));
            }
            Ok(_) => {
                self.status = "Análisis descartado porque el clip cambió".to_owned();
            }
            Err(error) => self.status = format!("Fallo al detectar escenas: {error}"),
        }
        self.scene_cut_result = None;
    }

    fn show_scene_cut_review(&mut self, context: &egui::Context) {
        let Some((key, cuts)) = self.pending_scene_cut.clone() else {
            return;
        };
        if !self.clip_job_is_current(&key) {
            self.pending_scene_cut = None;
            return;
        }
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Revisar cortes de escena")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 90.0))
            .show(context, |ui| {
                ui.label(format!(
                    "{} cambios de escena detectados. Se partirá el clip en {} planos.",
                    cuts.len(),
                    cuts.len() + 1
                ));
                ui.label("No se descarta metraje; puedes deshacer con Ctrl+Z.");
                ui.horizontal(|ui| {
                    apply = ui
                        .button("Partir en las escenas")
                        .on_hover_text(
                            "Parte el clip en cada cambio de plano detectado (se puede deshacer)",
                        )
                        .clicked();
                    cancel = ui
                        .button("Cancelar")
                        .on_hover_text("Descarta la propuesta")
                        .clicked();
                });
            });
        if apply {
            let before = self.project.clone();
            let segments = split_by_scene_cuts(&self.project.clips[key.index], &cuts);
            let segment_count = segments.len();
            self.project.clips.splice(key.index..=key.index, segments);
            if key.index < self.project.clips.len() {
                self.select_only(key.index);
            } else {
                self.clear_selection();
            }
            self.pending_scene_cut = None;
            self.finish_edit(before);
            self.status = format!("Clip partido en {segment_count} planos");
        } else if cancel {
            self.pending_scene_cut = None;
            self.status = "Detección de escenas cancelada".to_owned();
        }
    }

    fn import_srt(&mut self) {
        let Some(path) = FileDialog::new()
            .add_filter("Subtítulos SRT", &["srt"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|error| format!("No se pudo leer SRT: {error}"))
            .and_then(|content| parse_srt(&content))
        {
            Ok(subtitles) => {
                let count = subtitles.len();
                let before = self.project.clone();
                self.project.subtitles.extend(subtitles);
                self.finish_edit(before);
                self.status = format!("Importados {count} subtítulos");
            }
            Err(error) => self.status = error,
        }
    }

    fn nest_selected_track(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };
        if self.project.clip_locked(&self.project.clips[selected]) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let track = self.project.clips[selected].track;
        let video = self.project.clips[selected].has_video;
        let indices: Vec<usize> = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| clip.track == track && clip.has_video == video)
            .map(|(index, _)| index)
            .collect();
        if indices.len() < 2 {
            self.status = "La pista necesita al menos dos clips para anidarse".to_owned();
            return;
        }
        let before = self.project.clone();
        let start = indices
            .iter()
            .map(|&index| self.project.clips[index].timeline_start)
            .fold(f64::INFINITY, f64::min);
        let mut children: Vec<RoughClip> = indices
            .iter()
            .map(|&index| self.project.clips[index].clone())
            .collect();
        for child in &mut children {
            child.timeline_start -= start;
            child.track = 0;
        }
        let duration = children
            .iter()
            .map(|child| child.timeline_start + child.duration())
            .fold(0.0, f64::max);
        let has_audio = children.iter().any(|child| child.has_audio);
        let has_video = children.iter().any(|child| child.has_video);
        for &index in indices.iter().rev() {
            self.project.clips.remove(index);
        }
        let wrapper = RoughClip {
            out_seconds: duration.max(0.04),
            timeline_start: start,
            track,
            has_video,
            has_audio,
            nested: Some(children),
            ..Default::default()
        };
        self.project.clips.push(wrapper);
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = "Pista convertida en secuencia anidada".to_owned();
    }

    fn unnest_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if self.project.clip_locked(&self.project.clips[index]) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let Some(mut children) = self.project.clips[index].nested.clone() else {
            return;
        };
        let before = self.project.clone();
        let start = self.project.clips[index].timeline_start;
        let track = self.project.clips[index].track;
        for child in &mut children {
            child.timeline_start += start;
            child.track = track.saturating_add(child.track).min(15);
        }
        self.project.clips.splice(index..=index, children);
        if index < self.project.clips.len() {
            self.select_only(index);
        } else {
            self.clear_selection();
        }
        self.finish_edit(before);
        self.status = "Secuencia desanidada".to_owned();
    }

    fn import_nested_project(&mut self) {
        let Some(path) = FileDialog::new()
            .add_filter("Proyecto NovaCut", &["ncrough"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|error| format!("No se pudo leer el proyecto: {error}"))
            .and_then(|json| {
                serde_json::from_str::<RoughProject>(&json)
                    .map_err(|error| format!("Proyecto no válido: {error}"))
            });
        match result {
            Ok(mut project) if !project.clips.is_empty() => {
                resolve_project_paths(&mut project, path.parent());
                project.normalize();
                let first = project
                    .clips
                    .iter()
                    .map(|clip| clip.timeline_start)
                    .fold(f64::INFINITY, f64::min);
                for clip in &mut project.clips {
                    clip.timeline_start -= first;
                }
                let has_video = project.clips.iter().any(|clip| clip.has_video);
                let has_audio = project.clips.iter().any(|clip| clip.has_audio);
                let duration = project.duration();
                let track = if has_video {
                    self.project.video_track_count().min(15)
                } else {
                    self.project.audio_track_count().min(15)
                };
                let dropped_metadata = !project.subtitles.is_empty()
                    || project.subtitle_style.is_some()
                    || project.track_gains.iter().any(|gain| gain.abs() > 0.001)
                    || project.master_gain_db.abs() > 0.001
                    || project.normalize_loudness;
                let before = self.project.clone();
                self.project.clips.push(RoughClip {
                    out_seconds: duration.max(0.04),
                    timeline_start: self.playhead,
                    track,
                    has_video,
                    has_audio,
                    nested: Some(project.clips),
                    ..Default::default()
                });
                self.select_only(self.project.clips.len() - 1);
                self.finish_edit(before);
                self.status = if dropped_metadata {
                    format!(
                        "Secuencia anidada importada: {}. Sus subtítulos y ajustes de mezcla NO se importaron.",
                        path.display()
                    )
                } else {
                    format!("Secuencia anidada importada: {}", path.display())
                };
            }
            Ok(_) => self.status = "El proyecto no contiene clips".to_owned(),
            Err(error) => self.status = error,
        }
    }

    fn transcribe_with_whisper(&mut self) {
        if self.transcription_result.is_some() || self.project.clips.is_empty() {
            return;
        }
        let Some((whisper, model)) = whisper_files() else {
            self.status =
                "Instala whisper-cli.exe y un modelo ggml-*.bin en la carpeta whisper junto a NovaCut"
                    .to_owned();
            return;
        };
        let prepared = prepare_render_clips(&self.project.clips);
        let track_gains = self.project.track_gains.clone();
        let master_gain_db = self.project.master_gain_db;
        let timebase = self.project.timebase();
        let generation = self.document_generation;
        let (sender, receiver) = mpsc::channel();
        self.transcription_result = Some(receiver);
        self.status = "Preparando audio para Whisper...".to_owned();
        std::thread::spawn(move || {
            let result = transcribe_mix(
                &prepared,
                &track_gains,
                master_gain_db,
                timebase,
                &whisper,
                &model,
            );
            let _ = sender.send((generation, result));
        });
    }

    /// Quita del montaje el tiempo de las palabras indicadas en todas las
    /// pistas y cierra el hueco; transcripción, subtítulos y marcadores se
    /// desplazan con él. Es un único paso de deshacer.
    fn delete_transcript_words(&mut self, indices: &[usize], what: &str) {
        let ranges = transcripcion::ranges_for(&self.project.transcript, indices);
        let Some(first) = ranges.first().copied() else {
            self.status = "No hay palabras seleccionadas".to_owned();
            return;
        };
        if self.project.clips.iter().any(|clip| {
            self.project.clip_locked(clip) && clip.timeline_start + clip.duration() > first.0
        }) {
            self.status =
                "Hay pistas bloqueadas después del corte: desbloquéalas para editar por texto"
                    .to_owned();
            return;
        }
        let clips = match transcripcion::extract_ranges(&self.project.clips, &ranges) {
            Ok(clips) => clips,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let before = self.project.clone();
        let removed: f64 = ranges.iter().map(|(start, end)| end - start).sum();
        self.project.clips = clips;
        self.project.transcript = transcripcion::remap_words(&self.project.transcript, &ranges);
        self.project.subtitles = transcripcion::remap_subtitles(&self.project.subtitles, &ranges);
        self.project.markers = self
            .project
            .markers
            .iter()
            .filter_map(|marker| {
                Some(Marker {
                    time: transcripcion::remap_time(marker.time, &ranges)?,
                    name: marker.name.clone(),
                })
            })
            .collect();
        self.refresh_caption_clips();
        self.playhead = transcripcion::remap_time(first.0, &ranges).unwrap_or(first.0);
        self.transcript_selection = None;
        self.finish_edit(before);
        self.request_preview();
        self.status = format!(
            "{what}: {} palabra(s), {} menos de montaje",
            indices.len(),
            format_clock(removed)
        );
    }

    fn delete_transcript_selection(&mut self) {
        let Some((anchor, cursor)) = self.transcript_selection else {
            self.status = "Selecciona palabras en la transcripción".to_owned();
            return;
        };
        let indices: Vec<usize> = (anchor.min(cursor)..=anchor.max(cursor)).collect();
        self.delete_transcript_words(&indices, "Texto borrado");
    }

    fn remove_fillers(&mut self) {
        let fillers = transcripcion::filler_indices(&self.project.transcript);
        if fillers.is_empty() {
            self.status = "No se encontraron muletillas".to_owned();
            return;
        }
        self.delete_transcript_words(&fillers, "Muletillas quitadas");
    }

    /// Rehace las capas de subtítulos animados que siguen la transcripción:
    /// tras una edición por texto su contenido y su duración cambian.
    fn refresh_caption_clips(&mut self) {
        let followers: Vec<usize> = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.title
                    .as_ref()
                    .and_then(|title| title.style.captions.as_ref())
                    .is_some_and(|captions| captions.follow_transcript)
            })
            .map(|(index, _)| index)
            .collect();
        let Some(&template_index) = followers.first() else {
            return;
        };
        let mut template = self.project.clips[template_index].clone();
        for index in followers.into_iter().rev() {
            self.project.clips.remove(index);
        }
        let (Some(first), Some(last)) = (
            self.project.transcript.first(),
            self.project.transcript.last(),
        ) else {
            return;
        };
        let start = first.start;
        let end = last.end + 0.3;
        template.timeline_start = start;
        template.in_seconds = 0.0;
        template.out_seconds = end - start;
        template.speed = 1.0;
        template.fade_in_seconds = 0.0;
        template.fade_out_seconds = 0.0;
        template.transition = None;
        if let Some(captions) = template
            .title
            .as_mut()
            .and_then(|title| title.style.captions.as_mut())
        {
            captions.words = subtitulos_animados::local_words(&self.project.transcript, start, end);
        }
        self.project.clips.push(template);
    }

    /// Crea (o selecciona, si ya existe) la capa de subtítulos animados.
    fn create_animated_captions(&mut self) {
        if let Some(index) = self.project.clips.iter().position(|clip| {
            clip.title
                .as_ref()
                .is_some_and(|title| title.style.captions.is_some())
        }) {
            self.select_only(index);
            self.status =
                "Los subtítulos animados ya existen; edita su estilo en el inspector".to_owned();
            return;
        }
        if self.project.transcript.is_empty() {
            self.status = "Primero transcribe el montaje con Whisper".to_owned();
            return;
        }
        let before = self.project.clone();
        let track = self.project.video_track_count().min(15);
        self.project.clips.push(RoughClip {
            out_seconds: 1.0,
            track,
            has_audio: false,
            title: Some(Titulo {
                position_y: 0.78,
                size: 72.0,
                style: efectos::TitleStyle {
                    captions: Some(subtitulos_animados::AnimatedCaptions::default()),
                    ..Default::default()
                },
                ..Titulo::default()
            }),
            ..Default::default()
        });
        self.refresh_caption_clips();
        let index = self.project.clips.len() - 1;
        self.select_only(index);
        self.bottom_tab = BottomTab::Inspector;
        self.finish_edit(before);
        self.request_preview();
        self.status = "Subtítulos animados creados sobre todas las pistas".to_owned();
    }

    fn poll_transcription(&mut self) {
        let Some(receiver) = &self.transcription_result else {
            return;
        };
        let Ok((generation, result)) = receiver.try_recv() else {
            return;
        };
        if generation != self.document_generation {
            self.transcription_result = None;
            self.status = "Transcripción descartada porque el proyecto cambió".to_owned();
            return;
        }
        match result {
            Ok(words) => {
                let before = self.project.clone();
                self.project.subtitles = transcripcion::subtitles_from_words(&words);
                let count = self.project.subtitles.len();
                let word_count = words.len();
                self.project.transcript = words;
                self.transcript_selection = None;
                self.refresh_caption_clips();
                self.finish_edit(before);
                self.bottom_tab = BottomTab::Transcript;
                self.bottom_open = true;
                self.status = format!(
                    "Whisper transcribió {word_count} palabras ({count} subtítulos); edita el montaje desde la pestaña Transcripción"
                );
            }
            Err(error) => self.status = format!("Fallo de transcripción: {error}"),
        }
        self.transcription_result = None;
    }

    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.import_paths_to(paths, None);
    }

    fn import_paths_to(&mut self, paths: Vec<PathBuf>, target: Option<ImportTarget>) {
        if paths.is_empty() {
            return;
        }
        if self.import_result.is_some() {
            self.pending_drop_paths.extend(paths);
            if target.is_none() {
                self.pending_drop_position = None;
            }
            return;
        }
        let generation = self.document_generation;
        let (sender, receiver) = mpsc::channel();
        self.import_result = Some(receiver);
        self.status = format!("Analizando {} medio(s)...", paths.len());
        std::thread::spawn(move || {
            let results: Vec<MediaProbeResult> = paths
                .into_iter()
                .map(|path| {
                    let info = if is_image_file(&path) {
                        Ok(MediaProbe {
                            duration: DEFAULT_IMAGE_DURATION,
                            has_video: true,
                            has_audio: false,
                            frame_rate: None,
                            variable_frame_rate: false,
                            source_pts: None,
                        })
                    } else {
                        probe_media(&path)
                    };
                    (path, info)
                })
                .collect();
            let _ = sender.send((generation, target, results));
        });
    }

    fn poll_import(&mut self) {
        let Some(receiver) = &self.import_result else {
            return;
        };
        let Ok((generation, target, results)) = receiver.try_recv() else {
            return;
        };
        self.import_result = None;
        if generation != self.document_generation {
            self.status = "Importación descartada porque el proyecto cambió".to_owned();
            return;
        }
        let before = self.project.clone();
        let mut append_at = self.project.duration();
        let mut imported = 0;
        let mut variable_frame_rate = 0;
        let mut errors: Vec<String> = Vec::new();
        let mut clips_to_add = Vec::new();
        for (path, result) in results {
            match result {
                Ok(probe) if probe.duration > 0.0 && (probe.has_video || probe.has_audio) => {
                    if probe.variable_frame_rate {
                        variable_frame_rate += 1;
                    }
                    let mut clip = RoughClip {
                        path,
                        in_seconds: 0.0,
                        out_seconds: probe.duration,
                        source_duration_seconds: Some(probe.duration),
                        source_timebase: probe.frame_rate,
                        source_vfr: probe.variable_frame_rate,
                        source_pts: probe.source_pts,
                        has_video: probe.has_video,
                        has_audio: probe.has_audio,
                        speed: 1.0,
                        timeline_start: 0.0,
                        track: 0,
                        gain_db: 0.0,
                        muted: false,
                        pan: 0.0,
                        position_x: 0.0,
                        position_y: 0.0,
                        scale_percent: 100.0,
                        rotation: 0.0,
                        opacity: 100.0,
                        fade_in_seconds: 0.0,
                        fade_out_seconds: 0.0,
                        title: None,
                        is_adjustment: false,
                        exposure: 0.0,
                        contrast: 0.0,
                        saturation: 0.0,
                        vignette: 0.0,
                        transition: None,
                        transition_duration: 0.5,
                        label: 0,
                        blur: 0.0,
                        wheels: None,
                        chroma: None,
                        curves: None,
                        keyframes: None,
                        fusion: Fusion::Normal,
                        mask: None,
                        lut: None,
                        proxy: None,
                        speed_ramp: None,
                        fx: efectos::ClipFx::default(),
                        runtime: efectos::TransitionRuntime::default(),
                        nested: None,
                        freeze_at: None,
                        enabled: true,
                    };
                    if let Some(target) = target {
                        clip.timeline_start = target.timeline_start;
                        clip.track = target.track;
                        clip.has_video = target.is_video;
                        clip.has_audio = !target.is_video;
                    } else {
                        clip.timeline_start = append_at;
                    }
                    append_at += probe.duration;
                    clips_to_add.push(clip);
                    imported += 1;
                }
                Ok(_) => errors.push("FFprobe no devolvio una duracion valida".to_owned()),
                Err(error) => errors.push(error),
            }
        }
        if imported > 0 {
            self.project.clips.extend(clips_to_add);
            self.select_only(self.project.clips.len() - 1);
            self.status = if variable_frame_rate > 0 {
                format!(
                    "{imported} medio(s) importado(s) · {variable_frame_rate} con VFR detectado"
                )
            } else {
                format!("{imported} medio(s) importado(s)")
            };
            self.finish_edit(before);
        } else {
            self.status = if errors.is_empty() {
                "No se encontraron medios validos".to_owned()
            } else {
                errors.join(" · ")
            };
        }
    }

    fn save_project(&mut self, choose_path: bool) {
        // Confirma cualquier edición en curso primero para que el archivo
        // guardado y el historial de undo queden consistentes.
        self.flush_pending_edit();
        let path = if choose_path || self.project_path.is_none() {
            let Some(path) = FileDialog::new()
                .add_filter("Proyecto NovaCut Windows", &["ncrough"])
                .set_file_name("montaje.ncrough")
                .save_file()
            else {
                return;
            };
            path
        } else {
            self.project_path.clone().expect("checked above")
        };

        let stored_project = project_for_storage(&self.project, &path);
        match serde_json::to_string_pretty(&stored_project)
            .map_err(|error| error.to_string())
            .and_then(|json| write_text_atomically(&path, &json))
        {
            Ok(()) => {
                save_backup(&path, &stored_project);
                self.project_path = Some(path.clone());
                self.dirty = false;
                self.clean_project_json = serde_json::to_string(&self.project).ok();
                self.push_recent(&path);
                self.pending_recovery = None;
                clear_recovery();
                self.status = "Proyecto guardado".to_owned();
            }
            Err(error) => self.status = format!("No se pudo guardar: {error}"),
        }
    }

    fn open_project(&mut self) {
        let Some(path) = FileDialog::new()
            .add_filter("Proyecto NovaCut Windows", &["ncrough"])
            .add_filter("Proyecto Editorcito macOS", &["editorcito"])
            .pick_file()
        else {
            return;
        };
        self.load_project_from(path);
    }

    fn load_project_from(&mut self, path: PathBuf) {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("editorcito"))
        {
            self.import_mac_project(path);
            return;
        }
        let result = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|json| {
                serde_json::from_str::<RoughProject>(&json).map_err(|error| error.to_string())
            });
        match result {
            Ok(mut project) if project.version == 1 || project.version == 2 => {
                resolve_project_paths(&mut project, path.parent());
                project.normalize();
                self.cancel_document_jobs();
                self.project = project;
                self.project_path = Some(path.clone());
                self.clear_selection();
                self.playhead = 0.0;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.preview_texture = None;
                self.preview_result = None;
                self.preview_refresh_pending = false;
                self.dirty = false;
                self.clean_project_json = serde_json::to_string(&self.project).ok();
                self.pending_edit = None;
                self.document_generation = self.document_generation.wrapping_add(1);
                self.pending_recovery = None;
                clear_recovery();
                self.push_recent(&path);
                self.status = "Proyecto abierto".to_owned();
                // El monitor muestra ya el primer fotograma: antes esperaba a
                // un clic en la timeline y parecía vacío.
                if self.ffmpeg_ready && !self.project.clips.is_empty() {
                    self.request_preview();
                }
            }
            Ok(_) => self.status = "Versión de proyecto no compatible".to_owned(),
            Err(error) => self.status = format!("No se pudo abrir: {error}"),
        }
    }

    fn import_mac_project(&mut self, path: PathBuf) {
        let result = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|json| {
                serde_json::from_str::<MacProject>(&json).map_err(|error| error.to_string())
            });
        let mac = match result {
            Ok(project) => project,
            Err(error) => {
                self.status = format!("No se pudo abrir el proyecto Mac: {error}");
                return;
            }
        };
        let fps = f64::from(mac.montaje.timebase.numerador)
            / f64::from(mac.montaje.timebase.denominador.max(1));
        let timebase = Timebase::new(
            mac.montaje.timebase.numerador.max(1) as u32,
            mac.montaje.timebase.denominador.max(1) as u32,
            mac.montaje.timebase.drop_frame,
        )
        .unwrap_or_else(|_| Timebase::from_fps(fps));
        let project_directory = path.parent();
        let media: HashMap<String, PathBuf> = mac
            .medios
            .iter()
            .map(|item| {
                let path = resolve_mac_media(item, project_directory).unwrap_or_else(|| {
                    project_directory
                        .and_then(|directory| {
                            item.ruta_relativa
                                .as_deref()
                                .map(|relative| directory.join(relative))
                        })
                        .unwrap_or_else(|| PathBuf::from(&item.ruta))
                });
                (item.id.clone(), path)
            })
            .collect();
        let video_tracks: Vec<&MacTrack> = mac
            .montaje
            .pistas
            .iter()
            .filter(|track| track.tipo == "video")
            .collect();
        if video_tracks.is_empty() {
            self.status = "El proyecto Mac no contiene una pista de video importable".to_owned();
            return;
        }
        let mut clips = Vec::new();
        let mut offline = 0;
        let mut unsupported = 0;
        for (mac_track_index, track) in video_tracks.iter().enumerate() {
            let windows_track = video_tracks.len() - mac_track_index - 1;
            for clip in &track.clips {
                if !clip.habilitado || clip.es_ajuste {
                    unsupported += 1;
                    continue;
                }
                let title = if clip.es_titulo {
                    match clip.titulo.as_ref() {
                        Some(t)
                            if t.forma.as_deref().unwrap_or("texto") == "texto"
                                && !t.texto.trim().is_empty() =>
                        {
                            Some(Titulo {
                                text: t.texto.clone(),
                                position_x: t.position_x.clamp(0.0, 1.0),
                                position_y: t.position_y.clamp(0.0, 1.0),
                                size: t.tamano.max(8.0),
                                red: t.rojo.clamp(0.0, 1.0),
                                green: t.verde.clamp(0.0, 1.0),
                                blue: t.azul.clamp(0.0, 1.0),
                                style: efectos::TitleStyle::default(),
                            })
                        }
                        _ => {
                            unsupported += 1;
                            continue;
                        }
                    }
                } else {
                    None
                };
                let speed = clip.velocidad.abs().clamp(0.1, 8.0);
                let (
                    media_path,
                    source_in,
                    has_audio,
                    source_duration_seconds,
                    source_timebase,
                    source_vfr,
                    source_pts,
                ) = if clip.es_titulo {
                    (PathBuf::new(), 0.0, false, None, None, false, None)
                } else {
                    let Some(media_path) = media.get(&clip.media_id) else {
                        offline += 1;
                        continue;
                    };
                    if !media_path.exists() {
                        offline += 1;
                    }
                    let media_info = probe_media(media_path).ok();
                    let has_audio = media_info
                        .as_ref()
                        .map(|info| info.has_audio)
                        .unwrap_or(true);
                    let source_duration = media_info.as_ref().map(|info| info.duration);
                    let source_timebase = media_info.as_ref().and_then(|info| info.frame_rate);
                    let source_vfr = media_info
                        .as_ref()
                        .is_some_and(|info| info.variable_frame_rate);
                    (
                        media_path.clone(),
                        clip.source_in as f64 / fps,
                        has_audio,
                        source_duration,
                        source_timebase,
                        source_vfr,
                        media_info.as_ref().and_then(|info| info.source_pts.clone()),
                    )
                };
                clips.push(RoughClip {
                    path: media_path,
                    in_seconds: source_in,
                    out_seconds: source_in + clip.duracion as f64 / fps * speed,
                    source_duration_seconds,
                    source_timebase,
                    source_vfr,
                    source_pts,
                    has_video: true,
                    has_audio,
                    speed: if clip.es_titulo { 1.0 } else { speed },
                    timeline_start: clip.inicio as f64 / fps,
                    track: windows_track,
                    gain_db: clip.ganancia.clamp(-96.0, 24.0),
                    muted: false,
                    pan: 0.0,
                    position_x: clip.transformacion.position_x,
                    position_y: clip.transformacion.position_y,
                    scale_percent: clip.transformacion.scale_percent.clamp(1.0, 800.0),
                    rotation: clip.transformacion.rotation,
                    opacity: clip.transformacion.opacity.clamp(0.0, 100.0),
                    fade_in_seconds: clip.fade_in_frames.max(0) as f64 / fps,
                    fade_out_seconds: clip.fade_out_frames.max(0) as f64 / fps,
                    title,
                    is_adjustment: false,
                    exposure: clip.color.exposure.clamp(-4.0, 4.0),
                    contrast: clip.color.contrast.clamp(-1.0, 3.0),
                    saturation: clip.color.saturation.clamp(-1.0, 3.0),
                    vignette: clip.color.vignette.clamp(0.0, 1.0),
                    blur: clip.color.blur.clamp(0.0, 1.0),
                    wheels: clip.color.ruedas.as_ref().map(|w| Wheels {
                        shadows_r: w.shadows_r.clamp(-1.0, 1.0),
                        shadows_g: w.shadows_g.clamp(-1.0, 1.0),
                        shadows_b: w.shadows_b.clamp(-1.0, 1.0),
                        mid_r: w.mid_r.clamp(-1.0, 1.0),
                        mid_g: w.mid_g.clamp(-1.0, 1.0),
                        mid_b: w.mid_b.clamp(-1.0, 1.0),
                        high_r: w.high_r.clamp(-1.0, 1.0),
                        high_g: w.high_g.clamp(-1.0, 1.0),
                        high_b: w.high_b.clamp(-1.0, 1.0),
                    }),
                    chroma: clip.color.croma.as_ref().map(|c| Chroma {
                        red: c.rojo.clamp(0.0, 1.0),
                        green: c.verde.clamp(0.0, 1.0),
                        blue: c.azul.clamp(0.0, 1.0),
                        tolerance: c.tolerance.clamp(0.0, 1.0),
                        smooth: c.smooth.clamp(0.0, 1.0),
                        spill: c.spill.clamp(0.0, 1.0),
                    }),
                    curves: clip.color.curvas.as_ref().map(|curvas| {
                        let convert = |points: &[MacPunto]| {
                            points
                                .iter()
                                .map(|point| CurvePoint {
                                    x: point.x.clamp(0.0, 1.0),
                                    y: point.y.clamp(0.0, 1.0),
                                })
                                .collect()
                        };
                        Curves {
                            luma: convert(&curvas.luma),
                            red: convert(&curvas.rojo),
                            green: convert(&curvas.verde),
                            blue: convert(&curvas.azul),
                        }
                    }),
                    keyframes: None,
                    fusion: clip.fusion,
                    mask: clip.mask,
                    lut: None,
                    proxy: None,
                    speed_ramp: None,
                    fx: efectos::ClipFx::default(),
                    runtime: efectos::TransitionRuntime::default(),
                    nested: None,
                    freeze_at: None,
                    transition: None,
                    transition_duration: default_transition_duration(),
                    label: 0,
                    enabled: true,
                });
            }
        }
        if clips.is_empty() {
            self.status = "No se localizaron clips compatibles del proyecto Mac".to_owned();
            return;
        }
        self.cancel_document_jobs();
        self.project = RoughProject {
            version: 2,
            name: mac
                .nombre
                .unwrap_or_else(|| "Proyecto importado de Mac".to_owned()),
            clips,
            fps: timebase.fps(),
            timebase: Some(timebase),
            ..RoughProject::default()
        };
        self.project_path = None;
        self.clear_selection();
        self.playhead = 0.0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = true;
        self.clean_project_json = None;
        self.pending_edit = None;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.preview_texture = None;
        self.preview_result = None;
        self.preview_refresh_pending = false;
        self.pending_recovery = None;
        clear_recovery();
        self.status = format!(
            "Proyecto Mac importado: {} pista(s), {offline} medio(s) offline, {unsupported} clip(s) especiales pendientes",
            video_tracks.len()
        );
    }

    fn install_ffmpeg(&mut self) {
        if self.setup_result.is_some() {
            return;
        }
        let Some(app_dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        else {
            self.status = "No se pudo localizar la carpeta de la aplicacion".to_owned();
            return;
        };
        let (sender, receiver) = mpsc::channel();
        self.setup_result = Some(receiver);
        self.status = "Descargando FFmpeg (~80 MB). Esto puede tardar varios minutos...".to_owned();
        std::thread::spawn(move || {
            let result = if winget_available() {
                match run_winget_install() {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        let _ = sender.send(Err(format!(
                            "WinGet fallo ({error}); intentando descarga directa..."
                        )));
                        run_powershell_install(&app_dir)
                    }
                }
            } else {
                run_powershell_install(&app_dir)
            };
            let _ = sender.send(result);
        });
    }

    fn open_ffmpeg_download(&mut self, context: &egui::Context) {
        context.open_url(egui::OpenUrl {
            url: "https://www.gyan.dev/ffmpeg/builds/".to_owned(),
            new_tab: true,
        });
        self.status =
            "Descarga el build 'release essentials' y copia ffmpeg.exe, ffprobe.exe y ffplay.exe junto a novacut-windows.exe"
                .to_owned();
    }

    fn preview_selected(&mut self) {
        let Some(clip) = self
            .selected
            .and_then(|index| self.project.clips.get(index))
        else {
            self.status = "Selecciona un clip".to_owned();
            return;
        };
        let speed = clip.speed.clamp(0.1, 8.0);
        let mut command = Command::new(tool_path("ffplay.exe"));
        command
            .args(["-autoexit", "-ss", &format_seconds(clip.in_seconds)])
            .args(["-t", &format_seconds(clip.source_duration())])
            .arg(&clip.path);
        if (speed - 1.0).abs() > 0.0001 {
            if clip.has_video {
                command.args(["-vf", &format!("setpts=PTS/{speed:.6}")]);
            }
            if clip.has_audio {
                command.args(["-af", &atempo_filter(speed)]);
            }
        }
        let result = command
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        self.status = match result {
            Ok(_) => format!("Previsualizando {}", clip.name()),
            Err(error) => format!("No se pudo abrir FFplay: {error}"),
        };
    }

    /// Umbral de snapping: 0 cuando está desactivado, ~3 píxeles en tiempo
    /// según el zoom actual.
    /// Tolerancia del imán en segundos. Se calcula en píxeles (ocho, como en
    /// la app macOS) y se traduce con la escala real de la timeline, para que
    /// no dependa de la duración del montaje.
    fn snap_tolerance(&self) -> f64 {
        if !self.snap_enabled {
            return 0.0;
        }
        if self.timeline_pps > 0.001 {
            8.0 / self.timeline_pps
        } else {
            0.2
        }
    }

    fn request_preview(&mut self) {
        if self.preview_result.is_some() {
            self.preview_refresh_pending = true;
            return;
        }
        let timebase = self.project.timebase();
        let resolved = prepare_render_clips(&self.effective_clips());
        let mut active: Vec<(usize, RoughClip, f64)> = resolved
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.has_video
                    && self.playhead >= clip.timeline_start
                    && self.playhead < clip.timeline_start + clip.duration()
            })
            .map(|(index, clip)| {
                let advanced = (self.playhead - clip.timeline_start).clamp(0.0, clip.duration())
                    * clip.speed.clamp(0.1, 8.0);
                let source_time = if clip.fx.reverse {
                    (clip.out_seconds - advanced - 0.04).max(clip.in_seconds)
                } else {
                    clip.in_seconds + advanced
                };
                let mut clip = clip.clone();
                // Transiciones de movimiento: el monitor compone un único
                // fotograma, así que basta con desplazar la capa.
                let (offset_x, offset_y) = clip.runtime.offset_at(self.playhead);
                clip.position_x += offset_x * 1920.0;
                clip.position_y += offset_y * 1080.0;
                (index, clip, source_time)
            })
            .collect();
        active.sort_by(|left, right| {
            left.1
                .track
                .cmp(&right.1.track)
                .then_with(|| left.1.timeline_start.total_cmp(&right.1.timeline_start))
        });
        let Some(_) = active.last() else {
            let (sender, receiver) = mpsc::channel();
            self.preview_result = Some(receiver);
            let _ = sender.send(Ok(PreviewFrame {
                pixels: vec![0; 640 * 360 * 4],
                width: 640,
                height: 360,
            }));
            return;
        };
        let mut sources: Vec<(RoughClip, f64)> = active
            .into_iter()
            .map(|(_, clip, source_time)| (clip, source_time))
            .collect();
        if self.use_proxies {
            for (clip, _) in &mut sources {
                if let Some(proxy) = clip.proxy.as_ref().filter(|path| path.is_file()) {
                    clip.path = proxy.clone();
                }
            }
        }
        // Keyframes evaluados exactamente en el cabezal para el monitor.
        for (clip, _) in &mut sources {
            if clip.keyframes.is_some() {
                let local = (self.playhead - clip.timeline_start).max(0.0);
                let (x, y, scale, opacity) = clip.evaluate_transform(local);
                clip.position_x = x;
                clip.position_y = y;
                clip.scale_percent = scale.clamp(1.0, 800.0);
                clip.opacity = opacity.clamp(0.0, 100.0);
            }
        }
        // Los fundidos se ven en el monitor multiplicando la opacidad efectiva.
        for (clip, _) in &mut sources {
            let local = self.playhead - clip.timeline_start;
            let duration = clip.duration();
            if duration > 0.0 {
                let factor_in = if clip.fade_in_seconds > 0.0 {
                    (local / clip.fade_in_seconds).min(1.0)
                } else {
                    1.0
                };
                let factor_out = if clip.fade_out_seconds > 0.0 {
                    ((duration - local) / clip.fade_out_seconds).min(1.0)
                } else {
                    1.0
                };
                clip.opacity *= factor_in * factor_out;
            }
        }
        let (sender, receiver) = mpsc::channel();
        self.preview_result = Some(receiver);
        self.preview_refresh_pending = false;
        std::thread::spawn(move || {
            let result = render_preview_frame(&sources, timebase);
            let _ = sender.send(result);
        });
    }

    fn split_at_playhead(&mut self) {
        let contains_playhead = |clip: &RoughClip| {
            let local = self.playhead - clip.timeline_start;
            clip.nested.is_none()
                && clip
                    .speed_ramp
                    .as_ref()
                    .is_none_or(|points| points.is_empty())
                && local > 0.04
                && local < clip.duration() - 0.04
        };
        let index = self
            .selected
            .filter(|index| contains_playhead(&self.project.clips[*index]))
            .or_else(|| {
                self.project
                    .clips
                    .iter()
                    .enumerate()
                    .filter(|(_, clip)| contains_playhead(clip))
                    .max_by_key(|(_, clip)| clip.track)
                    .map(|(index, _)| index)
            });
        if let Some(index) = index {
            if self.project.clip_locked(&self.project.clips[index]) {
                self.status = "La pista está bloqueada".to_owned();
                return;
            }
            let before = self.project.clone();
            let local = self.playhead - self.project.clips[index].timeline_start;
            let source_split = self.project.clips[index].in_seconds
                + local * self.project.clips[index].speed.clamp(0.1, 8.0);
            let mut right = self.project.clips[index].clone();
            self.project.clips[index].out_seconds = source_split;
            right.in_seconds = source_split;
            right.timeline_start = self.playhead;
            self.project.clips.insert(index + 1, right);
            self.select_only(index + 1);
            self.finish_edit(before);
            self.status = "Clip partido en el cabezal".to_owned();
            return;
        }
        self.status = if self.project.clips.iter().any(|clip| {
            clip.speed_ramp
                .as_ref()
                .is_some_and(|points| !points.is_empty())
                && self.playhead > clip.timeline_start
                && self.playhead < clip.timeline_start + clip.duration()
        }) {
            "Quita la rampa de velocidad antes de partir el clip".to_owned()
        } else {
            "Coloca el cabezal dentro de un clip para partirlo".to_owned()
        };
    }

    /// Corte directo de la herramienta Tijeras, independiente del cabezal.
    fn split_clip_at(&mut self, index: usize, time: f64) {
        let Some(clip) = self.project.clips.get(index).cloned() else {
            return;
        };
        if self.project.clip_locked(&clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if clip.nested.is_some()
            || clip
                .speed_ramp
                .as_ref()
                .is_some_and(|points| !points.is_empty())
        {
            self.status = "Quita la rampa o desanida antes de usar las tijeras".to_owned();
            return;
        }
        let local = time - clip.timeline_start;
        if local <= 0.04 || local >= clip.duration() - 0.04 {
            self.status = "Pulsa dentro del clip, lejos de sus bordes".to_owned();
            return;
        }
        let cut = clip.timeline_start + local;
        let source_cut = clip.in_seconds + local * clip.speed.clamp(0.1, 8.0);
        let before = self.project.clone();
        let mut right = clip;
        self.project.clips[index].out_seconds = source_cut;
        self.project.clips[index].fade_out_seconds = 0.0;
        right.in_seconds = source_cut;
        right.timeline_start = cut;
        right.fade_in_seconds = 0.0;
        right.transition = None;
        self.project.clips.insert(index + 1, right);
        self.playhead = cut;
        self.select_only(index + 1);
        self.finish_edit(before);
        self.status = format!("Corte en {}", timecode(cut, self.project.fps));
    }

    /// La varita inicia el análisis apropiado para el medio y conserva la fase
    /// de revisión existente antes de aplicar cambios destructivos.
    fn run_magic_tool(&mut self, index: usize) {
        let Some(clip) = self.project.clips.get(index) else {
            return;
        };
        if self.project.clip_locked(clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let has_video = clip.has_video;
        let has_audio = clip.has_audio;
        let generated =
            clip.title.is_some() || clip.is_adjustment || clip.path.as_os_str().is_empty();
        self.select_only(index);
        if generated {
            self.quick_fade();
        } else if has_video {
            self.detect_scene_cuts_selected();
        } else if has_audio {
            self.cut_silences_selected();
        } else {
            self.status = "La varita no encontró una acción compatible".to_owned();
        }
    }

    fn export(&mut self) {
        if self.project.clips.is_empty() {
            self.status = "Importa al menos un clip".to_owned();
            return;
        }
        if self
            .project
            .clips
            .iter()
            .any(|clip| clip.duration() <= 0.01)
        {
            self.status = "Todos los clips necesitan una salida posterior a la entrada".to_owned();
            return;
        }
        let audio_only = self.export_format.is_audio_only();
        let Some(output) = FileDialog::new()
            .add_filter(
                self.export_format.label(),
                &[self.export_format.extension()],
            )
            .set_file_name(self.export_format.default_file_name())
            .save_file()
        else {
            return;
        };

        let clips = self.export_clips();
        if clips.is_empty() {
            self.status = "El rango de trabajo no contiene ningún clip".to_owned();
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = Arc::clone(&cancel);
        let progress = Arc::clone(&self.render_progress);
        if let Ok(mut state) = progress.lock() {
            state.pct = 0.0;
            state.eta_secs = 0.0;
        }
        let size = self.export_size;
        let format = self.export_format;
        let track_gains = self.project.track_gains.clone();
        let master_gain_db = self.project.master_gain_db;
        let normalize_loudness = self.project.normalize_loudness;
        let timebase = self.project.timebase();
        let measured_loudness = self.current_loudness_measurement().cloned();
        let hw = self.active_hw();
        if let Ok(mut state) = self.render_progress.lock() {
            state.note = None;
        }
        let (sender, receiver) = mpsc::channel();
        self.export_result = Some(receiver);
        self.export_cancel = Some(cancel);
        self.status = if audio_only {
            format!("Exportando audio {}...", format.extension().to_uppercase())
        } else {
            format!(
                "Exportando {} {}x{}...",
                format.short_name(),
                size.0,
                size.1
            )
        };
        std::thread::spawn(move || {
            let result = run_export(
                &clips,
                &output,
                &thread_cancel,
                false,
                size,
                audio_only,
                format,
                &track_gains,
                master_gain_db,
                normalize_loudness,
                timebase,
                measured_loudness.as_ref(),
                hw,
                &progress,
            )
            .map(|()| output);
            let _ = sender.send(result);
        });
    }

    fn start_hw_detection(&mut self) {
        if !self.ffmpeg_ready || self.hw_detect.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.hw_detect = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(aceleracion::detect(&tool_path("ffmpeg.exe")));
        });
    }

    fn poll_hw_detection(&mut self) {
        let Some(receiver) = &self.hw_detect else {
            if self.ffmpeg_ready && self.hw_backends.is_empty() {
                // FFmpeg pudo instalarse después de arrancar.
                self.start_hw_detection();
            }
            return;
        };
        if let Ok(backends) = receiver.try_recv() {
            self.hw_backends = backends;
            // Se deja el receptor consumido en su sitio para no repetir.
            self.hw_detect = Some(mpsc::channel().1);
        }
    }

    /// GPU que usará la próxima exportación H.264/HEVC, si procede.
    fn active_hw(&self) -> Option<aceleracion::HwBackend> {
        self.hardware_encoding
            .then(|| self.hw_backends.first().copied())
            .flatten()
    }

    fn poll_export(&mut self) {
        let Some(receiver) = &self.export_result else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            let note = self
                .render_progress
                .lock()
                .ok()
                .and_then(|mut state| state.note.take())
                .map(|note| format!(" ({note})"))
                .unwrap_or_default();
            self.status = match result {
                Ok(path) => format!("Exportado: {}{note}", path.display()),
                Err(error) if error == "Exportación cancelada" => error,
                Err(error) => format!("Fallo al exportar: {error}"),
            };
            self.export_result = None;
            self.export_cancel = None;
        }
    }

    /// Crea un clip de título en el cabezal, sobre la pista de vídeo superior.
    fn add_title_at_playhead(&mut self) {
        let before = self.project.clone();
        let track = self.project.video_track_count().saturating_sub(1).min(15);
        self.project.clips.push(RoughClip {
            out_seconds: 5.0,
            timeline_start: self.playhead.min(self.project.duration()),
            track,
            has_audio: false,
            title: Some(Titulo::default()),
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = "Titulo creado; edita su texto en el inspector".to_owned();
    }

    /// Título con estilo predefinido (rótulo inferior, mate de color…) en el
    /// cabezal. El mate va a la pista más baja libre para quedar de fondo;
    /// los rótulos, encima de todo.
    fn add_styled_title_at_playhead(&mut self, title: Titulo, status: &str) {
        let before = self.project.clone();
        let is_matte = title.style.matte.is_some() && title.text.trim().is_empty();
        let start = self.playhead.min(self.project.duration());
        let track = if is_matte {
            free_track(&self.project.clips, true, 0, start, start + 5.0)
        } else {
            self.project.video_track_count().min(15)
        };
        self.project.clips.push(RoughClip {
            out_seconds: 5.0,
            timeline_start: start,
            track,
            has_audio: false,
            title: Some(title),
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = status.to_owned();
    }

    /// Añade una capa de ajuste en la pista de vídeo más alta, para que
    /// gradúe todo lo compuesto por debajo (como en Premiere/DaVinci).
    fn add_adjustment_layer_at_playhead(&mut self) {
        let before = self.project.clone();
        let track = self.project.video_track_count().min(15);
        self.project.clips.push(RoughClip {
            out_seconds: 5.0,
            timeline_start: self.playhead.min(self.project.duration()),
            track,
            has_audio: false,
            is_adjustment: true,
            ..Default::default()
        });
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status =
            "Capa de ajuste creada en la pista superior; edita sus efectos en el inspector"
                .to_owned();
    }

    /// Añade un marcador con nombre en el cabezal actual.
    fn add_marker(&mut self) {
        let before = self.project.clone();
        let name = format!("M{}", self.project.markers.len() + 1);
        self.project.markers.push(Marker {
            time: self.playhead,
            name,
        });
        self.finish_edit(before);
        self.status = "Marcador añadido".to_owned();
    }

    /// Añade un subtítulo de 3 s en el cabezal y abre su edición.
    fn add_subtitle(&mut self) {
        self.subtitle_query.clear();
        let before = self.project.clone();
        self.project.subtitles.push(Subtitle {
            start: self.playhead,
            end: self.playhead + 3.0,
            text: String::new(),
        });
        self.finish_edit(before);
        self.status = "Subtitulo añadido; escribe su texto en la lista".to_owned();
    }

    /// Desplaza todos los cues como una única edición reversible.
    fn shift_all_subtitles(&mut self, offset: f64) {
        if self.project.subtitles.is_empty() || offset == 0.0 {
            return;
        }
        match shift_subtitles(&self.project.subtitles, offset) {
            Ok(cues) if cues != self.project.subtitles => {
                let before = self.project.clone();
                self.project.subtitles = cues;
                self.finish_edit(before);
                self.subtitle_offset_ms = 0.0;
                self.status = format!(
                    "{} subtítulos desplazados {offset:+.3} s",
                    self.project.subtitles.len()
                );
            }
            Ok(_) => {}
            Err(error) => self.status = error,
        }
    }

    /// Guarda los subtítulos como archivo .srt.
    fn export_srt(&mut self) {
        if self.project.subtitles.is_empty() {
            self.status = "No hay subtitulos que exportar".to_owned();
            return;
        }
        let Some(path) = FileDialog::new()
            .add_filter("Subtitulos SRT", &["srt"])
            .set_file_name("NovaCut Subtitulos.srt")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, build_srt(&self.project.subtitles)) {
            Ok(()) => self.status = format!("SRT guardado: {}", path.display()),
            Err(error) => self.status = format!("No se pudo guardar el SRT: {error}"),
        }
    }

    /// Exporta el montaje como EDL CMX 3600, el formato que cualquier sala de
    /// máster lee. Solo viajan los cortes: lo que el formato no representa se
    /// cuenta en el estado en vez de desaparecer en silencio.
    fn export_edl(&mut self) {
        let timebase = self.project.timebase();
        let (clips, skipped) = self.edl_clips(timebase);
        if clips.is_empty() {
            self.status = "No hay cortes que exportar a EDL".to_owned();
            return;
        }
        let Some(path) = FileDialog::new()
            .add_filter("Lista de cortes EDL", &["edl"])
            .set_file_name(format!("{}.edl", self.project.name))
            .save_file()
        else {
            return;
        };
        let text = edl::to_edl(&self.project.name, timebase, &clips);
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.status = format!("EDL guardado: {}", path.display());
                if !skipped.is_empty() {
                    self.status.push_str(&format!(
                        " · {} sin representar en EDL: {}",
                        skipped.len(),
                        skipped
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            Err(error) => self.status = format!("No se pudo guardar el EDL: {error}"),
        }
    }

    /// Traduce el montaje a cortes de EDL. Devuelve también lo que se queda
    /// fuera, que en un EDL es bastante: títulos, capas de ajuste, anidados,
    /// rampas de velocidad y transiciones.
    fn edl_clips(&self, timebase: Timebase) -> (Vec<edl::EdlClip>, Vec<String>) {
        project_to_edl_clips(&self.project.clips, timebase)
    }

    /// Lee un EDL y monta sus cortes. Un EDL describe cintas y timecodes, no
    /// archivos: los clips entran offline y se resuelven con la búsqueda en
    /// carpeta, que es justo el flujo de una sala de máster.
    fn import_edl(&mut self) {
        let Some(path) = FileDialog::new()
            .add_filter("Lista de cortes EDL", &["edl"])
            .pick_file()
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            self.status = format!("No se pudo leer {}", path.display());
            return;
        };
        // Los EDL de sala suelen venir en Latin-1; leerlos como UTF-8 estricto
        // los rechazaría por una tilde en un nombre de plano.
        let text = String::from_utf8(bytes.clone())
            .unwrap_or_else(|_| bytes.iter().map(|byte| *byte as char).collect());
        let document = edl::from_edl(&text, self.project.timebase());
        if document.clips.is_empty() {
            self.status = format!(
                "{} no trae eventos que montar",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            return;
        }
        let before = self.project.clone();
        if let Some(timebase) = document.timebase {
            self.project.set_timebase(timebase);
        }
        let timebase = self.project.timebase();
        let added = edl_clips_to_project(&document.clips, timebase, &self.project.clips);
        let added = {
            let count = added.len();
            self.project.clips.extend(added);
            count
        };
        self.project.normalize();
        self.finish_edit(before);
        self.selected = None;
        self.status = format!(
            "EDL importado: {added} corte{} offline · usa «Buscar todos en una carpeta...» para localizar los medios",
            if added == 1 { "" } else { "s" }
        );
        if !document.warnings.is_empty() {
            self.status.push_str(&format!(
                " · {} aviso(s): {}",
                document.warnings.len(),
                document
                    .warnings
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" · ")
            ));
        }
    }

    /// Clips del proyecto más los subtítulos como títulos superiores.
    fn effective_clips(&self) -> Vec<RoughClip> {
        let base = if self.burn_subtitles {
            clips_with_subtitles(
                &self.project.clips,
                &self.project.subtitles,
                self.project.subtitle_style.as_ref(),
            )
        } else {
            self.project.clips.clone()
        };
        // Clips desactivados y pistas ocultas fuera; pistas enmudecidas (o no
        // solistas) pasan sin audio.
        let clips: Vec<RoughClip> = base
            .into_iter()
            .filter(|clip| clip.enabled)
            .filter(|clip| !clip.has_video || self.project.track_visible(clip.track))
            .map(|mut clip| {
                if clip.has_audio && !self.project.track_audible(clip.track) {
                    clip.muted = true;
                }
                clip
            })
            .collect();
        clips
    }

    /// Clips que van al render final: como los efectivos, pero recortados al
    /// rango de trabajo cuando el usuario pide exportar solo ese tramo.
    fn export_clips(&self) -> Vec<RoughClip> {
        let clips = self.effective_clips();
        if self.export_range_only {
            if let Some((start, end)) = self.work_range() {
                return trim_clips_to_range(&clips, start, end);
            }
        }
        clips
    }

    /// Sincroniza los ángulos de multicámara por audio: alinea cada clip de vídeo
    /// de otras pistas que solape con el clip seleccionado (referencia).
    fn sync_angles_by_audio(&mut self) {
        let Some(base_index) = self.selected else {
            self.status = "Selecciona el clip de referencia para sincronizar".to_owned();
            return;
        };
        let base = self.project.clips[base_index].clone();
        if !base.has_audio {
            self.status = "El clip de referencia debe tener audio".to_owned();
            return;
        }
        let base_end = base.timeline_start + base.duration();
        let probe_duration = base.duration().min(90.0);
        let Ok(base_envelope) = extract_audio_envelope(&base.path, base.in_seconds, probe_duration)
        else {
            self.status = "No se pudo leer el audio del clip de referencia".to_owned();
            return;
        };
        let before = self.project.clone();
        let mut synced = 0;
        let mut messages: Vec<String> = Vec::new();
        for index in 0..self.project.clips.len() {
            if index == base_index {
                continue;
            }
            let clip = &self.project.clips[index];
            if !clip.has_video || !clip.has_audio || clip.path == base.path {
                continue;
            }
            let overlaps = clip.timeline_start < base_end
                && clip.timeline_start + clip.duration() > base.timeline_start;
            if !overlaps {
                continue;
            }
            let Ok(envelope) =
                extract_audio_envelope(&clip.path, clip.in_seconds, clip.duration().min(90.0))
            else {
                continue;
            };
            let Some(shift) = best_offset_buckets(&base_envelope, &envelope, 2000) else {
                messages.push(format!("V{}: sin coincidencia", clip.track + 1));
                continue;
            };
            let delta = shift as f64 / ENVELOPE_RATE;
            let new_start =
                (clip.in_seconds - base.in_seconds + base.timeline_start - delta).max(0.0);
            messages.push(format!(
                "V{}: desplazado {:.2} s",
                clip.track + 1,
                new_start - clip.timeline_start
            ));
            self.project.clips[index].timeline_start = new_start;
            synced += 1;
        }
        if synced > 0 {
            self.finish_edit(before);
        }
        self.status = if synced > 0 {
            format!("Sincronizados {synced} angulo(s): {}", messages.join("; "))
        } else {
            format!("Sin sincronizar: {}", messages.join("; "))
        };
    }

    /// Corta al ángulo de multicámara indicado en el cabezal actual.
    fn multicam_cut(&mut self, camera: usize) {
        // Base: el clip de vídeo con la pista más baja que contiene el cabezal.
        let base = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.has_video
                    && self.playhead >= clip.timeline_start
                    && self.playhead < clip.timeline_start + clip.duration()
            })
            .min_by_key(|(_, clip)| clip.track);
        let Some((base_idx, _)) = base else {
            self.status = "No hay plano base bajo el cabezal para multicam".to_owned();
            return;
        };
        match apply_multicam_cut(self.project.clips.clone(), base_idx, camera, self.playhead) {
            Ok(new_clips) => {
                let before = self.project.clone();
                self.project.clips = new_clips;
                self.finish_edit(before);
                self.status = format!("Corte a camara {camera}");
            }
            Err(message) => self.status = message,
        }
    }

    /// Detiene la reproducción del monitor si está activa.
    fn stop_playback(&mut self) {
        self.playback = None;
    }

    /// Exporta el fotograma compuesto del cabezal como PNG.
    fn export_frame(&mut self) {
        if !self.ffmpeg_ready || self.project.clips.is_empty() {
            self.status = "Importa clips antes de exportar un fotograma".to_owned();
            return;
        }
        if self.frame_result.is_some() {
            return;
        }
        let Some(output) = FileDialog::new()
            .add_filter("Imagen PNG", &["png"])
            .set_file_name(format!("fotograma-{:.3}s.png", self.playhead))
            .save_file()
        else {
            return;
        };
        let prepared = prepare_render_clips(&self.effective_clips());
        let mut command = Command::new(tool_path("ffmpeg.exe"));
        command.args(["-v", "error", "-y"]);
        let size = self.export_size;
        let timebase = self.project.timebase();
        let (input_indices, is_title_input) =
            push_render_inputs(&mut command, &prepared, size, self.use_proxies, timebase);
        let Ok(filters) = build_render_filters(
            &prepared,
            &input_indices,
            &is_title_input,
            size,
            true,
            false,
            &self.project.track_gains,
            self.project.master_gain_db,
            self.project.normalize_loudness,
            timebase,
            None,
        ) else {
            self.status = "No se pudo componer el fotograma".to_owned();
            return;
        };
        // Recorta el montaje al instante del cabezal y saca un solo frame.
        let clip = self.playhead.clamp(0.0, self.project.duration().max(0.001));
        let (sender, receiver) = mpsc::channel();
        self.frame_result = Some(receiver);
        std::thread::spawn(move || {
            let result = command
                .args(["-filter_complex", &filters.join(";")])
                .args(["-map", "[vout]"])
                .args(["-ss", &format_seconds(clip), "-frames:v", "1"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|error| format!("FFmpeg no esta disponible: {error}"))
                .and_then(|out| {
                    if out.status.success() {
                        Ok(())
                    } else {
                        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
                    }
                })
                .map(|()| output);
            let _ = sender.send(result);
        });
    }

    fn poll_frame(&mut self) {
        let Some(receiver) = &self.frame_result else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            self.status = match result {
                Ok(path) => format!("Fotograma exportado: {}", path.display()),
                Err(error) => format!("Fallo al exportar el fotograma: {error}"),
            };
            self.frame_result = None;
        }
    }

    /// Aplica el gesto en curso sobre el montaje. Vive en su propio
    /// método para que los cortes tempranos no interrumpan el dibujado
    /// del resto de la interfaz.
    fn apply_timeline_drag(&mut self, event: Option<TimelineDragEvent>) {
        match event {
            Some(TimelineDragEvent::Move(index, delta, track)) => {
                if self
                    .drag_edit
                    .as_ref()
                    .is_some_and(|(active, _, _)| *active == index)
                {
                    // El clip principal define el desplazamiento real (ya
                    // imantado) y el resto de la selección lo sigue en bloque.
                    let origin = self.project.clips[index].timeline_start;
                    let target = (origin + delta).max(0.0);
                    let snapped_start = snap_time(
                        target,
                        &self.project.clips,
                        Some(index),
                        &self.project.markers,
                        self.playhead,
                        self.snap_tolerance(),
                    );
                    let duration = self.project.clips[index].duration();
                    let snapped_end = snap_time(
                        target + duration,
                        &self.project.clips,
                        Some(index),
                        &self.project.markers,
                        self.playhead,
                        self.snap_tolerance(),
                    ) - duration;
                    let snapped = if (snapped_end - target).abs() < (snapped_start - target).abs() {
                        snapped_end
                    } else {
                        snapped_start
                    };
                    let group: Vec<usize> = self
                        .selected_indices()
                        .into_iter()
                        .filter(|other| !self.project.clip_locked(&self.project.clips[*other]))
                        .collect();
                    let group = if group.contains(&index) {
                        group
                    } else {
                        vec![index]
                    };
                    // Ningún clip del grupo puede cruzar el cero.
                    let earliest = group
                        .iter()
                        .map(|other| self.project.clips[*other].timeline_start)
                        .fold(f64::INFINITY, f64::min);
                    let shift = (snapped - origin).max(-earliest);
                    let track_shift = track as isize - self.project.clips[index].track as isize;
                    if group.iter().any(|other| {
                        let clip = &self.project.clips[*other];
                        let target_track =
                            (clip.track as isize + track_shift).clamp(0, 15) as usize;
                        self.project.lane_locked(target_track, clip.has_video)
                    }) {
                        self.status = "La pista de destino está bloqueada".to_owned();
                        return;
                    }
                    for other in group {
                        let clip = &mut self.project.clips[other];
                        clip.timeline_start = (clip.timeline_start + shift).max(0.0);
                        clip.track = (clip.track as isize + track_shift).clamp(0, 15) as usize;
                    }
                    self.selected = Some(index);
                    self.selection.insert(index);
                    self.request_preview();
                }
            }
            Some(TimelineDragEvent::TrimStart(index, target_time)) => {
                let Some((_, kind, before)) = self
                    .drag_edit
                    .as_ref()
                    .map(|state| (state.0, state.1, &state.2))
                    .filter(|state| {
                        state.0 == index
                            && matches!(state.1, DragKind::TrimStart | DragKind::RippleTrimStart)
                    })
                else {
                    return;
                };
                let ripple = kind == DragKind::RippleTrimStart;
                let orig = before.clips[index].clone();
                if orig.nested.is_some()
                    || orig
                        .speed_ramp
                        .as_ref()
                        .is_some_and(|points| !points.is_empty())
                {
                    self.status = "Desanida o quita la rampa antes de recortar bordes".to_owned();
                    return;
                }
                let speed = orig.speed.clamp(0.1, 8.0);
                let start_min = (orig.timeline_start - orig.in_seconds / speed).max(0.0);
                let max_start = (orig.timeline_start + orig.duration() - 0.04).max(start_min);
                let new_start = snap_time(
                    self.quantize_time(target_time),
                    &before.clips,
                    Some(index),
                    &before.markers,
                    self.playhead,
                    self.snap_tolerance(),
                )
                .clamp(start_min, max_start);
                let delta_t = new_start - orig.timeline_start;
                let clip_out = &mut self.project.clips[index];
                clip_out.timeline_start = if ripple {
                    orig.timeline_start
                } else {
                    new_start
                };
                clip_out.in_seconds = (orig.in_seconds + delta_t * speed).max(0.0);
                if ripple {
                    let orig_end = orig.timeline_start + orig.duration();
                    for (other, baseline) in before.clips.iter().enumerate() {
                        if other != index
                            && baseline.track == orig.track
                            && baseline.has_video == orig.has_video
                            && baseline.timeline_start >= orig_end - 0.001
                        {
                            self.project.clips[other].timeline_start =
                                (baseline.timeline_start - delta_t).max(0.0);
                        }
                    }
                }
                self.request_preview();
            }
            Some(TimelineDragEvent::TrimEnd(index, target_time)) => {
                let Some((_, kind, before)) = self
                    .drag_edit
                    .as_ref()
                    .map(|state| (state.0, state.1, &state.2))
                    .filter(|state| {
                        state.0 == index
                            && matches!(state.1, DragKind::TrimEnd | DragKind::RippleTrimEnd)
                    })
                else {
                    return;
                };
                let ripple = kind == DragKind::RippleTrimEnd;
                let orig = before.clips[index].clone();
                if orig.nested.is_some()
                    || orig
                        .speed_ramp
                        .as_ref()
                        .is_some_and(|points| !points.is_empty())
                {
                    self.status = "Desanida o quita la rampa antes de recortar bordes".to_owned();
                    return;
                }
                let speed = orig.speed.clamp(0.1, 8.0);
                let min_end = orig.timeline_start + 0.04;
                let max_end = orig
                    .source_duration_seconds
                    .map(|duration| {
                        orig.timeline_start + (duration - orig.in_seconds).max(0.04) / speed
                    })
                    .unwrap_or(f64::INFINITY);
                let new_end = snap_time(
                    self.quantize_time(target_time),
                    &before.clips,
                    Some(index),
                    &before.markers,
                    self.playhead,
                    self.snap_tolerance(),
                )
                .clamp(min_end, max_end.max(min_end));
                let clip_out = &mut self.project.clips[index];
                clip_out.in_seconds = orig.in_seconds;
                clip_out.out_seconds = orig.in_seconds + (new_end - orig.timeline_start) * speed;
                if ripple {
                    let orig_end = orig.timeline_start + orig.duration();
                    let delta_t = new_end - orig_end;
                    for (other, baseline) in before.clips.iter().enumerate() {
                        if other != index
                            && baseline.track == orig.track
                            && baseline.has_video == orig.has_video
                            && baseline.timeline_start >= orig_end - 0.001
                        {
                            self.project.clips[other].timeline_start =
                                (baseline.timeline_start + delta_t).max(0.0);
                        }
                    }
                }
                self.request_preview();
            }
            Some(TimelineDragEvent::Commit(index)) => {
                if let Some((active, kind, before)) = self.drag_edit.take() {
                    if active != index {
                        return;
                    }
                    if kind == DragKind::Move {
                        let current_start = self.project.clips[index].timeline_start;
                        let correction = self.quantize_time(current_start) - current_start;
                        let group = self.selected_indices();
                        let group = if group.contains(&index) {
                            group
                        } else {
                            vec![index]
                        };
                        for moved in group {
                            self.project.clips[moved].timeline_start =
                                (self.project.clips[moved].timeline_start + correction).max(0.0);
                        }
                    }
                    let now = &self.project.clips[index];
                    let orig = &before.clips[index];
                    let changed = now.timeline_start != orig.timeline_start
                        || now.track != orig.track
                        || now.in_seconds != orig.in_seconds
                        || now.out_seconds != orig.out_seconds;
                    if changed {
                        // Al soltar, lo que quede debajo se recorta o se parte
                        // en vez de quedar solapado sin criterio.
                        let mut overwritten = 0;
                        if self.overwrite_on_drop && matches!(kind, DragKind::Move) {
                            let candidates = self.selected_indices();
                            let candidates = if candidates.contains(&index) {
                                candidates
                            } else {
                                vec![index]
                            };
                            let moved: Vec<usize> = candidates
                                .into_iter()
                                .filter(|placed| {
                                    self.project.clips.get(*placed).is_some_and(|clip| {
                                        before.clips.get(*placed).is_some_and(|baseline| {
                                            clip.timeline_start != baseline.timeline_start
                                                || clip.track != baseline.track
                                        })
                                    })
                                })
                                .collect();
                            let primary_offset = moved.iter().position(|placed| *placed == index);
                            let placed: Vec<RoughClip> = moved
                                .iter()
                                .map(|placed| self.project.clips[*placed].clone())
                                .collect();
                            if placed.iter().any(|clip| {
                                span_hits_complex_clip(
                                    &self.project.clips,
                                    clip.track,
                                    clip.has_video,
                                    clip.timeline_start,
                                    clip.timeline_start + clip.duration(),
                                    &moved,
                                )
                            }) {
                                self.project = before;
                                self.prune_selection();
                                self.request_preview();
                                self.status = "Movimiento cancelado: no se puede sobrescribir una rampa o secuencia anidada".to_owned();
                                return;
                            }
                            for placed_index in moved.iter().rev() {
                                self.project.clips.remove(*placed_index);
                            }
                            for clip in &placed {
                                overwritten += clear_track_span(
                                    &mut self.project.clips,
                                    clip.track,
                                    clip.has_video,
                                    clip.timeline_start,
                                    clip.timeline_start + clip.duration(),
                                    &[],
                                );
                            }
                            let first_new = self.project.clips.len();
                            self.project.clips.extend(placed);
                            self.selection = (first_new..self.project.clips.len()).collect();
                            self.selected = primary_offset.map(|offset| first_new + offset);
                        }
                        self.finish_edit(before);
                        self.status = match kind {
                            DragKind::Move if overwritten > 0 => {
                                format!("Clip movido; {overwritten} clip(s) sobrescrito(s)")
                            }
                            DragKind::Move => "Clip movido".to_owned(),
                            DragKind::RippleTrimStart | DragKind::RippleTrimEnd => {
                                "Clip recortado con ripple".to_owned()
                            }
                            _ => "Clip recortado".to_owned(),
                        };
                    }
                }
            }
            Some(TimelineDragEvent::Select(index, mode)) => match mode {
                SelectionMode::Replace => {
                    // Clic en un clip: sus ajustes pasan a primer plano.
                    self.bottom_tab = BottomTab::Inspector;
                    self.select_only(index)
                }
                SelectionMode::Toggle => self.toggle_in_selection(index),
                SelectionMode::Extend => self.extend_selection_to(index),
            },
            None => {}
        }
    }

    /// Lista de marcadores del montaje.
    /// Edición por texto: la transcripción como documento. Clic selecciona y
    /// lleva el cabezal a la palabra, Mayús+clic extiende, Supr borra el
    /// tramo de todas las pistas.
    fn transcript_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.transcription_result.is_some();
        let fillers: HashSet<usize> = transcripcion::filler_indices(&self.project.transcript)
            .into_iter()
            .collect();
        let selection = self
            .transcript_selection
            .map(|(anchor, cursor)| anchor.min(cursor)..=anchor.max(cursor));
        let mut transcribe = false;
        let mut delete = false;
        let mut remove_fillers = false;
        let mut captions = false;
        ui.horizontal_wrapped(|ui| {
            let label = if busy {
                "Transcribiendo…"
            } else if self.project.transcript.is_empty() {
                "Transcribir con Whisper"
            } else {
                "Volver a transcribir"
            };
            transcribe = ui
                .add_enabled(!busy && self.ffmpeg_ready, egui::Button::new(label))
                .on_hover_text("Transcribe la mezcla del montaje palabra a palabra, en local")
                .clicked();
            let selected = selection.as_ref().map_or(0, |range| range.clone().count());
            delete = ui
                .add_enabled(
                    selected > 0,
                    egui::Button::new(format!("Borrar selección ({selected})")),
                )
                .on_hover_text("Quita ese tramo de todas las pistas y cierra el hueco (Supr)")
                .clicked();
            remove_fillers = ui
                .add_enabled(
                    !fillers.is_empty(),
                    egui::Button::new(format!("Quitar muletillas ({})", fillers.len())),
                )
                .on_hover_text("eh, em, mm, este, o sea… Revisa las resaltadas antes de quitarlas")
                .clicked();
            ui.checkbox(&mut self.transcript_show_fillers, "Resaltar muletillas");
            captions = ui
                .add_enabled(
                    !self.project.transcript.is_empty(),
                    egui::Button::new("✨ Subtítulos animados"),
                )
                .on_hover_text("Capa de subtítulos palabra a palabra al estilo TikTok/CapCut")
                .clicked();
        });
        if self.project.transcript.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Transcribe el montaje para editarlo como un documento: selecciona palabras y bórralas para cortar el vídeo en todas las pistas.",
                )
                .color(theme::TEXT_DIM),
            );
        } else {
            ui.separator();
            let playhead = self.playhead;
            let fps = self.project.fps;
            let mut clicked: Option<(usize, bool)> = None;
            let shift = ui.input(|input| input.modifiers.shift);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
                for (index, word) in self.project.transcript.iter().enumerate() {
                    let active = playhead >= word.start && playhead < word.end;
                    let chosen = selection
                        .as_ref()
                        .is_some_and(|range| range.contains(&index));
                    let mut text = egui::RichText::new(&word.text).size(13.5);
                    if chosen {
                        text = text
                            .background_color(theme::ACCENT_DIM)
                            .color(egui::Color32::WHITE);
                    } else if active {
                        text = text.color(theme::ACCENT).underline();
                    } else if self.transcript_show_fillers && fillers.contains(&index) {
                        text = text.color(theme::WARN).italics();
                    } else {
                        text = text.color(theme::TEXT);
                    }
                    let response = ui
                        .add(egui::Label::new(text).sense(egui::Sense::click()))
                        .on_hover_text(format!(
                            "{} → {}",
                            timecode(word.start, fps),
                            timecode(word.end, fps)
                        ));
                    if response.clicked() {
                        clicked = Some((index, shift));
                    }
                }
            });
            if let Some((index, extend)) = clicked {
                self.transcript_selection = match self.transcript_selection {
                    Some((anchor, _)) if extend => Some((anchor, index)),
                    _ => Some((index, index)),
                };
                self.playhead = self.project.transcript[index].start;
                self.request_preview();
            }
        }
        if transcribe {
            self.transcribe_with_whisper();
        }
        if delete {
            self.delete_transcript_selection();
        }
        if remove_fillers {
            self.remove_fillers();
        }
        if captions {
            self.create_animated_captions();
        }
    }

    fn markers_tab(&mut self, ui: &mut egui::Ui, metadata_changed: &mut bool) {
        if self.project.markers.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Sin marcadores. Pulsa M (o «+ Insertar › Marcador») para marcar el cabezal.",
                )
                .color(theme::TEXT_DIM),
            );
            if ui
                .button("+ Marcador en el cabezal")
                .on_hover_text("Añade un marcador donde está el cabezal (M)")
                .clicked()
            {
                self.add_marker();
            }
            return;
        }
        let mut marker_delete: Option<usize> = None;
        let mut marker_jump: Option<f64> = None;
        for (marker_index, marker) in self.project.markers.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{:02}", marker_index + 1));
                *metadata_changed |= ui
                    .add(
                        egui::DragValue::new(&mut marker.time)
                            .speed(0.05)
                            .suffix(" s"),
                    )
                    .changed();
                *metadata_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut marker.name)
                            .desired_width((ui.available_width() - 70.0).max(60.0)),
                    )
                    .changed();
                if ui
                    .button("Ir")
                    .on_hover_text("Lleva el cabezal a este marcador")
                    .clicked()
                {
                    marker_jump = Some(marker.time);
                }
                if ui
                    .button("×")
                    .on_hover_text("Elimina este marcador")
                    .clicked()
                {
                    marker_delete = Some(marker_index);
                }
            });
        }
        if let Some(time) = marker_jump {
            self.playhead = time;
            self.request_preview();
        }
        if let Some(index) = marker_delete {
            let before = self.project.clone();
            self.project.markers.remove(index);
            self.finish_edit(before);
            self.status = "Marcador eliminado".to_owned();
        }
    }

    /// Mezclador: buses por pista, master y sonoridad.
    fn mixer_tab(&mut self, ui: &mut egui::Ui, metadata_changed: &mut bool) {
        let tracks = self.project.audio_track_count().max(1);
        self.project.track_gains.resize(tracks, 0.0);
        for (track, gain) in self.project.track_gains.iter_mut().enumerate() {
            *metadata_changed |= ui
                .add(
                    egui::Slider::new(gain, -60.0..=12.0)
                        .text(format!("Bus {}", track + 1))
                        .suffix(" dB"),
                )
                .changed();
        }
        ui.separator();
        *metadata_changed |= ui
            .add(
                egui::Slider::new(&mut self.project.master_gain_db, -60.0..=12.0)
                    .text("Master")
                    .suffix(" dB"),
            )
            .changed();
        *metadata_changed |= ui
            .checkbox(
                &mut self.project.normalize_loudness,
                "Normalizar exportación a -14 LUFS",
            )
            .on_hover_text("Ajusta el volumen final al estándar de YouTube y Spotify (-14 LUFS)")
            .changed();
        if ui
            .add_enabled(
                self.loudness_result.is_none(),
                egui::Button::new("Medir LUFS"),
            )
            .on_hover_text("Mide el volumen percibido del montaje para normalizar con precisión")
            .on_disabled_hover_text(
                "Mide el volumen percibido del montaje para normalizar con precisión",
            )
            .clicked()
        {
            self.analyze_loudness();
        }
        let fresh = self.current_loudness_measurement().is_some();
        if let Some(report) = &self.loudness_report {
            ui.monospace(format!(
                "{:.1} LUFS | {:.1} dBTP | LRA {:.1}",
                report.integrated_lufs, report.true_peak_db, report.range_lu
            ));
            if fresh {
                ui.small("Medición vigente: la exportación normalizará con precisión (dos pasos).");
            } else {
                ui.small(
                    "El proyecto cambió desde la medición: vuelve a medir para \
                                 normalizar con precisión. Mientras tanto, la exportación usa \
                                 un paso aproximado.",
                );
            }
        } else if self.project.normalize_loudness {
            ui.small(
                "Sin medición: la exportación usará un paso aproximado. \
                             Pulsa Medir LUFS antes de exportar para más precisión.",
            );
        }
    }

    /// Subtítulos: edición, estilo, SRT y transcripción.
    fn subtitles_tab(
        &mut self,
        ui: &mut egui::Ui,
        metadata_changed: &mut bool,
        burn_changed: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("+ Subtítulo aquí")
                .on_hover_text("Añade un subtítulo vacío en el cabezal")
                .clicked()
            {
                self.add_subtitle();
            }
            if ui
                .button("Exportar SRT")
                .on_hover_text("Guarda los subtítulos como archivo .srt")
                .clicked()
            {
                self.export_srt();
            }
            if ui
                .button("Importar SRT")
                .on_hover_text("Carga subtítulos desde un archivo .srt")
                .clicked()
            {
                self.import_srt();
            }
            if ui
                .add_enabled(
                    self.transcription_result.is_none(),
                    egui::Button::new("Transcribir con Whisper"),
                )
                .on_hover_text("Genera transcripción y subtítulos con Whisper, en tu equipo")
                .on_disabled_hover_text(
                    "Genera transcripción y subtítulos con Whisper, en tu equipo",
                )
                .clicked()
            {
                self.transcribe_with_whisper();
            }
            *burn_changed |= ui
                .checkbox(&mut self.burn_subtitles, "Quemar en vídeo")
                .on_hover_text(
                    "Dibuja los subtítulos dentro del vídeo, en el monitor y al exportar",
                )
                .changed();
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.subtitle_query)
                    .hint_text("Buscar en los subtítulos…")
                    .desired_width(ui.available_width().min(230.0)),
            );
            if ui
                .button("Limpiar búsqueda")
                .on_hover_text("Borra el texto de búsqueda")
                .clicked()
            {
                self.subtitle_query.clear();
            }
            ui.label(format!(
                "{} de {}",
                search_subtitles(&self.project.subtitles, &self.subtitle_query).len(),
                self.project.subtitles.len()
            ));
        });
        ui.collapsing("Sincronizar todos los subtítulos", |ui| {
            ui.small("Negativo adelanta; positivo retrasa. Se aplica a todos, aunque haya una búsqueda activa.");
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut self.subtitle_offset_ms).speed(10.0).suffix(" ms"));
                if ui.add_enabled(!self.project.subtitles.is_empty() && self.subtitle_offset_ms.is_finite()
                    && self.subtitle_offset_ms != 0.0, egui::Button::new("Aplicar ajuste")).on_hover_text("Desplaza todos los subtítulos el tiempo indicado").on_disabled_hover_text("Desplaza todos los subtítulos el tiempo indicado").clicked() {
                    self.shift_all_subtitles(self.subtitle_offset_ms / 1000.0);
                }
                if ui.add_enabled(!self.project.subtitles.is_empty(), egui::Button::new("Alinear primero al cabezal")).on_hover_text("Mueve todos para que el primero empiece en el cabezal").on_disabled_hover_text("Mueve todos para que el primero empiece en el cabezal").clicked() {
                    if let Some(first) = self.project.subtitles.iter().map(|cue| cue.start).min_by(f64::total_cmp) {
                        self.shift_all_subtitles(self.playhead - first);
                    }
                }
            });
            ui.small("El ajuste conserva los intervalos y se puede deshacer con Ctrl+Z.");
        });
        ui.collapsing("Estilo", |ui| {
            let mut style = self.project.subtitle_style.clone().unwrap_or_default();
            let mut style_changed = false;
            style_changed |= ui
                .add(egui::Slider::new(&mut style.size, 12.0..=160.0).text("Tamaño"))
                .changed();
            style_changed |= ui
                .add(egui::Slider::new(&mut style.position_y, 0.0..=1.0).text("Posición vertical"))
                .changed();
            style_changed |= ui
                .add(egui::Slider::new(&mut style.red, 0.0..=1.0).text("Rojo"))
                .changed();
            style_changed |= ui
                .add(egui::Slider::new(&mut style.green, 0.0..=1.0).text("Verde"))
                .changed();
            style_changed |= ui
                .add(egui::Slider::new(&mut style.blue, 0.0..=1.0).text("Azul"))
                .changed();
            if style_changed {
                self.project.subtitle_style = Some(style);
                *metadata_changed = true;
            }
        });
        let mut subtitle_delete: Option<usize> = None;
        let mut subtitle_jump: Option<f64> = None;
        let indices = search_subtitles(&self.project.subtitles, &self.subtitle_query);
        if indices.is_empty() {
            ui.label(if self.project.subtitles.is_empty() {
                "Añade un subtítulo o importa un SRT."
            } else {
                "No hay subtítulos que coincidan."
            });
        }
        for subtitle_index in indices {
            let subtitle = &mut self.project.subtitles[subtitle_index];
            ui.push_id(("subtitle", subtitle_index), |ui| {
                // Cada subtítulo en dos líneas: tiempos y acciones arriba, el
                // texto a todo el ancho debajo (en una sola fila no cabía en la
                // columna y la ensanchaba).
                let mut start_changed = false;
                let mut end_changed = false;
                ui.horizontal(|ui| {
                    ui.monospace(format!("{:02}", subtitle_index + 1));
                    start_changed = ui
                        .add(
                            egui::DragValue::new(&mut subtitle.start)
                                .speed(0.05)
                                .suffix(" s"),
                        )
                        .changed();
                    ui.label("→");
                    end_changed = ui
                        .add(
                            egui::DragValue::new(&mut subtitle.end)
                                .speed(0.05)
                                .suffix(" s"),
                        )
                        .changed();
                    if ui.button("Ir").clicked() {
                        subtitle_jump = Some(subtitle.start);
                    }
                    if ui.button("×").on_hover_text("Eliminar subtítulo").clicked() {
                        subtitle_delete = Some(subtitle_index);
                    }
                });
                *metadata_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut subtitle.text)
                            .hint_text("Texto del subtítulo")
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
                if start_changed || end_changed {
                    subtitle.start = subtitle.start.max(0.0);
                    subtitle.end = subtitle.end.max(subtitle.start + 0.04);
                    *metadata_changed = true;
                }
                ui.add_space(4.0);
            });
        }
        if let Some(time) = subtitle_jump {
            self.playhead = time;
            self.request_preview();
        }
        if let Some(index) = subtitle_delete {
            let before = self.project.clone();
            self.project.subtitles.remove(index);
            self.finish_edit(before);
            self.status = "Subtítulo eliminado".to_owned();
        }
    }

    /// Lista de planos con orden de pista y borrado.
    fn clips_tab(&mut self, ui: &mut egui::Ui, _metadata_changed: &mut bool) {
        enum Action {
            Up(usize),
            Down(usize),
            Remove(usize),
            Select(usize),
        }
        let mut action = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, clip) in self.project.clips.iter().enumerate() {
                let selected = self.selected == Some(index);
                egui::Frame::group(ui.style())
                    .fill(if selected {
                        egui::Color32::from_rgb(49, 39, 76)
                    } else {
                        egui::Color32::from_rgb(29, 32, 39)
                    })
                    .show(ui, |ui| {
                        // Dos líneas: nombre y posición arriba, acciones
                        // debajo. En una sola fila se pisaban en la columna.
                        ui.set_width(ui.available_width());
                        if ui
                            .selectable_label(
                                selected,
                                format!("{:02}  {}", index + 1, clip.name()),
                            )
                            .on_hover_text("Selecciona este clip")
                            .clicked()
                        {
                            action = Some(Action::Select(index));
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{} · empieza en {:.2} s · dura {:.2} s",
                                if clip.has_video { "V" } else { "A" },
                                clip.track + 1,
                                clip.timeline_start,
                                clip.duration()
                            ))
                            .size(11.5)
                            .color(theme::TEXT_DIM),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(clip.track < 15, egui::Button::new("▲ Subir pista"))
                                .on_hover_text("Mueve el clip a la pista superior")
                                .clicked()
                            {
                                action = Some(Action::Up(index));
                            }
                            if ui
                                .add_enabled(clip.track > 0, egui::Button::new("▼ Bajar pista"))
                                .on_hover_text("Mueve el clip a la pista inferior")
                                .clicked()
                            {
                                action = Some(Action::Down(index));
                            }
                            if ui
                                .button("Eliminar")
                                .on_hover_text("Quita el clip del montaje (se puede deshacer)")
                                .clicked()
                            {
                                action = Some(Action::Remove(index));
                            }
                        });
                    });
                ui.add_space(5.0);
            }
        });
        match action {
            Some(Action::Up(index)) if self.project.clips[index].track < 15 => {
                let before = self.project.clone();
                self.project.clips[index].track += 1;
                self.select_only(index);
                self.finish_edit(before);
            }
            Some(Action::Down(index)) if self.project.clips[index].track > 0 => {
                let before = self.project.clone();
                self.project.clips[index].track -= 1;
                self.select_only(index);
                self.finish_edit(before);
            }
            Some(Action::Remove(index)) => {
                let before = self.project.clone();
                self.project.clips.remove(index);
                self.clear_selection();
                self.finish_edit(before);
            }
            Some(Action::Select(index)) => self.select_only(index),
            _ => {}
        }
    }

    /// Timecode del cabezal y duración total, bajo el monitor.
    fn transport_timecode(&self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(timecode(self.playhead, self.project.fps))
                .monospace()
                .size(15.0)
                .strong()
                .color(theme::TEXT),
        )
        .on_hover_text("Posición del cabezal (horas:minutos:segundos:fotogramas)");
        ui.label(
            egui::RichText::new(format!(
                "/ {}",
                timecode(self.project.duration(), self.project.fps)
            ))
            .monospace()
            .size(11.0)
            .color(theme::TEXT_FAINT),
        )
        .on_hover_text("Duración total del montaje");
    }

    /// Pestañas de la columna derecha: el Inspector y los paneles que antes
    /// ocupaban el alto de la timeline (transcripción, subtítulos, etc.).
    fn side_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for tab in BottomTab::SHOWN {
                let count = match tab {
                    BottomTab::Subtitles => Some(self.project.subtitles.len()),
                    BottomTab::Markers => Some(self.project.markers.len()),
                    BottomTab::Clips => Some(self.project.clips.len()),
                    BottomTab::Transcript => Some(self.project.transcript.len()),
                    BottomTab::Mixer | BottomTab::Inspector => None,
                };
                let selected = self.bottom_tab == tab;
                let mut text = egui::RichText::new(tab.label());
                if let Some(count) = count.filter(|count| *count > 0) {
                    text = egui::RichText::new(format!("{} {count}", tab.label()));
                }
                let text = if selected {
                    text.strong().color(egui::Color32::from_rgb(8, 24, 27))
                } else {
                    text.color(theme::TEXT_DIM)
                };
                let response = ui.add(
                    egui::Button::new(text)
                        .fill(if selected { theme::ACCENT } else { theme::BG })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if selected {
                                theme::ACCENT
                            } else {
                                theme::STROKE_SOFT
                            },
                        ))
                        .min_size(egui::vec2(0.0, 28.0)),
                );
                if response.clicked() {
                    self.bottom_tab = tab;
                }
            }
        });
        ui.add_space(6.0);
        ui.separator();
    }

    /// Barra de estado inferior: progreso, mensaje y resumen del montaje.
    fn status_bar(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BAR)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(context, |ui| {
                let rendering = self.export_result.is_some() || self.montage_render.is_some();
                ui.horizontal(|ui| {
                    if rendering {
                        let (pct, eta) = self
                            .render_progress
                            .lock()
                            .map(|state| (state.pct, state.eta_secs))
                            .unwrap_or((0.0, 0.0));
                        ui.add(
                            egui::ProgressBar::new(pct as f32)
                                .show_percentage()
                                .desired_height(12.0)
                                .desired_width(110.0),
                        );
                        if pct > 0.002 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Restante aproximado: {}",
                                    format_clock(eta)
                                ))
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                            );
                        }
                        if theme::bar_button(ui, "Cancelar").clicked() {
                            if let Some(cancel) = &self.export_cancel {
                                cancel.store(true, Ordering::Relaxed);
                                self.status = "Cancelando exportación...".to_owned();
                            }
                        }
                        ui.separator();
                    }
                    let failed = self.status.contains("FALLO") || self.status.contains("error");
                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(11.5)
                            .color(if failed { theme::WARN } else { theme::TEXT_DIM }),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let total = self.project.duration();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} clips · {}",
                                self.project.clips.len(),
                                timecode(total, self.project.fps)
                            ))
                            .monospace()
                            .size(11.5)
                            .color(theme::TEXT_FAINT),
                        );
                        if let Some((start, end)) = self.work_range() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Rango {} → {}",
                                    timecode(start, self.project.fps),
                                    timecode(end, self.project.fps)
                                ))
                                .monospace()
                                .size(11.5)
                                .color(theme::ACCENT),
                            );
                        }
                        ui.label(
                            egui::RichText::new("FFmpeg · Windows x64")
                                .size(11.0)
                                .color(theme::TEXT_FAINT),
                        );
                        let offline = self.missing_media_indices().len();
                        if offline > 0 {
                            ui.label(
                                egui::RichText::new(format!("{offline} medio(s) OFFLINE"))
                                    .size(11.5)
                                    .strong()
                                    .color(theme::DANGER),
                            );
                        }
                    });
                });
            });
    }

    /// Aplica los conmutadores de pista pulsados en una sola entrada de undo.
    fn apply_track_toggles(&mut self, toggles: &[TrackToggle]) {
        let before = self.project.clone();
        let mut affects_render = false;
        for toggle in toggles {
            let (slot, render) = match *toggle {
                TrackToggle::Mute(track) => (self.project.track_mutes.get_mut(track), true),
                TrackToggle::Solo(track) => (self.project.track_solos.get_mut(track), true),
                TrackToggle::Hide(track) => (self.project.video_hidden.get_mut(track), true),
                TrackToggle::LockAudio(track) => (self.project.audio_locked.get_mut(track), false),
                TrackToggle::LockVideo(track) => (self.project.video_locked.get_mut(track), false),
            };
            if let Some(value) = slot {
                *value = !*value;
                affects_render |= render;
            }
        }
        self.finish_edit(before);
        if affects_render {
            self.preview_texture = None;
            self.request_preview();
        }
    }

    /// Deja seleccionado un único clip.
    fn select_only(&mut self, index: usize) {
        self.selected = Some(index);
        self.selection.clear();
        self.selection.insert(index);
    }

    /// Añade o quita un clip de la selección (Ctrl+clic).
    fn toggle_in_selection(&mut self, index: usize) {
        if self.selection.contains(&index) {
            self.selection.remove(&index);
            if self.selected == Some(index) {
                self.selected = self.selection.iter().next().copied();
            }
        } else {
            self.selection.insert(index);
            self.selected = Some(index);
        }
    }

    /// Extiende la selección desde el clip principal hasta el indicado,
    /// tomando todos los que caen en ese intervalo de tiempo (Mayús+clic).
    fn extend_selection_to(&mut self, index: usize) {
        let Some(anchor) = self.selected.filter(|i| *i < self.project.clips.len()) else {
            self.select_only(index);
            return;
        };
        if index >= self.project.clips.len() {
            return;
        }
        let anchor_clip = &self.project.clips[anchor];
        let target_clip = &self.project.clips[index];
        let start = anchor_clip.timeline_start.min(target_clip.timeline_start);
        let end = (anchor_clip.timeline_start + anchor_clip.duration())
            .max(target_clip.timeline_start + target_clip.duration());
        for (other, clip) in self.project.clips.iter().enumerate() {
            if clip.timeline_start >= start - 0.001
                && clip.timeline_start + clip.duration() <= end + 0.001
            {
                self.selection.insert(other);
            }
        }
        self.selection.insert(index);
    }

    fn select_all(&mut self) {
        self.selection = (0..self.project.clips.len()).collect();
        self.selected = self.selection.iter().next().copied();
        self.status = format!("{} clip(s) seleccionado(s)", self.selection.len());
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.selection.clear();
    }

    fn set_edit_tool(&mut self, tool: EditTool) {
        if self.edit_tool == tool {
            return;
        }
        if let Some((_, _, baseline)) = self.drag_edit.take() {
            self.project = baseline;
            self.prune_selection();
            self.request_preview();
        }
        self.edit_tool = tool;
        self.marquee_origin = None;
        self.status = format!("Herramienta: {}", tool.label());
    }

    fn select_track_from(&mut self, index: usize, whole_track: bool) {
        let Some(anchor) = self.project.clips.get(index) else {
            return;
        };
        let track = anchor.track;
        let is_video = anchor.has_video;
        let start = if whole_track {
            0.0
        } else {
            anchor.timeline_start
        };
        self.selection = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.track == track
                    && clip.has_video == is_video
                    && clip.timeline_start >= start - 0.001
            })
            .map(|(clip_index, _)| clip_index)
            .collect();
        self.selected = Some(index);
        self.status = format!("{} clip(s) de pista seleccionados", self.selection.len());
    }

    /// Fila de la biblioteca de medios: miniatura, nombre, duración y avisos.
    fn draw_media_row(&mut self, ui: &mut egui::Ui, row: &media_browser::Row) {
        let index = self
            .selected
            .filter(|index| row.uses.contains(index))
            .unwrap_or(row.uses[0]);
        let Some(clip) = self.project.clips.get(index).cloned() else {
            return;
        };
        let selected = row.uses.iter().any(|index| self.selection.contains(index));
        let row_h = 42.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::Vec2::new(ui.available_width(), row_h),
            egui::Sense::click_and_drag(),
        );
        let kind = match row.kind {
            media_browser::Kind::Video => "Video",
            media_browser::Kind::Audio => "Audio",
            _ if clip.title.is_some() => "Titulo",
            _ if clip.is_adjustment => "Ajuste",
            _ if clip.nested.is_some() => "Anidada",
            _ => "Generado",
        };
        let response = response.on_hover_text(format!(
            "{}\n{}\n{} | {:.1} s | {} usos\n{}\nProxies por uso: {} disponibles, {} ausentes, {} sin proxy\nUso actual: pista {}, {}\nClic: ir al uso; doble clic: monitor de origen\nBoton derecho: seleccionar todos / uso anterior / siguiente",
            row.name, row.path.display(), kind, row.duration, row.uses.len(),
            if row.kind == media_browser::Kind::Generated { "Sin archivo fuente" } else if row.offline { "Fuente OFFLINE" } else { "Fuente disponible" },
            row.proxies[1], row.proxies[2], row.proxies[0], clip.track + 1,
            timecode(clip.timeline_start, self.project.fps),
        ));
        // Arrastrable hacia la timeline: el payload es el índice del clip.
        if response.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), index);
        }
        let painter = ui.painter();
        let fill = if selected {
            theme::ACCENT_SOFT
        } else if response.hovered() {
            theme::CARD_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 4.0, fill);
        if selected {
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        // Barra de la etiqueta de color, como en macOS.
        if let Some(color) = label_color(clip.label) {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 5.0, rect.top() + 7.0),
                    egui::pos2(rect.left() + 8.0, rect.bottom() - 7.0),
                ),
                1.5,
                color,
            );
        }
        // Miniatura si ya está generada.
        let thumb_x = rect.left() + 14.0;
        let thumb_size = row_h - 10.0;
        let mut text_x = thumb_x;
        if clip.has_video {
            if let Some(texture) = self.thumbnails.get(&clip.path) {
                painter.image(
                    texture.id(),
                    egui::Rect::from_min_size(
                        egui::pos2(thumb_x, rect.top() + 5.0),
                        egui::vec2(thumb_size, thumb_size),
                    ),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                text_x += thumb_size + 6.0;
            }
        }
        let name_color = if selected { theme::ACCENT } else { theme::TEXT };
        let mut name_job = egui::text::LayoutJob::simple_singleline(
            row.name.clone(),
            egui::FontId::proportional(12.0),
            name_color,
        );
        name_job.wrap.max_width =
            (rect.right() - text_x - if row.offline { 54.0 } else { 8.0 }).max(1.0);
        name_job.wrap.max_rows = 1;
        painter.galley(
            egui::pos2(text_x, rect.top() + 5.0),
            painter.layout_job(name_job),
            name_color,
        );
        painter.with_clip_rect(rect.intersect(ui.clip_rect())).text(
            egui::pos2(text_x, rect.bottom() - 11.0),
            egui::Align2::LEFT_CENTER,
            format!("{kind} | {:.1} s | {} usos", row.duration, row.uses.len()),
            egui::FontId::proportional(11.0),
            theme::TEXT_FAINT,
        );
        // Un medio ausente se marca en rojo, como en la app Mac.
        if row.offline {
            painter.text(
                egui::pos2(rect.right() - 8.0, rect.top() + 9.0),
                egui::Align2::RIGHT_CENTER,
                "OFFLINE",
                egui::FontId::proportional(10.5),
                theme::DANGER,
            );
        }
        if response.clicked() {
            self.select_only(index);
            self.preview_texture = None;
            self.seek(clip.timeline_start.max(0.0));
        }
        if response.double_clicked() {
            self.select_only(index);
            self.open_source_monitor(index);
        }
        response.context_menu(|ui| {
            if ui
                .button(format!("Seleccionar los {} usos", row.uses.len()))
                .clicked()
            {
                self.select_only(index);
                self.selection.extend(row.uses.iter().copied());
                ui.close_menu();
            }
            for (next, label) in [(false, "Ir al uso anterior"), (true, "Ir al uso siguiente")] {
                if ui
                    .add_enabled(row.uses.len() > 1, egui::Button::new(label))
                    .clicked()
                {
                    if let Some(target) = row.adjacent(self.selected, next) {
                        self.select_only(target);
                        self.preview_texture = None;
                        self.seek(self.project.clips[target].timeline_start.max(0.0));
                    }
                    ui.close_menu();
                }
            }
        });
        ui.add_space(1.0);
    }

    /// Herramientas de edición como tira de iconos (Premiere las tiene en
    /// una barra vertical); el nombre, el atajo y lo que hace van en el
    /// tooltip. Ocupa una fracción del alto del antiguo bloque de botones.
    fn show_edit_toolbar(&mut self, ui: &mut egui::Ui) {
        for tool in EditTool::ALL {
            let active = self.edit_tool == tool;
            let response = ui
                .add(
                    egui::Button::new(egui::RichText::new(tool.icon()).size(15.0).color(
                        if active {
                            egui::Color32::from_rgb(8, 24, 27)
                        } else {
                            theme::TEXT
                        },
                    ))
                    .fill(if active { theme::ACCENT } else { theme::CARD })
                    .min_size(egui::vec2(30.0, 28.0)),
                )
                .on_hover_text(format!(
                    "{} ({})\n{}",
                    tool.label(),
                    tool.shortcut(),
                    tool.hint()
                ));
            if response.clicked() {
                self.set_edit_tool(tool);
            }
        }
    }

    /// Índices seleccionados válidos, en orden ascendente.
    fn selected_indices(&self) -> Vec<usize> {
        self.selection
            .iter()
            .copied()
            .filter(|index| *index < self.project.clips.len())
            .collect()
    }

    /// Descarta índices que ya no existen tras una edición estructural.
    fn prune_selection(&mut self) {
        let count = self.project.clips.len();
        self.selection.retain(|index| *index < count);
        if self.selected.is_some_and(|index| index >= count) {
            self.selected = self.selection.iter().next().copied();
        }
        if let Some(index) = self.selected {
            self.selection.insert(index);
        }
    }

    /// Acerca o aleja el montaje manteniendo el cabezal en su sitio.
    fn zoom_timeline(&mut self, factor: f32) {
        self.zoom_timeline_at(factor, self.playhead);
    }

    fn timeline_extent(&self) -> f64 {
        let duration = self.project.duration();
        if duration < 1.0 {
            10.0
        } else {
            (duration * 1.08).max(duration + 2.0).min(duration + 30.0)
        }
    }

    /// Cambia el zoom manteniendo bajo el puntero el instante indicado.
    fn zoom_timeline_at(&mut self, factor: f32, anchor: f64) {
        let total = self.timeline_extent();
        let view_old = total / self.zoom as f64;
        let ratio = ((anchor - self.hscroll) / view_old).clamp(0.0, 1.0);
        self.zoom = (self.zoom * factor).clamp(1.0, 400.0);
        let view_new = total / self.zoom as f64;
        self.hscroll = (anchor - ratio * view_new).clamp(0.0, (total - view_new).max(0.0));
    }

    /// Hoja de atajos, equivalente al menú de teclado de la app macOS.
    fn show_shortcuts_window(&mut self, context: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = true;
        egui::Window::new("Atajos de teclado")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                let groups: [(&str, &[(&str, &str)]); 6] = [
                    (
                        "Reproducción",
                        &[
                            ("Espacio / L", "Reproducir / detener"),
                            ("K", "Detener la reproducción"),
                            ("J", "Corte anterior"),
                            ("← →", "Un fotograma atrás / adelante"),
                            ("Mayús + ← →", "Un segundo atrás / adelante"),
                            ("↑ ↓", "Corte anterior / siguiente"),
                            ("Inicio / Fin", "Principio / final del montaje"),
                            ("Ctrl+G", "Ir a timecode (HH:MM:SS:FF)"),
                            ("+ / −", "Zoom adelante / atrás"),
                        ],
                    ),
                    (
                        "Herramientas",
                        &[
                            ("A", "Selección y movimiento"),
                            ("U", "Seleccionar hacia delante en una pista"),
                            ("C", "Tijeras: partir donde pulses"),
                            ("R", "Recortar desde cualquier mitad del clip"),
                            ("T", "Recorte ripple que cierra el montaje"),
                            ("H", "Mano: desplazar la timeline"),
                            ("Z / Mayús+clic", "Acercar / alejar con Zoom"),
                            ("G", "Varita: escenas o silencios"),
                        ],
                    ),
                    (
                        "Selección",
                        &[
                            ("Clic", "Seleccionar un clip"),
                            ("Ctrl + clic", "Añadir o quitar de la selección"),
                            ("Mayús + clic", "Extender la selección"),
                            ("Arrastrar el fondo", "Caja de selección"),
                            ("Ctrl+A / Esc", "Seleccionar todo / deseleccionar"),
                            ("Mayús+1…6 / 0", "Etiqueta de color / sin etiqueta"),
                        ],
                    ),
                    (
                        "Edición",
                        &[
                            ("S o Ctrl+K", "Partir en el cabezal"),
                            ("Ctrl+Mayús+K", "Partir todas las pistas"),
                            ("B / V", "Superponer / insertar en el cabezal"),
                            ("D", "Activar o desactivar los clips marcados"),
                            ("Supr", "Quitar dejando hueco"),
                            ("Mayús + Supr", "Quitar y cerrar hueco"),
                            ("Q / W", "Recortar entrada / salida al cabezal"),
                            ("E", "Extender el borde más cercano"),
                            ("F", "Match frame: clip bajo el cabezal"),
                            ("Ctrl + ← →", "Mover los clips un fotograma"),
                            ("Ctrl+D", "Duplicar clip"),
                            ("Ctrl+L", "Separar audio del clip"),
                            ("Ctrl+C / Ctrl+V", "Copiar / pegar clip"),
                            ("Ctrl+Alt+C / V", "Copiar / pegar atributos"),
                        ],
                    ),
                    (
                        "Marcas y multicámara",
                        &[
                            ("M", "Marcador en el cabezal"),
                            ("I / O", "Entrada / salida de trabajo"),
                            ("Alt+X", "Limpiar rango de trabajo"),
                            ("1…4", "Corte a la cámara indicada"),
                        ],
                    ),
                    (
                        "Vista y documento",
                        &[
                            ("Mayús+Z", "Ajustar el montaje a la ventana"),
                            ("Ctrl+[ / ]", "Alejar / acercar"),
                            ("Alt+↑ / ↓", "Pistas más altas / bajas"),
                            ("Ctrl+S", "Guardar"),
                            ("Ctrl+O / Ctrl+I", "Abrir / importar"),
                            ("Ctrl+Mayús+E", "Exportar"),
                            (
                                "Ctrl+Z / Ctrl+Y",
                                "Deshacer / rehacer (también Ctrl+Mayús+Z)",
                            ),
                        ],
                    ),
                ];
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        for (title, rows) in groups {
                            theme::section_label(ui, title);
                            for (keys, description) in rows {
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [120.0, 16.0],
                                        egui::Label::new(
                                            egui::RichText::new(*keys)
                                                .monospace()
                                                .size(11.0)
                                                .color(theme::ACCENT),
                                        ),
                                    );
                                    ui.label(
                                        egui::RichText::new(*description)
                                            .size(11.5)
                                            .color(theme::TEXT_DIM),
                                    );
                                });
                            }
                            ui.add_space(6.0);
                        }
                    });
            });
        self.show_shortcuts = open;
    }

    /// Duración de un fotograma del montaje.
    fn frame_duration(&self) -> f64 {
        self.project.timebase().frame_duration()
    }

    fn quantize_time(&self, time: f64) -> f64 {
        let frame = self.frame_duration();
        (time / frame).round() * frame
    }

    /// Lleva el cabezal a un instante concreto y refresca el monitor.
    fn seek(&mut self, time: f64) {
        let total = self.project.duration();
        self.playhead = time.clamp(0.0, total.max(0.0));
        self.follow_playhead();
        self.request_preview();
    }

    /// Desplaza la vista del montaje para que el cabezal siga visible al navegar.
    fn follow_playhead(&mut self) {
        let total = self.project.duration();
        if self.zoom <= 1.0 || total <= 0.0 {
            self.hscroll = 0.0;
            return;
        }
        let view = total / self.zoom as f64;
        let margin = view * 0.1;
        if self.playhead < self.hscroll + margin {
            self.hscroll = (self.playhead - margin).max(0.0);
        } else if self.playhead > self.hscroll + view - margin {
            self.hscroll = (self.playhead - view + margin).max(0.0);
        }
        self.hscroll = self.hscroll.clamp(0.0, (total - view).max(0.0));
    }

    /// Avanza o retrocede un número de fotogramas exacto.
    fn step_frames(&mut self, frames: i64) {
        let target = self.playhead + frames as f64 * self.frame_duration();
        self.stop_playback();
        self.seek(target);
    }

    /// Salta al corte anterior o siguiente del montaje (flechas arriba/abajo).
    fn go_to_cut(&mut self, forward: bool) {
        let points = self.project.cut_points();
        let epsilon = self.frame_duration() / 2.0;
        let target = if forward {
            points
                .iter()
                .copied()
                .find(|point| *point > self.playhead + epsilon)
        } else {
            points
                .iter()
                .rev()
                .copied()
                .find(|point| *point < self.playhead - epsilon)
        };
        match target {
            Some(time) => {
                self.stop_playback();
                self.seek(time);
                self.status = format!("Corte en {}", timecode(time, self.project.fps));
            }
            None => {
                self.status = if forward {
                    "No hay más cortes por delante".to_owned()
                } else {
                    "No hay más cortes por detrás".to_owned()
                }
            }
        }
    }

    /// Rango de trabajo efectivo, ya ordenado y acotado al montaje.
    fn work_range(&self) -> Option<(f64, f64)> {
        let total = self.project.duration();
        let start = self.work_in.unwrap_or(0.0).clamp(0.0, total);
        let end = self.work_out.unwrap_or(total).clamp(0.0, total);
        if self.work_in.is_none() && self.work_out.is_none() {
            return None;
        }
        if end - start < 0.04 {
            return None;
        }
        Some((start, end))
    }

    fn mark_work_in(&mut self) {
        let time = self.playhead;
        if self.work_out.is_some_and(|out| out <= time + 0.04) {
            self.work_out = None;
        }
        self.work_in = Some(time);
        self.status = format!("Entrada de trabajo en {}", timecode(time, self.project.fps));
    }

    fn mark_work_out(&mut self) {
        let time = self.playhead;
        if self.work_in.is_some_and(|input| input >= time - 0.04) {
            self.work_in = None;
        }
        self.work_out = Some(time);
        self.status = format!("Salida de trabajo en {}", timecode(time, self.project.fps));
    }

    fn clear_work_range(&mut self) {
        self.work_in = None;
        self.work_out = None;
        self.export_range_only = false;
        self.status = "Rango de trabajo limpiado".to_owned();
    }

    /// Copia la selección al portapapeles interno conservando las distancias
    /// entre clips y sus pistas relativas.
    fn copy_selected_clip(&mut self) {
        let indices = self.selected_indices();
        if indices.is_empty() {
            self.status = "Selecciona al menos un clip para copiarlo".to_owned();
            return;
        }
        let origin = indices
            .iter()
            .map(|index| self.project.clips[*index].timeline_start)
            .fold(f64::INFINITY, f64::min);
        self.clip_clipboard_group = indices
            .iter()
            .map(|index| {
                let mut clip = self.project.clips[*index].clone();
                clip.timeline_start -= origin;
                clip
            })
            .collect();
        self.clip_clipboard = self.clip_clipboard_group.first().cloned();
        self.status = format!("{} clip(s) copiado(s)", self.clip_clipboard_group.len());
    }

    /// Pega lo copiado empezando en el cabezal, respetando las distancias
    /// originales entre clips.
    fn paste_clip(&mut self) {
        if self.clip_clipboard_group.is_empty() {
            self.status = "No hay ningún clip copiado".to_owned();
            return;
        }
        if self
            .clip_clipboard_group
            .iter()
            .any(|clip| self.project.lane_locked(clip.track, clip.has_video))
        {
            self.status = "La pista de destino está bloqueada".to_owned();
            return;
        }
        let before = self.project.clone();
        let start = self.playhead.max(0.0);
        let first_new = self.project.clips.len();
        let pasted: Vec<RoughClip> = self
            .clip_clipboard_group
            .iter()
            .map(|clip| {
                let mut copy = clip.clone();
                copy.timeline_start += start;
                copy
            })
            .collect();
        let count = pasted.len();
        self.project.clips.extend(pasted);
        self.selection = (first_new..self.project.clips.len()).collect();
        self.selected = Some(first_new);
        self.finish_edit(before);
        self.status = format!("{count} clip(s) pegado(s) en el cabezal");
    }

    /// Duplica la selección justo detrás de sí misma.
    fn duplicate_selected(&mut self) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        if indices.is_empty() {
            self.status = "Selecciona al menos un clip para duplicarlo".to_owned();
            return;
        }
        let before = self.project.clone();
        let span_start = indices
            .iter()
            .map(|index| self.project.clips[*index].timeline_start)
            .fold(f64::INFINITY, f64::min);
        let span_end = indices
            .iter()
            .map(|index| {
                let clip = &self.project.clips[*index];
                clip.timeline_start + clip.duration()
            })
            .fold(0.0, f64::max);
        let shift = span_end - span_start;
        let first_new = self.project.clips.len();
        let copies: Vec<RoughClip> = indices
            .iter()
            .map(|index| {
                let mut copy = self.project.clips[*index].clone();
                copy.timeline_start += shift;
                copy
            })
            .collect();
        let count = copies.len();
        self.project.clips.extend(copies);
        self.selection = (first_new..self.project.clips.len()).collect();
        self.selected = Some(first_new);
        self.finish_edit(before);
        self.status = format!("{count} clip(s) duplicado(s)");
    }

    /// Superpone la selección (o el portapapeles) en el cabezal: lo que haya
    /// debajo en esa pista se recorta o se parte para dejarle sitio.
    fn overwrite_at_playhead(&mut self) {
        let Some(source) = self.clips_to_place() else {
            self.status = "Copia o selecciona un clip antes de superponer".to_owned();
            return;
        };
        if source
            .iter()
            .any(|clip| self.project.lane_locked(clip.track, clip.has_video))
        {
            self.status = "La pista de destino está bloqueada".to_owned();
            return;
        }
        let start = self.playhead.max(0.0);
        if source.iter().any(|clip| {
            let clip_start = start + clip.timeline_start;
            span_hits_complex_clip(
                &self.project.clips,
                clip.track,
                clip.has_video,
                clip_start,
                clip_start + clip.duration(),
                &[],
            )
        }) {
            self.status =
                "Superposición cancelada: hay una rampa o secuencia anidada debajo".to_owned();
            return;
        }
        let before = self.project.clone();
        let mut placed = Vec::new();
        for mut clip in source {
            clip.timeline_start += start;
            let end = clip.timeline_start + clip.duration();
            clear_track_span(
                &mut self.project.clips,
                clip.track,
                clip.has_video,
                clip.timeline_start,
                end,
                &[],
            );
            placed.push(clip);
        }
        let first_new = self.project.clips.len();
        let count = placed.len();
        self.project.clips.extend(placed);
        self.selection = (first_new..self.project.clips.len()).collect();
        self.selected = Some(first_new);
        self.finish_edit(before);
        self.request_preview();
        self.status = format!("{count} clip(s) superpuesto(s) en el cabezal");
    }

    /// Inserta la selección (o el portapapeles) en el cabezal empujando hacia
    /// la derecha todo lo que venía después en esa pista.
    fn insert_at_playhead(&mut self) {
        let Some(source) = self.clips_to_place() else {
            self.status = "Copia o selecciona un clip antes de insertar".to_owned();
            return;
        };
        if source
            .iter()
            .any(|clip| self.project.lane_locked(clip.track, clip.has_video))
        {
            self.status = "La pista de destino está bloqueada".to_owned();
            return;
        }
        let before = self.project.clone();
        let start = self.playhead.max(0.0);
        let span = source
            .iter()
            .map(|clip| clip.timeline_start + clip.duration())
            .fold(0.0, f64::max);
        let mut lanes: Vec<(usize, bool)> = Vec::new();
        for clip in &source {
            let lane = (clip.track, clip.has_video);
            if !lanes.contains(&lane) {
                lanes.push(lane);
            }
        }
        let mut edited = self.project.clips.clone();
        for (track, is_video) in lanes {
            if !ripple_track_at(&mut edited, track, is_video, start, span) {
                self.status =
                    "No se puede insertar dentro de una rampa o secuencia anidada".to_owned();
                return;
            }
        }
        self.project.clips = edited;
        let placed: Vec<RoughClip> = source
            .into_iter()
            .map(|mut clip| {
                clip.timeline_start += start;
                clip
            })
            .collect();
        let first_new = self.project.clips.len();
        let count = placed.len();
        self.project.clips.extend(placed);
        self.selection = (first_new..self.project.clips.len()).collect();
        self.selected = Some(first_new);
        self.finish_edit(before);
        self.request_preview();
        self.status = format!("{count} clip(s) insertado(s) en el cabezal");
    }

    /// Material a colocar: lo copiado si hay algo, si no la selección actual,
    /// siempre con tiempos relativos al primer clip.
    fn clips_to_place(&self) -> Option<Vec<RoughClip>> {
        if !self.clip_clipboard_group.is_empty() {
            return Some(self.clip_clipboard_group.clone());
        }
        let indices = self.selected_indices();
        if indices.is_empty() {
            return None;
        }
        let origin = indices
            .iter()
            .map(|index| self.project.clips[*index].timeline_start)
            .fold(f64::INFINITY, f64::min);
        Some(
            indices
                .iter()
                .map(|index| {
                    let mut clip = self.project.clips[*index].clone();
                    clip.timeline_start -= origin;
                    clip
                })
                .collect(),
        )
    }

    /// Parte en el cabezal todos los clips de todas las pistas, no solo uno.
    fn split_all_tracks(&mut self) {
        let playhead = self.playhead;
        let splittable = |clip: &RoughClip| {
            let local = playhead - clip.timeline_start;
            clip.nested.is_none()
                && clip
                    .speed_ramp
                    .as_ref()
                    .is_none_or(|points| points.is_empty())
                && local > 0.04
                && local < clip.duration() - 0.04
        };
        let targets: Vec<usize> = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| splittable(clip) && !self.project.clip_locked(clip))
            .map(|(index, _)| index)
            .collect();
        if targets.is_empty() {
            self.status = "No hay clips bajo el cabezal que partir".to_owned();
            return;
        }
        let before = self.project.clone();
        // De mayor a menor para que las inserciones no muevan los índices
        // pendientes.
        for index in targets.iter().rev() {
            let clip = self.project.clips[*index].clone();
            let local = playhead - clip.timeline_start;
            let source_split = clip.in_seconds + local * clip.speed.clamp(0.1, 8.0);
            let mut right = clip.clone();
            self.project.clips[*index].out_seconds = source_split;
            self.project.clips[*index].fade_out_seconds = 0.0;
            right.in_seconds = source_split;
            right.timeline_start = playhead;
            right.fade_in_seconds = 0.0;
            right.transition = None;
            self.project.clips.insert(index + 1, right);
        }
        self.clear_selection();
        self.finish_edit(before);
        self.status = format!("{} clip(s) partido(s) en el cabezal", targets.len());
    }

    /// Aplica una etiqueta de color a toda la selección.
    fn label_selection(&mut self, label: u8) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        if indices.is_empty() {
            self.status = "La selección está en pistas bloqueadas".to_owned();
            return;
        }
        let before = self.project.clone();
        for index in indices {
            self.project.clips[index].label = label;
        }
        self.finish_edit(before);
    }

    /// Activa o desactiva la selección: los clips desactivados siguen en el
    /// montaje pero no se componen ni se exportan.
    fn toggle_selection_enabled(&mut self) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        if indices.is_empty() {
            self.status = "Selecciona al menos un clip".to_owned();
            return;
        }
        let before = self.project.clone();
        // Si hay alguno activo, se apagan todos; si estaban todos apagados,
        // se encienden.
        let turn_off = indices
            .iter()
            .any(|index| self.project.clips[*index].enabled);
        for index in &indices {
            self.project.clips[*index].enabled = !turn_off;
        }
        self.finish_edit(before);
        self.preview_texture = None;
        self.request_preview();
        self.status = if turn_off {
            format!("{} clip(s) desactivado(s)", indices.len())
        } else {
            format!("{} clip(s) activado(s)", indices.len())
        };
    }

    /// Desplaza la selección un número de fotogramas por el montaje.
    fn nudge_selection(&mut self, frames: i64) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        if indices.is_empty() {
            self.status = "Selecciona al menos un clip para moverlo".to_owned();
            return;
        }
        let step = frames as f64 * self.frame_duration();
        let earliest = indices
            .iter()
            .map(|index| self.project.clips[*index].timeline_start)
            .fold(f64::INFINITY, f64::min);
        let step = step.max(-earliest);
        if step.abs() < 1e-9 {
            return;
        }
        let before = self.project.clone();
        for index in &indices {
            let clip = &mut self.project.clips[*index];
            clip.timeline_start = (clip.timeline_start + step).max(0.0);
        }
        self.finish_edit(before);
        self.request_preview();
        self.status = format!("Movidos {:+} fotograma(s)", frames);
    }

    fn copy_attributes(&mut self) {
        let Some(index) = self.selected.filter(|i| *i < self.project.clips.len()) else {
            self.status = "Selecciona un clip del que copiar atributos".to_owned();
            return;
        };
        self.attribute_clipboard = Some(Box::new(self.project.clips[index].clone()));
        self.status = "Atributos copiados; el pegado permite elegir categorias".to_owned();
    }

    /// Pega color, transformación y audio del clip fuente sin tocar sus puntos
    /// de edición ni su posición en el montaje.
    fn paste_attributes(&mut self) {
        let Some(source) = self.attribute_clipboard.clone() else {
            self.status = "No hay atributos copiados".to_owned();
            return;
        };
        let indices = self.selected_indices();
        if indices.is_empty() {
            self.status = "Selecciona el clip destino".to_owned();
            return;
        }
        self.batch_state.paste = Some((indices, self.document_generation, source));
    }

    /// Recorta el borde indicado del clip seleccionado hasta el cabezal (Q/W).
    fn trim_to_playhead(&mut self, start_edge: bool) {
        let Some(index) = self.selected.filter(|i| *i < self.project.clips.len()) else {
            self.status = "Selecciona un clip para recortarlo".to_owned();
            return;
        };
        let clip = self.project.clips[index].clone();
        if self.project.clip_locked(&clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
        {
            self.status = "Desanida o quita la rampa antes de recortar".to_owned();
            return;
        }
        let end = clip.timeline_start + clip.duration();
        if self.playhead <= clip.timeline_start + 0.04 || self.playhead >= end - 0.04 {
            self.status = "Coloca el cabezal dentro del clip para recortarlo".to_owned();
            return;
        }
        let speed = clip.speed.clamp(0.1, 8.0);
        let before = self.project.clone();
        if start_edge {
            let delta = self.playhead - clip.timeline_start;
            let target = &mut self.project.clips[index];
            target.timeline_start = self.playhead;
            target.in_seconds = (clip.in_seconds + delta * speed).max(0.0);
        } else {
            let target = &mut self.project.clips[index];
            target.out_seconds = clip.in_seconds + (self.playhead - clip.timeline_start) * speed;
        }
        self.finish_edit(before);
        self.request_preview();
        self.status = if start_edge {
            "Entrada recortada al cabezal".to_owned()
        } else {
            "Salida recortada al cabezal".to_owned()
        };
    }

    /// Extiende el borde más cercano del clip seleccionado hasta el cabezal (E).
    fn extend_edit(&mut self) {
        let Some(index) = self.selected.filter(|i| *i < self.project.clips.len()) else {
            self.status = "Selecciona un clip para extenderlo".to_owned();
            return;
        };
        let clip = self.project.clips[index].clone();
        if self.project.clip_locked(&clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if clip.nested.is_some()
            || clip
                .speed_ramp
                .as_ref()
                .is_some_and(|points| !points.is_empty())
        {
            self.status = "Desanida o quita la rampa antes de extender".to_owned();
            return;
        }
        let speed = clip.speed.clamp(0.1, 8.0);
        let end = clip.timeline_start + clip.duration();
        let before = self.project.clone();
        if self.playhead > end {
            let source_end = clip
                .source_duration_seconds
                .unwrap_or(clip.out_seconds)
                .max(clip.out_seconds);
            let requested = clip.in_seconds + (self.playhead - clip.timeline_start) * speed;
            if requested > source_end + 0.001 {
                self.status = "No queda más material en el archivo fuente".to_owned();
                return;
            }
            let target = &mut self.project.clips[index];
            target.out_seconds = requested.min(source_end);
            self.status = "Salida extendida al cabezal".to_owned();
        } else if self.playhead < clip.timeline_start {
            let delta = clip.timeline_start - self.playhead;
            let available = clip.in_seconds / speed;
            let delta = delta.min(available);
            if delta <= 0.001 {
                self.status = "No queda material antes de la entrada".to_owned();
                return;
            }
            let target = &mut self.project.clips[index];
            target.timeline_start = clip.timeline_start - delta;
            target.in_seconds = (clip.in_seconds - delta * speed).max(0.0);
            self.status = "Entrada extendida al cabezal".to_owned();
        } else {
            self.status = "Coloca el cabezal fuera del clip para extenderlo".to_owned();
            return;
        }
        self.finish_edit(before);
        self.request_preview();
    }

    /// Selecciona el clip que hay bajo el cabezal en la pista más alta (F).
    fn match_frame(&mut self) {
        let found = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                self.playhead >= clip.timeline_start
                    && self.playhead < clip.timeline_start + clip.duration()
            })
            .max_by_key(|(_, clip)| (clip.has_video, clip.track));
        match found {
            Some((index, clip)) => {
                let name = clip.name();
                let source =
                    clip.in_seconds + (self.playhead - clip.timeline_start) * clip.speed.max(0.1);
                self.select_only(index);
                self.status = format!("{name} · origen {}", timecode(source, self.project.fps));
            }
            None => self.status = "No hay ningún clip bajo el cabezal".to_owned(),
        }
    }

    /// Fundido rápido de 1 s a la entrada y a la salida del clip seleccionado.
    fn quick_fade(&mut self) {
        self.run_batch(
            self.selected_indices(),
            batch::Operation::Fields(vec![
                (batch::Field::FadeIn, 1.0),
                (batch::Field::FadeOut, 1.0),
            ]),
        );
    }

    /// Elimina la selección, opcionalmente cerrando los huecos (ripple).
    fn remove_selected_clip(&mut self, ripple: bool) {
        let indices: Vec<usize> = self.selected_indices();
        if indices.is_empty() {
            return;
        }
        let locked = indices
            .iter()
            .filter(|index| self.project.clip_locked(&self.project.clips[**index]))
            .count();
        let indices: Vec<usize> = indices
            .into_iter()
            .filter(|index| !self.project.clip_locked(&self.project.clips[*index]))
            .collect();
        if indices.is_empty() {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let before = self.project.clone();
        // De mayor a menor para que los índices sigan siendo válidos.
        let mut removed = Vec::new();
        for index in indices.iter().rev() {
            removed.push(self.project.clips.remove(*index));
        }
        if ripple {
            // Ripple en la misma pista: los clips posteriores se desplazan
            // para cerrar cada hueco, empezando por el más tardío.
            removed.sort_by(|left, right| right.timeline_start.total_cmp(&left.timeline_start));
            for gap in &removed {
                let gap_end = gap.timeline_start + gap.duration();
                for clip in &mut self.project.clips {
                    if clip.track == gap.track
                        && clip.has_video == gap.has_video
                        && clip.timeline_start >= gap_end - 0.001
                    {
                        clip.timeline_start -= gap.duration();
                    }
                }
            }
        }
        let count = removed.len();
        self.clear_selection();
        self.finish_edit(before);
        self.status = match (ripple, locked) {
            (_, blocked) if blocked > 0 => {
                format!("{count} clip(s) eliminado(s); {blocked} en pista bloqueada")
            }
            (true, _) => format!("{count} clip(s) eliminado(s) con ripple"),
            (false, _) => format!("{count} clip(s) eliminado(s)"),
        };
    }

    /// Ejecuta una orden del menú contextual sobre el clip indicado.
    fn apply_clip_command(&mut self, index: usize, command: ClipCommand) {
        if index >= self.project.clips.len() {
            return;
        }
        let group_command = matches!(
            command,
            ClipCommand::Copy
                | ClipCommand::CopyAttributes
                | ClipCommand::Duplicate
                | ClipCommand::PasteAttributes
                | ClipCommand::RemoveLeavingGap
                | ClipCommand::QuickFade
                | ClipCommand::ToggleEnabled
        );
        if !group_command
            && self.project.clip_locked(&self.project.clips[index])
            && !matches!(command, ClipCommand::Copy | ClipCommand::CopyAttributes)
        {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        if !group_command || !self.selection.contains(&index) {
            self.select_only(index);
        } else {
            self.selected = Some(index);
        }
        match command {
            ClipCommand::Split => self.split_at_playhead(),
            ClipCommand::Duplicate => self.duplicate_selected(),
            ClipCommand::Copy => self.copy_selected_clip(),
            ClipCommand::CopyAttributes => self.copy_attributes(),
            ClipCommand::PasteAttributes => self.paste_attributes(),
            ClipCommand::RemoveLeavingGap => self.remove_selected_clip(false),
            ClipCommand::RemoveClosingGap => self.remove_selected_clip(true),
            ClipCommand::TrimStartToPlayhead => self.trim_to_playhead(true),
            ClipCommand::TrimEndToPlayhead => self.trim_to_playhead(false),
            ClipCommand::CloseGap => self.close_gap(),
            ClipCommand::Nest => self.nest_selected_track(),
            ClipCommand::Unnest => self.unnest_selected(),
            ClipCommand::Relink => self.relink_selected(),
            ClipCommand::QuickFade => self.quick_fade(),
            ClipCommand::ToggleEnabled => self.toggle_selection_enabled(),
            ClipCommand::SplitAllTracks => self.split_all_tracks(),
            ClipCommand::Overwrite => self.overwrite_at_playhead(),
            ClipCommand::Insert => self.insert_at_playhead(),
            ClipCommand::DetachAudio => self.detach_audio_of(index),
            ClipCommand::FreezeFrame => self.freeze_frame_of(index),
        }
    }

    /// Congela el fotograma bajo el cabezal: parte el clip original y coloca
    /// un still de 3 s (sin audio) justo ahí, como una imagen fija.
    fn freeze_frame_of(&mut self, index: usize) {
        let Some(clip) = self.project.clips.get(index).cloned() else {
            return;
        };
        if !clip.has_video || clip.title.is_some() || clip.nested.is_some() {
            self.status = "Solo se puede congelar un clip de vídeo normal".to_owned();
            return;
        }
        let playhead = self.playhead;
        if playhead <= clip.timeline_start + 0.001
            || playhead >= clip.timeline_start + clip.duration() - 0.001
        {
            self.status = "Sitúa el cabezal dentro del clip primero".to_owned();
            return;
        }
        let speed = clip.speed.clamp(0.1, 8.0);
        let source_t = clip.in_seconds + (playhead - clip.timeline_start) * speed;
        let before = self.project.clone();
        let mut frozen = clip;
        frozen.in_seconds = 0.0;
        frozen.out_seconds = DEFAULT_IMAGE_DURATION;
        frozen.freeze_at = Some(source_t);
        frozen.has_audio = false;
        frozen.speed = 1.0;
        frozen.timeline_start = playhead;
        frozen.fade_in_seconds = 0.0;
        frozen.fade_out_seconds = 0.0;
        frozen.transition = None;
        let span_end = playhead + frozen.duration();
        clear_track_span(
            &mut self.project.clips,
            frozen.track,
            true,
            playhead,
            span_end,
            &[],
        );
        self.project.clips.push(frozen);
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = format!(
            "Fotograma congelado {} s en {}",
            DEFAULT_IMAGE_DURATION,
            timecode(playhead, self.project.fps)
        );
    }

    /// Separa el audio de un clip de vídeo: el original se queda solo con
    /// imagen y una copia solo-audio se coloca en la primera pista de audio
    /// libre en ese tramo.
    fn detach_audio_of(&mut self, index: usize) {
        let Some(clip) = self.project.clips.get(index).cloned() else {
            return;
        };
        if !clip.has_audio {
            self.status = "El clip no tiene audio que separar".to_owned();
            return;
        }
        if clip.title.is_some() || clip.nested.is_some() {
            self.status = "Este tipo de clip no admite separar audio".to_owned();
            return;
        }
        let start = clip.timeline_start;
        let end = clip.timeline_start + clip.duration();
        let free_audio_track = (0..self.project.audio_track_count().min(15))
            .find(|track| {
                !self.project.clips.iter().any(|other| {
                    !other.has_video
                        && other.track == *track
                        && other.timeline_start < end - 0.001
                        && other.timeline_start + other.duration() > start + 0.001
                })
            })
            .unwrap_or_else(|| self.project.audio_track_count().min(15));
        let before = self.project.clone();
        let mut audio_only = clip;
        self.project.clips[index].has_audio = false;
        audio_only.has_video = false;
        audio_only.has_audio = true;
        audio_only.track = free_audio_track;
        audio_only.muted = false;
        audio_only.fade_in_seconds = 0.0;
        audio_only.fade_out_seconds = 0.0;
        audio_only.transition = None;
        self.project.clips.push(audio_only);
        self.select_only(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = format!("Audio separado a la pista A{}", free_audio_track + 1);
    }

    /// Genera de una en una las envolventes de audio que faltan para dibujar la
    /// onda en el montaje, sin bloquear la interfaz.
    fn pump_waveforms(&mut self, context: &egui::Context) {
        if let Some((path, receiver)) = &self.waveform_inflight {
            match receiver.try_recv() {
                Ok(Ok(envelope)) => {
                    self.waveforms.insert(path.clone(), Arc::new(envelope));
                    self.waveform_inflight = None;
                    context.request_repaint();
                }
                Ok(Err(_)) => {
                    // Cachea vacío para no reintentar en bucle un medio ilegible.
                    self.waveforms.insert(path.clone(), Arc::new(Vec::new()));
                    self.waveform_inflight = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    context.request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.waveform_inflight = None,
            }
        }
        if self.waveform_inflight.is_some() || !self.ffmpeg_ready {
            return;
        }
        let pending = self.project.clips.iter().find(|clip| {
            clip.has_audio
                && clip.title.is_none()
                && clip.nested.is_none()
                && !clip.path.as_os_str().is_empty()
                && clip.path.exists()
                && !self.waveforms.contains_key(&clip.path)
        });
        let Some(clip) = pending else {
            return;
        };
        let path = clip.path.clone();
        // Diez minutos de envolvente cubren de sobra los medios habituales.
        let span = clip.out_seconds.max(clip.duration()).clamp(1.0, 600.0);
        let (sender, receiver) = mpsc::channel();
        self.waveform_inflight = Some((path.clone(), receiver));
        std::thread::spawn(move || {
            let _ = sender.send(extract_audio_envelope(&path, 0.0, span));
        });
    }

    /// Cierra el hueco entre el clip seleccionado y el clip anterior de su pista.
    fn close_gap(&mut self) {
        let Some(index) = self.selected else {
            self.status = "Selecciona un clip para cerrar el hueco".to_owned();
            return;
        };
        let clip = &self.project.clips[index];
        if self.project.clip_locked(clip) {
            self.status = "La pista está bloqueada".to_owned();
            return;
        }
        let (track, start) = (clip.track, clip.timeline_start);
        let previous_end = self
            .project
            .clips
            .iter()
            .filter(|other| {
                other.track == track
                    && other.has_video == clip.has_video
                    && other.timeline_start + other.duration() <= start + 0.001
            })
            .map(|other| other.timeline_start + other.duration())
            .fold(0.0_f64, f64::max);
        let gap = start - previous_end;
        if gap <= 0.01 {
            self.status = "No hay hueco que cerrar delante de este clip".to_owned();
            return;
        }
        let before = self.project.clone();
        self.project.clips[index].timeline_start = previous_end;
        self.finish_edit(before);
        self.status = format!("Hueco cerrado ({gap:.2} s)");
    }

    /// Lanza la reproducción fluida del montaje completo en el monitor.
    fn toggle_playback(&mut self) {
        if self.playback.is_some() {
            self.stop_playback();
            return;
        }
        if !self.ffmpeg_ready || self.project.clips.is_empty() {
            return;
        }
        if self.export_result.is_some() || self.montage_render.is_some() {
            self.status = "Espera a que termine el render antes de reproducir".to_owned();
            return;
        }
        // Reproducir dentro del rango arranca en su entrada si el cabezal
        // está fuera, para no esperar a que llegue.
        if self.export_range_only {
            if let Some((start, end)) = self.work_range() {
                if self.playhead < start || self.playhead >= end - 0.04 {
                    self.playhead = start;
                }
            }
        }
        let timebase = self.project.timebase();
        let start_playhead = timebase.seconds(timebase.frames(self.playhead));
        let prepared = prepare_render_clips(&self.effective_clips());
        let mut command = Command::new(tool_path("ffmpeg.exe"));
        command.args(["-v", "error"]);
        let (input_indices, is_title_input) = push_render_inputs(
            &mut command,
            &prepared,
            (MONITOR_WIDTH as u32, MONITOR_HEIGHT as u32),
            self.use_proxies,
            timebase,
        );
        let Ok(mut filters) = build_render_filters(
            &prepared,
            &input_indices,
            &is_title_input,
            (MONITOR_WIDTH as u32, MONITOR_HEIGHT as u32),
            true,
            false,
            &self.project.track_gains,
            self.project.master_gain_db,
            self.project.normalize_loudness,
            timebase,
            None,
        ) else {
            self.status = "El montaje no se puede reproducir (revisa titulos y medios)".to_owned();
            return;
        };
        let monitor_label =
            append_monitor_scopes(&mut filters, self.show_waveform, self.show_vectorscope);
        let Ok(mut child) = command
            .args(["-filter_complex", &filters.join(";")])
            .args(["-map", &format!("[{monitor_label}]")])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            self.status = "No se pudo lanzar FFmpeg para la reproduccion".to_owned();
            return;
        };
        let (sender, receiver) = mpsc::channel::<Option<PreviewFrame>>();
        let skip_frames = timebase.frames(start_playhead).max(0) as u64;
        let stdout = child.stdout.take();
        std::thread::spawn(move || {
            use std::io::Read;
            let frame_len = MONITOR_WIDTH * MONITOR_HEIGHT * 4;
            let Some(mut reader) = stdout else {
                let _ = sender.send(None);
                return;
            };
            let started = std::time::Instant::now();
            let mut index: u64 = 0;
            let mut buffer = vec![0u8; frame_len];
            loop {
                let mut filled = 0;
                while filled < frame_len {
                    match reader.read(&mut buffer[filled..]) {
                        Ok(0) => {
                            let _ = sender.send(None);
                            return;
                        }
                        Ok(n) => filled += n,
                        Err(_) => {
                            let _ = sender.send(None);
                            return;
                        }
                    }
                }
                index += 1;
                if index <= skip_frames {
                    continue;
                }
                // Ritmo en tiempo real: el frame j se entrega en j/fps.
                let due = started
                    + std::time::Duration::from_secs_f64(
                        timebase.seconds((index - skip_frames - 1) as i64),
                    );
                let now = std::time::Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                let frame = PreviewFrame {
                    pixels: buffer.clone(),
                    width: MONITOR_WIDTH,
                    height: MONITOR_HEIGHT,
                };
                if sender.send(Some(frame)).is_err() {
                    return;
                }
            }
        });
        // Proceso de audio: mismo grafo, solo el bus [aout], PCM s16le por pipe.
        let meter: Arc<std::sync::Mutex<(f32, f32)>> = Arc::new(std::sync::Mutex::new((0.0, 0.0)));
        let mut audio_command = Command::new(tool_path("ffmpeg.exe"));
        audio_command.args(["-v", "error"]);
        let (audio_indices, audio_titles) = push_render_inputs(
            &mut audio_command,
            &prepared,
            (MONITOR_WIDTH as u32, MONITOR_HEIGHT as u32),
            self.use_proxies,
            timebase,
        );
        let Ok(audio_filters) = build_render_filters(
            &prepared,
            &audio_indices,
            &audio_titles,
            (MONITOR_WIDTH as u32, MONITOR_HEIGHT as u32),
            false,
            true,
            &self.project.track_gains,
            self.project.master_gain_db,
            self.project.normalize_loudness,
            timebase,
            self.current_loudness_measurement(),
        ) else {
            self.status = "El montaje no se puede reproducir (revisa titulos y medios)".to_owned();
            return;
        };
        let Ok(mut audio_child) = audio_command
            .args(["-filter_complex", &audio_filters.join(";")])
            .args(["-map", "[aout]"])
            .args(["-ss", &format_seconds(start_playhead)])
            .args(["-f", "s16le", "-ar", "48000", "-ac", "2", "pipe:1"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            self.status = "No se pudo lanzar el audio de la reproduccion".to_owned();
            return;
        };
        let Ok((stream, stream_handle)) = rodio::OutputStream::try_default() else {
            let _ = audio_child.kill();
            let _ = audio_child.wait();
            self.status = "No hay dispositivo de audio disponible".to_owned();
            return;
        };
        let Ok(sink) = rodio::Sink::try_new(&stream_handle) else {
            let _ = audio_child.kill();
            let _ = audio_child.wait();
            self.status = "No se pudo abrir el canal de audio".to_owned();
            return;
        };
        let sink = Arc::new(sink);
        if let Some(audio_stdout) = audio_child.stdout.take() {
            let meter_thread = Arc::clone(&meter);
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || {
                use std::io::Read;
                const CHUNK: usize = 9600; // 50 ms a 48 kHz estéreo s16
                let mut reader = audio_stdout;
                let mut buffer = vec![0u8; CHUNK];
                loop {
                    if reader.read_exact(&mut buffer).is_err() {
                        break;
                    }
                    let samples: Vec<i16> = buffer
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    let (mut sum_l, mut sum_r, mut count) = (0.0f64, 0.0f64, 0usize);
                    for pair in samples.chunks_exact(2) {
                        sum_l += (pair[0] as f64 / 32768.0).powi(2);
                        sum_r += (pair[1] as f64 / 32768.0).powi(2);
                        count += 1;
                    }
                    if count > 0 {
                        let rms = (
                            (sum_l / count as f64).sqrt() as f32,
                            (sum_r / count as f64).sqrt() as f32,
                        );
                        if let Ok(mut state) = meter_thread.lock() {
                            *state = rms;
                        }
                    }
                    sink.append(rodio::buffer::SamplesBuffer::new(2, 48000, samples));
                    // Contrapresión: no acumular más de ~1 s en el sink.
                    while sink.len() > 20 {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            });
        }
        self.playback = Some(Playback {
            child,
            audio_child,
            rx: receiver,
            start_playhead,
            last_consumed: 0,
            timebase,
            meter,
            _stream: stream,
            sink,
        });
        self.status = "Reproduciendo el montaje".to_owned();
    }

    /// Consume vídeo con el reloj de audio como maestro para evitar deriva.
    fn poll_playback(&mut self, context: &egui::Context) {
        // Con "solo rango" activo, la reproducción se detiene en la salida.
        let total = match self.work_range() {
            Some((_, end)) if self.export_range_only => end,
            _ => self.project.duration(),
        };
        let Some(playback) = &mut self.playback else {
            return;
        };
        let audio_time = playback.sink.get_pos().as_secs_f64();
        let expected = playback.timebase.frames(audio_time).max(0) as u64;
        let mut reached_end = false;
        while playback.last_consumed < expected {
            match playback.rx.try_recv() {
                Ok(Some(frame)) => {
                    playback.last_consumed += 1;
                    self.playhead = (playback.start_playhead
                        + playback.timebase.seconds(playback.last_consumed as i64))
                    .min(total);
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.pixels,
                    );
                    self.preview_texture = Some(context.load_texture(
                        "program-monitor",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                Ok(None) => {
                    reached_end = true;
                    break;
                }
                Err(_) => break,
            }
        }
        if reached_end || self.playhead >= total {
            let restart_at = self
                .playback
                .as_ref()
                .map(|playback| playback.start_playhead)
                .unwrap_or(0.0);
            self.playback = None;
            if self.loop_playback && reached_end {
                self.playhead = restart_at;
                self.toggle_playback();
                return;
            }
            self.playhead = self.playhead.min(total);
            self.status = "Reproducción terminada".to_owned();
        }
    }

    /// Previsualiza el montaje entero: render rapido a un temporal y ffplay.
    fn play_whole_edit(&mut self) {
        if self.project.clips.is_empty() {
            self.status = "Importa al menos un clip".to_owned();
            return;
        }
        if self.montage_render.is_some() || self.export_result.is_some() {
            return;
        }
        cleanup_old_previews();
        let clips = self.effective_clips();
        let progress = Arc::clone(&self.render_progress);
        if let Ok(mut state) = progress.lock() {
            state.pct = 0.0;
            state.eta_secs = 0.0;
        }
        let size = self.export_size;
        let track_gains = self.project.track_gains.clone();
        let master_gain_db = self.project.master_gain_db;
        let normalize_loudness = self.project.normalize_loudness;
        let timebase = self.project.timebase();
        let measured_loudness = self.current_loudness_measurement().cloned();
        let hw = self.active_hw();
        let (sender, receiver) = mpsc::channel();
        self.montage_render = Some(receiver);
        self.status = "Renderizando previsualización del montaje...".to_owned();
        std::thread::spawn(move || {
            let cancel = Arc::new(AtomicBool::new(false));
            let target = montage_preview_path();
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let result = run_export(
                &clips,
                &target,
                &cancel,
                true,
                size,
                false,
                ExportFormat::Mp4Video,
                &track_gains,
                master_gain_db,
                normalize_loudness,
                timebase,
                measured_loudness.as_ref(),
                hw,
                &progress,
            )
            .map(|()| target);
            let _ = sender.send(result);
        });
    }

    fn poll_montage_render(&mut self) {
        let Some(receiver) = &self.montage_render else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            match result {
                Ok(path) => {
                    let spawn = Command::new(tool_path("ffplay.exe"))
                        .arg("-autoexit")
                        .arg(&path)
                        .creation_flags(CREATE_NO_WINDOW)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                    self.status = match spawn {
                        Ok(_) => "Reproduciendo el montaje en la ventana de FFplay".to_owned(),
                        Err(error) => format!("No se pudo abrir FFplay: {error}"),
                    };
                }
                Err(error) => self.status = format!("Fallo la previsualización: {error}"),
            }
            self.montage_render = None;
        }
    }

    /// Genera miniaturas secuencialmente para los clips de video visibles.
    fn pump_thumbnails(&mut self, context: &egui::Context) {
        if let Some((path, receiver)) = &mut self.thumb_inflight {
            if let Ok(result) = receiver.try_recv() {
                if let Ok(frame) = result {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.pixels,
                    );
                    self.thumbnails.insert(
                        path.clone(),
                        context.load_texture("thumb", image, egui::TextureOptions::LINEAR),
                    );
                }
                self.thumb_inflight = None;
            }
        }
        if self.thumb_inflight.is_none() {
            for clip in &self.project.clips {
                if !clip.has_video || clip.title.is_some() {
                    continue;
                }
                let path = clip.path.clone();
                if path.as_os_str().is_empty()
                    || !path.exists()
                    || self.thumbnails.contains_key(&path)
                {
                    continue;
                }
                let time = clip.in_seconds + (clip.source_duration() * 0.25).min(2.0);
                let thumb_path = path.clone();
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(generate_thumbnail(&thumb_path, time));
                });
                self.thumb_inflight = Some((path, receiver));
                break;
            }
        }
        if self.thumb_inflight.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn poll_setup(&mut self) {
        let Some(receiver) = &self.setup_result else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            self.ffmpeg_ready = multimedia_tools_available();
            self.status = match result {
                Ok(()) if self.ffmpeg_ready => {
                    "Motor multimedia instalado. Ya puedes importar videos.".to_owned()
                }
                Ok(()) => "FFmpeg se instalo, pero Windows aun no lo encuentra. Cierra y vuelve a abrir NovaCut.".to_owned(),
                Err(error) => format!("No se pudo instalar FFmpeg: {error}"),
            };
            self.setup_result = None;
        }
    }

    fn poll_preview(&mut self, context: &egui::Context) {
        let Some(receiver) = &self.preview_result else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            let refresh_pending = self.preview_refresh_pending;
            self.preview_result = None;
            if refresh_pending {
                self.preview_refresh_pending = false;
                self.request_preview();
                return;
            }
            match result {
                Ok(frame) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.pixels,
                    );
                    self.preview_texture = Some(context.load_texture(
                        "program-monitor",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                Err(error) => {
                    self.preview_texture = None;
                    self.status = format!("No se pudo cargar el monitor: {error}");
                }
            }
        }
    }

    fn finish_edit(&mut self, previous: RoughProject) {
        // Si una edición de inspector seguía viva, conserva su baseline como
        // paso independiente antes del comando explícito que llega ahora.
        if let Some((pending_baseline, _)) = self.pending_edit.take() {
            self.undo_stack.push(pending_baseline);
        }
        self.stop_playback();
        self.undo_stack.push(previous);
        while self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.dirty = true;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.preview_texture = None;
        save_recovery(&self.project);
        if self.ffmpeg_ready {
            self.request_preview();
        }
    }

    fn undo(&mut self) {
        // Confirma primero cualquier edición en curso como su propio paso de
        // undo, para que deshacer sea predecible en vez de descartar en
        // silencio texto o arrastres todavía sin comprometer.
        self.flush_pending_edit();
        if let Some(previous) = self.undo_stack.pop() {
            self.stop_playback();
            self.redo_stack.push(self.project.clone());
            self.project = previous;
            self.clear_selection();
            self.playhead = self.playhead.min(self.project.duration());
            save_recovery(&self.project);
            self.status = "Deshacer".to_owned();
            self.refresh_dirty_from_saved();
            self.document_generation = self.document_generation.wrapping_add(1);
            self.preview_texture = None;
            self.request_preview();
        }
    }

    fn redo(&mut self) {
        self.flush_pending_edit();
        if let Some(next) = self.redo_stack.pop() {
            self.stop_playback();
            self.undo_stack.push(self.project.clone());
            self.project = next;
            self.clear_selection();
            save_recovery(&self.project);
            self.status = "Rehacer".to_owned();
            self.refresh_dirty_from_saved();
            self.document_generation = self.document_generation.wrapping_add(1);
            self.preview_texture = None;
            self.request_preview();
        }
    }
}

impl eframe::App for NovaCutWindows {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_export();
        self.poll_import();
        self.poll_settings_save();
        self.poll_autosave();
        self.poll_setup();
        self.poll_preview(context);
        self.poll_montage_render();
        self.poll_playback(context);
        self.pump_thumbnails(context);
        self.pump_waveforms(context);
        self.poll_hw_detection();
        // Primer arranque: en pantallas muy grandes en puntos (un 4K al
        // 100 %, un 1440p al 100 %) la interfaz se agranda sola; después
        // manda lo que elija el usuario.
        if !self.ui_scale_chosen {
            if let Some(monitor) = context.input(|input| input.viewport().monitor_size) {
                let scale = if monitor.x >= 3200.0 {
                    1.5
                } else if monitor.x >= 2400.0 {
                    1.25
                } else {
                    1.0
                };
                context.set_zoom_factor(scale);
                self.ui_scale_chosen = true;
            }
        }
        // Ctrl+= / Ctrl+- / Ctrl+0 los gestiona egui; aquí solo se recuerda.
        self.ui_scale = context.zoom_factor();
        self.project.sync_track_state();
        self.poll_proxy();
        self.poll_loudness();
        self.poll_silences();
        self.poll_scene_cuts();
        self.poll_transcription();
        self.poll_pending_edit(context);
        if self.montage_render.is_some()
            || self.proxy_result.is_some()
            || self.loudness_result.is_some()
            || self.silence_result.is_some()
            || self.scene_cut_result.is_some()
            || self.transcription_result.is_some()
            || self.frame_result.is_some()
            || self.import_result.is_some()
        {
            context.request_repaint_after(std::time::Duration::from_millis(150));
        }
        if self.playback.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(15));
        }
        // Decaimiento del medidor y volumen del monitor.
        if let Some(playback) = &self.playback {
            playback.sink.set_volume(self.monitor_volume);
            if let Ok(state) = playback.meter.lock() {
                self.meter_display.0 = (self.meter_display.0 * 0.85).max(state.0);
                self.meter_display.1 = (self.meter_display.1 * 0.85).max(state.1);
            }
        } else {
            self.meter_display = (0.0, 0.0);
        }
        self.poll_frame();
        let dropped_paths: Vec<PathBuf> = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if self.ffmpeg_ready && !dropped_paths.is_empty() {
            if self.import_result.is_some() {
                self.pending_drop_paths.extend(dropped_paths);
            } else {
                let position = context.input(|input| input.pointer.hover_pos());
                self.pending_drop_paths = dropped_paths;
                self.pending_drop_position = position;
            }
        }
        let center_key = context.input_mut(|input| {
            input.consume_key(
                egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
                egui::Key::P,
            )
        });
        if center_key && self.pending_document_action.is_none() && self.pending_recovery.is_none() {
            if self.command_center.open {
                self.command_center.open = false;
            } else {
                self.command_center.open();
            }
        }
        let center_blocks_keys = self.command_center.open || center_key;
        let keyboard_shortcuts = !center_blocks_keys && !context.wants_keyboard_input();
        let select_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::A));
        let track_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::U));
        let blade_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::C));
        let trim_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::R));
        let ripple_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::T));
        let hand_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::H));
        let zoom_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Z));
        let magic_tool_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::G));
        let goto_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::G));
        let detach_audio_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::L));
        let undo_shortcut = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
        let redo_shortcut = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
                    || input.consume_key(
                        egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
                        egui::Key::Z,
                    )
            });
        let save_shortcut = !center_blocks_keys
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::S));
        let split_shortcut = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::K));
        let nle_split = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::S));
        let delete_selected = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            });
        let ripple_delete = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::SHIFT, egui::Key::Delete)
                    || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Backspace)
            });
        let go_home = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Home));
        let go_end = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::End));
        let nudge_left = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
        let nudge_right = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
        let jump_left = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft));
        let jump_right = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight)
            });
        let previous_cut = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let next_cut = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let mark_in = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::I));
        let mark_out = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::O));
        let clear_range = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::X));
        let trim_start_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Q));
        let trim_end_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::W));
        let extend_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::E));
        let match_frame_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F));
        let copy_clip_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::C));
        let paste_clip_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::V));
        let duplicate_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::D));
        let copy_attributes_key = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::CTRL.plus(egui::Modifiers::ALT),
                    egui::Key::C,
                )
            });
        let paste_attributes_key = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::CTRL.plus(egui::Modifiers::ALT),
                    egui::Key::V,
                )
            });
        let fit_zoom_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::Z));
        let zoom_in_key = !center_blocks_keys
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, egui::Key::CloseBracket)
            });
        let zoom_out_key = !center_blocks_keys
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, egui::Key::OpenBracket)
            });
        let zoom_plus_key = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
            });
        let zoom_minus_key = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Minus));
        let taller_tracks = !center_blocks_keys
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowUp));
        let shorter_tracks = !center_blocks_keys
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown));
        let import_key = !center_blocks_keys
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::I));
        let open_key = !center_blocks_keys
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::O));
        let export_key = !center_blocks_keys
            && context.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
                    egui::Key::E,
                )
            });
        let select_all_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::A));
        let deselect_key = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let split_all_key = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
                    egui::Key::K,
                )
            });
        let overwrite_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::B));
        let insert_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::V));
        let toggle_enabled_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::D));
        let nudge_clip_left = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft));
        let nudge_clip_right = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight));
        let shortcuts_key = keyboard_shortcuts
            && context.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
            });
        let add_marker_shortcut = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::M));
        let cam1 = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Num1));
        let cam2 = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Num2));
        let cam3 = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Num3));
        let cam4 = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Num4));
        let preview_shortcut = keyboard_shortcuts
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space));
        let play_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::L));
        let stop_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::K));
        let prev_cut_key = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::J));
        for (pressed, tool) in [
            (select_tool_key, EditTool::Select),
            (track_tool_key, EditTool::TrackSelect),
            (blade_tool_key, EditTool::Blade),
            (trim_tool_key, EditTool::Trim),
            (ripple_tool_key, EditTool::RippleTrim),
            (hand_tool_key, EditTool::Hand),
            (zoom_tool_key, EditTool::Zoom),
            (magic_tool_key, EditTool::Magic),
        ] {
            if pressed {
                self.set_edit_tool(tool);
            }
        }
        if undo_shortcut {
            self.undo();
        }
        if redo_shortcut {
            self.redo();
        }
        if save_shortcut {
            self.save_project(false);
        }
        if goto_key {
            self.show_goto = true;
            self.goto_text.clear();
        }
        if detach_audio_key {
            if let Some(index) = self.selected {
                self.apply_clip_command(index, ClipCommand::DetachAudio);
            }
        }
        if split_shortcut || nle_split {
            self.split_at_playhead();
        }
        // Con palabras seleccionadas en la transcripción, Supr edita por
        // texto (siempre con ripple) en lugar de borrar el clip.
        let text_delete =
            self.bottom_tab == BottomTab::Transcript && self.transcript_selection.is_some();
        if (delete_selected || ripple_delete) && text_delete {
            self.delete_transcript_selection();
        } else if delete_selected || ripple_delete {
            self.remove_selected_clip(ripple_delete);
        }
        if go_home {
            self.seek(0.0);
        }
        if go_end {
            self.seek(self.project.duration());
        }
        if nudge_left {
            self.step_frames(-1);
        }
        if nudge_right {
            self.step_frames(1);
        }
        if jump_left {
            self.stop_playback();
            self.seek(self.playhead - 1.0);
        }
        if jump_right {
            self.stop_playback();
            self.seek(self.playhead + 1.0);
        }
        if previous_cut {
            self.go_to_cut(false);
        }
        if next_cut {
            self.go_to_cut(true);
        }
        if mark_in {
            self.mark_work_in();
        }
        if mark_out {
            self.mark_work_out();
        }
        if clear_range {
            self.clear_work_range();
        }
        if trim_start_key {
            self.trim_to_playhead(true);
        }
        if trim_end_key {
            self.trim_to_playhead(false);
        }
        if extend_key {
            self.extend_edit();
        }
        if match_frame_key {
            self.match_frame();
        }
        if copy_attributes_key {
            self.copy_attributes();
        } else if copy_clip_key {
            self.copy_selected_clip();
        }
        if paste_attributes_key {
            self.paste_attributes();
        } else if paste_clip_key {
            self.paste_clip();
        }
        if duplicate_key {
            self.duplicate_selected();
        }
        if fit_zoom_key {
            self.zoom = 1.0;
            self.hscroll = 0.0;
            self.status = "Montaje completo ajustado a la timeline".to_owned();
        }
        if zoom_in_key || zoom_plus_key {
            self.zoom_timeline(1.3);
        }
        if zoom_out_key || zoom_minus_key {
            self.zoom_timeline(1.0 / 1.3);
        }
        if taller_tracks {
            self.track_height = (self.track_height * 1.2).min(180.0);
        }
        if shorter_tracks {
            self.track_height = (self.track_height / 1.2).max(28.0);
        }
        if import_key && self.ffmpeg_ready {
            self.import_media();
        }
        if open_key {
            self.request_document_action(DocumentAction::Open);
        }
        if export_key && self.ffmpeg_ready && self.export_result.is_none() {
            self.export();
        }
        if shortcuts_key {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if select_all_key {
            self.select_all();
        }
        if deselect_key {
            self.clear_selection();
        }
        if split_all_key {
            self.split_all_tracks();
        }
        if overwrite_key {
            self.overwrite_at_playhead();
        }
        if insert_key {
            self.insert_at_playhead();
        }
        if toggle_enabled_key {
            self.toggle_selection_enabled();
        }
        if nudge_clip_left {
            self.nudge_selection(-1);
        }
        if nudge_clip_right {
            self.nudge_selection(1);
        }
        // Etiquetas de color rápidas con Mayús+1…6, como en la app macOS.
        for (key, label) in [
            (egui::Key::Num1, 1u8),
            (egui::Key::Num2, 2),
            (egui::Key::Num3, 3),
            (egui::Key::Num4, 4),
            (egui::Key::Num5, 5),
            (egui::Key::Num6, 6),
            (egui::Key::Num0, 0),
        ] {
            if keyboard_shortcuts
                && context.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, key))
            {
                self.label_selection(label);
            }
        }
        if add_marker_shortcut {
            self.add_marker();
        }
        for (pressed, camera) in [(cam1, 1usize), (cam2, 2), (cam3, 3), (cam4, 4)] {
            if pressed {
                self.multicam_cut(camera);
            }
        }
        if preview_shortcut && self.ffmpeg_ready {
            self.toggle_playback();
        }
        // Transporte JKL, como en los montadores clásicos.
        if play_key && self.ffmpeg_ready {
            self.toggle_playback();
        }
        if stop_key && self.playback.is_some() {
            self.stop_playback();
            self.status = "Reproducción detenida (K)".to_owned();
        }
        if prev_cut_key {
            self.go_to_cut(false);
        }
        let title = format!(
            "{}{} - NovaCut Windows",
            self.project.name,
            if self.has_unsaved_changes() { " *" } else { "" }
        );
        context.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if self.export_result.is_some()
            || self.setup_result.is_some()
            || self.preview_result.is_some()
            || self.montage_render.is_some()
        {
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(theme::BAR)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    // Barra en una fila desde 1280 px: la palabra NOVACUT solo
                    // aparece si sobra sitio (el logo ya identifica la app).
                    let roomy = ui.available_width() >= 1400.0;
                    ui.spacing_mut().item_spacing.x = if roomy { 8.0 } else { 6.0 };
                    // Marca de la app + nombre del proyecto + punto de estado.
                    theme::logo_mark(ui);
                    if roomy {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new("NOVACUT")
                                .strong()
                                .size(15.0)
                                .color(theme::TEXT),
                        );
                    }
                    theme::bar_separator(ui);
                    let before_name = self.project.clone();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.project.name)
                                .desired_width(140.0)
                                .font(egui::TextStyle::Small),
                        )
                        .changed()
                    {
                        self.queue_edit(before_name);
                    }
                    theme::dirty_dot(ui, self.has_unsaved_changes());
                    theme::bar_separator(ui);

                    // Documento: un menú «Archivo», como en cualquier app de
                    // Windows; siete botones sueltos partían la barra en dos
                    // filas en pantallas de 1280 px.
                    let mut file_action: Option<u8> = None;
                    egui::menu::menu_button(ui, egui::RichText::new("Archivo ▾"), |ui| {
                        ui.set_min_width(220.0);
                        let mut item = |ui: &mut egui::Ui, label: &str, shortcut: &str, code: u8| {
                            if ui
                                .add(egui::Button::new(label).shortcut_text(shortcut))
                                .clicked()
                            {
                                file_action = Some(code);
                                ui.close_menu();
                            }
                        };
                        item(ui, "Nuevo", "", 0);
                        item(ui, "Abrir…", "", 1);
                        ui.separator();
                        item(ui, "Guardar", "Ctrl+S", 2);
                        item(ui, "Guardar como…", "", 3);
                        ui.separator();
                        item(ui, "Importar secuencia (.ncrough)…", "", 4);
                        item(ui, "Importar EDL…", "", 5);
                        item(ui, "Exportar EDL…", "", 6);
                        if !self.recent_projects.is_empty() {
                            ui.separator();
                            ui.menu_button("Recientes", |ui| {
                                for recent in self.recent_projects.clone() {
                                    let label = recent
                                        .file_name()
                                        .map(|name| name.to_string_lossy().to_string())
                                        .unwrap_or_else(|| recent.display().to_string());
                                    if ui
                                        .button(label)
                                        .on_hover_text(recent.display().to_string())
                                        .clicked()
                                    {
                                        ui.close_menu();
                                        self.request_document_action(DocumentAction::OpenPath(
                                            recent,
                                        ));
                                    }
                                }
                            });
                        }
                    }).response.on_hover_text("Nuevo, abrir, guardar, recientes, EDL e importar secuencia");
                    match file_action {
                        Some(0) => self.request_document_action(DocumentAction::New),
                        Some(1) => self.request_document_action(DocumentAction::Open),
                        Some(2) => self.save_project(false),
                        Some(3) => self.save_project(true),
                        Some(4) => self.import_nested_project(),
                        Some(5) => self.import_edl(),
                        Some(6) => self.export_edl(),
                        _ => {}
                    }
                    theme::bar_separator(ui);

                    // Historial
                    if ui
                        .add_enabled(
                            !self.undo_stack.is_empty() || self.pending_edit.is_some(),
                            egui::Button::new(egui::RichText::new("⟲").size(15.0)),
                        )
                        .on_hover_text("Deshacer (Ctrl+Z)")
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(
                            !self.redo_stack.is_empty(),
                            egui::Button::new(egui::RichText::new("⟳").size(15.0)),
                        )
                        .on_hover_text("Rehacer (Ctrl+Y)")
                        .clicked()
                    {
                        self.redo();
                    }
                    theme::bar_separator(ui);

                    // Medio
                    if ui
                        .add_enabled(
                            self.ffmpeg_ready,
                            egui::Button::new(
                                egui::RichText::new(if self.import_result.is_some() {
                                    "+ Importar…"
                                } else {
                                    "+ Importar"
                                })
                                .size(11.0),
                            ),
                        )
                        .on_hover_text(
                            if self.import_result.is_some() {
                                "Analizando archivos en segundo plano…"
                            } else {
                                "Añadir vídeos o audio al montaje"
                            },
                        )
                        .clicked()
                    {
                        self.import_media();
                    }
                    if self.import_result.is_some() {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    theme::bar_separator(ui);

                    // Timeline
                    if theme::bar_button(ui, "✂ Partir")
                        .on_hover_text("Partir en el cabezal (S)")
                        .clicked()
                    {
                        self.split_at_playhead();
                    }
                    if theme::bar_button(ui, "✂ Todas")
                        .on_hover_text("Partir todas las pistas en el cabezal (Ctrl+Mayús+K)")
                        .clicked()
                    {
                        self.split_all_tracks();
                    }
                    let mut insert_action: Option<u8> = None;
                    egui::menu::menu_custom_button(
                        ui,
                        egui::Button::new(egui::RichText::new("+ Insertar").size(11.0)),
                        |ui| {
                            if ui.button("Título en el cabezal").clicked() {
                                insert_action = Some(0);
                                ui.close_menu();
                            }
                            if ui
                                .button("Rótulo inferior (lower third)")
                                .on_hover_text("Texto con caja semitransparente en el tercio inferior")
                                .clicked()
                            {
                                insert_action = Some(20);
                                ui.close_menu();
                            }
                            if ui
                                .button("Mate de color")
                                .on_hover_text("Fondo liso a pantalla completa, como Nuevo elemento › Mate de color")
                                .clicked()
                            {
                                insert_action = Some(21);
                                ui.close_menu();
                            }
                            if ui
                                .button("Capa de ajuste")
                                .on_hover_text(
                                    "Gradúa todo lo compuesto por debajo en su rango de tiempo",
                                )
                                .clicked()
                            {
                                insert_action = Some(1);
                                ui.close_menu();
                            }
                            if ui.button("Marcador (M)").on_hover_text("Añade un marcador en el cabezal (M)").clicked() {
                                insert_action = Some(2);
                                ui.close_menu();
                            }
                            if ui.button("Subtítulo en el cabezal").clicked() {
                                insert_action = Some(3);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Pegar clip copiado (Ctrl+V)").clicked() {
                                insert_action = Some(4);
                                ui.close_menu();
                            }
                            if ui
                                .button("Superponer en el cabezal (B)")
                                .on_hover_text("Coloca lo copiado recortando lo que haya debajo")
                                .clicked()
                            {
                                insert_action = Some(6);
                                ui.close_menu();
                            }
                            if ui
                                .button("Insertar en el cabezal (V)")
                                .on_hover_text("Empuja hacia la derecha lo que venía después")
                                .clicked()
                            {
                                insert_action = Some(7);
                                ui.close_menu();
                            }
                            if ui.button("Duplicar seleccionado (Ctrl+D)").on_hover_text("Copia el clip seleccionado justo a continuación (Ctrl+D)").clicked() {
                                insert_action = Some(5);
                                ui.close_menu();
                            }
                        },
                    );
                    match insert_action {
                        Some(0) => self.add_title_at_playhead(),
                        Some(20) => self.add_styled_title_at_playhead(
                            Titulo {
                                text: "Nombre Apellido".to_owned(),
                                position_x: 0.3,
                                position_y: 0.84,
                                size: 54.0,
                                style: efectos::TitleStyle {
                                    box_opacity: 0.6,
                                    shadow: true,
                                    ..Default::default()
                                },
                                ..Titulo::default()
                            },
                            "Rótulo inferior creado; edita el texto en el inspector",
                        ),
                        Some(21) => self.add_styled_title_at_playhead(
                            Titulo {
                                style: efectos::TitleStyle {
                                    matte: Some([0.08, 0.08, 0.1]),
                                    ..Default::default()
                                },
                                ..Titulo::default()
                            },
                            "Mate de color creado; elige su color en el inspector",
                        ),
                        Some(1) => self.add_adjustment_layer_at_playhead(),
                        Some(2) => self.add_marker(),
                        Some(3) => self.add_subtitle(),
                        Some(4) => self.paste_clip(),
                        Some(5) => self.duplicate_selected(),
                        Some(6) => self.overwrite_at_playhead(),
                        Some(7) => self.insert_at_playhead(),
                        _ => {}
                    }
                    ui.toggle_value(&mut self.snap_enabled, egui::RichText::new("⊓ Imán").size(11.0))
                        .on_hover_text("Ajuste magnético a bordes, marcadores y cabezal");
                    theme::bar_separator(ui);

                    // Rango de trabajo: entrada, salida y limpiar.
                    if theme::bar_button(ui, "[ Entrada")
                        .on_hover_text("Entrada de trabajo en el cabezal (I)")
                        .clicked()
                    {
                        self.mark_work_in();
                    }
                    if theme::bar_button(ui, "Salida ]")
                        .on_hover_text("Salida de trabajo en el cabezal (O)")
                        .clicked()
                    {
                        self.mark_work_out();
                    }
                    if let Some((start, end)) = self.work_range() {
                        ui.label(
                            egui::RichText::new(format_clock(end - start))
                                .monospace()
                                .size(11.5)
                                .color(theme::ACCENT),
                        );
                        ui.toggle_value(
                            &mut self.export_range_only,
                            egui::RichText::new("Solo rango").size(11.0),
                        )
                        .on_hover_text(
                            "La exportación se limita al rango; la reproducción se detiene en la salida",
                        );
                        if theme::bar_button(ui, "×")
                            .on_hover_text("Limpiar rango (Alt+X)")
                            .clicked()
                        {
                            self.clear_work_range();
                        }
                    }
                    theme::bar_separator(ui);

                    // Exportación: presets + botón principal en acento.
                    // horizontal_wrapped no mide bien los ComboBox: si no
                    // caben, se salta de fila a mano en vez de cortarlos.
                    // En un layout con salto, `available_width` es el ancho
                    // total; lo que queda en la fila es hasta el borde.
                    if ui.max_rect().right() - ui.cursor().min.x < 110.0 {
                        ui.end_row();
                    }
                    egui::ComboBox::from_id_salt("preset-export")
                        .width(92.0)
                        .selected_text(match self.export_size {
                            (1280, 720) => "720p",
                            (1920, 1080) => "1080p",
                            (3840, 2160) => "4K",
                            (1080, 1920) => "Shorts/Reels/TikTok 1080x1920",
                            (1080, 1080) => "Cuadrado (Instagram) 1080x1080",
                            _ => "Personalizado",
                        })
                        .show_ui(ui, |ui| {
                            let mut chosen = self.export_size;
                            ui.selectable_value(&mut chosen, (1280, 720), "720p").on_hover_text("1280×720, horizontal");
                            ui.selectable_value(&mut chosen, (1920, 1080), "1080p (YouTube)").on_hover_text("1920×1080, horizontal: la resolución más habitual");
                            ui.selectable_value(&mut chosen, (3840, 2160), "4K").on_hover_text("3840×2160, horizontal");
                            ui.selectable_value(
                                &mut chosen,
                                (1080, 1920),
                                "Shorts/Reels/TikTok 1080x1920",
                            ).on_hover_text("Vertical 1080×1920 con reencuadre centrado");
                            ui.selectable_value(
                                &mut chosen,
                                (1080, 1080),
                                "Cuadrado (Instagram) 1080x1080",
                            ).on_hover_text("1080×1080 para el feed de Instagram");
                            self.export_size = chosen;
                        });
                    if ui.max_rect().right() - ui.cursor().min.x < 136.0 {
                        ui.end_row();
                    }
                    egui::ComboBox::from_id_salt("formato-export")
                        .width(118.0)
                        .selected_text(self.export_format.short_name())
                        .show_ui(ui, |ui| {
                            let mut chosen = self.export_format;
                            for format in ExportFormat::ALL {
                                ui.selectable_value(&mut chosen, format, format.label());
                            }
                            self.export_format = chosen;
                        });
                    if let Some(backend) = self.hw_backends.first().copied() {
                        let accelerable = matches!(
                            self.export_format,
                            ExportFormat::Mp4Video | ExportFormat::Mp4Hevc
                        );
                        let changed = ui
                            .add_enabled(
                                accelerable,
                                egui::SelectableLabel::new(
                                    self.hardware_encoding,
                                    egui::RichText::new("⚡ GPU").size(11.0),
                                ),
                            )
                            .on_hover_text(format!(
                                "Codifica con {} (varias veces más rápido). Si falla, se repite con CPU.",
                                backend.label()
                            ))
                            .on_disabled_hover_text("Solo H.264 y HEVC usan la GPU")
                            .clicked();
                        if changed {
                            self.hardware_encoding = !self.hardware_encoding;
                        }
                    }
                    if ui
                        .add_enabled(
                            self.ffmpeg_ready && self.export_result.is_none(),
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "Exportar {}{}",
                                    self.export_format.extension().to_uppercase(),
                                    if self.export_range_only && self.work_range().is_some() {
                                        " · rango"
                                    } else {
                                        ""
                                    }
                                ))
                                .size(11.0)
                                .strong()
                                .color(egui::Color32::from_rgb(8, 24, 27)),
                            )
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(0.0, 24.0)),
                        ).on_hover_text("Exporta el montaje (o solo el rango, si está activo) con el formato elegido").on_disabled_hover_text("Exporta el montaje (o solo el rango, si está activo) con el formato elegido")
                        .clicked()
                    {
                        self.export();
                    }
                    if self.export_result.is_some() {
                        let (pct, _) = self
                            .render_progress
                            .lock()
                            .map(|state| (state.pct as f32, state.eta_secs))
                            .unwrap_or((0.0, 0.0));
                        ui.add(
                            egui::ProgressBar::new(pct)
                                .desired_height(14.0)
                                .desired_width(150.0)
                                .show_percentage(),
                        );
                        if theme::bar_button(ui, "Cancelar").on_hover_text("Detiene la exportación; el archivo de destino anterior no se toca").clicked() {
                            if let Some(cancel) = &self.export_cancel {
                                cancel.store(true, Ordering::Relaxed);
                                self.status = "Cancelando exportación...".to_owned();
                            }
                        }
                    }
                    theme::bar_separator(ui);
                    let current_scale = self.ui_scale;
                    egui::menu::menu_button(ui, egui::RichText::new("Aa"), |ui| {
                        ui.label(
                            egui::RichText::new("Tamaño de la interfaz")
                                .strong()
                                .color(theme::TEXT),
                        );
                        for scale in UI_SCALES {
                            let label = format!("{:.0} %", scale * 100.0);
                            if ui
                                .selectable_label((current_scale - scale).abs() < 0.01, label)
                                .clicked()
                            {
                                ui.ctx().set_zoom_factor(scale);
                                ui.close_menu();
                            }
                        }
                        ui.label(
                            egui::RichText::new("Ctrl+= / Ctrl+- / Ctrl+0")
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                        );
                    })
                    .response
                    .on_hover_text("Tamaño de la interfaz (Ctrl+= / Ctrl+-)");
                    if theme::bar_button(ui, "?")
                        .on_hover_text("Atajos de teclado")
                        .clicked()
                    {
                        self.show_shortcuts = !self.show_shortcuts;
                    }
                    if theme::bar_button(ui, "🔍")
                        .on_hover_text("Ctrl+Mayus+P | Clips, marcadores, subtitulos y acciones")
                        .clicked()
                    {
                        self.command_center.open();
                    }
                });
            });
        self.show_recovery_dialog(context);
        self.show_unsaved_dialog(context);
        self.show_goto_dialog(context);
        self.show_source_monitor(context);
        self.poll_source_frame(context);
        self.show_silence_review(context);
        self.show_scene_cut_review(context);
        self.show_shortcuts_window(context);
        self.show_command_center(context);

        if self.monitor_fullscreen {
            self.show_fullscreen_monitor(context);
            return;
        }

        if !self.ffmpeg_ready {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(20)))
                .show(context, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(70.0);
                        theme::logo_mark(ui);
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("Prepara NovaCut para editar")
                                .strong()
                                .size(17.0),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(
                                "NovaCut necesita FFmpeg para leer, previsualizar y exportar video.",
                            )
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                        );
                        ui.label(
                            egui::RichText::new("La instalacion es automatica y solo se hace una vez.")
                                .size(12.0)
                                .color(theme::TEXT_DIM),
                        );
                        ui.add_space(18.0);
                        if self.setup_result.is_some() {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new(
                                    "Descargando e instalando FFmpeg (~100 MB)...\nEsto puede tardar varios minutos.",
                                )
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                            );
                        } else {
                            if theme::accent_button(ui, "Instalar FFmpeg automáticamente")
                                .on_hover_text("Descarga e instala FFmpeg sin usar la terminal")
                                .clicked()
                            {
                                self.install_ffmpeg();
                            }
                            ui.add_space(6.0);
                            if theme::bar_button(ui, "Descargar FFmpeg manualmente").on_hover_text("Abre la página de descarga de FFmpeg").clicked() {
                                self.open_ffmpeg_download(context);
                            }
                        }
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(
                                "Otra opcion: copia ffmpeg.exe, ffprobe.exe y ffplay.exe junto a\nnovacut-windows.exe y pulsa Volver a comprobar.",
                            )
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                        );
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Volver a comprobar").size(11.0),
                            )).on_hover_text("Vuelve a buscar FFmpeg en el equipo")
                            .clicked()
                        {
                            self.ffmpeg_ready = multimedia_tools_available();
                            self.status = if self.ffmpeg_ready {
                                "Motor multimedia encontrado".to_owned()
                            } else {
                                "FFmpeg sigue sin estar disponible".to_owned()
                            };
                        }
                    });
                });
            egui::TopBottomPanel::bottom("status")
                .frame(
                    egui::Frame::new()
                        .fill(theme::BAR)
                        .inner_margin(egui::Margin::symmetric(12, 5)),
                )
                .show(context, |ui| {
                    ui.horizontal(|ui| {
                        if self.setup_result.is_some() {
                            ui.spinner();
                        }
                        ui.label(
                            egui::RichText::new(&self.status)
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                        );
                    });
                });
            return;
        }

        let project_before_inspector = self.project.clone();
        let mut trim_changed = false;
        let mut toggle_enabled = false;
        let mut label_request: Option<u8> = None;
        let mut relink_requested = false;
        let mut relink_all_requested = false;
        let mut proxy_requested = false;
        // El resumen se calcula antes: dentro del inspector `self` ya está
        // prestado en exclusiva por el clip que se está editando.
        let proxy_summary = self.proxy_cache_summary();
        let proxy_is_current = self.selected_proxy_is_current();
        let mut proxy_limit_gb = self.proxy_limit_gb;
        let mut trim_cache_requested = false;
        let mut nest_requested = false;
        let mut unnest_requested = false;
        // Panel de medios a la izquierda, como el "MEDIOS" de la app macOS:
        // lista de clips del proyecto; un clic selecciona y centra el cabezal.
        // Columnas laterales proporcionales a la ventana: en 900 px de ancho
        // dejaban al centro solo 320 px; en 4K se quedaban diminutas.
        let screen_width = context.screen_rect().width();
        egui::SidePanel::left("medios")
            .default_width((screen_width * 0.17).clamp(180.0, 320.0))
            .min_width(160.0)
            .max_width((screen_width * 0.24).max(170.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(context, |ui| {
                use media_browser::{Kind, Proxy, Sort};
                theme::panel_header(ui, "Medios", None);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media_filter)
                            .desired_width(ui.available_width() - 30.0)
                            .font(egui::TextStyle::Small)
                            .hint_text("Nombre o ruta: palabras…"),
                    );
                    if !self.media_filter.is_empty() && ui.small_button("×").on_hover_text("Borra el texto de búsqueda").clicked() {
                        self.media_filter.clear();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (kind, label) in [(Kind::All, "Todos"), (Kind::Video, "Video"), (Kind::Audio, "Audio"), (Kind::Generated, "Generados")] {
                        ui.selectable_value(&mut self.media_options.kind, kind, label);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.media_options.offline_only, "Offline").on_hover_text("Muestra solo medios cuyo archivo original no está en disco");
                    egui::ComboBox::from_id_salt("media_proxy_filter")
                        .width(115.0)
                        .selected_text(match self.media_options.proxy {
                            Proxy::All => "Proxy: todos",
                            Proxy::None => "Sin proxy",
                            Proxy::Available => "Proxy disponible",
                            Proxy::Missing => "Proxy ausente",
                        })
                        .show_ui(ui, |ui| {
                            for (state, label) in [(Proxy::All, "Proxy: todos"), (Proxy::None, "Sin proxy"), (Proxy::Available, "Proxy disponible"), (Proxy::Missing, "Proxy ausente")] {
                                ui.selectable_value(&mut self.media_options.proxy, state, label);
                            }
                        }).response.on_hover_text("Coincide si al menos un uso tiene este estado. Disponible comprueba el archivo, no su vigencia.");
                });
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("media_sort")
                        .width(85.0)
                        .selected_text(match self.media_options.sort {
                            Sort::Name => "Nombre", Sort::Duration => "Duración", Sort::Usage => "Usos",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.media_options.sort, Sort::Name, "Nombre").on_hover_text("Ordenar por nombre");
                            ui.selectable_value(&mut self.media_options.sort, Sort::Duration, "Duración").on_hover_text("Ordenar por duración");
                            ui.selectable_value(&mut self.media_options.sort, Sort::Usage, "Usos").on_hover_text("Ordenar por número de usos en el montaje");
                        });
                    if ui.small_button(if self.media_options.descending { "Desc." } else { "Asc." }).on_hover_text("Cambia el sentido del orden").clicked() {
                        self.media_options.descending = !self.media_options.descending;
                    }
                    if ui.small_button("Restablecer").on_hover_text("Quita búsqueda, filtros y orden").clicked() {
                        self.media_filter.clear();
                        self.media_options = media_browser::Options::default();
                    }
                });
                self.media_file_status.begin_frame();
                let uses = self.project.clips.iter().enumerate().map(|(index, clip)| {
                    let generated = clip.title.is_some() || clip.is_adjustment
                        || clip.nested.is_some() || clip.path.as_os_str().is_empty();
                    media_browser::Use {
                        index, path: clip.path.clone(), name: clip.name(),
                        kind: if generated { Kind::Generated } else if clip.has_video { Kind::Video } else { Kind::Audio },
                        duration: if generated { clip.duration() } else { clip.source_duration_seconds.unwrap_or(clip.out_seconds) },
                        start: clip.timeline_start, track: clip.track,
                        offline: !generated && !self.media_file_status.is_file(&clip.path),
                        proxy: if generated { Proxy::None } else {
                            match &clip.proxy {
                                None => Proxy::None,
                                Some(path) if self.media_file_status.is_file(path) => Proxy::Available,
                                Some(_) => Proxy::Missing,
                            }
                        },
                    }
                }).collect();
                let rows = media_browser::rows(uses, &self.media_filter, &self.media_options);
                ui.small(format!("{} fuentes / {} usos", rows.len(), rows.iter().map(|row| row.uses.len()).sum::<usize>()));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in &rows {
                        self.draw_media_row(ui, row);
                    }
                    if rows.is_empty() {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(if self.project.clips.is_empty() {
                                    "Arrastra vídeos o audio aquí"
                                } else {
                                    "Ningún medio coincide con los filtros"
                                })
                                .size(11.0)
                                .color(theme::TEXT_FAINT),
                            );
                            if !self.project.clips.is_empty() && ui.small_button("Restablecer filtros").on_hover_text("Quita búsqueda, filtros y orden").clicked() {
                                self.media_filter.clear();
                                self.media_options = media_browser::Options::default();
                            }
                        });
                    }
                });
            });
        // Los paneles de la columna derecha pueden editar metadatos (subtítulos,
        // marcadores, mezcla): la foto previa sirve de paso de deshacer.
        let metadata_before = self.project.clone();
        let mut metadata_changed = false;
        let mut burn_changed = false;
        egui::SidePanel::right("inspector")
            .default_width((screen_width * 0.24).clamp(250.0, 440.0))
            .min_width(240.0)
            .max_width((screen_width * 0.34).max(250.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(context, |ui| {
                self.side_tabs(ui);
                // El contenido se adapta a la columna; nunca la ensancha.
                ui.set_max_width(ui.available_width());
                if self.bottom_tab != BottomTab::Inspector {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.bottom_tab {
                            BottomTab::Mixer => self.mixer_tab(ui, &mut metadata_changed),
                            BottomTab::Subtitles => {
                                self.subtitles_tab(ui, &mut metadata_changed, &mut burn_changed)
                            }
                            BottomTab::Markers => self.markers_tab(ui, &mut metadata_changed),
                            BottomTab::Clips => self.clips_tab(ui, &mut metadata_changed),
                            BottomTab::Transcript => self.transcript_tab(ui),
                            BottomTab::Inspector => {}
                        });
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let fps = self.project.fps;
                    self.batch_paste_dialog(context);
                    let selection_size = self.selected_indices().len();
                    if selection_size > 1 {
                        self.batch_inspector(ui);
                        return;
                    }
                    if let Some(clip) = self
                        .selected
                        .and_then(|index| self.project.clips.get_mut(index))
                    {
                        // Ficha del clip: nombre, pista y posición en timecode.
                        ui.label(
                            egui::RichText::new(clip.name())
                                .strong()
                                .size(13.0)
                                .color(theme::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{} · {} → {}",
                                if clip.has_video { "V" } else { "A" },
                                clip.track + 1,
                                timecode(clip.timeline_start, fps),
                                timecode(clip.timeline_start + clip.duration(), fps)
                            ))
                            .monospace()
                            .size(11.5)
                            .color(theme::TEXT_FAINT),
                        );
                        toggle_enabled |= ui
                            .selectable_label(
                                !clip.enabled,
                                egui::RichText::new(if clip.enabled {
                                    "Activo · pulsa para desactivar (D)"
                                } else {
                                    "DESACTIVADO · pulsa para activar (D)"
                                })
                                .size(11.5)
                                .color(if clip.enabled {
                                    theme::TEXT_DIM
                                } else {
                                    theme::WARN
                                }),
                            ).on_hover_text("Activa o desactiva el clip sin borrarlo (D)")
                            .clicked();
                        ui.separator();
                        if clip.nested.is_some() {
                            ui.colored_label(
                                egui::Color32::from_rgb(130, 190, 255),
                                "Secuencia compuesta: edita sus clips después de desanidarla.",
                            );
                            unnest_requested |= ui.button("Desanidar secuencia").on_hover_text("Devuelve los clips de la secuencia al montaje").clicked();
                            ui.separator();
                            ui.disable();
                        }
                        if clip.title.is_some() {
                            let Some(title) = clip.title.as_mut() else {
                                unreachable!()
                            };
                            if let Some(captions) = title.style.captions.as_mut() {
                                theme::section_label(ui, "Subtítulos animados");
                                egui::ComboBox::from_id_salt("caption_preset")
                                    .selected_text(captions.preset.label())
                                    .show_ui(ui, |ui| {
                                        for preset in subtitulos_animados::CaptionPreset::ALL {
                                            trim_changed |= ui
                                                .selectable_value(
                                                    &mut captions.preset,
                                                    preset,
                                                    preset.label(),
                                                )
                                                .changed();
                                        }
                                    });
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut captions.max_words, 1..=8)
                                            .text("Palabras por página"),
                                    )
                                    .changed();
                                trim_changed |=
                                    ui.checkbox(&mut captions.uppercase, "MAYÚSCULAS").on_hover_text("Muestra los subtítulos en mayúsculas").changed();
                                let mut rgb = [
                                    captions.highlight[0] as f32,
                                    captions.highlight[1] as f32,
                                    captions.highlight[2] as f32,
                                ];
                                ui.horizontal(|ui| {
                                    ui.label("Resaltado");
                                    if ui.color_edit_button_rgb(&mut rgb).changed() {
                                        captions.highlight =
                                            [rgb[0] as f64, rgb[1] as f64, rgb[2] as f64];
                                        trim_changed = true;
                                    }
                                });
                                trim_changed |= ui
                                    .checkbox(
                                        &mut captions.follow_transcript,
                                        "Seguir la transcripción",
                                    )
                                    .on_hover_text(
                                        "Se rehace sola al editar por texto o volver a transcribir",
                                    )
                                    .changed();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} palabras · tamaño, Y y color de texto abajo",
                                        captions.words.len()
                                    ))
                                    .size(11.5)
                                    .color(theme::TEXT_FAINT),
                                );
                            } else {
                                ui.label("Texto del título");
                                trim_changed |=
                                    ui.text_edit_singleline(&mut title.text).changed();
                            }
                            ui.label("Tamaño");
                            trim_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut title.size)
                                        .speed(0.5)
                                        .range(8.0..=400.0),
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label("X");
                                trim_changed |= ui
                                    .add(egui::Slider::new(&mut title.position_x, 0.0..=1.0))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y");
                                trim_changed |= ui
                                    .add(egui::Slider::new(&mut title.position_y, 0.0..=1.0))
                                    .changed();
                            });
                            theme::section_label(ui, "Color");
                            ui.horizontal(|ui| {
                                ui.label("R");
                                trim_changed |= ui
                                    .add(egui::Slider::new(&mut title.red, 0.0..=1.0))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("G");
                                trim_changed |= ui
                                    .add(egui::Slider::new(&mut title.green, 0.0..=1.0))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("B");
                                trim_changed |= ui
                                    .add(egui::Slider::new(&mut title.blue, 0.0..=1.0))
                                    .changed();
                            });
                            theme::section_label(ui, "Apariencia");
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut title.style.box_opacity, 0.0..=1.0)
                                        .text("Caja de fondo"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut title.style.outline, 0.0..=12.0)
                                        .text("Contorno (px)"),
                                )
                                .changed();
                            trim_changed |=
                                ui.checkbox(&mut title.style.shadow, "Sombra paralela").on_hover_text("Sombra detrás del texto para leerlo sobre fondos claros").changed();
                            let mut matte = title.style.matte.is_some();
                            if ui.checkbox(&mut matte, "Fondo a pantalla completa").on_hover_text("Rellena todo el lienzo con un color bajo el texto").changed() {
                                title.style.matte = matte.then_some([0.08, 0.08, 0.1]);
                                trim_changed = true;
                            }
                            if let Some(color) = title.style.matte.as_mut() {
                                let mut rgb = [color[0] as f32, color[1] as f32, color[2] as f32];
                                ui.horizontal(|ui| {
                                    ui.label("Color de fondo");
                                    if ui.color_edit_button_rgb(&mut rgb).changed() {
                                        *color = [rgb[0] as f64, rgb[1] as f64, rgb[2] as f64];
                                        trim_changed = true;
                                    }
                                });
                            }
                            if !clip.path.as_os_str().is_empty()
                                && ui.button("Quitar título").on_hover_text("Vuelve a mostrar la imagen del clip").clicked()
                            {
                                clip.title = None;
                                trim_changed = true;
                            }
                        } else if clip.is_adjustment {
                            ui.colored_label(
                                egui::Color32::from_rgb(130, 190, 255),
                                "Gradúa todo lo compuesto por debajo en su pista, dentro de su rango de tiempo.",
                            );
                        } else {
                            // Ruta, proxy y caché: se consultan poco, así que
                            // van plegados salvo que haya un problema.
                            let media_problem = (!clip.path.as_os_str().is_empty()
                                && !clip.path.exists())
                                || clip.proxy.as_ref().is_some_and(|proxy| !proxy.is_file())
                                || proxy_is_current == Some(false);
                            egui::CollapsingHeader::new("Medio y proxy")
                                .id_salt("inspector_media")
                                .open(media_problem.then_some(true))
                                .show(ui, |ui| {
                            ui.small(clip.path.display().to_string());
                            if !clip.path.as_os_str().is_empty() && !clip.path.exists() {
                                ui.colored_label(
                                    egui::Color32::from_rgb(246, 83, 83),
                                    "MEDIO OFFLINE",
                                );
                                ui.horizontal(|ui| {
                                    relink_requested |= ui.button("Revincular...").clicked();
                                    relink_all_requested |= ui
                                        .button("Buscar todos en una carpeta...")
                                        .on_hover_text(
                                            "Localiza de una vez todos los medios offline dentro de la carpeta elegida",
                                        )
                                        .clicked();
                                });
                            }
                            if let Some(proxy) = &clip.proxy {
                                ui.small(format!("Proxy: {}", proxy.display()));
                                if !proxy.is_file() {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(246, 83, 83),
                                        "PROXY AUSENTE",
                                    );
                                    proxy_requested |=
                                        ui.button("Regenerar proxy").on_hover_text("Vuelve a crear el proxy ligero de este medio").clicked();
                                } else if proxy_is_current == Some(false) {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(232, 176, 68),
                                        "PROXY DESACTUALIZADO",
                                    );
                                    ui.small("El medio cambió en disco o el proxy es de una versión anterior. La exportación sigue usando el original.");
                                    proxy_requested |=
                                        ui.button("Regenerar proxy").on_hover_text("Vuelve a crear el proxy ligero de este medio").clicked();
                                }
                                if ui.button("Quitar proxy").on_hover_text("El monitor usará el archivo original").clicked() {
                                    clip.proxy = None;
                                    trim_changed = true;
                                }
                            } else if clip.has_video && clip.nested.is_none() {
                                proxy_requested |= ui.button("Crear proxy 540p").clicked();
                            }
                            ui.small(&proxy_summary);
                            ui.horizontal(|ui| {
                                ui.small("Límite");
                                ui.add(
                                    egui::DragValue::new(&mut proxy_limit_gb)
                                        .speed(1.0)
                                        .range(0.0..=1000.0)
                                        .suffix(" GB"),
                                )
                                .on_hover_text("Cero desactiva el recorte automático. Los proxies enlazados nunca se desalojan.");
                                trim_cache_requested |= ui
                                    .add_enabled(proxy_limit_gb > 0.0, egui::Button::new("Recortar"))
                                    .on_hover_text("Desaloja los proxies sin usar más antiguos hasta entrar en el límite")
                                    .clicked();
                            });
                                });
                        }
                        ui.add_space(12.0);
                        theme::section_label(ui, "Etiqueta");
                        ui.horizontal(|ui| {
                            for option in 0u8..=6 {
                                let selected = clip.label == option;
                                let fill =
                                    label_color(option).unwrap_or(egui::Color32::from_gray(60));
                                let text = if option == 0 { "×" } else { "" };
                                let button = egui::Button::new(
                                    egui::RichText::new(text).color(egui::Color32::WHITE),
                                )
                                .min_size(egui::vec2(20.0, 18.0))
                                .fill(fill)
                                .stroke(if selected {
                                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE)
                                } else {
                                    egui::Stroke::NONE
                                });
                                if ui.add(button).clicked() {
                                    label_request = Some(option);
                                }
                            }
                        });
                        let trim_allowed = clip.nested.is_none()
                            && clip
                                .speed_ramp
                                .as_ref()
                                .is_none_or(|points| points.is_empty());
                        if !trim_allowed {
                            ui.small("Desanida o quita la rampa para editar entrada/salida.");
                        }
                        let source_limit = clip
                            .source_duration_seconds
                            .unwrap_or(clip.out_seconds)
                            .max(clip.out_seconds);
                        theme::section_label(ui, "Tiempo");
                        // Etiqueta | valor en rejilla: la mitad de alto que la
                        // lista de secciones anterior.
                        egui::Grid::new("clip_timing")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("Entrada");
                                trim_changed |= ui
                                    .add_enabled(
                                        trim_allowed,
                                        egui::DragValue::new(&mut clip.in_seconds)
                                            .speed(0.04)
                                            .max_decimals(2)
                                            .suffix(" s")
                                            .range(0.0..=(clip.out_seconds - 0.001).max(0.0)),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("Salida");
                                trim_changed |= ui
                                    .add_enabled(
                                        trim_allowed,
                                        egui::DragValue::new(&mut clip.out_seconds)
                                            .speed(0.04)
                                            .max_decimals(2)
                                            .suffix(" s")
                                            .range((clip.in_seconds + 0.001)..=source_limit),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("Duración");
                                ui.label(format!("{:.2} s", clip.duration()));
                                ui.end_row();
                                ui.label("Inicio");
                                trim_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut clip.timeline_start)
                                            .speed(0.04)
                                            .max_decimals(2)
                                            .range(0.0..=86_400.0)
                                            .suffix(" s"),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("Pista");
                                // Se muestra igual que en la timeline (V1, A1…),
                                // aunque internamente las pistas cuenten desde 0.
                                trim_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut clip.track)
                                            .speed(0.1)
                                            .range(0..=15)
                                            .prefix(if clip.has_video { "V" } else { "A" })
                                            .custom_formatter(|value, _| {
                                                format!("{}", value as usize + 1)
                                            })
                                            .custom_parser(|text| {
                                                text.trim()
                                                    .trim_start_matches(['V', 'A', 'v', 'a'])
                                                    .parse::<f64>()
                                                    .ok()
                                                    .map(|number| number - 1.0)
                                            }),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("Velocidad");
                                trim_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut clip.speed)
                                            .speed(0.05)
                                            .range(0.1..=8.0)
                                            .suffix("x"),
                                    )
                                    .changed();
                                ui.end_row();
                            });
                        if let Some(source_duration) = clip.source_duration_seconds {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Medio de {:.2} s · quedan {:.2} s después de la salida",
                                    source_duration,
                                    (source_duration - clip.out_seconds).max(0.0)
                                ))
                                .size(11.5)
                                .color(theme::TEXT_FAINT),
                            );
                        }
                        ui.collapsing("Rampa de velocidad", |ui| {
                            if clip.speed_ramp.is_none() {
                                if ui.button("Activar rampa").on_hover_text("Cambia la velocidad a lo largo del clip").clicked() {
                                    clip.speed_ramp = Some(vec![
                                        SpeedPoint {
                                            source_t: 0.0,
                                            speed: clip.speed,
                                        },
                                        SpeedPoint {
                                            source_t: clip.source_duration(),
                                            speed: clip.speed,
                                        },
                                    ]);
                                    trim_changed = true;
                                }
                                return;
                            }
                            let source_duration = clip.source_duration();
                            let timeline_duration = clip.duration().max(0.001);
                            let local_t =
                                (self.playhead - clip.timeline_start).clamp(0.0, timeline_duration);
                            let source_here = source_duration * local_t / timeline_duration;
                            let points = clip.speed_ramp.as_mut().expect("rampa comprobada");
                            let can_remove = points.len() > 2;
                            let mut remove = None;
                            for (index, point) in points.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    trim_changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut point.source_t)
                                                .range(0.0..=source_duration)
                                                .speed(0.05)
                                                .prefix("t ")
                                                .suffix(" s"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut point.speed)
                                                .range(0.1..=8.0)
                                                .speed(0.05)
                                                .suffix("x"),
                                        )
                                        .changed();
                                    if can_remove && ui.button("×").on_hover_text("Quita este punto de velocidad").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            }
                            if let Some(index) = remove {
                                points.remove(index);
                                trim_changed = true;
                            }
                            if ui.button("Añadir punto en cabezal").on_hover_text("Nuevo punto de velocidad donde está el cabezal").clicked() {
                                points.push(SpeedPoint {
                                    source_t: source_here,
                                    speed: clip.speed,
                                });
                                points.sort_by(|left, right| {
                                    left.source_t.total_cmp(&right.source_t)
                                });
                                trim_changed = true;
                            }
                            if ui.button("Quitar rampa").on_hover_text("Vuelve a una velocidad constante").clicked() {
                                clip.speed_ramp = None;
                                trim_changed = true;
                            }
                        });
                        if clip.has_video && clip.title.is_none() {
                            ui.separator();
                            theme::section_label(ui, "Color");
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.exposure, -1.0..=1.0)
                                        .text("Exposición"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.contrast, -1.0..=1.0)
                                        .text("Contraste"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.saturation, -1.0..=1.0)
                                        .text("Saturación"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.vignette, -1.0..=1.0)
                                        .text("Viñeta"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.blur, 0.0..=1.0).text("Desenfoque"),
                                )
                                .changed();
                            trim_changed |=
                                efectos::inspector_video(ui, &mut clip.fx, clip.is_adjustment);
                            if !clip.is_adjustment {
                                ui.collapsing("Croma (pantalla verde/azul)", |ui| {
                                if clip.chroma.is_none() {
                                    if ui.button("Activar croma").on_hover_text("Vuelve transparente el fondo verde o azul").clicked() {
                                        clip.chroma = Some(Chroma {
                                            red: 0.0,
                                            green: 1.0,
                                            blue: 0.0,
                                            tolerance: default_tolerance(),
                                            smooth: default_smooth(),
                                            spill: default_spill(),
                                        });
                                        trim_changed = true;
                                    }
                                } else {
                                    let Some(chroma) = clip.chroma.as_mut() else {
                                        unreachable!()
                                    };
                                    ui.label("Color de pantalla");
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.red, 0.0..=1.0).text("R"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.green, 0.0..=1.0)
                                                .text("V"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.blue, 0.0..=1.0)
                                                .text("A"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.tolerance, 0.0..=1.0)
                                                .text("Tolerancia"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.smooth, 0.0..=1.0)
                                                .text("Suavizado"),
                                        )
                                        .changed();
                                    trim_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut chroma.spill, 0.0..=1.0)
                                                .text("Derrame"),
                                        )
                                        .changed();
                                    if ui.button("Desactivar croma").on_hover_text("Quita el croma").clicked() {
                                        clip.chroma = None;
                                        trim_changed = true;
                                    }
                                }
                                });
                            }
                            ui.collapsing("Ruedas de color", |ui| {
                                let Some(wheels) = clip.wheels.as_mut() else {
                                    clip.wheels = Some(Wheels::default());
                                    return;
                                };
                                let rows = [
                                    (
                                        "Sombras",
                                        [
                                            &mut wheels.shadows_r,
                                            &mut wheels.shadows_g,
                                            &mut wheels.shadows_b,
                                        ],
                                    ),
                                    (
                                        "Medios",
                                        [&mut wheels.mid_r, &mut wheels.mid_g, &mut wheels.mid_b],
                                    ),
                                    (
                                        "Altas",
                                        [
                                            &mut wheels.high_r,
                                            &mut wheels.high_g,
                                            &mut wheels.high_b,
                                        ],
                                    ),
                                ];
                                let labels = ["R", "G", "B"];
                                for (name, channels) in rows.into_iter() {
                                    ui.horizontal(|ui| {
                                        ui.monospace(name);
                                        for (channel_index, channel) in
                                            channels.into_iter().enumerate()
                                        {
                                            trim_changed |= ui
                                                .add_sized(
                                                    [54.0, 16.0],
                                                    egui::Slider::new(channel, -1.0..=1.0)
                                                        .text(labels[channel_index]),
                                                )
                                                .changed();
                                        }
                                    });
                                }
                                if ui.button("Neutras").on_hover_text("Pone las tres ruedas en neutro").clicked() {
                                    clip.wheels = Some(Wheels::default());
                                    trim_changed = true;
                                }
                            });
                            ui.collapsing("Curvas", |ui| {
                                if clip.curves.is_none() {
                                    if ui.button("Activar curvas").on_hover_text("Curvas de luminancia y de cada canal R, G y B").clicked() {
                                        clip.curves = Some(Curves {
                                            luma: identity_channel(),
                                            red: identity_channel(),
                                            green: identity_channel(),
                                            blue: identity_channel(),
                                        });
                                        trim_changed = true;
                                    }
                                } else {
                                    let Some(curves) = clip.curves.as_mut() else {
                                        unreachable!()
                                    };
                                    let channels: [(&str, &mut Vec<CurvePoint>); 4] = [
                                        ("Luma", &mut curves.luma),
                                        ("R", &mut curves.red),
                                        ("G", &mut curves.green),
                                        ("B", &mut curves.blue),
                                    ];
                                    for (name, points) in channels {
                                        ui.monospace(name);
                                        let point_count = points.len();
                                        let mut remove_point: Option<usize> = None;
                                        for (point_index, point) in points.iter_mut().enumerate() {
                                            ui.horizontal(|ui| {
                                                trim_changed |= ui
                                                    .add(
                                                        egui::DragValue::new(&mut point.x)
                                                            .speed(0.01)
                                                            .range(0.0..=1.0)
                                                            .prefix("x "),
                                                    )
                                                    .changed();
                                                trim_changed |= ui
                                                    .add(
                                                        egui::DragValue::new(&mut point.y)
                                                            .speed(0.01)
                                                            .range(0.0..=1.0)
                                                            .prefix("y "),
                                                    )
                                                    .changed();
                                                if point_count > 2 && ui.button("−").on_hover_text("Quita el último punto de la curva").clicked() {
                                                    remove_point = Some(point_index);
                                                }
                                            });
                                        }
                                        if let Some(index) = remove_point {
                                            points.remove(index);
                                            trim_changed = true;
                                        }
                                        if point_count < 8 && ui.button("+ punto").on_hover_text("Añade un punto a la curva").clicked() {
                                            let last_x = points.last().map(|p| p.x).unwrap_or(1.0);
                                            points.push(CurvePoint {
                                                x: ((last_x + 1.0) / 2.0).clamp(0.0, 1.0),
                                                y: 0.5,
                                            });
                                            trim_changed = true;
                                        }
                                    }
                                    if ui.button("Identidad").on_hover_text("Vuelve la curva a una recta (sin cambios)").clicked() {
                                        curves.luma = identity_channel();
                                        curves.red = identity_channel();
                                        curves.green = identity_channel();
                                        curves.blue = identity_channel();
                                        trim_changed = true;
                                    }
                                }
                            });
                            ui.separator();
                            if !clip.is_adjustment {
                                ui.label("Composición");
                                egui::ComboBox::from_id_salt("fusion_mode")
                                    .selected_text(clip.fusion.label())
                                    .show_ui(ui, |ui| {
                                        for mode in Fusion::ALL {
                                            trim_changed |= ui
                                                .selectable_value(
                                                    &mut clip.fusion,
                                                    mode,
                                                    mode.label(),
                                                )
                                                .changed();
                                        }
                                    });
                                if matches!(clip.fusion, Fusion::Color | Fusion::Luminosity) {
                                    ui.small("FFmpeg aproxima este modo con composición normal.");
                                }
                            }
                            ui.collapsing("Máscara", |ui| {
                                if clip.mask.is_none() {
                                    if ui.button("Activar máscara").on_hover_text("Limita el clip a un rectángulo o una elipse").clicked() {
                                        clip.mask = Some(Mask::default());
                                        trim_changed = true;
                                    }
                                    return;
                                }
                                let mask = clip.mask.as_mut().expect("máscara comprobada");
                                egui::ComboBox::from_id_salt("mask_shape")
                                    .selected_text(match mask.shape {
                                        MaskShape::Rectangle => "Rectángulo",
                                        MaskShape::Ellipse => "Elipse",
                                    })
                                    .show_ui(ui, |ui| {
                                        trim_changed |= ui
                                            .selectable_value(
                                                &mut mask.shape,
                                                MaskShape::Rectangle,
                                                "Rectángulo",
                                            ).on_hover_text("Máscara rectangular")
                                            .changed();
                                        trim_changed |= ui
                                            .selectable_value(
                                                &mut mask.shape,
                                                MaskShape::Ellipse,
                                                "Elipse",
                                            ).on_hover_text("Máscara elíptica")
                                            .changed();
                                    });
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut mask.position_x, 0.0..=1.0)
                                            .text("X"),
                                    )
                                    .changed();
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut mask.position_y, 0.0..=1.0)
                                            .text("Y"),
                                    )
                                    .changed();
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut mask.size_x, 0.01..=1.0)
                                            .text("Ancho"),
                                    )
                                    .changed();
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut mask.size_y, 0.01..=1.0)
                                            .text("Alto"),
                                    )
                                    .changed();
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut mask.feather, 0.0..=1.0)
                                            .text("Pluma"),
                                    )
                                    .changed();
                                trim_changed |=
                                    ui.checkbox(&mut mask.inverted, "Invertida").on_hover_text("Muestra lo de fuera de la máscara en lugar de lo de dentro").changed();
                                if ui.button("Desactivar máscara").on_hover_text("Quita la máscara").clicked() {
                                    clip.mask = None;
                                    trim_changed = true;
                                }
                            });
                            ui.collapsing("LUT 3D", |ui| {
                                if let Some(path) = &clip.lut {
                                    ui.small(path.display().to_string());
                                    if ui.button("Quitar LUT").on_hover_text("Quita la LUT del clip").clicked() {
                                        clip.lut = None;
                                        trim_changed = true;
                                    }
                                } else if ui.button("Cargar .cube...").on_hover_text("Aplica una LUT 3D (.cube) al clip").clicked() {
                                    if let Some(path) = FileDialog::new()
                                        .add_filter("LUT 3D", &["cube"])
                                        .pick_file()
                                    {
                                        clip.lut = Some(path);
                                        trim_changed = true;
                                    }
                                }
                            });
                            ui.separator();
                            if clip.keyframes.is_some() {
                                ui.separator();
                                ui.label("Animación (keyframes)");
                                ui.small("t local del clip; export e interpolan por tramos");
                                let duration = clip.duration();
                                let local_t =
                                    (self.playhead - clip.timeline_start).clamp(0.0, duration);
                                if ui.button("Añadir keyframe aquí").on_hover_text("Guarda posición, escala y opacidad donde está el cabezal").clicked() {
                                    let keyframes = clip.keyframes.get_or_insert_with(Vec::new);
                                    let keyframe = TransformKeyframe {
                                        t: local_t,
                                        x: clip.position_x,
                                        y: clip.position_y,
                                        scale: clip.scale_percent,
                                        opacity: clip.opacity,
                                    };
                                    match keyframes
                                        .iter_mut()
                                        .find(|existing| (existing.t - local_t).abs() < 0.03)
                                    {
                                        Some(existing) => *existing = keyframe,
                                        None => {
                                            keyframes.push(keyframe);
                                            keyframes
                                                .sort_by(|left, right| left.t.total_cmp(&right.t));
                                        }
                                    }
                                    trim_changed = true;
                                }
                                let Some(keyframes) = clip.keyframes.as_mut() else {
                                    unreachable!()
                                };
                                let mut remove_keyframe: Option<usize> = None;
                                for (keyframe_index, keyframe) in keyframes.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.monospace(format!("kf{:02}", keyframe_index + 1));
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut keyframe.t)
                                                    .speed(0.05)
                                                    .range(0.0..=duration.max(0.01))
                                                    .prefix("t "),
                                            )
                                            .changed();
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut keyframe.x)
                                                    .speed(1.0)
                                                    .prefix("x "),
                                            )
                                            .changed();
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut keyframe.y)
                                                    .speed(1.0)
                                                    .prefix("y "),
                                            )
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut keyframe.scale)
                                                    .speed(0.5)
                                                    .range(1.0..=800.0)
                                                    .prefix("esc "),
                                            )
                                            .changed();
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut keyframe.opacity)
                                                    .speed(0.5)
                                                    .range(0.0..=100.0)
                                                    .prefix("op "),
                                            )
                                            .changed();
                                        if ui.button("×").on_hover_text("Quita este keyframe").clicked() {
                                            remove_keyframe = Some(keyframe_index);
                                        }
                                    });
                                }
                                if let Some(index) = remove_keyframe {
                                    keyframes.remove(index);
                                    if keyframes.len() < 2 {
                                        clip.keyframes = None;
                                    }
                                    trim_changed = true;
                                }
                                if ui.button("Quitar animación").on_hover_text("Borra todos los keyframes del clip").clicked() {
                                    clip.keyframes = None;
                                    trim_changed = true;
                                }
                            } else {
                                theme::section_label(ui, "Transformación");
                                egui::Grid::new("clip_transform")
                                    .num_columns(2)
                                    .spacing([12.0, 6.0])
                                    .show(ui, |ui| {
                                        ui.label("Posición");
                                        ui.horizontal(|ui| {
                                            trim_changed |= ui
                                                .add(
                                                    egui::DragValue::new(&mut clip.position_x)
                                                        .speed(1.0)
                                                        .range(-7680.0..=7680.0)
                                                        .prefix("X ")
                                                        .suffix(" px"),
                                                )
                                                .changed();
                                            trim_changed |= ui
                                                .add(
                                                    egui::DragValue::new(&mut clip.position_y)
                                                        .speed(1.0)
                                                        .range(-4320.0..=4320.0)
                                                        .prefix("Y ")
                                                        .suffix(" px"),
                                                )
                                                .changed();
                                        });
                                        ui.end_row();
                                        ui.label("Escala");
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut clip.scale_percent)
                                                    .speed(0.5)
                                                    .range(1.0..=800.0)
                                                    .suffix(" %"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        ui.label("Rotación");
                                        trim_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut clip.rotation)
                                                    .speed(0.25)
                                                    .range(-3600.0..=3600.0)
                                                    .suffix(" °"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        ui.label("Opacidad");
                                        trim_changed |= ui
                                            .add(
                                                egui::Slider::new(&mut clip.opacity, 0.0..=100.0)
                                                    .suffix(" %"),
                                            )
                                            .changed();
                                        ui.end_row();
                                    });
                                if ui.button("Animar (keyframes)").on_hover_text("Empieza a animar posición, escala y opacidad").clicked() {
                                    clip.keyframes = Some(vec![TransformKeyframe {
                                        t: 0.0,
                                        x: clip.position_x,
                                        y: clip.position_y,
                                        scale: clip.scale_percent,
                                        opacity: clip.opacity,
                                    }]);
                                    trim_changed = true;
                                }
                            }
                        }
                        ui.add_space(6.0);
                        theme::section_label(ui, "Audio");
                        ui.label(
                            egui::RichText::new(if clip.has_audio {
                                "Audio detectado"
                            } else {
                                "Sin audio"
                            })
                            .size(11.5)
                            .color(theme::TEXT_FAINT),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Ganancia");
                            trim_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut clip.gain_db)
                                        .speed(0.2)
                                        .range(-96.0..=24.0)
                                        .suffix(" dB"),
                                )
                                .changed();
                            trim_changed |= ui.checkbox(&mut clip.muted, "Silenciar").on_hover_text("Silencia el audio de este clip").changed();
                        });
                        trim_changed |= ui
                            .add(
                                egui::Slider::new(&mut clip.pan, -1.0..=1.0)
                                    .text("Balance L/R")
                                    .custom_formatter(|value, _| {
                                        if value.abs() < 0.01 {
                                            "Centro".to_owned()
                                        } else if value < 0.0 {
                                            format!("{:.0}% Izq", -value * 100.0)
                                        } else {
                                            format!("{:.0}% Der", value * 100.0)
                                        }
                                    }),
                            )
                            .changed();
                        if clip.has_audio {
                            let local = self.playhead - clip.timeline_start;
                            let duration = clip.duration();
                            trim_changed |= efectos::inspector_audio(
                                ui,
                                &mut clip.fx,
                                duration,
                                Some(local),
                            );
                        }
                        ui.add_space(6.0);
                        theme::section_label(ui, "Fundidos y transición");
                        let half = (clip.duration() / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.label("Fundido de entrada");
                            trim_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut clip.fade_in_seconds)
                                        .speed(0.05)
                                        .range(0.0..=half.max(0.01))
                                        .max_decimals(2)
                                        .suffix(" s"),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Fundido de salida");
                            trim_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut clip.fade_out_seconds)
                                        .speed(0.05)
                                        .range(0.0..=half.max(0.01))
                                        .max_decimals(2)
                                        .suffix(" s"),
                                )
                                .changed();
                        });
                        let mut chosen = clip.transition.clone();
                        ui.horizontal(|ui| {
                            ui.label("Transición de entrada:");
                            egui::ComboBox::from_id_salt("clip_transition")
                                .selected_text(
                                    chosen
                                        .as_deref()
                                        .map(efectos::transition_label)
                                        .unwrap_or("Ninguna"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut chosen, None, "Ninguna");
                                    for (id, label) in efectos::TRANSITIONS {
                                        ui.selectable_value(
                                            &mut chosen,
                                            Some((*id).to_owned()),
                                            *label,
                                        );
                                    }
                                });
                        });
                        if chosen != clip.transition {
                            clip.transition = chosen;
                            trim_changed = true;
                        }
                        if clip.transition.is_some() {
                            trim_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut clip.transition_duration)
                                        .speed(0.02)
                                        .range(0.04..=4.0)
                                        .prefix("dur ")
                                        .suffix(" s"),
                                )
                                .changed();
                        }
                        ui.add_space(10.0);
                        theme::section_label(ui, "Acciones");
                        let busy_render =
                            self.montage_render.is_some() || self.export_result.is_some();
                        // Acciones en filas que se reparten el ancho, no una columna
                        // de ocho botones.
                        let (mut replay_clip, mut to_title, mut watch_edit, mut close_gap) =
                            (false, false, false, false);
                        let (mut sync_audio, mut cut_silences, mut detect_scenes) =
                            (false, false, false);
                        ui.horizontal_wrapped(|ui| {
        replay_clip = ui
                                .add_enabled(
                                    self.preview_result.is_none(),
                                    egui::Button::new("Reproducir recorte"),
                                ).on_hover_text("Reproduce solo este clip").on_disabled_hover_text("Reproduce solo este clip")
                                .clicked();
                            to_title = clip.has_video
                                && clip.title.is_none()
                                && ui.button("Convertir en título").on_hover_text("Sustituye la imagen del clip por un título").clicked();
                            watch_edit = clip.has_video
                                && ui
                                    .add_enabled(
                                        !busy_render,
                                        egui::Button::new(if busy_render {
                                            "Renderizando..."
                                        } else {
                                            "Ver montaje completo"
                                        }),
                                    ).on_hover_text("Renderiza y reproduce todo el montaje").on_disabled_hover_text("Renderiza y reproduce todo el montaje")
                                    .clicked();
                            close_gap = ui.button("Cerrar hueco").on_hover_text("Pega el clip al anterior de su pista").clicked();
                            sync_audio =
                                clip.has_audio && ui.button("Sincronizar ángulos por audio").on_hover_text("Alinea por el sonido los otros ángulos con este (multicámara)").clicked();
                            cut_silences = clip.has_audio
                                && ui
                                    .add_enabled(
                                        self.silence_result.is_none()
                                            && self.pending_silence_cut.is_none(),
                                        egui::Button::new("Detectar y cortar silencios"),
                                    ).on_hover_text("Busca pausas y propone quitarlas").on_disabled_hover_text("Busca pausas y propone quitarlas")
                                    .clicked();
                            detect_scenes = clip.has_video
                                && clip.title.is_none()
                                && clip.nested.is_none()
                                && ui
                                    .add_enabled(
                                        self.scene_cut_result.is_none()
                                            && self.pending_scene_cut.is_none(),
                                        egui::Button::new("Detectar cortes de escena"),
                                    ).on_hover_text("Busca cambios de plano y propone partir el clip").on_disabled_hover_text("Busca cambios de plano y propone partir el clip")
                                    .clicked();
                            if clip.nested.is_some() {
                                unnest_requested |= ui.button("Desanidar secuencia").on_hover_text("Devuelve los clips de la secuencia al montaje").clicked();
                            } else {
                                nest_requested |= ui.button("Anidar pista").on_hover_text("Agrupa la pista del clip en una secuencia anidada").clicked();
                            }
                        });
                        let _ = clip;
                        if replay_clip {
                            self.preview_selected();
                        }
                        if watch_edit {
                            self.play_whole_edit();
                        }
                        if close_gap {
                            self.close_gap();
                        }
                        if sync_audio {
                            self.sync_angles_by_audio();
                        }
                        if cut_silences {
                            self.cut_silences_selected();
                        }
                        if detect_scenes {
                            self.detect_scene_cuts_selected();
                        }
                        if to_title {
                            if let Some(index) = self.selected {
                                let before_project = self.project.clone();
                                self.project.clips[index].title = Some(Titulo {
                                    text: self.project.clips[index].name(),
                                    ..Titulo::default()
                                });
                                self.finish_edit(before_project);
                                self.status = "Clip convertido en titulo".to_owned();
                            }
                        }
                    } else {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Sin selección")
                                    .strong()
                                    .size(12.0)
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Haz clic en un clip del montaje para ver sus ajustes.\nPulsa ? para la hoja de atajos.",
                                )
                                .size(11.0)
                                .color(theme::TEXT_FAINT),
                            );
                        });
                    }
                });
            });
        if toggle_enabled {
            self.toggle_selection_enabled();
        }
        if let Some(label) = label_request {
            self.label_selection(label);
        }
        if trim_changed {
            // Vista previa inmediata; el undo/guardado en disco se agrupa en
            // una sola entrada cuando el usuario deja de interactuar
            // (ver poll_pending_edit), en vez de una por tecla o píxel.
            self.queue_edit(project_before_inspector);
            self.request_preview();
        }
        if relink_requested {
            self.relink_selected();
        }
        if relink_all_requested {
            self.relink_all_from_folder();
        }
        if proxy_requested {
            self.create_proxy_for_selected();
        }
        if proxy_limit_gb != self.proxy_limit_gb {
            self.proxy_limit_gb = proxy_limit_gb;
        }
        if trim_cache_requested {
            self.status = self
                .enforce_proxy_budget()
                .unwrap_or_else(|| "La caché de proxies está dentro del límite".to_owned());
        }
        if nest_requested {
            self.nest_selected_track();
        }
        if unnest_requested {
            self.unnest_selected();
        }

        // Orden obligatorio en egui: primero los paneles laterales e inferiores,
        // y el central al final; si no, se solapan.
        self.status_bar(context);
        let mut scopes_changed = false;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(context, |ui| {
                // Barra de título del monitor, al estilo de las cabeceras de panel.
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("MONITOR")
                            .strong()
                            .size(11.5)
                            .color(theme::TEXT),
                    );
                    if self.preview_result.is_some() {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(
                            self.frame_result.is_none(),
                            egui::Button::new(
                                egui::RichText::new(if self.frame_result.is_some() {
                                    "Exportando fotograma…"
                                } else {
                                    "Exportar fotograma"
                                })
                                .size(11.0),
                            ),
                        ).on_hover_text("Guarda el fotograma del cabezal como imagen PNG").on_disabled_hover_text("Guarda el fotograma del cabezal como imagen PNG")
                        .clicked()
                    {
                        self.export_frame();
                    }
                    // Visores y proxies en un menú: como casillas sueltas se
                    // metían bajo el volumen en ventanas estrechas.
                    let mut proxies_changed = false;
                    let active_views = [self.show_waveform, self.show_vectorscope, self.use_proxies]
                        .iter()
                        .filter(|active| **active)
                        .count();
                    egui::menu::menu_button(
                        ui,
                        egui::RichText::new(if active_views > 0 {
                            format!("Ver ({active_views}) ▾")
                        } else {
                            "Ver ▾".to_owned()
                        }),
                        |ui| {
                            scopes_changed |= ui
                                .checkbox(&mut self.show_waveform, "Forma de onda (waveform)")
                                .on_hover_text("Luminancia por columnas, superpuesta abajo a la izquierda")
                                .changed();
                            scopes_changed |= ui
                                .checkbox(&mut self.show_vectorscope, "Vectorscopio")
                                .on_hover_text("Tono y saturación, superpuesto abajo a la derecha")
                                .changed();
                            ui.separator();
                            proxies_changed |= ui
                                .checkbox(&mut self.use_proxies, "Usar proxies")
                                .on_hover_text("Monitor y previsualización con proxies ligeros; la exportación siempre usa el original")
                                .changed();
                        },
                    )
                    .response
                    .on_hover_text("Visores de vídeo y proxies del monitor");
                    if proxies_changed {
                        self.request_preview();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_sized(
                            [80.0, 16.0],
                            egui::Slider::new(&mut self.monitor_volume, 0.0..=1.0)
                                .show_value(false),
                        )
                        .on_hover_text("Volumen del monitor");
                        ui.label(egui::RichText::new("🔊").size(11.0).color(theme::TEXT_DIM));
                    });
                });
                ui.add_space(8.0);
                // El monitor cede altura al montaje: la timeline tiene
                // garantizada su cabecera, la regla y hasta cuatro pistas
                // legibles; el monitor se queda con el resto (y, en pantallas
                // grandes, con como mucho el 60 %).
                let visible_tracks = (self.project.video_track_count()
                    + self.project.audio_track_count())
                .clamp(2, 4) as f32;
                // En pantallas estrechas las herramientas ocupan otra fila.
                let tools_row = if ui.available_width() >= 800.0 {
                    0.0
                } else {
                    34.0
                };
                let timeline_reserve = 36.0 + tools_row + 20.0 + visible_tracks * 38.0 + 18.0;
                let transport_reserve = 48.0;
                let monitor_height = (ui.available_height() - timeline_reserve - transport_reserve)
                    .min(ui.available_height() * 0.6)
                    .max(120.0);
                let monitor_width = (monitor_height * 16.0 / 9.0).min(ui.available_width());
                let monitor_size = egui::vec2(monitor_width, monitor_width * 9.0 / 16.0);
                ui.vertical_centered(|ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::BLACK)
                        .corner_radius(6.0)
                        .stroke(egui::Stroke::new(1.0_f32, theme::STROKE))
                        .show(ui, |ui| {
                            // Mínimo y máximo: sin imagen, el aviso centrado
                            // se expandía y dejaba fuera transporte y timeline.
                            ui.set_min_size(monitor_size);
                            ui.set_max_size(monitor_size);
                            if let Some(texture) = &self.preview_texture {
                                ui.image((texture.id(), monitor_size));
                            } else {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Haz clic en la timeline para cargar el monitor",
                                        )
                                        .size(11.5)
                                        .color(theme::TEXT_FAINT),
                                    );
                                });
                            }
                        });
                });
                // Transporte: timecode grande, paso a paso y marcas de trabajo,
                // como el visor de la app macOS.
                ui.add_space(6.0);
                let mut transport: Option<u8> = None;
                // La fila de transporte no se estrecha con el monitor: por
                // debajo de ~640 px los botones tapaban el timecode.
                let transport_width = ui.available_width();
                // En ventanas estrechas el timecode va en su propia fila: si
                // no, los botones lo tapaban.
                let narrow_transport = transport_width < 640.0;
                if narrow_transport {
                    ui.horizontal(|ui| self.transport_timecode(ui));
                }
                ui.vertical_centered(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(transport_width, 26.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if !narrow_transport {
                                self.transport_timecode(ui);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Entrada/salida de trabajo están en la barra
                                    // superior (y en I/O): aquí restaban sitio
                                    // al timecode en pantallas de 1280 px.
                                    if ui
                                        .add(
                                            egui::Button::new(egui::RichText::new("⟲").size(13.0))
                                                .fill(if self.loop_playback {
                                                    theme::ACCENT
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                }),
                                        )
                                        .on_hover_text("Reproducir en bucle")
                                        .clicked()
                                    {
                                        transport = Some(7);
                                    }
                                    if ui
                                        .add(egui::Button::new(egui::RichText::new("⛶").size(13.0)))
                                        .on_hover_text(
                                            "Monitor a pantalla completa (Esc para salir)",
                                        )
                                        .clicked()
                                    {
                                        transport = Some(8);
                                    }
                                    ui.separator();
                                    if ui
                                        .add(egui::Button::new(egui::RichText::new("⏭").size(13.0)))
                                        .on_hover_text("Corte siguiente (↓)")
                                        .clicked()
                                    {
                                        transport = Some(4);
                                    }
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("▶|").size(12.0),
                                        ))
                                        .on_hover_text("Un fotograma adelante (→)")
                                        .clicked()
                                    {
                                        transport = Some(3);
                                    }
                                    let playing = self.playback.is_some();
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(if playing {
                                                    "⏸"
                                                } else {
                                                    "▶"
                                                })
                                                .size(14.0)
                                                .color(egui::Color32::from_rgb(8, 24, 27)),
                                            )
                                            .fill(theme::ACCENT)
                                            .min_size(egui::vec2(46.0, 22.0)),
                                        )
                                        .on_hover_text("Reproducir / detener (Espacio)")
                                        .clicked()
                                    {
                                        transport = Some(2);
                                    }
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("|◀").size(12.0),
                                        ))
                                        .on_hover_text("Un fotograma atrás (←)")
                                        .clicked()
                                    {
                                        transport = Some(1);
                                    }
                                    if ui
                                        .add(egui::Button::new(egui::RichText::new("⏮").size(13.0)))
                                        .on_hover_text("Corte anterior (↑)")
                                        .clicked()
                                    {
                                        transport = Some(0);
                                    }
                                },
                            );
                        },
                    );
                });
                match transport {
                    Some(0) => self.go_to_cut(false),
                    Some(1) => self.step_frames(-1),
                    Some(2) => self.toggle_playback(),
                    Some(3) => self.step_frames(1),
                    Some(4) => self.go_to_cut(true),
                    Some(5) => self.mark_work_in(),
                    Some(6) => self.mark_work_out(),
                    Some(7) => {
                        self.loop_playback = !self.loop_playback;
                        self.status = if self.loop_playback {
                            "Reproducción en bucle activada".to_owned()
                        } else {
                            "Reproducción en bucle desactivada".to_owned()
                        };
                    }
                    Some(8) => self.monitor_fullscreen = true,
                    _ => {}
                }
                if self.playback.is_some() {
                    let meter_width = monitor_size.x.min(420.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.monospace("L");
                            ui.add_sized(
                                [meter_width, 10.0],
                                egui::ProgressBar::new(self.meter_display.0)
                                    .desired_height(10.0)
                                    .show_percentage(),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.monospace("R");
                            ui.add_sized(
                                [meter_width, 10.0],
                                egui::ProgressBar::new(self.meter_display.1)
                                    .desired_height(10.0)
                                    .show_percentage(),
                            );
                        });
                        let peak = self.meter_display.0.max(self.meter_display.1);
                        let db = if peak > 0.0001 {
                            format!("{:.1} dBFS", 20.0 * peak.log10())
                        } else {
                            "-inf dBFS".to_owned()
                        };
                        ui.small(db);
                    });
                }
                ui.add_space(12.0);
                let timeline_extent = self.timeline_extent();
                let video_tracks = self.project.video_track_count();
                let audio_tracks = self.project.audio_track_count();
                let track_count = video_tracks + audio_tracks;
                /// Ancho de la columna de cabeceras de pista.
                const HEADER_WIDTH: f32 = 96.0;
                /// Alto de la regla de tiempo sobre las pistas.
                const RULER_HEIGHT: f32 = 20.0;
                // --- Cabecera del timeline: título y herramientas a la
                // izquierda, zoom a la derecha. Una sola fila si cabe; en
                // pantallas estrechas las herramientas van en su propia fila
                // en vez de solaparse con el zoom.
                let tools_inline = ui.available_width() >= 800.0;
                // Por debajo de ~620 px se quitan el recuento de pistas y
                // «Ajustar» (queda en el menú Pistas y en Mayús+Z).
                let compact_header = ui.available_width() < 620.0;
                if !tools_inline {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.show_edit_toolbar(ui);
                    });
                    ui.add_space(2.0);
                }
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("TIMELINE")
                            .strong()
                            .size(11.5)
                            .color(theme::TEXT),
                    );
                    if !compact_header {
                        ui.label(
                            egui::RichText::new(format!("{video_tracks}V · {audio_tracks}A"))
                                .size(11.5)
                                .color(theme::TEXT_FAINT),
                        );
                    }
                    if tools_inline {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.show_edit_toolbar(ui);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        theme::bar_separator(ui);
                    }
                    // Ajustes de pistas y cadencia: se usan poco, van en un menú.
                    egui::menu::menu_button(ui, egui::RichText::new("Pistas ▾"), |ui| {
                        ui.set_min_width(190.0);
                        ui.menu_button(
                            format!("Cadencia: {} fps", self.project.fps.round()),
                            |ui| {
                                for option in [
                                    ("23.976 fps", 23.976_f64),
                                    ("24 fps", 24.0),
                                    ("25 fps", 25.0),
                                    ("29.97 fps DF", 29.97),
                                    ("30 fps", 30.0),
                                    ("50 fps", 50.0),
                                    ("59.94 fps DF", 59.94),
                                    ("60 fps", 60.0),
                                ] {
                                    let (label, requested_fps) = option;
                                    let rate = Timebase::from_fps(requested_fps);
                                    let active = self.project.timebase() == rate;
                                    if ui
                                        .add_enabled(
                                            !active,
                                            egui::Button::new(
                                                egui::RichText::new(label).size(11.5),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        let before = self.project.clone();
                                        self.project.set_timebase(rate);
                                        self.finish_edit(before);
                                        self.status = format!("Proyecto cambiado a {label}");
                                        ui.close_menu();
                                    }
                                }
                            },
                        ).response.on_hover_text("Fotogramas por segundo del montaje");
                        if ui
                            .add(egui::Button::new("+ Pista de vídeo"))
                            .on_hover_text("Añadir una pista de vídeo vacía")
                            .clicked()
                        {
                            let before = self.project.clone();
                            self.project.min_video_tracks = (video_tracks + 1).min(16);
                            self.finish_edit(before);
                            ui.close_menu();
                        }
                        if ui
                            .add(egui::Button::new("+ Pista de audio"))
                            .on_hover_text("Añadir una pista de audio vacía")
                            .clicked()
                        {
                            let before = self.project.clone();
                            self.project.min_audio_tracks = (audio_tracks + 1).min(16);
                            self.finish_edit(before);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Ajustar el montaje a la ventana (Mayús+Z)").on_hover_text("Encaja todo el montaje en el ancho de la timeline").clicked() {
                            self.zoom = 1.0;
                            self.hscroll = 0.0;
                            ui.close_menu();
                        }
                        if ui.button("Pistas más altas (Alt+↑)").clicked() {
                            self.track_height = (self.track_height * 1.2).min(180.0);
                        }
                        if ui.button("Pistas más bajas (Alt+↓)").clicked() {
                            self.track_height = (self.track_height / 1.2).max(28.0);
                        }
                        ui.separator();
                        if ui
                            .add(egui::Button::new("Quitar pistas vacías"))
                            .on_hover_text("Quita las pistas vacías sobrantes")
                            .clicked()
                        {
                            let before = self.project.clone();
                            self.project.min_video_tracks = 0;
                            self.project.min_audio_tracks = 0;
                            self.finish_edit(before);
                            ui.close_menu();
                        }
                    }).response.on_hover_text("Cadencia, pistas nuevas, altura de pistas y ajuste a la ventana");
                    ui.toggle_value(
                        &mut self.overwrite_on_drop,
                        egui::RichText::new("Sobrescribir").size(11.5),
                    )
                    .on_hover_text("Al soltar un clip encima de otro, recorta lo que haya debajo");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !compact_header
                            && ui
                                .add(egui::Button::new(egui::RichText::new("Ajustar").size(11.5)))
                                .on_hover_text("Ajustar el montaje a la ventana (Mayús+Z)")
                                .clicked()
                        {
                            self.zoom = 1.0;
                            self.hscroll = 0.0;
                        }
                        if ui
                            .add(egui::Button::new(egui::RichText::new("+").size(11.0)))
                            .on_hover_text("Acercar (Ctrl+])")
                            .clicked()
                        {
                            self.zoom_timeline(1.4);
                        }
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                                .monospace()
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                        );
                        if ui
                            .add(egui::Button::new(egui::RichText::new("−").size(11.0)))
                            .on_hover_text("Alejar (Ctrl+[)")
                            .clicked()
                        {
                            self.zoom_timeline(1.0 / 1.4);
                        }
                        if self.zoom > 1.0 {
                            let view_seconds = timeline_extent / self.zoom as f64;
                            let max_off = (timeline_extent - view_seconds).max(0.0);
                            let mut value = self.hscroll;
                            let slider = egui::Slider::new(&mut value, 0.0..=max_off.max(0.001))
                                .show_value(false)
                                .text("");
                            let response = ui.add(slider);
                            if response.changed() {
                                self.hscroll = value;
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                // Con el montaje vacío se muestra una regla de diez segundos
                // en lugar de una escala degenerada.
                let scale_total = timeline_extent;
                let view_seconds = scale_total / self.zoom as f64;
                // El montaje siempre cabe: si las pistas no entran a la altura
                // elegida, se comprimen en vez de recortarse.
                let available_for_tracks =
                    (ui.available_height() - RULER_HEIGHT - 14.0).max(track_count as f32 * 24.0);
                // Las pistas llenan el alto disponible (hasta 96 px) en vez
                // de dejar una franja vacía en pantallas grandes; con poco
                // sitio se comprimen hasta 22 px.
                let row_height = (available_for_tracks / track_count as f32)
                    .min(self.track_height.max(96.0))
                    .max(22.0);
                let timeline_height = RULER_HEIGHT + row_height * track_count as f32 + 8.0;
                let (timeline_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), timeline_height),
                    egui::Sense::hover(),
                );
                let header_rect = egui::Rect::from_min_max(
                    timeline_rect.min,
                    egui::pos2(timeline_rect.left() + HEADER_WIDTH, timeline_rect.bottom()),
                );
                let lane_rect = egui::Rect::from_min_max(
                    egui::pos2(timeline_rect.left() + HEADER_WIDTH, timeline_rect.top()),
                    timeline_rect.max,
                );
                let ruler_rect = egui::Rect::from_min_max(
                    lane_rect.min,
                    egui::pos2(lane_rect.right(), lane_rect.top() + RULER_HEIGHT),
                );
                let tracks_bottom = timeline_rect.bottom() - 4.0;
                let painter = ui.painter().with_clip_rect(timeline_rect);
                let pps = lane_rect.width().max(1.0) as f64 / view_seconds.max(0.001);
                self.timeline_pps = pps;
                let hs = self.hscroll;
                let to_x = |t: f64| lane_rect.left() + ((t - hs) * pps) as f32;
                let frame_duration = 1.0 / self.project.fps.max(1.0);
                let pointer_time = |x: f32| {
                    let raw = (hs
                        + ((x - lane_rect.left()) / lane_rect.width().max(1.0)) as f64
                            * view_seconds)
                        .clamp(0.0, scale_total);
                    ((raw / frame_duration).round() * frame_duration).clamp(0.0, scale_total)
                };
                // Arrastrar sobre la regla mueve el cabezal; el fondo de pistas
                // deselecciona sin robar el arrastre a los clips.
                let ruler_response = ui.interact(
                    ruler_rect,
                    ui.id().with("timeline-ruler"),
                    egui::Sense::click_and_drag(),
                );
                let lanes_area = egui::Rect::from_min_max(
                    egui::pos2(lane_rect.left(), ruler_rect.bottom()),
                    lane_rect.max,
                );
                let lane_background = ui.interact(
                    lanes_area,
                    ui.id().with("timeline-lanes"),
                    egui::Sense::click_and_drag(),
                );
                if ui.rect_contains_pointer(timeline_rect) {
                    let (primary_down, pointer_delta, pointer_position) = context.input(|input| {
                        (
                            input.pointer.primary_down(),
                            input.pointer.delta(),
                            input.pointer.hover_pos(),
                        )
                    });
                    if self.edit_tool == EditTool::Hand && primary_down {
                        self.hscroll -= pointer_delta.x as f64 / pps;
                    }
                    if self.drag_edit.is_some() && primary_down {
                        if let Some(pointer) = pointer_position {
                            let edge_step = view_seconds * 0.018;
                            if pointer.x < lane_rect.left() + 26.0 {
                                self.hscroll -= edge_step;
                                context.request_repaint();
                            } else if pointer.x > lane_rect.right() - 26.0 {
                                self.hscroll += edge_step;
                                context.request_repaint();
                            }
                        }
                    }
                    context.set_cursor_icon(match self.edit_tool {
                        EditTool::Select => egui::CursorIcon::Default,
                        EditTool::TrackSelect => egui::CursorIcon::PointingHand,
                        EditTool::Blade => egui::CursorIcon::Crosshair,
                        EditTool::Trim | EditTool::RippleTrim => egui::CursorIcon::ResizeHorizontal,
                        EditTool::Hand if primary_down => egui::CursorIcon::Grabbing,
                        EditTool::Hand => egui::CursorIcon::Grab,
                        EditTool::Zoom => egui::CursorIcon::ZoomIn,
                        EditTool::Magic => egui::CursorIcon::PointingHand,
                    });
                }
                // Rueda del ratón: Ctrl+rueda = zoom hacia el cabezal; rueda = desplazar.
                if ui.rect_contains_pointer(timeline_rect) {
                    let (wheel_x, wheel_y, ctrl) = context.input(|input| {
                        (
                            input.raw_scroll_delta.x,
                            input.raw_scroll_delta.y,
                            input.modifiers.ctrl,
                        )
                    });
                    if ctrl && wheel_y.abs() > 0.0 {
                        let factor = (1.0 + wheel_y / 400.0).clamp(0.7, 1.4);
                        let anchor = context
                            .input(|input| input.pointer.hover_pos())
                            .map(|pointer| pointer_time(pointer.x))
                            .unwrap_or(self.playhead);
                        self.zoom_timeline_at(factor, anchor);
                    } else if wheel_x.abs() > 0.0 || wheel_y.abs() > 0.0 {
                        let max_off = (scale_total - view_seconds).max(0.0);
                        self.hscroll =
                            (self.hscroll - (wheel_x + wheel_y) as f64 / pps).clamp(0.0, max_off);
                    }
                }
                let max_off = (scale_total - view_seconds).max(0.0);
                self.hscroll = self.hscroll.clamp(0.0, max_off);

                painter.rect_filled(timeline_rect, 6.0, egui::Color32::from_rgb(13, 14, 16));
                painter.rect_stroke(
                    timeline_rect,
                    6.0,
                    egui::Stroke::new(1.0_f32, theme::STROKE_SOFT),
                    egui::StrokeKind::Inside,
                );
                painter.rect_filled(header_rect, 0.0, theme::PANEL_HEADER);
                painter.rect_filled(ruler_rect, 0.0, egui::Color32::from_rgb(18, 19, 22));
                painter.line_segment(
                    [
                        egui::pos2(lane_rect.left(), timeline_rect.top()),
                        egui::pos2(lane_rect.left(), timeline_rect.bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, egui::Color32::from_gray(48)),
                );
                // Rango de trabajo: banda sobre la regla y sombreado del resto.
                if let Some((range_start, range_end)) = self.work_range() {
                    let band = egui::Rect::from_min_max(
                        egui::pos2(to_x(range_start).max(lane_rect.left()), ruler_rect.top()),
                        egui::pos2(to_x(range_end).min(lane_rect.right()), ruler_rect.bottom()),
                    );
                    if band.width() > 0.0 {
                        painter.rect_filled(band, 0.0, theme::ACCENT_DIM.gamma_multiply(0.55));
                    }
                    for edge in [range_start, range_end] {
                        let x = to_x(edge);
                        if x >= lane_rect.left() && x <= lane_rect.right() {
                            painter.line_segment(
                                [
                                    egui::pos2(x, ruler_rect.top()),
                                    egui::pos2(x, timeline_rect.bottom()),
                                ],
                                egui::Stroke::new(1.0_f32, theme::ACCENT),
                            );
                        }
                    }
                }
                // Regla de tiempo: marcas cada 1/5/10/30/60/300 s según el zoom.
                {
                    let steps: [f64; 9] = [0.1, 0.25, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0];
                    let step = steps
                        .iter()
                        .find(|&&step| step * pps >= 70.0)
                        .unwrap_or(&steps[8]);
                    let first = (hs / step).floor() * step;
                    let mut t = first;
                    while t <= hs + view_seconds {
                        let x = to_x(t);
                        if x >= lane_rect.left() {
                            painter.line_segment(
                                [
                                    egui::pos2(x, ruler_rect.bottom() - 6.0),
                                    egui::pos2(x, ruler_rect.bottom()),
                                ],
                                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
                            );
                            if step * pps >= 60.0 {
                                painter.text(
                                    egui::pos2(x + 3.0, ruler_rect.top() + 2.0),
                                    egui::Align2::LEFT_TOP,
                                    format_clock(t),
                                    egui::FontId::monospace(10.5),
                                    egui::Color32::from_gray(150),
                                );
                            }
                        }
                        t += step;
                    }
                }
                // Cabeceras de pista: nombre, silencio, solo, visibilidad y bloqueo.
                let mut track_toggles: Vec<TrackToggle> = Vec::new();
                for row in 0..track_count {
                    let bottom = tracks_bottom - row as f32 * row_height;
                    let top = bottom - row_height;
                    let is_audio = row < audio_tracks;
                    let track = if is_audio {
                        audio_tracks - row - 1
                    } else {
                        row - audio_tracks
                    };
                    painter.line_segment(
                        [
                            egui::pos2(timeline_rect.left(), bottom),
                            egui::pos2(timeline_rect.right(), bottom),
                        ],
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(30, 32, 36)),
                    );
                    // Fondo alterno para distinguir pistas de un vistazo.
                    if row % 2 == 1 {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(lane_rect.left() + 1.0, top),
                                egui::pos2(lane_rect.right(), bottom),
                            ),
                            0.0,
                            egui::Color32::from_rgb(17, 18, 21),
                        );
                    }
                    let muted_track = is_audio && !self.project.track_audible(track);
                    let hidden_track = !is_audio && !self.project.track_visible(track);
                    painter.text(
                        egui::pos2(header_rect.left() + 6.0, top + row_height / 2.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{}{}", if is_audio { "A" } else { "V" }, track + 1),
                        egui::FontId::monospace(10.5),
                        if muted_track || hidden_track {
                            theme::TEXT_FAINT
                        } else {
                            theme::TEXT
                        },
                    );
                    // Botones de pista: 20×18 px (antes 15×13, difíciles de
                    // acertar); dos filas si la pista es alta.
                    let button_size = egui::vec2(20.0, 18.0);
                    let mut origin = egui::pos2(header_rect.left() + 26.0, top + 3.0);
                    let mut toggle = |ui: &egui::Ui,
                                      painter: &egui::Painter,
                                      label: &str,
                                      active: bool,
                                      accent: egui::Color32,
                                      hint: &str,
                                      id: usize|
                     -> bool {
                        let rect = egui::Rect::from_min_size(origin, button_size);
                        origin.x += button_size.x + 2.0;
                        if origin.x + button_size.x > header_rect.right() - 2.0 {
                            origin.x = header_rect.left() + 26.0;
                            origin.y += button_size.y + 2.0;
                        }
                        if rect.bottom() > bottom {
                            return false;
                        }
                        let response = ui
                            .interact(
                                rect,
                                ui.id().with(("track-toggle", row, id)),
                                egui::Sense::click(),
                            )
                            .on_hover_text(hint);
                        painter.rect_filled(
                            rect,
                            3.0,
                            if active {
                                accent
                            } else if response.hovered() {
                                theme::CARD_HOVER
                            } else {
                                theme::CARD
                            },
                        );
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(11.0),
                            if active {
                                egui::Color32::from_rgb(10, 14, 16)
                            } else {
                                theme::TEXT_DIM
                            },
                        );
                        response.clicked()
                    };
                    if is_audio {
                        if toggle(
                            ui,
                            &painter,
                            "M",
                            self.project
                                .track_mutes
                                .get(track)
                                .copied()
                                .unwrap_or(false),
                            theme::WARN,
                            "Silenciar pista",
                            0,
                        ) {
                            track_toggles.push(TrackToggle::Mute(track));
                        }
                        if toggle(
                            ui,
                            &painter,
                            "S",
                            self.project
                                .track_solos
                                .get(track)
                                .copied()
                                .unwrap_or(false),
                            theme::ACCENT,
                            "Solo: enmudece las demás pistas",
                            1,
                        ) {
                            track_toggles.push(TrackToggle::Solo(track));
                        }
                        if toggle(
                            ui,
                            &painter,
                            "L",
                            self.project
                                .audio_locked
                                .get(track)
                                .copied()
                                .unwrap_or(false),
                            theme::DANGER,
                            "Bloquear pista",
                            2,
                        ) {
                            track_toggles.push(TrackToggle::LockAudio(track));
                        }
                    } else {
                        if toggle(
                            ui,
                            &painter,
                            "V",
                            !self
                                .project
                                .video_hidden
                                .get(track)
                                .copied()
                                .unwrap_or(false),
                            theme::ACCENT,
                            "Mostrar u ocultar la pista al componer",
                            0,
                        ) {
                            track_toggles.push(TrackToggle::Hide(track));
                        }
                        if toggle(
                            ui,
                            &painter,
                            "L",
                            self.project
                                .video_locked
                                .get(track)
                                .copied()
                                .unwrap_or(false),
                            theme::DANGER,
                            "Bloquear pista",
                            1,
                        ) {
                            track_toggles.push(TrackToggle::LockVideo(track));
                        }
                    }
                }
                if !track_toggles.is_empty() {
                    self.apply_track_toggles(&track_toggles);
                }
                // Los clips se recortan al área de pistas para que nunca invadan
                // la columna de cabeceras ni la regla al desplazar la vista.
                let lane_painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
                    egui::pos2(lane_rect.left(), ruler_rect.bottom()),
                    egui::pos2(lane_rect.right(), timeline_rect.bottom()),
                ));
                const EDGE_GRAB: f32 = 6.0;
                let mut timeline_drag = None;
                let mut clip_command: Option<(usize, ClipCommand)> = None;
                let mut tool_action: Option<TimelineToolAction> = None;
                let mut open_menu: Option<usize> = None;
                for (index, clip) in self.project.clips.iter().enumerate() {
                    let left = to_x(clip.timeline_start);
                    let right = to_x(clip.timeline_start + clip.duration());
                    let row = if clip.has_video {
                        audio_tracks + clip.track
                    } else {
                        audio_tracks.saturating_sub(clip.track + 1)
                    };
                    let bottom = tracks_bottom - row as f32 * row_height;
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(left + 1.0, bottom - row_height + 3.0),
                        egui::pos2((right - 1.0).max(left + 3.0), bottom - 2.0),
                    );
                    // Fuera de la vista: ni se dibuja ni se interactúa.
                    if rect.right() < lane_rect.left() || rect.left() > lane_rect.right() {
                        continue;
                    }
                    let locked = self.project.clip_locked(clip);
                    let inactive = (clip.has_video && !self.project.track_visible(clip.track))
                        || (!clip.has_video && !self.project.track_audible(clip.track));
                    let primary =
                        self.selected == Some(index) || self.context_menu_clip == Some(index);
                    let selected = primary || self.selection.contains(&index);
                    let mut color = if selected {
                        egui::Color32::from_rgb(0, 132, 150)
                    } else if clip.title.is_some() {
                        egui::Color32::from_rgb(96, 72, 132)
                    } else if clip.is_adjustment {
                        egui::Color32::from_rgb(112, 84, 40)
                    } else if clip.has_video {
                        egui::Color32::from_rgb(44, 78, 124)
                    } else {
                        egui::Color32::from_rgb(38, 100, 82)
                    };
                    if inactive || !clip.enabled {
                        color = color.gamma_multiply(0.45);
                    }
                    lane_painter.rect_filled(rect, 4.0, color);
                    lane_painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(
                            if primary {
                                2.0_f32
                            } else if selected {
                                1.5_f32
                            } else {
                                1.0_f32
                            },
                            if primary {
                                egui::Color32::WHITE
                            } else if selected {
                                theme::ACCENT
                            } else {
                                egui::Color32::from_rgb(16, 18, 22)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    // Miniatura del clip cuando ya está generada.
                    let thumb = if clip.has_video && clip.title.is_none() {
                        self.thumbnails.get(&clip.path).map(|texture| texture.id())
                    } else {
                        None
                    };
                    if let Some(texture_id) = thumb {
                        if rect.width() > 40.0 && rect.height() > 26.0 {
                            lane_painter.image(
                                texture_id,
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                    // Onda de audio para los clips de las pistas A, calculada en
                    // segundo plano y cacheada por medio.
                    if !clip.has_video && rect.width() > 8.0 && rect.height() > 14.0 {
                        if let Some(envelope) = self.waveforms.get(&clip.path) {
                            if !envelope.is_empty() {
                                let mid = rect.center().y;
                                let amplitude = (rect.height() / 2.0 - 3.0).max(1.0);
                                let speed = clip.speed.clamp(0.1, 8.0);
                                let wave_color = egui::Color32::from_rgb(150, 235, 200)
                                    .gamma_multiply(if inactive { 0.4 } else { 0.85 });
                                let mut x = rect.left().max(lane_rect.left());
                                let end_x = rect.right().min(lane_rect.right());
                                while x < end_x {
                                    let local = (x - rect.left()) as f64 / pps;
                                    let source = clip.in_seconds + local * speed;
                                    let sample_index = (source * ENVELOPE_RATE) as usize;
                                    let level = envelope
                                        .get(sample_index)
                                        .copied()
                                        .unwrap_or(0.0)
                                        .clamp(0.0, 1.0);
                                    let half = amplitude * level;
                                    if half > 0.4 {
                                        lane_painter.line_segment(
                                            [egui::pos2(x, mid - half), egui::pos2(x, mid + half)],
                                            egui::Stroke::new(1.0_f32, wave_color),
                                        );
                                    }
                                    x += 1.0;
                                }
                            }
                        }
                    }
                    // Fundidos de entrada y salida dibujados como triángulos.
                    let (fade_in, fade_out) = clip.effective_fades();
                    if fade_in > 0.001 || fade_out > 0.001 {
                        let shade = egui::Color32::from_black_alpha(120);
                        if fade_in > 0.001 {
                            let width = (fade_in * pps) as f32;
                            lane_painter.add(egui::Shape::convex_polygon(
                                vec![
                                    egui::pos2(rect.left(), rect.bottom()),
                                    egui::pos2(rect.left() + width.min(rect.width()), rect.top()),
                                    egui::pos2(rect.left(), rect.top()),
                                ],
                                shade,
                                egui::Stroke::NONE,
                            ));
                        }
                        if fade_out > 0.001 {
                            let width = (fade_out * pps) as f32;
                            lane_painter.add(egui::Shape::convex_polygon(
                                vec![
                                    egui::pos2(rect.right(), rect.bottom()),
                                    egui::pos2(rect.right() - width.min(rect.width()), rect.top()),
                                    egui::pos2(rect.right(), rect.top()),
                                ],
                                shade,
                                egui::Stroke::NONE,
                            ));
                        }
                    }
                    // Franja de la etiqueta de color.
                    if let Some(label) = label_color(clip.label) {
                        lane_painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(rect.left() + 1.0, rect.top()),
                                egui::pos2(rect.right() - 1.0, rect.top() + 4.0),
                            ),
                            2.0,
                            label,
                        );
                    }
                    // Clip desactivado: aspa diagonal, como el enable/disable
                    // de cualquier montador.
                    if !clip.enabled && rect.width() > 10.0 {
                        let cross = egui::Stroke::new(1.0_f32, theme::TEXT_FAINT);
                        lane_painter.line_segment([rect.left_top(), rect.right_bottom()], cross);
                        lane_painter.line_segment([rect.left_bottom(), rect.right_top()], cross);
                    }
                    // Medio ausente: rayado rojo para verlo desde el montaje.
                    if !clip.path.as_os_str().is_empty()
                        && clip.title.is_none()
                        && clip.nested.is_none()
                        && !clip.path.exists()
                    {
                        lane_painter.rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(2.0_f32, theme::DANGER),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let response = ui.interact(
                        rect,
                        ui.id().with(("timeline-clip", index)),
                        egui::Sense::click_and_drag(),
                    );
                    // El cursor anticipa la acción: recorte en los bordes, mover
                    // en el cuerpo, prohibido si la pista está bloqueada.
                    if response.hovered() {
                        let pointer = context.input(|input| input.pointer.hover_pos());
                        let cursor = if locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            match self.edit_tool {
                                EditTool::TrackSelect => egui::CursorIcon::PointingHand,
                                EditTool::Blade => egui::CursorIcon::Crosshair,
                                EditTool::Trim | EditTool::RippleTrim => {
                                    egui::CursorIcon::ResizeHorizontal
                                }
                                EditTool::Hand if response.dragged() => egui::CursorIcon::Grabbing,
                                EditTool::Hand => egui::CursorIcon::Grab,
                                EditTool::Zoom => egui::CursorIcon::ZoomIn,
                                EditTool::Magic => egui::CursorIcon::PointingHand,
                                EditTool::Select
                                    if pointer.is_some_and(|position| {
                                        (position.x - rect.left()).abs() <= EDGE_GRAB
                                            || (rect.right() - position.x).abs() <= EDGE_GRAB
                                    }) =>
                                {
                                    egui::CursorIcon::ResizeHorizontal
                                }
                                EditTool::Select => egui::CursorIcon::Grab,
                            }
                        };
                        context.set_cursor_icon(cursor);
                    }
                    let toggle_indices: Vec<usize> = if self.selection.contains(&index) {
                        self.selection.iter().copied().collect()
                    } else {
                        vec![index]
                    };
                    let toggle_off = toggle_indices
                        .iter()
                        .filter_map(|selected| self.project.clips.get(*selected))
                        .any(|selected| selected.enabled);
                    let toggle_scope = if toggle_indices.len() > 1 {
                        "selección"
                    } else {
                        "clip"
                    };
                    let toggle_label = format!(
                        "{} {toggle_scope} (D)",
                        if toggle_off { "Desactivar" } else { "Activar" }
                    );
                    let mut command: Option<ClipCommand> = None;
                    let menu = response.context_menu(|ui| {
                        ui.set_min_width(220.0);
                        ui.label(
                            egui::RichText::new(clip.name())
                                .strong()
                                .size(11.5)
                                .color(theme::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{} · {} · {}",
                                if clip.has_video { "V" } else { "A" },
                                clip.track + 1,
                                timecode(clip.timeline_start, self.project.fps),
                                format_clock(clip.duration())
                            ))
                            .size(11.5)
                            .color(theme::TEXT_FAINT),
                        );
                        ui.separator();
                        let mut pick = |ui: &mut egui::Ui, label: &str, value: ClipCommand| {
                            if ui.button(label).clicked() {
                                command = Some(value);
                                ui.close_menu();
                            }
                        };
                        if locked {
                            ui.label(
                                egui::RichText::new("Pista bloqueada · edición protegida")
                                    .size(11.5)
                                    .color(theme::DANGER),
                            );
                            pick(ui, "Copiar (Ctrl+C)", ClipCommand::Copy);
                            pick(
                                ui,
                                "Copiar atributos (Ctrl+Alt+C)",
                                ClipCommand::CopyAttributes,
                            );
                            return;
                        }
                        pick(ui, "Partir en el cabezal (S)", ClipCommand::Split);
                        pick(
                            ui,
                            "Partir todas las pistas (Ctrl+Mayús+K)",
                            ClipCommand::SplitAllTracks,
                        );
                        pick(
                            ui,
                            "Recortar entrada al cabezal (Q)",
                            ClipCommand::TrimStartToPlayhead,
                        );
                        pick(
                            ui,
                            "Recortar salida al cabezal (W)",
                            ClipCommand::TrimEndToPlayhead,
                        );
                        pick(ui, "Fundido de 1 s en ambos bordes", ClipCommand::QuickFade);
                        ui.separator();
                        pick(ui, &toggle_label, ClipCommand::ToggleEnabled);
                        pick(ui, "Duplicar (Ctrl+D)", ClipCommand::Duplicate);
                        pick(ui, "Separar audio (Ctrl+L)", ClipCommand::DetachAudio);
                        pick(ui, "Copiar (Ctrl+C)", ClipCommand::Copy);
                        pick(
                            ui,
                            "Copiar atributos (Ctrl+Alt+C)",
                            ClipCommand::CopyAttributes,
                        );
                        if self.attribute_clipboard.is_some() {
                            pick(
                                ui,
                                "Pegar atributos (Ctrl+Alt+V)",
                                ClipCommand::PasteAttributes,
                            );
                        }
                        pick(ui, "Congelar fotograma (3 s)", ClipCommand::FreezeFrame);
                        ui.separator();
                        pick(ui, "Superponer en el cabezal (B)", ClipCommand::Overwrite);
                        pick(ui, "Insertar en el cabezal (V)", ClipCommand::Insert);
                        pick(ui, "Cerrar hueco con el anterior", ClipCommand::CloseGap);
                        if clip.nested.is_some() {
                            pick(ui, "Desanidar secuencia", ClipCommand::Unnest);
                        } else {
                            pick(ui, "Anidar pista", ClipCommand::Nest);
                        }
                        if !clip.path.as_os_str().is_empty() && !clip.path.exists() {
                            pick(ui, "Revincular medio…", ClipCommand::Relink);
                        }
                        ui.separator();
                        pick(
                            ui,
                            "Quitar dejando hueco (Supr)",
                            ClipCommand::RemoveLeavingGap,
                        );
                        pick(
                            ui,
                            "Quitar y cerrar hueco (Mayús+Supr)",
                            ClipCommand::RemoveClosingGap,
                        );
                    });
                    if let Some(value) = command {
                        clip_command = Some((index, value));
                    }
                    if menu.is_some() {
                        open_menu = Some(index);
                    }
                    if locked && rect.width() > 24.0 {
                        lane_painter.text(
                            egui::pos2(rect.right() - 4.0, rect.top() + 9.0),
                            egui::Align2::RIGHT_CENTER,
                            "🔒",
                            egui::FontId::proportional(11.0),
                            theme::DANGER,
                        );
                    }
                    let modifiers = context.input(|input| input.modifiers);
                    if locked {
                        // Pista bloqueada: solo se puede seleccionar.
                        if response.clicked() {
                            if self.edit_tool == EditTool::TrackSelect {
                                tool_action =
                                    Some(TimelineToolAction::SelectTrack(index, modifiers.shift));
                            } else {
                                timeline_drag = Some(TimelineDragEvent::Select(
                                    index,
                                    SelectionMode::from_modifiers(modifiers),
                                ));
                            }
                        }
                    } else if self.edit_tool == EditTool::Hand {
                        // La mano desplaza la vista incluso si el gesto empezó
                        // encima de un clip; nunca altera el montaje.
                    } else if self.edit_tool == EditTool::Blade {
                        if response.clicked() {
                            let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                            tool_action =
                                Some(TimelineToolAction::Split(index, pointer_time(pointer.x)));
                        }
                    } else if self.edit_tool == EditTool::TrackSelect {
                        if response.clicked() {
                            tool_action =
                                Some(TimelineToolAction::SelectTrack(index, modifiers.shift));
                        }
                    } else if self.edit_tool == EditTool::Magic {
                        if response.clicked() {
                            tool_action = Some(TimelineToolAction::Magic(index));
                        }
                    } else if self.edit_tool == EditTool::Zoom {
                        if response.clicked() {
                            let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                            let factor = if modifiers.shift { 1.0 / 1.8 } else { 1.8 };
                            tool_action =
                                Some(TimelineToolAction::Zoom(pointer_time(pointer.x), factor));
                        }
                    } else if response.drag_started() {
                        let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                        let kind = if self.edit_tool == EditTool::Trim {
                            if pointer.x < rect.center().x {
                                DragKind::TrimStart
                            } else {
                                DragKind::TrimEnd
                            }
                        } else if self.edit_tool == EditTool::RippleTrim {
                            if pointer.x < rect.center().x {
                                DragKind::RippleTrimStart
                            } else {
                                DragKind::RippleTrimEnd
                            }
                        } else if (pointer.x - rect.left()).abs() <= EDGE_GRAB {
                            DragKind::TrimStart
                        } else if (rect.right() - pointer.x).abs() <= EDGE_GRAB {
                            DragKind::TrimEnd
                        } else {
                            DragKind::Move
                        };
                        self.drag_edit = Some((index, kind, self.project.clone()));
                        // Arrastrar un clip no seleccionado pasa a moverlo solo
                        // a él; si ya estaba en la selección, se mueve el grupo.
                        if !self.selection.contains(&index) {
                            self.selected = Some(index);
                            self.selection.clear();
                            self.selection.insert(index);
                        } else {
                            self.selected = Some(index);
                        }
                    } else if response.dragged() {
                        if let Some((_, kind, _)) = self.drag_edit.as_ref().filter(|s| s.0 == index)
                        {
                            let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                            match *kind {
                                DragKind::Move => {
                                    let target_row = (((tracks_bottom - pointer.y) / row_height)
                                        .floor()
                                        .max(0.0)
                                        as usize)
                                        .min(track_count - 1);
                                    let target_track = if clip.has_video {
                                        target_row.saturating_sub(audio_tracks).min(15)
                                    } else if target_row < audio_tracks {
                                        audio_tracks - target_row - 1
                                    } else {
                                        0
                                    };
                                    let delta =
                                        context.input(|input| input.pointer.delta().x) as f64 / pps;
                                    timeline_drag =
                                        Some(TimelineDragEvent::Move(index, delta, target_track));
                                }
                                DragKind::TrimStart | DragKind::RippleTrimStart => {
                                    timeline_drag = Some(TimelineDragEvent::TrimStart(
                                        index,
                                        pointer_time(pointer.x),
                                    ));
                                }
                                DragKind::TrimEnd | DragKind::RippleTrimEnd => {
                                    timeline_drag = Some(TimelineDragEvent::TrimEnd(
                                        index,
                                        pointer_time(pointer.x),
                                    ));
                                }
                            }
                        }
                    } else if response.drag_stopped() {
                        timeline_drag = Some(TimelineDragEvent::Commit(index));
                    } else if response.clicked() {
                        timeline_drag = Some(TimelineDragEvent::Select(
                            index,
                            SelectionMode::from_modifiers(modifiers),
                        ));
                    }
                    // Nombre arriba a la izquierda, como en los montadores clásicos,
                    // sobre una banda oscura para que se lea encima de la miniatura.
                    if rect.width() > 42.0 {
                        let band = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + 1.0, rect.top() + 4.0),
                            egui::pos2(rect.right() - 1.0, (rect.top() + 19.0).min(rect.bottom())),
                        );
                        if band.height() > 8.0 {
                            lane_painter.rect_filled(
                                band,
                                0.0,
                                egui::Color32::from_black_alpha(140),
                            );
                            let mut caption = clip.name();
                            if clip.speed != 1.0 {
                                caption.push_str(&format!("  {:.2}x", clip.speed));
                            }
                            if clip.muted {
                                caption.push_str("  🔇");
                            }
                            if clip.fx.reverse {
                                caption.push_str("  ◀◀");
                            }
                            if clip.fx.has_video_effects() {
                                caption.push_str("  fx");
                            }
                            if clip.fx.role == efectos::AudioRole::Musica && clip.fx.duck_db < -0.5
                            {
                                caption.push_str("  ♪↓");
                            }
                            lane_painter.text(
                                egui::pos2(band.left() + 4.0, band.center().y),
                                egui::Align2::LEFT_CENTER,
                                caption,
                                egui::FontId::proportional(11.5),
                                if inactive {
                                    theme::TEXT_FAINT
                                } else {
                                    egui::Color32::WHITE
                                },
                            );
                        }
                    }
                    // Banda elástica de volumen: 0 dB a media altura, de
                    // −60 dB (abajo) a +12 dB (arriba), como en Premiere.
                    if clip.has_audio && !clip.fx.volume_keys.is_empty() && rect.width() > 6.0 {
                        let duration = clip.duration().max(1e-6);
                        let band = rect.shrink2(egui::vec2(1.0, 4.0));
                        let to_y = |db: f64| {
                            let unit = if db >= 0.0 {
                                0.5 + db / 24.0
                            } else {
                                0.5 + db / 120.0
                            };
                            band.bottom() - band.height() * unit.clamp(0.0, 1.0) as f32
                        };
                        let steps = (rect.width() / 3.0).clamp(2.0, 400.0) as usize;
                        let points: Vec<egui::Pos2> = (0..=steps)
                            .map(|step| {
                                let local = duration * step as f64 / steps as f64;
                                egui::pos2(
                                    band.left() + band.width() * step as f32 / steps as f32,
                                    to_y(clip.fx.volume_at(local)),
                                )
                            })
                            .collect();
                        lane_painter.add(egui::Shape::line(
                            points,
                            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 214, 90)),
                        ));
                        for key in &clip.fx.volume_keys {
                            let x = band.left()
                                + band.width() * (key.t / duration).clamp(0.0, 1.0) as f32;
                            lane_painter.circle_filled(
                                egui::pos2(x, to_y(key.db)),
                                3.0,
                                egui::Color32::from_rgb(255, 214, 90),
                            );
                        }
                    }
                }
                if let Some(pointer) = context.input(|input| input.pointer.hover_pos()) {
                    if lanes_area.contains(pointer)
                        && matches!(
                            self.edit_tool,
                            EditTool::Blade | EditTool::Trim | EditTool::RippleTrim
                        )
                    {
                        let hover_time = pointer_time(pointer.x);
                        let guide_x = to_x(hover_time);
                        if self.edit_tool == EditTool::Blade {
                            lane_painter.line_segment(
                                [
                                    egui::pos2(guide_x, lanes_area.top()),
                                    egui::pos2(guide_x, lanes_area.bottom()),
                                ],
                                egui::Stroke::new(1.0_f32, theme::WARN),
                            );
                        }
                        let hint_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                (pointer.x + 12.0).min(lane_rect.right() - 142.0),
                                (pointer.y + 12.0).min(lanes_area.bottom() - 25.0),
                            ),
                            egui::vec2(136.0, 22.0),
                        );
                        lane_painter.rect_filled(
                            hint_rect,
                            4.0,
                            egui::Color32::from_black_alpha(220),
                        );
                        lane_painter.text(
                            hint_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!(
                                "{}  {}",
                                self.edit_tool.label(),
                                timecode(hover_time, self.project.fps)
                            ),
                            egui::FontId::monospace(11.0),
                            theme::TEXT,
                        );
                    }
                    if let Some((index, kind, baseline)) = &self.drag_edit {
                        if let (Some(current), Some(original)) =
                            (self.project.clips.get(*index), baseline.clips.get(*index))
                        {
                            let message = match kind {
                                DragKind::Move => format!(
                                    "Δ {:+.2} s  ·  {}{}",
                                    current.timeline_start - original.timeline_start,
                                    if current.has_video { "V" } else { "A" },
                                    current.track + 1
                                ),
                                DragKind::TrimStart | DragKind::RippleTrimStart => format!(
                                    "Entrada {}  ·  duración {}",
                                    timecode(current.timeline_start, self.project.fps),
                                    format_clock(current.duration())
                                ),
                                DragKind::TrimEnd | DragKind::RippleTrimEnd => format!(
                                    "Salida {}  ·  duración {}",
                                    timecode(
                                        current.timeline_start + current.duration(),
                                        self.project.fps
                                    ),
                                    format_clock(current.duration())
                                ),
                            };
                            let info_rect = egui::Rect::from_min_size(
                                egui::pos2(
                                    (pointer.x + 14.0).min(lane_rect.right() - 212.0),
                                    (pointer.y - 34.0).max(lanes_area.top() + 4.0),
                                ),
                                egui::vec2(206.0, 25.0),
                            );
                            lane_painter.rect_filled(
                                info_rect,
                                4.0,
                                egui::Color32::from_black_alpha(230),
                            );
                            lane_painter.rect_stroke(
                                info_rect,
                                4.0,
                                egui::Stroke::new(1.0_f32, theme::ACCENT),
                                egui::StrokeKind::Inside,
                            );
                            lane_painter.text(
                                info_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                message,
                                egui::FontId::monospace(11.0),
                                theme::TEXT,
                            );
                        }
                    }
                }
                // Arrastre desde la biblioteca: previsualiza dónde caerá el
                // medio y lo inserta al soltar, sobrescribiendo como un drop
                // de archivo.
                if let Some(source_index) = egui::DragAndDrop::payload::<usize>(context) {
                    if let Some(pointer) = context.input(|input| input.pointer.hover_pos()) {
                        if lanes_area.contains(pointer) {
                            let drop_time = pointer_time(pointer.x);
                            let row = (((tracks_bottom - pointer.y) / row_height).floor().max(0.0)
                                as usize)
                                .min(track_count.saturating_sub(1));
                            let is_video = row >= audio_tracks;
                            let track = if is_video {
                                row.saturating_sub(audio_tracks).min(15)
                            } else {
                                audio_tracks.saturating_sub(row + 1)
                            };
                            let released = context.input(|input| input.pointer.any_released());
                            if released {
                                let source = self.project.clips.get(*source_index).cloned();
                                let Some(mut clip) = source else {
                                    egui::DragAndDrop::clear_payload(context);
                                    return;
                                };
                                let file_based = clip.title.is_none()
                                    && clip.nested.is_none()
                                    && !clip.path.as_os_str().is_empty();
                                if !file_based {
                                    self.status =
                                        "Arrastra medios de archivo, no capas generadas".to_owned();
                                    egui::DragAndDrop::clear_payload(context);
                                    return;
                                }
                                if let Err(reason) = media_browser::validate_drop_destination(
                                    self.project.lane_locked(track, is_video),
                                    is_video,
                                    clip.has_video,
                                    clip.has_audio,
                                ) {
                                    self.status = reason.to_owned();
                                    egui::DragAndDrop::clear_payload(context);
                                    return;
                                }
                                let before = self.project.clone();
                                clip.timeline_start = drop_time;
                                clip.track = track;
                                clip.has_video = is_video;
                                // Preserve source audio capability; never invent a stream.
                                let span_end = clip.timeline_start + clip.duration();
                                clear_track_span(
                                    &mut self.project.clips,
                                    clip.track,
                                    clip.has_video,
                                    clip.timeline_start,
                                    span_end,
                                    &[],
                                );
                                self.project.clips.push(clip);
                                self.select_only(self.project.clips.len() - 1);
                                self.finish_edit(before);
                                self.status = format!(
                                    "Medio colocado en {}{} · {}",
                                    if is_video { "V" } else { "A" },
                                    track + 1,
                                    timecode(drop_time, self.project.fps)
                                );
                            } else {
                                let x0 = to_x(drop_time);
                                let row_bottom = tracks_bottom - row as f32 * row_height;
                                let preview = egui::Rect::from_min_max(
                                    egui::pos2(x0, row_bottom - row_height),
                                    egui::pos2(x0 + 120.0, row_bottom),
                                );
                                lane_painter.rect_filled(
                                    preview,
                                    4.0,
                                    egui::Color32::from_rgba_premultiplied(0, 132, 150, 70),
                                );
                                lane_painter.rect_stroke(
                                    preview,
                                    4.0,
                                    egui::Stroke::new(1.5_f32, theme::ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    }
                }
                // Marcadores: triángulos verdes en la regla; clic para saltar,
                // clic derecho para borrarlos.
                let mut marker_jump: Option<f64> = None;
                let mut marker_delete: Option<usize> = None;
                for (marker_index, marker) in self.project.markers.iter().enumerate() {
                    let x = to_x(marker.time);
                    if x < lane_rect.left() || x > lane_rect.right() {
                        continue;
                    }
                    let triangle = [
                        egui::pos2(x - 5.0, ruler_rect.top() + 1.0),
                        egui::pos2(x + 5.0, ruler_rect.top() + 1.0),
                        egui::pos2(x, ruler_rect.top() + 9.0),
                    ];
                    painter.add(egui::Shape::convex_polygon(
                        triangle.to_vec(),
                        egui::Color32::from_rgb(90, 220, 120),
                        egui::Stroke::NONE,
                    ));
                    painter.line_segment(
                        [
                            egui::pos2(x, ruler_rect.bottom()),
                            egui::pos2(x, timeline_rect.bottom()),
                        ],
                        egui::Stroke::new(
                            1.0_f32,
                            egui::Color32::from_rgba_premultiplied(40, 110, 60, 90),
                        ),
                    );
                    let hit = ui
                        .interact(
                            egui::Rect::from_min_size(
                                egui::pos2(x - 7.0, ruler_rect.top()),
                                egui::vec2(14.0, 12.0),
                            ),
                            ui.id().with(("marker", marker_index)),
                            egui::Sense::click(),
                        )
                        .on_hover_text(format!(
                            "{} · {}",
                            marker.name,
                            timecode(marker.time, self.project.fps)
                        ));
                    if hit.clicked() {
                        marker_jump = Some(marker.time);
                    }
                    if hit.secondary_clicked() {
                        marker_delete = Some(marker_index);
                    }
                    if (marker.time - self.hscroll) * pps < lane_rect.width() as f64 - 40.0
                        && lane_rect.width() > 120.0
                    {
                        painter.text(
                            egui::pos2(x + 7.0, ruler_rect.top() + 5.0),
                            egui::Align2::LEFT_CENTER,
                            &marker.name,
                            egui::FontId::monospace(10.5),
                            egui::Color32::from_rgb(140, 240, 160),
                        );
                    }
                }
                // Cabezal por encima de todo, con tirador en la regla.
                let playhead_x = to_x(self.playhead);
                if playhead_x >= lane_rect.left() && playhead_x <= lane_rect.right() {
                    painter.line_segment(
                        [
                            egui::pos2(playhead_x, ruler_rect.top()),
                            egui::pos2(playhead_x, timeline_rect.bottom()),
                        ],
                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(246, 83, 83)),
                    );
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(playhead_x - 6.0, ruler_rect.bottom() - 9.0),
                            egui::pos2(playhead_x + 6.0, ruler_rect.bottom() - 9.0),
                            egui::pos2(playhead_x, ruler_rect.bottom()),
                        ],
                        egui::Color32::from_rgb(246, 83, 83),
                        egui::Stroke::NONE,
                    ));
                }
                if let Some(time) = marker_jump {
                    self.seek(time);
                }
                if let Some(index) = marker_delete {
                    let before = self.project.clone();
                    self.project.markers.remove(index);
                    self.finish_edit(before);
                    self.status = "Marcador eliminado".to_owned();
                }
                // La regla mueve el cabezal salvo cuando una herramienta de
                // navegación ha tomado el control del gesto.
                if self.edit_tool == EditTool::Zoom {
                    if ruler_response.clicked() {
                        if let Some(pointer) = ruler_response.interact_pointer_pos() {
                            let shift = context.input(|input| input.modifiers.shift);
                            tool_action = Some(TimelineToolAction::Zoom(
                                pointer_time(pointer.x),
                                if shift { 1.0 / 1.8 } else { 1.8 },
                            ));
                        }
                    }
                } else if self.edit_tool != EditTool::Hand {
                    if let Some(pointer) = ruler_response.interact_pointer_pos() {
                        self.stop_playback();
                        self.playhead = pointer_time(pointer.x);
                        if ruler_response.drag_stopped() || ruler_response.clicked() {
                            self.request_preview();
                        }
                    }
                }
                if ruler_response.hovered() {
                    context.set_cursor_icon(match self.edit_tool {
                        EditTool::Hand if ruler_response.dragged() => egui::CursorIcon::Grabbing,
                        EditTool::Hand => egui::CursorIcon::Grab,
                        EditTool::Zoom => egui::CursorIcon::ZoomIn,
                        EditTool::Blade => egui::CursorIcon::Crosshair,
                        EditTool::Magic => egui::CursorIcon::PointingHand,
                        EditTool::TrackSelect => egui::CursorIcon::PointingHand,
                        EditTool::Select | EditTool::Trim | EditTool::RippleTrim => {
                            egui::CursorIcon::ResizeHorizontal
                        }
                    });
                }
                // Caja de selección: arrastrar sobre el fondo marca todos los
                // clips que quedan dentro del rectángulo.
                if self.edit_tool == EditTool::Select && lane_background.drag_started() {
                    self.marquee_origin = lane_background.interact_pointer_pos();
                }
                if self.edit_tool == EditTool::Select {
                    if let (Some(origin), Some(current)) =
                        (self.marquee_origin, lane_background.interact_pointer_pos())
                    {
                        let box_rect =
                            egui::Rect::from_two_pos(origin, current).intersect(lanes_area);
                        painter.rect_filled(box_rect, 2.0, theme::ACCENT.gamma_multiply(0.12));
                        painter.rect_stroke(
                            box_rect,
                            2.0,
                            egui::Stroke::new(1.0_f32, theme::ACCENT),
                            egui::StrokeKind::Inside,
                        );
                        if lane_background.drag_stopped() {
                            let mut hits = std::collections::BTreeSet::new();
                            for (index, clip) in self.project.clips.iter().enumerate() {
                                let row = if clip.has_video {
                                    audio_tracks + clip.track
                                } else {
                                    audio_tracks.saturating_sub(clip.track + 1)
                                };
                                let bottom = tracks_bottom - row as f32 * row_height;
                                let clip_rect = egui::Rect::from_min_max(
                                    egui::pos2(to_x(clip.timeline_start), bottom - row_height),
                                    egui::pos2(to_x(clip.timeline_start + clip.duration()), bottom),
                                );
                                if clip_rect.intersects(box_rect) {
                                    hits.insert(index);
                                }
                            }
                            let extend = context.input(|input| input.modifiers.shift);
                            if extend {
                                self.selection.extend(hits.iter().copied());
                            } else {
                                self.selection = hits;
                            }
                            self.selected = self.selection.iter().next().copied();
                            self.marquee_origin = None;
                            self.status =
                                format!("{} clip(s) seleccionado(s)", self.selection.len());
                        }
                    }
                    if lane_background.clicked() {
                        self.clear_selection();
                    }
                } else if self.edit_tool == EditTool::Zoom && lane_background.clicked() {
                    if let Some(pointer) = lane_background.interact_pointer_pos() {
                        let shift = context.input(|input| input.modifiers.shift);
                        tool_action = Some(TimelineToolAction::Zoom(
                            pointer_time(pointer.x),
                            if shift { 1.0 / 1.8 } else { 1.8 },
                        ));
                    }
                }
                if self.project.clips.is_empty() {
                    painter.text(
                        lane_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Arrastra vídeos o audio aquí, o pulsa + Importar",
                        egui::FontId::proportional(12.0),
                        theme::TEXT_FAINT,
                    );
                }
                if let Some((index, command)) = clip_command {
                    self.apply_clip_command(index, command);
                }
                if let Some(action) = tool_action {
                    match action {
                        TimelineToolAction::Split(index, time) => self.split_clip_at(index, time),
                        TimelineToolAction::Magic(index) => self.run_magic_tool(index),
                        TimelineToolAction::SelectTrack(index, whole_track) => {
                            self.select_track_from(index, whole_track)
                        }
                        TimelineToolAction::Zoom(time, factor) => {
                            self.zoom_timeline_at(factor, time)
                        }
                    }
                }
                self.context_menu_clip = open_menu;
                self.apply_timeline_drag(timeline_drag);
                // Drop de archivos: el destino se decide ahora que la timeline
                // está completamente dibujada y sus coordenadas son conocidas.
                if !self.pending_drop_paths.is_empty() && self.import_result.is_none() {
                    let paths = std::mem::take(&mut self.pending_drop_paths);
                    let position = self.pending_drop_position.take();
                    let target = position
                        .filter(|pointer| lanes_area.contains(*pointer))
                        .map(|pointer| {
                            let time = pointer_time(pointer.x);
                            let row = (((tracks_bottom - pointer.y) / row_height).floor().max(0.0)
                                as usize)
                                .min(track_count.saturating_sub(1));
                            let is_video = row >= audio_tracks;
                            let track = if is_video {
                                row.saturating_sub(audio_tracks).min(15)
                            } else {
                                audio_tracks.saturating_sub(row + 1)
                            };
                            ImportTarget {
                                timeline_start: time,
                                track,
                                is_video,
                            }
                        });
                    self.import_paths_to(paths, target);
                }
                // Mientras el drop espera, se muestra dónde caerá.
                if !self.pending_drop_paths.is_empty() {
                    if let Some(pointer) = context.input(|input| input.pointer.hover_pos()) {
                        if lanes_area.contains(pointer) {
                            let drop_time = pointer_time(pointer.x);
                            let row = (((tracks_bottom - pointer.y) / row_height).floor().max(0.0)
                                as usize)
                                .min(track_count.saturating_sub(1));
                            let is_video = row >= audio_tracks;
                            let track = if is_video {
                                row.saturating_sub(audio_tracks).min(15)
                            } else {
                                audio_tracks.saturating_sub(row + 1)
                            };
                            let drop_rect = egui::Rect::from_min_max(
                                egui::pos2(to_x(drop_time), lanes_area.top()),
                                egui::pos2(to_x(drop_time) + 150.0, lanes_area.top() + row_height),
                            );
                            lane_painter.rect_filled(
                                drop_rect,
                                4.0,
                                egui::Color32::from_rgba_premultiplied(0, 132, 150, 70),
                            );
                            lane_painter.rect_stroke(
                                drop_rect,
                                4.0,
                                egui::Stroke::new(1.5_f32, theme::ACCENT),
                                egui::StrokeKind::Inside,
                            );
                            lane_painter.text(
                                egui::pos2(drop_rect.left() + 6.0, drop_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!(
                                    "Soltar aquí · {}{} · {}",
                                    if is_video { "V" } else { "A" },
                                    track + 1,
                                    timecode(drop_time, self.project.fps)
                                ),
                                egui::FontId::proportional(11.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            });
        if metadata_changed {
            self.queue_edit(metadata_before);
            self.request_preview();
        }
        if burn_changed {
            self.preview_texture = None;
            self.request_preview();
        }
        if scopes_changed && self.playback.is_some() {
            self.stop_playback();
            self.toggle_playback();
        }
    }
}

#[derive(Deserialize)]
struct ProbeDocument {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    duration: Option<ProbeNumber>,
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    r_frame_rate: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    #[serde(default)]
    duration: Option<ProbeNumber>,
}

/// FFprobe serializa algunos números como texto y otros como JSON numbers.
#[derive(Deserialize)]
#[serde(untagged)]
enum ProbeNumber {
    Number(f64),
    Text(String),
}

impl ProbeNumber {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.parse().ok(),
        }
        .filter(|value: &f64| value.is_finite())
    }
}

fn parse_frame_rate(value: Option<&str>) -> Option<Timebase> {
    let (numerator, denominator) = value?.split_once('/')?;
    let numerator = numerator.parse::<u32>().ok()?;
    let denominator = denominator.parse::<u32>().ok()?;
    Timebase::new(numerator, denominator, false).ok()
}

/// Resume una secuencia de PTS para que el diagnóstico sobreviva al proyecto y
/// pueda justificar por qué el render aplica conformado CFR.
fn summarize_video_pts(pts: &[f64]) -> Option<SourcePtsSummary> {
    let mut ordered: Vec<f64> = pts
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    ordered.sort_by(f64::total_cmp);
    ordered.dedup_by(|left, right| (*left - *right).abs() < 1e-7);
    if ordered.len() < 5 {
        return None;
    }
    let deltas: Vec<f64> = ordered
        .windows(2)
        .map(|window| window[1] - window[0])
        .filter(|delta| *delta > 0.0)
        .collect();
    if deltas.len() < 4 {
        return None;
    }
    let mut sorted_deltas = deltas.clone();
    sorted_deltas.sort_by(f64::total_cmp);
    let median = sorted_deltas[sorted_deltas.len() / 2];
    if median <= f64::EPSILON {
        return None;
    }
    let gap_count = deltas.iter().filter(|delta| **delta > median * 1.5).count() as u64;
    let variable_delta_count = deltas
        .iter()
        .filter(|delta| (**delta - median).abs() > median * 0.02)
        .count() as u64;
    Some(SourcePtsSummary {
        frame_count: ordered.len() as u64,
        first_seconds: ordered[0],
        last_seconds: *ordered.last().unwrap_or(&ordered[0]),
        median_frame_duration_seconds: median,
        max_frame_duration_seconds: deltas.iter().copied().fold(0.0, f64::max),
        variable_delta_count,
        gap_count,
    })
}

fn is_variable_frame_rate(pts: &[f64]) -> bool {
    summarize_video_pts(pts).is_some_and(|summary| summary.is_variable())
}

/// Lee una ventana inicial del reloj de presentación. Un fallo del scan no
/// invalida un medio que sí tiene metadatos válidos: en ese caso no se marca VFR.
fn probe_video_pts(path: &Path) -> Vec<f64> {
    let output = Command::new(tool_path("ffprobe.exe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-read_intervals",
            "%+10",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut pts: Vec<f64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .collect();
    pts.sort_by(f64::total_cmp);
    pts.dedup_by(|left, right| (*left - *right).abs() < 1e-7);
    pts
}

fn probe_media(path: &Path) -> Result<MediaProbe, String> {
    let output = Command::new(tool_path("ffprobe.exe"))
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFprobe no esta disponible: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let document: ProbeDocument = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("FFprobe devolvio JSON invalido: {error}"))?;
    let video = document
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video");
    let audio = document
        .streams
        .iter()
        .any(|stream| stream.codec_type == "audio");
    let duration = document
        .format
        .as_ref()
        .and_then(|format| format.duration.as_ref())
        .and_then(ProbeNumber::as_f64)
        .or_else(|| {
            document
                .streams
                .iter()
                .filter_map(|stream| stream.duration.as_ref().and_then(ProbeNumber::as_f64))
                .max_by(f64::total_cmp)
        })
        .filter(|value| *value > 0.0)
        .ok_or_else(|| "FFprobe no pudo leer la duracion".to_owned())?;
    let frame_rate = video.and_then(|stream| {
        parse_frame_rate(stream.avg_frame_rate.as_deref())
            .or_else(|| parse_frame_rate(stream.r_frame_rate.as_deref()))
    });
    let pts = video.map(|_| probe_video_pts(path)).unwrap_or_default();
    let source_pts = summarize_video_pts(&pts);
    let variable_frame_rate = is_variable_frame_rate(&pts);
    Ok(MediaProbe {
        duration,
        has_video: video.is_some(),
        has_audio: audio,
        frame_rate,
        variable_frame_rate,
        source_pts,
    })
}

/// Conformado único de vídeo para todas las salidas Windows. Un medio marcado
/// VFR fija también el origen del filtro en cero antes de cuantizar sus PTS a la
/// cadencia racional del proyecto; el camino CFR usa la misma cuantización sin
/// reinterpretar el reloj de origen.
fn conform_video_filter(clip: &RoughClip, frame_rate: &str) -> String {
    if clip.source_vfr
        || clip
            .source_pts
            .as_ref()
            .is_some_and(SourcePtsSummary::is_variable)
    {
        format!(",fps=fps={frame_rate}:start_time=0:round=near")
    } else {
        format!(",fps=fps={frame_rate}:round=near")
    }
}

fn render_preview_frame(
    sources: &[(RoughClip, f64)],
    timebase: Timebase,
) -> Result<PreviewFrame, String> {
    const WIDTH: usize = 640;
    const HEIGHT: usize = 360;
    let frame_rate = timebase.ffmpeg_rate();
    let mut command = Command::new(tool_path("ffmpeg.exe"));
    command.args(["-v", "error"]);
    let mut is_title_input = Vec::with_capacity(sources.len());
    for (clip, source_time) in sources {
        if clip.title.is_some() || clip.is_adjustment {
            command.args([
                "-f",
                "lavfi",
                "-i",
                // `format=rgba` es imprescindible: sin él la fuente `color`
                // sale opaca y el título tapaba con negro todo lo de debajo.
                &format!(
                    "color=c=black@0.0:s=640x360:r={},format=rgba",
                    timebase.ffmpeg_rate()
                ),
            ]);
            is_title_input.push(true);
        } else {
            command
                .args(["-ss", &format_seconds(*source_time), "-i"])
                .arg(&clip.path);
            is_title_input.push(false);
        }
    }
    let mut filters = vec![format!(
        "color=c=black:s={WIDTH}x{HEIGHT}:r={}:d=0.1[base]",
        timebase.ffmpeg_rate()
    )];
    for (index, (clip, source_time)) in sources.iter().enumerate() {
        let angle = clip.rotation.to_radians();
        let opacity = (clip.opacity / 100.0).clamp(0.0, 1.0);
        if clip.is_adjustment {
            // Sin `[pv{index}]`: se gradúa directamente `[previous]` en el
            // bucle de composición.
        } else if is_title_input[index] {
            let Some(title) = clip.title.as_ref() else {
                continue;
            };
            let Some(font) = find_font() else {
                return Err("No se encontro una fuente TTF del sistema para los titulos".to_owned());
            };
            // El tamano se define sobre 1080p y se escala al monitor.
            let fontsize = title.size.max(8.0) * HEIGHT as f64 / 1080.0;
            let local = (source_time - clip.in_seconds) / clip.speed.clamp(0.1, 8.0);
            let drawing = title_layer_filters(
                title,
                &font,
                fontsize,
                (WIDTH as u32, HEIGHT as u32),
                Some(local.max(0.0)),
            );
            filters.push(format!(
                "[{index}:v:0]{drawing},format=rgba,rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none,colorchannelmixer=aa={opacity:.6}[pv{index}]",
            ));
        } else {
            let is_blend = clip.fusion.blend_mode().is_some();
            let width = if is_blend {
                WIDTH as u32
            } else {
                even_dimension(WIDTH as f64 * clip.scale_percent / 100.0)
            };
            let height = if is_blend {
                HEIGHT as u32
            } else {
                even_dimension(HEIGHT as f64 * clip.scale_percent / 100.0)
            };
            let eq = color_eq_filter(clip.exposure, clip.contrast, clip.saturation);
            let vig = vignette_filter(clip.vignette);
            let blur = blur_filter(clip.blur, WIDTH.min(HEIGHT) as f64);
            let wheels = wheels_filter(clip.wheels.as_ref());
            let chroma = chroma_filter(clip.chroma.as_ref());
            let curves = curves_filter(clip.curves.as_ref());
            let lut = lut_filter(clip.lut.as_deref());
            let mask = mask_filter(clip.mask.as_ref(), width as f64, height as f64);
            let cadence = conform_video_filter(clip, &frame_rate);
            let geometry = clip.fx.geometry_chain(false);
            let fx_color = clip.fx.video_color_chain();
            let fx_alpha = clip.fx.alpha_chain();
            let scale_mode = if is_blend { "increase" } else { "decrease" };
            let crop = if is_blend {
                format!(",crop={WIDTH}:{HEIGHT}")
            } else {
                String::new()
            };
            let blend_canvas = if is_blend {
                format!(
                    ",scale={WIDTH}:{HEIGHT}:force_original_aspect_ratio=increase,crop={WIDTH}:{HEIGHT}"
                )
            } else {
                String::new()
            };
            filters.push(format!(
                // `setpts=PTS-STARTPTS`: tras `-ss` el primer fotograma no
                // empieza en 0 si el cabezal cae entre dos fotogramas del
                // medio, y `overlay` componía el fondo negro sin él.
                "[{index}:v:0]setpts=PTS-STARTPTS{geometry},scale={width}:{height}:force_original_aspect_ratio={scale_mode}{crop},setsar=1{cadence}{wheels}{curves}{lut}{eq}{vig}{blur}{fx_color},format=rgba{chroma}{mask}{fx_alpha},rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none{blend_canvas},colorchannelmixer=aa={opacity:.6}[pv{index}]"
            ));
        }
    }
    let mut previous = "base".to_owned();
    for (index, (clip, _)) in sources.iter().enumerate() {
        let output = if index + 1 == sources.len() {
            "vout".to_owned()
        } else {
            format!("po{index}")
        };
        let x = clip.position_x * WIDTH as f64 / 1920.0;
        let y = clip.position_y * HEIGHT as f64 / 1080.0;
        if clip.is_adjustment {
            // El monitor ya solo compone clips activos en el cabezal, así
            // que no hace falta puerta temporal: solo máscara y opacidad.
            let opacity = (clip.opacity / 100.0).clamp(0.0, 1.0);
            let wheels = wheels_filter(clip.wheels.as_ref());
            let curves = curves_filter(clip.curves.as_ref());
            let lut = lut_filter(clip.lut.as_deref());
            let eq = color_eq_filter(clip.exposure, clip.contrast, clip.saturation);
            let vig = vignette_filter(clip.vignette);
            let blur = blur_filter(clip.blur, WIDTH.min(HEIGHT) as f64);
            let spatial = clip
                .mask
                .as_ref()
                .map(|mask| mask.alpha_expression(WIDTH as f64, HEIGHT as f64))
                .unwrap_or_else(|| "alpha(X,Y)".to_owned())
                .replace(',', "\\,");
            let alpha_expr = format!("{spatial}*{opacity:.6}");
            filters.push(format!(
                "[{previous}]split=2[padjbase{index}][padjsrc{index}]"
            ));
            let fx_color = clip.fx.video_color_chain();
            filters.push(format!(
                "[padjsrc{index}]null{wheels}{curves}{lut}{eq}{vig}{blur}{fx_color},format=rgba,geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='{alpha_expr}'[padjfx{index}]"
            ));
            filters.push(format!(
                "[padjbase{index}][padjfx{index}]overlay=eof_action=pass:shortest=0:format=auto[{output}]"
            ));
        } else if let Some(mode) = clip.fusion.blend_mode() {
            // Misma cadena verificada que en build_render_filters: evita
            // maskedmerge (trunca duración/timing) a favor de
            // blend+tpad+alphamerge+overlay.
            filters.push(format!("[{previous}]split=2[pbase{index}a][pbase{index}b]"));
            filters.push(format!("[pv{index}]split=2[prgb{index}][palpha{index}]"));
            filters.push(format!(
                "[palpha{index}]format=rgba,alphaextract,tpad=stop=-1:stop_mode=add:color=black[pmask{index}]"
            ));
            filters.push(format!(
                "[pbase{index}a][prgb{index}]blend=all_mode={mode}:shortest=0:repeatlast=1[pblend{index}]"
            ));
            filters.push(format!(
                "[pblend{index}][pmask{index}]alphamerge=shortest=0:repeatlast=0:eof_action=pass[pblenda{index}]"
            ));
            filters.push(format!(
                "[pbase{index}b][pblenda{index}]overlay=eof_action=pass:shortest=0:format=auto[{output}]"
            ));
        } else {
            filters.push(format!(
                "[{previous}][pv{index}]overlay=x=(W-w)/2+{x:.3}:y=(H-h)/2+{y:.3}:format=auto[{output}]"
            ));
        }
        previous = output;
    }
    let result = command
        .args(["-filter_complex", &filters.join(";"), "-map", "[vout]"])
        .args([
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgba",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFmpeg no esta disponible: {error}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    if result.stdout.len() != WIDTH * HEIGHT * 4 {
        return Err("FFmpeg devolvio un fotograma incompleto".to_owned());
    }
    Ok(PreviewFrame {
        pixels: result.stdout,
        width: WIDTH,
        height: HEIGHT,
    })
}

fn even_dimension(value: f64) -> u32 {
    ((value.clamp(2.0, 8192.0) / 2.0).round() as u32 * 2).max(2)
}

/// Multicámara por cobertura: corta al ángulo `camera` (1 = pista base) en el
/// instante `p`. Los ángulos deben estar alineados en pistas consecutivas
/// (base en `t0`, cámara 2 en `t0+1`, ...). Devuelve los clips resultantes.
fn apply_multicam_cut(
    clips: Vec<RoughClip>,
    base_idx: usize,
    camera: usize,
    p: f64,
) -> Result<Vec<RoughClip>, String> {
    if camera == 0 || camera > 4 {
        return Err("Camara fuera de rango (1-4)".to_owned());
    }
    let base = clips[base_idx].clone();
    let base_start = base.timeline_start;
    let base_end = base.timeline_start + base.duration();
    if p <= base_start + 0.02 || p >= base_end - 0.02 {
        return Err("Coloca el cabezal dentro del plano base para cortar".to_owned());
    }
    let target_track = base.track + camera - 1;

    // Fuente del ángulo: el clip alineado en la pista objetivo.
    let angle_source: Option<RoughClip> = clips
        .iter()
        .find(|clip| {
            clip.track == target_track
                && clip.has_video
                && clip.timeline_start <= base_start + 0.5
                && clip.timeline_start + clip.duration() >= base_end - 0.5
        })
        .cloned();

    // 1) Recortar o eliminar coberturas de otras pistas superiores.
    let mut keep: Vec<RoughClip> = Vec::with_capacity(clips.len());
    for (index, clip) in clips.into_iter().enumerate() {
        let is_base = index == base_idx;
        let on_target = clip.track == target_track && !is_base;
        let covers_span = clip.has_video
            && clip.track > base.track
            && clip.timeline_start < base_end
            && clip.timeline_start + clip.duration() > base_start;
        let mut clip = clip;
        if !is_base && covers_span {
            if on_target && camera > 1 {
                // La pista del ángulo elegido se sustituye por la cobertura nueva.
                continue;
            }
            let clip_end = clip.timeline_start + clip.duration();
            if clip.timeline_start >= p - 0.001 {
                continue; // empieza tras el corte: fuera
            }
            if clip_end > p + 0.001 {
                // Abarca el corte: recortar hasta p.
                clip.out_seconds =
                    clip.in_seconds + (p - clip.timeline_start) * clip.speed.clamp(0.1, 8.0);
            }
        }
        keep.push(clip);
    }

    // 2) Cobertura nueva del ángulo elegido desde p hasta el fin del ángulo.
    if camera > 1 {
        let Some(source) = angle_source else {
            return Err(format!(
                "No hay medio de la camara {camera} alineado en V{}",
                target_track + 1
            ));
        };
        let speed = source.speed.clamp(0.1, 8.0);
        let local = (p - source.timeline_start).max(0.0);
        let source_in = source.in_seconds + local * speed;
        let coverage_end = (source.timeline_start + source.duration()).min(base_end);
        if coverage_end - p > 0.05 {
            keep.push(RoughClip {
                path: source.path.clone(),
                in_seconds: source_in,
                out_seconds: source.in_seconds + (coverage_end - p) * speed,
                has_video: true,
                has_audio: source.has_audio,
                speed,
                timeline_start: p,
                track: target_track,
                gain_db: source.gain_db,
                muted: source.muted,
                ..Default::default()
            });
        }
    }

    Ok(keep)
}

/// Resolución de la envolvente de audio: buckets RMS por segundo.
const ENVELOPE_RATE: f64 = 100.0;

/// Extrae la envolvente RMS (100 buckets/s, mono 8 kHz) de un tramo de audio.
fn extract_audio_envelope(path: &Path, start: f64, duration: f64) -> Result<Vec<f32>, String> {
    let duration = duration.clamp(1.0, 120.0);
    let output = Command::new(tool_path("ffmpeg.exe"))
        .args([
            "-v",
            "error",
            "-ss",
            &format_seconds(start),
            "-t",
            &format_seconds(duration),
            "-i",
        ])
        .arg(path)
        .args(["-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .creation_flags(CREATE_NO_WINDOW)
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("FFmpeg no esta disponible: {error}"))?;
    if !output.status.success() {
        return Err("No se pudo leer el audio del medio".to_owned());
    }
    let samples_per_bucket = (8000.0 / ENVELOPE_RATE).round() as usize;
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
        .collect();
    let mut envelope = Vec::with_capacity(samples.len() / samples_per_bucket + 1);
    for chunk in samples.chunks(samples_per_bucket) {
        let energy: f32 = chunk.iter().map(|sample| sample * sample).sum();
        envelope.push((energy / chunk.len().max(1) as f32).sqrt());
    }
    Ok(envelope)
}

/// Mejor desfase (en buckets) tal que `other[i + shift] ≈ base[i]`.
/// Correlación cruzada normalizada sobre la ventana de solape.
fn best_offset_buckets(base: &[f32], other: &[f32], max_shift: i64) -> Option<i64> {
    if base.len() < 50 || other.len() < 50 {
        return None;
    }
    let mean = |data: &[f32]| data.iter().sum::<f32>() / data.len() as f32;
    let (base_mean, other_mean) = (mean(base), mean(other));
    let mut best: Option<(i64, f32)> = None;
    // Explorar por |shift| creciente: con contenido periódico hay varios
    // desfases exactos y nos quedamos con el desplazamiento más pequeño.
    let shifts: Vec<i64> = (0..=max_shift)
        .flat_map(|offset| {
            if offset == 0 {
                vec![0]
            } else {
                vec![-offset, offset]
            }
        })
        .collect();
    for shift in shifts {
        let (start_base, start_other) = if shift >= 0 {
            (0usize, shift as usize)
        } else {
            ((-shift) as usize, 0usize)
        };
        let len = base
            .len()
            .saturating_sub(start_base)
            .min(other.len().saturating_sub(start_other));
        if len < 50 {
            continue;
        }
        let mut dot = 0.0f32;
        let mut norm_base = 0.0f32;
        let mut norm_other = 0.0f32;
        for index in 0..len {
            let b = base[start_base + index] - base_mean;
            let o = other[start_other + index] - other_mean;
            dot += b * o;
            norm_base += b * b;
            norm_other += o * o;
        }
        if norm_base <= 1e-9 || norm_other <= 1e-9 {
            continue;
        }
        let score = dot / (norm_base.sqrt() * norm_other.sqrt());
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((shift, score));
        }
    }
    let (shift, score) = best?;
    // Umbral: sin señal común la correlación no supera ~0.5.
    (score > 0.5).then_some(shift)
}

/// Registra en el comando las entradas (medios o lienzos de título) y devuelve
/// el índice de entrada por clip y si cada entrada es título.
fn push_render_inputs(
    command: &mut Command,
    clips: &[RoughClip],
    size: (u32, u32),
    allow_proxy: bool,
    timebase: Timebase,
) -> (Vec<usize>, Vec<bool>) {
    let mut input_indices = Vec::with_capacity(clips.len());
    let mut is_title_input = Vec::with_capacity(clips.len());
    for clip in clips {
        let media_index = input_indices.len();
        if clip.title.is_some() || clip.is_adjustment {
            // Las capas de ajuste tampoco leen medio propio: reutilizan el
            // mismo lienzo lavfi de descarte que los títulos.
            command.args([
                "-f",
                "lavfi",
                "-t",
                &format_seconds(clip.source_duration().max(0.04)),
                "-i",
                &format!(
                    // Transparente de verdad solo con `format=rgba`; sin él
                    // la fuente sale opaca y el título tapaba el vídeo.
                    "color=c=black@0.0:s={}x{}:r={},format=rgba",
                    size.0,
                    size.1,
                    timebase.ffmpeg_rate()
                ),
            ]);
            is_title_input.push(true);
        } else {
            let path = if allow_proxy {
                clip.proxy
                    .as_ref()
                    .filter(|proxy| proxy.is_file())
                    .unwrap_or(&clip.path)
            } else {
                &clip.path
            };
            if is_image_file(path) {
                // Las imágenes fijas entran en bucle: el fotograma es siempre
                // el mismo, así que no hace falta buscar el punto de entrada.
                command
                    .args([
                        "-loop",
                        "1",
                        "-t",
                        &format_seconds(clip.source_duration()),
                        "-i",
                    ])
                    .arg(path);
            } else if let Some(freeze_at) = clip.freeze_at {
                // Fotograma congelado: se lee un único fotograma del instante
                // pedido y la cadena de filtros lo estira con tpad.
                command
                    .args(["-ss", &format_seconds(freeze_at), "-t", "0.04", "-i"])
                    .arg(path);
            } else {
                command
                    .args([
                        "-ss",
                        &format_seconds(clip.in_seconds),
                        "-t",
                        &format_seconds(clip.source_duration()),
                        "-i",
                    ])
                    .arg(path);
            }
            is_title_input.push(false);
        }
        input_indices.push(media_index);
    }
    (input_indices, is_title_input)
}

fn append_monitor_scopes(
    filters: &mut Vec<String>,
    waveform: bool,
    vectorscope: bool,
) -> &'static str {
    match (waveform, vectorscope) {
        (false, false) => "vout",
        (true, false) => {
            filters.push("[vout]split=2[scopebase][wavein]".to_owned());
            filters.push(
                "[wavein]waveform=mode=column:components=7:display=overlay,scale=240:135[wave]"
                    .to_owned(),
            );
            filters.push("[scopebase][wave]overlay=0:H-h[vscoped]".to_owned());
            "vscoped"
        }
        (false, true) => {
            filters.push("[vout]split=2[scopebase][vecin]".to_owned());
            filters.push("[vecin]vectorscope=mode=color3,scale=240:135[vec]".to_owned());
            filters.push("[scopebase][vec]overlay=W-w:H-h[vscoped]".to_owned());
            "vscoped"
        }
        (true, true) => {
            filters.push("[vout]split=3[scopebase][wavein][vecin]".to_owned());
            filters.push(
                "[wavein]waveform=mode=column:components=7:display=overlay,scale=240:135[wave]"
                    .to_owned(),
            );
            filters.push("[vecin]vectorscope=mode=color3,scale=240:135[vec]".to_owned());
            filters.push("[scopebase][wave]overlay=0:H-h[scopewave]".to_owned());
            filters.push("[scopewave][vec]overlay=W-w:H-h[vscoped]".to_owned());
            "vscoped"
        }
    }
}

/// Grafo de composición compartido por exportación y reproducción del monitor.
#[allow(clippy::too_many_arguments)]
fn build_render_filters(
    clips: &[RoughClip],
    input_indices: &[usize],
    is_title_input: &[bool],
    size: (u32, u32),
    include_video: bool,
    include_audio: bool,
    track_gains: &[f64],
    master_gain_db: f64,
    normalize_loudness: bool,
    timebase: Timebase,
    measured_loudness: Option<&LoudnessReport>,
) -> Result<Vec<String>, String> {
    let (out_w, out_h) = size;
    let frame_rate = timebase.ffmpeg_rate();
    let mut filters = Vec::new();
    let total = clips
        .iter()
        .map(|clip| clip.timeline_start + clip.duration())
        .fold(0.0, f64::max);
    if include_video {
        filters.push(format!(
            "color=c=black:s={out_w}x{out_h}:r={frame_rate}:d={total:.6}[base]"
        ));
    }
    let mut audio_inputs: Vec<efectos::MixInput> = Vec::new();
    for (index, media) in input_indices.iter().enumerate() {
        let speed = clips[index].speed.clamp(0.1, 8.0);
        let start = clips[index].timeline_start.max(0.0);
        if clips[index].has_video && include_video {
            let angle = clips[index].rotation.to_radians();
            let opacity = (clips[index].opacity / 100.0).clamp(0.0, 1.0);
            let (fade_in, fade_out) = clips[index].effective_fades();
            let duration = clips[index].duration();
            let mut fade_filters = String::new();
            let white = |on: bool| if on { ":color=white" } else { "" };
            if fade_in > 0.004 {
                fade_filters.push_str(&format!(
                    ",fade=t=in:st=0:d={fade_in:.3}{}",
                    white(clips[index].runtime.white_in)
                ));
            }
            if fade_out > 0.004 {
                fade_filters.push_str(&format!(
                    ",fade=t=out:st={:.3}:d={fade_out:.3}{}",
                    (duration - fade_out),
                    white(clips[index].runtime.white_out)
                ));
            }
            let eq = color_eq_filter(
                clips[index].exposure,
                clips[index].contrast,
                clips[index].saturation,
            );
            if clips[index].is_adjustment {
                // Sin `[v{index}]`: la capa de ajuste no aporta imagen propia,
                // se aplica directamente sobre lo compuesto debajo en el
                // bucle de composición (más abajo).
            } else if is_title_input[index] {
                let Some(title) = clips[index].title.as_ref() else {
                    return Err("Entrada de titulo sin titulo".to_owned());
                };
                let Some(font) = find_font() else {
                    return Err(
                        "No se encontro una fuente TTF del sistema para los titulos".to_owned()
                    );
                };
                // El tamaño se define sobre 1080p, igual que en el monitor:
                // así un 720p o un 4K conservan la proporción que se ve.
                let fontsize = title.size.max(8.0) * out_h as f64 / 1080.0;
                let drawing = title_layer_filters(title, &font, fontsize, (out_w, out_h), None);
                filters.push(format!(
                    "[{media}:v:0]{drawing},format=rgba,setpts=(PTS-STARTPTS)+{start:.6}/TB,rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none,colorchannelmixer=aa={opacity:.6}{fade_filters}[v{index}]",
                ));
            } else {
                let is_blend = clips[index].fusion.blend_mode().is_some();
                // Los lienzos verticales usan reframe centrado; evita pillarbox sin
                // cambiar el comportamiento de proyectos horizontales existentes.
                // Vertical y cuadrado (Shorts/Reels/TikTok, Instagram) rellenan
                // el lienzo recortando en vez de dejar barras; el horizontal
                // clásico (16:9) conserva el comportamiento previo (ajustar).
                let cover_canvas = is_blend || out_h >= out_w;
                let width = if cover_canvas {
                    out_w
                } else {
                    even_dimension(out_w as f64 * clips[index].scale_percent / 100.0)
                };
                let height = if cover_canvas {
                    out_h
                } else {
                    even_dimension(out_h as f64 * clips[index].scale_percent / 100.0)
                };
                let vig = vignette_filter(clips[index].vignette);
                let blur = blur_filter(clips[index].blur, out_w.min(out_h) as f64);
                let wheels = wheels_filter(clips[index].wheels.as_ref());
                let chroma = chroma_filter(clips[index].chroma.as_ref());
                let curves = curves_filter(clips[index].curves.as_ref());
                let lut = lut_filter(clips[index].lut.as_deref());
                let mask = mask_filter(clips[index].mask.as_ref(), width as f64, height as f64);
                let scale_mode = if cover_canvas { "increase" } else { "decrease" };
                let crop = if cover_canvas {
                    format!(",crop={out_w}:{out_h}")
                } else {
                    String::new()
                };
                let blend_canvas = if is_blend {
                    format!(
                        ",scale={out_w}:{out_h}:force_original_aspect_ratio=increase,crop={out_w}:{out_h}"
                    )
                } else {
                    String::new()
                };
                let freeze_pad = clips[index]
                    .freeze_at
                    .map(|_| {
                        let pad = (clips[index].duration() - 0.04).max(0.0);
                        format!(",tpad=stop_mode=clone:stop_duration={pad:.3}")
                    })
                    .unwrap_or_default();
                let cadence = conform_video_filter(&clips[index], &frame_rate);
                let fx = &clips[index].fx;
                let prefix = fx.input_prefix(clips[index].freeze_at.is_some());
                let geometry = fx.geometry_chain(true);
                let fx_color = fx.video_color_chain();
                let fx_alpha = fx.alpha_chain();
                filters.push(format!(
                    "[{media}:v:0]{prefix}{freeze_pad}setpts=(PTS-STARTPTS)/{speed:.6}{cadence}{geometry},scale={width}:{height}:force_original_aspect_ratio={scale_mode}{crop},setsar=1{wheels}{curves}{lut}{eq}{vig}{blur}{fx_color},format=rgba{chroma}{mask}{fx_alpha},rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none{blend_canvas},colorchannelmixer=aa={opacity:.6}{fade_filters},setpts=PTS+{start:.6}/TB[v{index}]"
                ));
            }
        }
        if clips[index].has_audio && include_audio && !clips[index].is_adjustment {
            let delay_ms = (start * 1000.0).round() as u64;
            let track_gain = track_gains.get(clips[index].track).copied().unwrap_or(0.0);
            let volume = if clips[index].muted {
                0.0
            } else {
                10.0_f64.powf((clips[index].gain_db + track_gain).clamp(-96.0, 24.0) / 20.0)
            };
            let (fade_in_audio, fade_out_audio) = clips[index].effective_fades();
            let fade_in_audio = fade_in_audio.max(clips[index].runtime.audio_fade_in);
            let fade_out_audio = fade_out_audio.max(clips[index].runtime.audio_fade_out);
            let audio_duration = clips[index].duration();
            let mut afade_filters = String::new();
            if fade_in_audio > 0.004 {
                afade_filters.push_str(&format!(",afade=t=in:st=0:d={fade_in_audio:.3}"));
            }
            if fade_out_audio > 0.004 {
                afade_filters.push_str(&format!(
                    ",afade=t=out:st={:.3}:d={fade_out_audio:.3}",
                    (audio_duration - fade_out_audio)
                ));
            }
            let pan = clips[index].pan.clamp(-1.0, 1.0);
            let pan_filter = if pan.abs() > 0.001 {
                format!(",stereotools=balance_in={pan:.4}")
            } else {
                String::new()
            };
            // Los fundidos van antes del retardo, en tiempo local del clip.
            // Tras `adelay` se renumeran los timestamps: algunas versiones de
            // FFmpeg dejan huecos que `amix` y el muxer malinterpretan (el
            // audio salía truncado o con dts no monótonos).
            let fx = &clips[index].fx;
            filters.push(format!(
                "[{media}:a:0]{}asetpts=PTS-STARTPTS,{},aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo{pan_filter}{},volume={volume:.8}{}{afade_filters},adelay={delay_ms}|{delay_ms},asetpts=N/SR/TB[a{index}]",
                fx.audio_prefix(),
                atempo_filter(speed),
                fx.audio_chain(),
                fx.volume_envelope(),
            ));
            audio_inputs.push(efectos::MixInput {
                label: format!("a{index}"),
                role: fx.role,
                duck_db: fx.duck_db,
            });
        }
    }

    let mut overlay_order: Vec<usize> = (0..clips.len())
        .filter(|index| clips[*index].has_video && include_video)
        .collect();
    overlay_order.sort_by(|left, right| {
        clips[*left].track.cmp(&clips[*right].track).then_with(|| {
            clips[*left]
                .timeline_start
                .total_cmp(&clips[*right].timeline_start)
        })
    });
    let video_count = overlay_order.len();
    let mut previous = "base".to_owned();
    for (layer, index) in overlay_order.into_iter().enumerate() {
        let output_label = if layer + 1 == video_count {
            "vout".to_owned()
        } else {
            format!("overlay{layer}")
        };
        let x = clips[index].position_x;
        let y = clips[index].position_y;
        if clips[index].is_adjustment {
            // Capa de ajuste: sin overlay de medio propio. Gradúa una copia
            // de todo lo compuesto debajo (`[previous]`) y la recompone solo
            // donde el alfa lo permite. El alfa combina máscara espacial
            // (opcional), opacidad y una puerta temporal por expresión `T`
            // dentro del propio geq — evita alphamerge/tpad, cuyo manejo del
            // "antes de empezar" no es fiable (verificado con FFmpeg real:
            // sin la puerta por T, el efecto se filtraba antes de tiempo).
            let start = clips[index].timeline_start.max(0.0);
            let end = start + clips[index].duration().max(0.04);
            let opacity = (clips[index].opacity / 100.0).clamp(0.0, 1.0);
            let wheels = wheels_filter(clips[index].wheels.as_ref());
            let curves = curves_filter(clips[index].curves.as_ref());
            let lut = lut_filter(clips[index].lut.as_deref());
            let eq = color_eq_filter(
                clips[index].exposure,
                clips[index].contrast,
                clips[index].saturation,
            );
            let vig = vignette_filter(clips[index].vignette);
            let blur = blur_filter(clips[index].blur, out_w.min(out_h) as f64);
            let spatial = clips[index]
                .mask
                .as_ref()
                .map(|mask| mask.alpha_expression(out_w as f64, out_h as f64))
                .unwrap_or_else(|| "alpha(X,Y)".to_owned())
                .replace(',', "\\,");
            let alpha_expr =
                format!("{spatial}*if(between(T\\,{start:.6}\\,{end:.6})\\,{opacity:.6}\\,0)");
            filters.push(format!(
                "[{previous}]split=2[adjbase{layer}][adjsrc{layer}]"
            ));
            let fx_color = clips[index].fx.video_color_chain();
            filters.push(format!(
                "[adjsrc{layer}]null{wheels}{curves}{lut}{eq}{vig}{blur}{fx_color},format=rgba,geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='{alpha_expr}'[adjfx{layer}]"
            ));
            filters.push(format!(
                "[adjbase{layer}][adjfx{layer}]overlay=eof_action=pass:shortest=0:format=auto[{output_label}]"
            ));
        } else if let Some(mode) = clips[index].fusion.blend_mode() {
            // maskedmerge no expone opciones de framesync y trunca la
            // duración cuando la máscara empieza tarde o acaba antes que la
            // base (verificado con FFmpeg real). En su lugar: blend produce
            // el color ya con la duración completa de la base; tpad extiende
            // la máscara con negro (alfa 0) tras el fin del clip para que el
            // efecto se apague exactamente ahí; alphamerge + overlay son las
            // mismas primitivas de framesync ya validadas para el overlay
            // normal, así que heredan su manejo correcto de inicio tardío.
            filters.push(format!("[{previous}]split=2[base{layer}a][base{layer}b]"));
            filters.push(format!(
                "[v{index}]split=2[layer{layer}rgb][layer{layer}alpha]"
            ));
            filters.push(format!(
                "[layer{layer}alpha]format=rgba,alphaextract,tpad=stop=-1:stop_mode=add:color=black[mask{layer}]"
            ));
            filters.push(format!(
                "[base{layer}a][layer{layer}rgb]blend=all_mode={mode}:shortest=0:repeatlast=1[blend{layer}]"
            ));
            filters.push(format!(
                "[blend{layer}][mask{layer}]alphamerge=shortest=0:repeatlast=0:eof_action=pass[blenda{layer}]"
            ));
            filters.push(format!(
                "[base{layer}b][blenda{layer}]overlay=eof_action=pass:shortest=0:format=auto[{output_label}]"
            ));
        } else if let Some((dx, dy)) = clips[index].runtime.offset_expressions() {
            let term = |expression: String| {
                if expression.is_empty() {
                    String::new()
                } else {
                    format!("+{expression}")
                }
            };
            filters.push(format!(
                "[{previous}][v{index}]overlay=x='(W-w)/2+{x:.3}{}':y='(H-h)/2+{y:.3}{}':eval=frame:eof_action=pass:shortest=0:format=auto[{output_label}]",
                term(dx),
                term(dy)
            ));
        } else {
            filters.push(format!(
                "[{previous}][v{index}]overlay=x=(W-w)/2+{x:.3}:y=(H-h)/2+{y:.3}:eof_action=pass:shortest=0:format=auto[{output_label}]"
            ));
        }
        previous = output_label;
    }
    if include_video && video_count == 0 {
        filters.push("[base]null[vout]".to_owned());
    }
    if include_audio {
        let master = 10.0_f64.powf(master_gain_db.clamp(-96.0, 24.0) / 20.0);
        let mastering = match (normalize_loudness, measured_loudness) {
            (true, Some(report)) => format!(
                ",volume={master:.8},loudnorm=I=-14:TP=-1:LRA=11:{}",
                report.two_pass_args()
            ),
            (true, None) => format!(",volume={master:.8},loudnorm=I=-14:TP=-1:LRA=11"),
            (false, _) => format!(",volume={master:.8}"),
        };
        filters.extend(efectos::mix_audio(&audio_inputs, &mastering, total));
    }
    Ok(filters)
}

/// Exporta con la GPU si se pide y, si la GPU falla (driver viejo, sesión
/// remota, límite de sesiones de NVENC…), repite con CPU en vez de fallar.
#[allow(clippy::too_many_arguments)]
fn run_export(
    clips: &[RoughClip],
    output: &Path,
    cancel: &AtomicBool,
    fast: bool,
    size: (u32, u32),
    audio_only: bool,
    format: ExportFormat,
    track_gains: &[f64],
    master_gain_db: f64,
    normalize_loudness: bool,
    timebase: Timebase,
    measured_loudness: Option<&LoudnessReport>,
    hw: Option<aceleracion::HwBackend>,
    progress: &Arc<std::sync::Mutex<RenderProgress>>,
) -> Result<(), String> {
    let attempt = |hw| {
        run_export_once(
            clips,
            output,
            cancel,
            fast,
            size,
            audio_only,
            format,
            track_gains,
            master_gain_db,
            normalize_loudness,
            timebase,
            measured_loudness,
            hw,
            progress,
        )
    };
    let result = attempt(hw);
    match (result, hw) {
        (Err(_), Some(backend)) if !cancel.load(Ordering::Relaxed) => {
            if let Ok(mut state) = progress.lock() {
                state.pct = 0.0;
                state.note = Some(format!("{} falló; se exportó con CPU", backend.label()));
            }
            attempt(None)
        }
        (result, _) => result,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_export_once(
    clips: &[RoughClip],
    output: &Path,
    cancel: &AtomicBool,
    fast: bool,
    size: (u32, u32),
    audio_only: bool,
    format: ExportFormat,
    track_gains: &[f64],
    master_gain_db: f64,
    normalize_loudness: bool,
    timebase: Timebase,
    measured_loudness: Option<&LoudnessReport>,
    hw: Option<aceleracion::HwBackend>,
    progress: &Arc<std::sync::Mutex<RenderProgress>>,
) -> Result<(), String> {
    let prepared = prepare_render_clips(clips);
    let clips: &[RoughClip] = &prepared;
    if let Some(missing) = clips
        .iter()
        .find_map(|clip| clip.lut.as_deref().filter(|path| !path.is_file()))
    {
        return Err(format!(
            "LUT no encontrada, exportación cancelada: {}",
            missing.display()
        ));
    }
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export.mp4")
        .to_owned();
    let ext = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or(format.extension())
        .to_owned();
    let temporary =
        output.with_file_name(format!(".{file_name}.{}.part.{ext}", std::process::id()));
    let error_log =
        output.with_file_name(format!(".{file_name}.{}.ffmpeg.log", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let _ = std::fs::remove_file(&error_log);
    let mut command = Command::new(tool_path("ffmpeg.exe"));
    command.arg("-y");
    let (input_indices, is_title_input) =
        push_render_inputs(&mut command, clips, size, false, timebase);
    let filters = build_render_filters(
        clips,
        &input_indices,
        &is_title_input,
        size,
        !audio_only,
        audio_only || format.has_audio_track(),
        track_gains,
        master_gain_db,
        normalize_loudness,
        timebase,
        measured_loudness,
    )?;
    let total = clips
        .iter()
        .map(|clip| clip.timeline_start + clip.duration())
        .fold(0.0, f64::max);

    let log_file = std::fs::File::create(&error_log)
        .map_err(|error| format!("No se pudo crear el registro de exportación: {error}"))?;
    let mut filters = filters;
    let video_label = if format == ExportFormat::Gif && !audio_only {
        filters.extend(gif_palette_filters("vout", "gifout"));
        "[gifout]"
    } else {
        "[vout]"
    };
    let child = command.args(["-filter_complex", &filters.join(";")]);
    if !audio_only {
        child.args(["-map", video_label]);
    }
    if audio_only || format.has_audio_track() {
        child.args(["-map", "[aout]"]);
    }
    child.args(["-progress", "pipe:1", "-nostats"]);
    child.args(format.export_args(fast, hw));
    let mut child = child
        .args(["-t", &format_seconds(total)])
        .arg(&temporary)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log_file))
        .spawn()
        .map_err(|error| format!("FFmpeg no esta disponible: {error}"))?;
    // Hilo lector de progreso: -progress escribe pares clave=valor por stdout.
    if let Some(stdout) = child.stdout.take() {
        let progress = Arc::clone(progress);
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stdout);
            let started = std::time::Instant::now();
            let mut out_us: f64 = 0.0;
            for line in reader.lines().map_while(Result::ok) {
                if let Some(value) = line.strip_prefix("out_time_us=") {
                    out_us = value.trim().parse::<f64>().unwrap_or(out_us);
                    let pct = (out_us / 1_000_000.0 / total.max(0.001)).clamp(0.0, 1.0);
                    let elapsed = started.elapsed().as_secs_f64();
                    let eta = if pct > 0.002 {
                        elapsed * (1.0 - pct) / pct
                    } else {
                        0.0
                    };
                    if let Ok(mut state) = progress.lock() {
                        state.pct = pct;
                        state.eta_secs = eta;
                    }
                }
            }
        });
    }
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&temporary);
            let _ = std::fs::remove_file(&error_log);
            return Err("Exportación cancelada".to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(error) => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&temporary);
                let _ = std::fs::remove_file(&error_log);
                return Err(format!("No se pudo supervisar FFmpeg: {error}"));
            }
        }
    };
    if status.success() {
        let _ = std::fs::remove_file(&error_log);
        if output.exists() {
            std::fs::remove_file(output)
                .map_err(|error| format!("No se pudo sustituir el destino: {error}"))?;
        }
        std::fs::rename(&temporary, output)
            .map_err(|error| format!("No se pudo instalar la exportación terminada: {error}"))
    } else {
        let _ = std::fs::remove_file(&temporary);
        let message = std::fs::read_to_string(&error_log).unwrap_or_default();
        let _ = std::fs::remove_file(&error_log);
        Err(message
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | "))
    }
}

fn tool_path(name: &str) -> PathBuf {
    // En el host de desarrollo (macOS/Linux) las herramientas no llevan
    // extensión y se toman del PATH.
    #[cfg(not(windows))]
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let beside_app = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|directory| directory.join(name)))
        .filter(|path| path.exists());
    if let Some(path) = beside_app {
        return path;
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local_app_data);
        let winget_link = local
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
            .join(name);
        if winget_link.exists() {
            return winget_link;
        }
        // Algunas versiones de WinGet no crean el enlace en Links y dejan el
        // binario dentro de Packages\Gyan.FFmpeg_*\ffmpeg-*\bin\*.
        let packages = local.join("Microsoft").join("WinGet").join("Packages");
        if packages.is_dir() {
            if let Some(found) = find_in_winget_packages(&packages, name) {
                return found;
            }
        }
    }
    PathBuf::from(name)
}

/// Búsqueda acotada (máximo 4 niveles) de un ejecutable dentro del árbol de
/// paquetes de WinGet.
fn find_in_winget_packages(root: &Path, name: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
        if depth > 4 {
            return None;
        }
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = walk(&path, name, depth + 1) {
                    return Some(found);
                }
            } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
        }
        None
    }
    walk(root, name, 0)
}

fn recovery_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local)
            .join("NovaCut")
            .join("Recovery")
            .join("last-session.ncrough")
    })
}

fn clear_recovery() {
    if let Some(path) = recovery_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Escribe primero fuera del destino para que un cierre durante la escritura
/// no deje un `.ncrough` truncado. Windows no reemplaza con `rename` si el
/// destino existe, por eso se elimina solo despues de que el temporal cierre.
fn write_text_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "El proyecto no tiene carpeta contenedora".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("No se pudo crear la carpeta del proyecto: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "El proyecto no tiene un nombre valido".to_owned())?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, contents)
        .map_err(|error| format!("No se pudo escribir el temporal: {error}"))?;
    if path.exists() {
        if let Err(error) = std::fs::remove_file(path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("No se pudo sustituir el proyecto: {error}"));
        }
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("No se pudo instalar el proyecto: {error}"));
    }
    Ok(())
}

fn resolve_project_paths(project: &mut RoughProject, project_directory: Option<&Path>) {
    let Some(directory) = project_directory.filter(|path| !path.as_os_str().is_empty()) else {
        return;
    };
    for clip in &mut project.clips {
        resolve_clip_paths(clip, directory);
    }
}

fn resolve_clip_paths(clip: &mut RoughClip, project_directory: &Path) {
    resolve_path(&mut clip.path, project_directory);
    if let Some(proxy) = &mut clip.proxy {
        resolve_path(proxy, project_directory);
    }
    if let Some(lut) = &mut clip.lut {
        resolve_path(lut, project_directory);
    }
    if let Some(children) = &mut clip.nested {
        for child in children {
            resolve_clip_paths(child, project_directory);
        }
    }
}

fn project_for_storage(project: &RoughProject, project_path: &Path) -> RoughProject {
    let mut stored = project.clone();
    let Some(directory) = project_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    else {
        return stored;
    };
    for clip in &mut stored.clips {
        relativize_clip_paths(clip, directory);
    }
    stored
}

fn relativize_clip_paths(clip: &mut RoughClip, project_directory: &Path) {
    relativize_path(&mut clip.path, project_directory);
    if let Some(proxy) = &mut clip.proxy {
        relativize_path(proxy, project_directory);
    }
    if let Some(lut) = &mut clip.lut {
        relativize_path(lut, project_directory);
    }
    if let Some(children) = &mut clip.nested {
        for child in children {
            relativize_clip_paths(child, project_directory);
        }
    }
}

fn resolve_path(path: &mut PathBuf, project_directory: &Path) {
    if !path.as_os_str().is_empty() && path.is_relative() {
        *path = project_directory.join(&*path);
    }
}

fn relativize_path(path: &mut PathBuf, project_directory: &Path) {
    if path.as_os_str().is_empty() || path.is_relative() {
        return;
    }
    if let Ok(relative) = path.strip_prefix(project_directory) {
        if !relative.as_os_str().is_empty() {
            *path = relative.to_path_buf();
        }
    }
}

fn resolve_mac_media(media: &MacMedia, project_directory: Option<&Path>) -> Option<PathBuf> {
    if let (Some(directory), Some(relative)) = (project_directory, &media.ruta_relativa) {
        let candidate = directory.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let absolute = PathBuf::from(&media.ruta);
    if absolute.exists() {
        return Some(absolute);
    }
    if let Some(directory) = project_directory {
        let neighbor = directory.join(&media.nombre);
        if neighbor.exists() {
            return Some(neighbor);
        }
    }
    None
}

fn montage_preview_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("Preview"))
}

fn montage_preview_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    montage_preview_directory()
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("montaje-{stamp}.mp4"))
}

/// Borra previsualizaciones con más de una hora de antigüedad.
fn cleanup_old_previews() {
    let Some(directory) = montage_preview_directory() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() > 3600);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Guarda una copia de seguridad con marca de tiempo junto al proyecto y
/// conserva solo las 10 más recientes.
fn save_backup(project_path: &Path, project: &RoughProject) {
    let Some(parent) = project_path.parent() else {
        return;
    };
    let Some(stem) = project_path.file_stem().and_then(|stem| stem.to_str()) else {
        return;
    };
    let backup_dir = parent.join("NovaCut-Backups");
    if std::fs::create_dir_all(&backup_dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let backup_path = backup_dir.join(format!("{stem}-{stamp}.ncrough.bak"));
    if let Ok(json) = serde_json::to_string_pretty(project) {
        let _ = std::fs::write(&backup_path, json);
    }
    // Podar: quedarse con las 10 más recientes.
    let mut backups: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&backup_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
                    Some((modified, entry.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    backups.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, stale) in backups.into_iter().skip(10) {
        let _ = std::fs::remove_file(stale);
    }
}

fn save_recovery(project: &RoughProject) {
    let Some(path) = recovery_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_ok() {
        if let Ok(json) = serde_json::to_string_pretty(project) {
            let temporary = path.with_extension("tmp");
            if std::fs::write(&temporary, json).is_ok() {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::rename(temporary, path);
            }
        }
    }
}

fn load_recovery() -> Option<RoughProject> {
    let json = std::fs::read_to_string(recovery_path()?).ok()?;
    serde_json::from_str(&json).ok()
}

/// ¿Está disponible el instalador `winget` (App Installer)?
fn winget_available() -> bool {
    Command::new("winget.exe")
        .arg("--version")
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Intenta instalar Gyan.FFmpeg con WinGet.
fn run_winget_install() -> Result<(), String> {
    let output = Command::new("winget.exe")
        .args([
            "install",
            "--id",
            "Gyan.FFmpeg",
            "--exact",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--silent",
            "--disable-interactivity",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("WinGet no se pudo ejecutar: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

/// Descarga el build "release essentials" de gyan.dev y copia los binarios
/// junto a la aplicacion, sin depender de WinGet ni de la Microsoft Store.
/// PowerShell está disponible en todo Windows 10/11.
fn run_powershell_install(app_dir: &Path) -> Result<(), String> {
    let script_path = std::env::temp_dir().join("novacut-install-ffmpeg.ps1");
    let app_dir_ps = app_dir.display().to_string().replace('\'', "''");
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$appDir = '{app_dir_ps}'
$zip = Join-Path $env:TEMP 'novacut-ffmpeg.zip'
$unzip = Join-Path $env:TEMP 'novacut-ffmpeg'
if (Test-Path $unzip) {{ Remove-Item $unzip -Recurse -Force }}
if (Test-Path $zip) {{ Remove-Item $zip -Force }}
Invoke-WebRequest -UseBasicParsing -Uri 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $unzip -Force
$bin = Get-ChildItem $unzip -Recurse -Filter 'ffmpeg.exe' | Select-Object -First 1
if (-not $bin) {{ throw 'El paquete descargado no contiene ffmpeg.exe' }}
Copy-Item (Join-Path $bin.DirectoryName '*') $appDir -Force
Remove-Item $unzip -Recurse -Force
Remove-Item $zip -Force
"#
    );
    let write_result = std::fs::write(&script_path, script);
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("PowerShell no se pudo ejecutar: {error}"));
    let _ = std::fs::remove_file(&script_path);
    write_result.map_err(|error| format!("No se pudo preparar el instalador: {error}"))?;
    let output = output?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn multimedia_tools_available() -> bool {
    ["ffmpeg.exe", "ffprobe.exe", "ffplay.exe"]
        .iter()
        .all(|name| {
            let path = tool_path(name);
            let locator = if cfg!(windows) { "where.exe" } else { "which" };
            path.is_absolute()
                || Command::new(locator)
                    .arg(&path)
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
        })
}

fn format_seconds(seconds: f64) -> String {
    format!("{:.6}", seconds.max(0.0))
}

fn default_fps() -> f64 {
    30.0
}

/// Timecode HH:MM:SS:FF como el visor de la app macOS.
fn timecode(seconds: f64, fps: f64) -> String {
    let timebase = Timebase::from_fps(if fps >= 1.0 { fps } else { default_fps() });
    timebase.timecode(timebase.frames(seconds))
}

/// Duración legible para reglas y mensajes: 8s, 1:05, 1:02:03.
fn format_clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds as u64;
    if whole >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            whole / 3600,
            (whole / 60) % 60,
            whole % 60
        )
    } else if whole >= 60 {
        format!("{}:{:02}", whole / 60, whole % 60)
    } else if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{whole}s")
    }
}

fn normal_speed() -> f64 {
    1.0
}

fn normal_scale() -> f64 {
    100.0
}

fn full_opacity() -> f64 {
    100.0
}

fn enabled_by_default() -> bool {
    true
}

fn atempo_filter(mut speed: f64) -> String {
    speed = speed.clamp(0.1, 8.0);
    let mut stages = Vec::new();
    while speed > 2.0 {
        stages.push("atempo=2.0".to_owned());
        speed /= 2.0;
    }
    while speed < 0.5 {
        stages.push("atempo=0.5".to_owned());
        speed /= 0.5;
    }
    stages.push(format!("atempo={speed:.6}"));
    stages.join(",")
}

/// Icono de ventana (RGBA crudo de 64×64 generado por `tools/make_icon.py`).
fn window_icon() -> egui::IconData {
    egui::IconData {
        width: 64,
        height: 64,
        rgba: include_bytes!("../../assets/icon_64.rgba").to_vec(),
    }
}

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("NovaCut Windows")
        .with_inner_size([1280.0, 760.0])
        .with_min_inner_size([800.0, 500.0])
        .with_icon(window_icon());
    // Revisión de interfaz: NOVACUT_UI_SIZE=1366x768 fija tamaño y posición
    // para reproducir pantallas típicas (p. ej. 1080p al 150 % = 1280x720).
    if let Some((width, height)) = std::env::var("NOVACUT_UI_SIZE").ok().and_then(|value| {
        let (width, height) = value.split_once('x')?;
        Some((width.parse::<f32>().ok()?, height.parse::<f32>().ok()?))
    }) {
        viewport = viewport
            .with_inner_size([width, height])
            .with_min_inner_size([width, height])
            .with_position([0.0, 40.0]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "NovaCut Windows",
        options,
        Box::new(|context| {
            theme::apply(&context.egui_ctx);
            Ok(Box::new(NovaCutWindows::new(context)))
        }),
    )
}

/// Lenguaje visual unificado con la app macOS: grises neutros, acento cian,
/// esquinas suaves y densidad compacta. Todos los valores salen del diseño de
/// `src/ui/App.swift` (0.075 de fondo, 0.115 la barra superior, 0.095 las
/// cabeceras de panel, 0.12 las tarjetas).
mod theme {
    use crate::egui::{
        self, Align2, Color32, CornerRadius, FontId, Margin, Pos2, Response, RichText, Sense,
        Stroke, Ui, Vec2,
    };

    /// Acento cian, equivalente al tint(.cyan) de la app macOS.
    pub const ACCENT: Color32 = Color32::from_rgb(0, 190, 212);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(0, 132, 150);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(0, 21, 23, 28);

    /// Fondo general del lienzo (calibratedWhite 0.075).
    pub const BG: Color32 = Color32::from_rgb(19, 19, 19);
    /// Barra superior (calibratedWhite 0.115).
    pub const BAR: Color32 = Color32::from_rgb(29, 29, 29);
    /// Cabeceras de panel (calibratedWhite 0.095).
    pub const PANEL_HEADER: Color32 = Color32::from_rgb(24, 24, 24);
    /// Tarjetas y filas (calibratedWhite 0.12).
    pub const CARD: Color32 = Color32::from_rgb(31, 31, 31);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(42, 42, 42);
    pub const STROKE: Color32 = Color32::from_rgb(52, 52, 52);
    pub const STROKE_SOFT: Color32 = Color32::from_rgb(38, 38, 38);

    pub const TEXT: Color32 = Color32::from_rgb(228, 228, 228);
    /// Secundario y terciario con contraste AA sobre el fondo (≥ 7:1 y
    /// ≥ 5:1): los grises anteriores se perdían en monitores de oficina.
    pub const TEXT_DIM: Color32 = Color32::from_rgb(182, 182, 182);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(146, 146, 146);

    pub const OK: Color32 = Color32::from_rgb(90, 220, 120);
    pub const WARN: Color32 = Color32::from_rgb(246, 140, 40);
    pub const DANGER: Color32 = Color32::from_rgb(246, 83, 83);

    pub const R: CornerRadius = CornerRadius::same(5);

    /// Fuentes de la interfaz: las de egui, con la monoespaciada (Hack) como
    /// último respaldo de la proporcional. Sin ella, flechas, ▾, ▲▼ y los
    /// iconos de herramientas salían como cuadrados vacíos.
    pub fn fonts() -> egui::FontDefinitions {
        let mut fonts = egui::FontDefinitions::default();
        let monospace = fonts.families[&egui::FontFamily::Monospace].clone();
        let proportional = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .expect("egui siempre define la familia proporcional");
        for name in monospace {
            if !proportional.contains(&name) {
                proportional.push(name);
            }
        }
        fonts
    }

    /// Aplica el tema completo al contexto. Sustituye al antiguo
    /// `Visuals::dark()` con retoques sueltos.
    pub fn apply(ctx: &egui::Context) {
        ctx.set_fonts(fonts());
        let mut style = (*ctx.style()).clone();
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        // Menús y tooltips: más claros que el fondo y con borde, para que
        // se distingan de lo que hay debajo.
        v.window_fill = Color32::from_rgb(38, 38, 40);
        v.extreme_bg_color = Color32::from_rgb(14, 14, 15);
        v.faint_bg_color = Color32::from_rgb(28, 28, 30);
        v.hyperlink_color = ACCENT;
        v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(70, 70, 74));
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(120, 235, 255));
        v.widgets.noninteractive.bg_fill = CARD;
        v.widgets.noninteractive.weak_bg_fill = CARD;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, STROKE_SOFT);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);
        v.widgets.noninteractive.corner_radius = CornerRadius::same(4);
        v.widgets.inactive.bg_fill = CARD;
        v.widgets.inactive.weak_bg_fill = CARD;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, STROKE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.inactive.corner_radius = R;
        v.widgets.hovered.bg_fill = CARD_HOVER;
        v.widgets.hovered.weak_bg_fill = CARD_HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(72, 72, 72));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        v.widgets.hovered.corner_radius = R;
        v.widgets.active.bg_fill = ACCENT_DIM;
        v.widgets.active.weak_bg_fill = ACCENT_DIM;
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        v.widgets.active.corner_radius = R;
        v.widgets.open.bg_fill = CARD;
        v.widgets.open.weak_bg_fill = CARD;
        v.widgets.open.bg_stroke = Stroke::new(1.0_f32, STROKE);
        v.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.open.corner_radius = R;

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(10.0, 5.0);
        s.interact_size = Vec2::new(40.0, 24.0);
        s.indent = 18.0;
        s.slider_width = 130.0;
        s.slider_rail_height = 4.0;
        s.window_margin = Margin::same(10);
        s.menu_margin = Margin::same(6);
        s.combo_width = 140.0;
        style.animation_time = 0.1;

        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(12.5));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.5));
        ctx.set_style(style);
    }

    /// Marca de la app: cuadrado redondeado con la N, sustituye al icono real.
    pub fn logo_mark(ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, ACCENT_DIM);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "N",
            FontId::proportional(13.0),
            Color32::WHITE,
        );
    }

    /// Separador vertical fino de 22 px, como los Divider().frame(height: 22).
    pub fn bar_separator(ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 22.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(56));
    }

    /// Botón de la barra superior: texto 11 pt, como en macOS.
    pub fn bar_button(ui: &mut Ui, label: &str) -> Response {
        ui.add(egui::Button::new(RichText::new(label).size(11.0)))
    }

    /// Botón principal con el acento (buttonStyle .borderedProminent + tint cyan).
    pub fn accent_button(ui: &mut Ui, label: &str) -> Response {
        ui.add(
            egui::Button::new(
                RichText::new(label)
                    .size(11.0)
                    .color(Color32::from_rgb(8, 24, 27)),
            )
            .fill(ACCENT)
            .min_size(Vec2::new(0.0, 24.0)),
        )
    }

    /// Cabecera de panel a lo `panelTitle` de la app Mac: barra de 30 px con
    /// el título en mayúsculas, negrita y el contador en terciario.
    pub fn panel_header(ui: &mut Ui, title: &str, count: Option<usize>) {
        let height = 30.0;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, PANEL_HEADER);
        painter.line_segment(
            [
                Pos2::new(rect.left(), rect.bottom()),
                Pos2::new(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(14, 14, 14)),
        );
        painter.text(
            Pos2::new(rect.left() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            title.to_uppercase(),
            FontId::proportional(11.5),
            TEXT,
        );
        if let Some(count) = count {
            let title_width = painter
                .layout_no_wrap(title.to_uppercase(), FontId::proportional(11.5), TEXT)
                .size()
                .x;
            painter.text(
                Pos2::new(rect.left() + 12.0 + title_width + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                count.to_string(),
                FontId::proportional(11.5),
                TEXT_FAINT,
            );
        }
        ui.add_space(6.0);
    }

    /// Etiqueta de sección dentro del inspector, al estilo de los títulos
    /// pequeños de la app Mac.
    ///
    /// Cada apartado arranca con aire, una marca de acento y una línea a
    /// todo el ancho, para que se distinga dónde empieza y acaba.
    pub fn section_label(ui: &mut Ui, title: &str) {
        ui.add_space(10.0);
        let height = 18.0;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(rect.left(), rect.center().y - 6.0),
                Vec2::new(3.0, 12.0),
            ),
            1.0,
            ACCENT,
        );
        let galley =
            painter.layout_no_wrap(title.to_uppercase(), FontId::proportional(11.5), TEXT_DIM);
        let text_width = galley.size().x;
        painter.galley(
            Pos2::new(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0),
            galley,
            TEXT_DIM,
        );
        let line_start = rect.left() + 9.0 + text_width + 8.0;
        if line_start < rect.right() {
            painter.line_segment(
                [
                    Pos2::new(line_start, rect.center().y),
                    Pos2::new(rect.right(), rect.center().y),
                ],
                Stroke::new(1.0_f32, STROKE),
            );
        }
        ui.add_space(4.0);
    }

    /// Punto de estado del documento: naranja si hay cambios sin guardar,
    /// verde si todo está guardado.
    pub fn dirty_dot(ui: &mut Ui, dirty: bool) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, 7.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 3.5, if dirty { WARN } else { OK });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El montaje debe sobrevivir a salir por EDL y volver a entrar: cortes,
    /// entradas en origen, canales y velocidad. Es la comprobación que no
    /// depende de que el formato esté escrito como yo creo.
    #[test]
    fn a_project_survives_the_round_trip_through_an_edl() {
        let timebase = Timebase::P25;
        let original = vec![
            RoughClip {
                path: PathBuf::from("C:/medios/Entrevista.mov"),
                in_seconds: 0.0,
                out_seconds: 4.8,
                timeline_start: 0.0,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("C:/medios/B roll.mov"),
                in_seconds: 2.0,
                out_seconds: 5.2,
                has_audio: false,
                timeline_start: 4.8,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("C:/medios/Ambiente.wav"),
                in_seconds: 1.6,
                out_seconds: 9.6,
                has_video: false,
                track: 1,
                speed: 2.0,
                timeline_start: 8.0,
                ..Default::default()
            },
        ];
        let (cuts, skipped) = project_to_edl_clips(&original, timebase);
        assert!(skipped.is_empty());
        assert_eq!(cuts.len(), 3);
        assert_eq!(cuts[0].channel, edl::Channel::Both);
        assert_eq!(cuts[1].channel, edl::Channel::Video);
        assert_eq!(cuts[2].channel, edl::Channel::Audio(2));

        let text = edl::to_edl("Round trip", timebase, &cuts);
        let document = edl::from_edl(&text, timebase);
        assert!(document.warnings.is_empty(), "{:?}", document.warnings);
        let rebuilt = edl_clips_to_project(&document.clips, timebase, &[]);

        // El evento B se parte en vídeo y audio; los otros dos siguen sueltos.
        assert_eq!(rebuilt.len(), 4);
        let video: Vec<&RoughClip> = rebuilt.iter().filter(|clip| clip.has_video).collect();
        let audio: Vec<&RoughClip> = rebuilt.iter().filter(|clip| clip.has_audio).collect();
        assert_eq!(video.len(), 2);
        assert_eq!(audio.len(), 2);
        for (before, after) in original.iter().zip([video[0], video[1], audio[1]]) {
            assert!((after.timeline_start - before.timeline_start).abs() < 0.001);
            assert!((after.duration() - before.duration()).abs() < 0.001);
            assert!((after.in_seconds - before.in_seconds).abs() < 0.001);
        }
        assert!((audio[1].speed - 2.0).abs() < 0.001, "{:?}", audio[1].speed);
        assert_eq!(audio[1].track, 1, "el canal A2 vuelve a su pista");
    }

    #[test]
    fn what_an_edl_cannot_carry_is_reported_instead_of_dropped() {
        let clips = vec![
            RoughClip {
                path: PathBuf::from("C:/medios/plano.mov"),
                title: Some(Titulo::default()),
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("C:/medios/plano.mov"),
                is_adjustment: true,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("C:/medios/plano.mov"),
                out_seconds: 2.0,
                speed_ramp: Some(vec![SpeedPoint {
                    source_t: 0.0,
                    speed: 1.0,
                }]),
                ..Default::default()
            },
        ];
        let (cuts, skipped) = project_to_edl_clips(&clips, Timebase::P25);
        assert_eq!(skipped.len(), 3, "{skipped:?}");
        // La rampa se avisa, pero el corte viaja: mejor el corte sin retime que
        // perder el plano entero.
        assert_eq!(cuts.len(), 1);
        assert!(skipped.iter().any(|note| note.contains("rampa")));
    }

    #[test]
    fn imported_cuts_never_overlap_on_the_same_track() {
        let timebase = Timebase::P25;
        let text = "TITLE: SOLAPE\n\
001  CINTA01 V     C        00:00:00:00 00:00:04:00 00:00:00:00 00:00:04:00\n\
002  CINTA02 V     C        00:00:00:00 00:00:04:00 00:00:02:00 00:00:06:00\n\
003  CINTA03 V     C        00:00:00:00 00:00:04:00 00:00:03:00 00:00:07:00\n";
        let document = edl::from_edl(text, timebase);
        let clips = edl_clips_to_project(&document.clips, timebase, &[]);
        assert_eq!(clips.len(), 3);
        let mut tracks: Vec<usize> = clips.iter().map(|clip| clip.track).collect();
        tracks.sort_unstable();
        assert_eq!(tracks, vec![0, 1, 2]);
    }

    #[test]
    fn clip_duration_never_becomes_negative() {
        let clip = RoughClip {
            path: PathBuf::from("plan.mp4"),
            in_seconds: 8.0,
            out_seconds: 3.0,
            source_duration_seconds: None,
            source_timebase: None,
            source_vfr: false,
            source_pts: None,
            has_video: true,
            has_audio: true,
            speed: 1.0,
            timeline_start: 0.0,
            track: 0,
            gain_db: 0.0,
            muted: false,
            pan: 0.0,
            position_x: 0.0,
            position_y: 0.0,
            scale_percent: 100.0,
            rotation: 0.0,
            opacity: 100.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            title: None,
            is_adjustment: false,
            exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            vignette: 0.0,
            transition: None,
            transition_duration: 0.5,
            label: 0,
            blur: 0.0,
            wheels: None,
            chroma: None,
            curves: None,
            keyframes: None,
            fusion: Fusion::Normal,
            mask: None,
            lut: None,
            proxy: None,
            speed_ramp: None,
            fx: efectos::ClipFx::default(),
            runtime: efectos::TransitionRuntime::default(),
            nested: None,
            freeze_at: None,
            enabled: true,
        };
        assert_eq!(clip.duration(), 0.0);
    }

    #[test]
    fn seconds_are_safe_for_ffmpeg_arguments() {
        assert_eq!(format_seconds(-2.0), "0.000000");
        assert_eq!(format_seconds(1.25), "1.250000");
    }

    #[test]
    fn project_round_trip_keeps_edit_points() {
        let project = RoughProject {
            version: 2,
            name: "Prueba".to_owned(),
            clips: vec![RoughClip {
                path: PathBuf::from(r"C:\video\plan.mp4"),
                in_seconds: 1.5,
                out_seconds: 4.0,
                source_duration_seconds: Some(12.0),
                source_timebase: None,
                source_vfr: false,
                source_pts: None,
                has_video: true,
                has_audio: false,
                speed: 1.0,
                timeline_start: 3.0,
                track: 1,
                gain_db: -3.0,
                muted: false,
                pan: 0.0,
                position_x: 120.0,
                position_y: -40.0,
                scale_percent: 75.0,
                rotation: 5.0,
                opacity: 80.0,
                fade_in_seconds: 1.0,
                fade_out_seconds: 2.0,
                title: None,
                is_adjustment: false,
                exposure: 0.0,
                contrast: 0.0,
                saturation: 0.0,
                vignette: 0.0,
                transition: None,
                transition_duration: 0.5,
                label: 0,
                blur: 0.0,
                wheels: None,
                chroma: None,
                curves: None,
                keyframes: None,
                fusion: Fusion::Normal,
                mask: None,
                lut: None,
                proxy: None,
                speed_ramp: None,
                fx: efectos::ClipFx::default(),
                runtime: efectos::TransitionRuntime::default(),
                nested: None,
                freeze_at: None,
                enabled: true,
            }],
            markers: vec![],
            subtitles: vec![],
            subtitle_style: None,
            ..RoughProject::default()
        };
        let json = serde_json::to_string(&project).unwrap();
        let restored: RoughProject = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.clips[0].in_seconds, 1.5);
        assert_eq!(restored.clips[0].out_seconds, 4.0);
        assert!(!restored.clips[0].has_audio);
        assert_eq!(restored.clips[0].speed, 1.0);
        assert_eq!(restored.clips[0].timeline_start, 3.0);
        assert_eq!(restored.clips[0].track, 1);
        assert_eq!(restored.clips[0].gain_db, -3.0);
        assert_eq!(restored.clips[0].position_x, 120.0);
        assert_eq!(restored.clips[0].opacity, 80.0);
        assert_eq!(restored.clips[0].fade_in_seconds, 1.0);
        assert_eq!(restored.clips[0].fade_out_seconds, 2.0);
        assert_eq!(restored.clips[0].source_duration_seconds, Some(12.0));
    }

    #[test]
    fn normalize_clamps_edit_points_to_the_physical_source() {
        let mut project = RoughProject {
            clips: vec![RoughClip {
                path: PathBuf::from("corto.mp4"),
                in_seconds: 8.0,
                out_seconds: 12.0,
                source_duration_seconds: Some(5.0),
                ..Default::default()
            }],
            ..RoughProject::default()
        };
        project.normalize();
        assert_eq!(project.clips[0].out_seconds, 5.0);
        assert!(project.clips[0].in_seconds < project.clips[0].out_seconds);
    }

    #[test]
    fn normalize_shares_known_source_duration_between_instances() {
        let path = PathBuf::from("reutilizado.mp4");
        let mut project = RoughProject {
            clips: vec![
                RoughClip {
                    path: path.clone(),
                    out_seconds: 4.0,
                    source_duration_seconds: Some(20.0),
                    ..Default::default()
                },
                RoughClip {
                    path,
                    in_seconds: 2.0,
                    out_seconds: 6.0,
                    source_duration_seconds: None,
                    ..Default::default()
                },
            ],
            ..RoughProject::default()
        };
        project.normalize();
        assert_eq!(project.clips[1].source_duration_seconds, Some(20.0));
    }

    #[test]
    fn fades_never_exceed_half_clip() {
        let mut clip = RoughClip {
            path: PathBuf::from("plan.mp4"),
            in_seconds: 0.0,
            out_seconds: 4.0,
            source_duration_seconds: Some(4.0),
            source_timebase: None,
            source_vfr: false,
            source_pts: None,
            has_video: true,
            has_audio: true,
            speed: 1.0,
            timeline_start: 0.0,
            track: 0,
            gain_db: 0.0,
            muted: false,
            pan: 0.0,
            position_x: 0.0,
            position_y: 0.0,
            scale_percent: 100.0,
            rotation: 0.0,
            opacity: 100.0,
            fade_in_seconds: 9.0,
            fade_out_seconds: 5.0,
            title: None,
            is_adjustment: false,
            exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            vignette: 0.0,
            transition: None,
            transition_duration: 0.5,
            label: 0,
            blur: 0.0,
            wheels: None,
            chroma: None,
            curves: None,
            keyframes: None,
            fusion: Fusion::Normal,
            mask: None,
            lut: None,
            proxy: None,
            speed_ramp: None,
            fx: efectos::ClipFx::default(),
            runtime: efectos::TransitionRuntime::default(),
            nested: None,
            freeze_at: None,
            enabled: true,
        };
        clip.fade_in_seconds = clip.fade_in_seconds.max(0.0).min(clip.duration() / 2.0);
        clip.fade_out_seconds = clip.fade_out_seconds.max(0.0).min(clip.duration() / 2.0);
        let (fin, fout) = clip.effective_fades();
        assert!((fin - 2.0).abs() < f64::EPSILON);
        assert!((fout - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn speed_changes_timeline_duration_and_audio_filter() {
        let clip = RoughClip {
            path: PathBuf::from("plan.mp4"),
            in_seconds: 2.0,
            out_seconds: 10.0,
            source_duration_seconds: Some(10.0),
            source_timebase: None,
            source_vfr: false,
            source_pts: None,
            has_video: true,
            has_audio: true,
            speed: 2.0,
            timeline_start: 0.0,
            track: 0,
            gain_db: 0.0,
            muted: false,
            pan: 0.0,
            position_x: 0.0,
            position_y: 0.0,
            scale_percent: 100.0,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
            title: None,
            is_adjustment: false,
            exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            vignette: 0.0,
            transition: None,
            transition_duration: 0.5,
            label: 0,
            blur: 0.0,
            wheels: None,
            chroma: None,
            curves: None,
            keyframes: None,
            rotation: 0.0,
            opacity: 100.0,
            fusion: Fusion::Normal,
            mask: None,
            lut: None,
            proxy: None,
            speed_ramp: None,
            fx: efectos::ClipFx::default(),
            runtime: efectos::TransitionRuntime::default(),
            nested: None,
            freeze_at: None,
            enabled: true,
        };
        assert_eq!(clip.duration(), 4.0);
        assert_eq!(atempo_filter(4.0), "atempo=2.0,atempo=2.000000");
    }

    #[test]
    fn pan_adds_stereotools_only_when_off_center() {
        let mut clip = RoughClip {
            path: PathBuf::from("voz.wav"),
            has_video: false,
            has_audio: true,
            out_seconds: 2.0,
            ..Default::default()
        };
        let mut command = Command::new("ffmpeg");
        let (indices, titles) = push_render_inputs(
            &mut command,
            &[clip.clone()],
            (1920, 1080),
            false,
            Timebase::default(),
        );
        let filters = build_render_filters(
            &[clip.clone()],
            &indices,
            &titles,
            (1920, 1080),
            false,
            true,
            &[],
            0.0,
            false,
            Timebase::default(),
            None,
        )
        .unwrap();
        assert!(!filters.iter().any(|f| f.contains("stereotools")));

        clip.pan = -0.6;
        let filters = build_render_filters(
            &[clip],
            &indices,
            &titles,
            (1920, 1080),
            false,
            true,
            &[],
            0.0,
            false,
            Timebase::default(),
            None,
        )
        .unwrap();
        assert!(filters
            .iter()
            .any(|f| f.contains("stereotools=balance_in=-0.6000")));
    }

    #[test]
    fn adjustment_layer_grades_previous_with_time_gate_and_no_media_input() {
        let base = RoughClip {
            path: PathBuf::from("base.mp4"),
            has_video: true,
            has_audio: false,
            out_seconds: 3.0,
            track: 0,
            ..Default::default()
        };
        let adjustment = RoughClip {
            has_video: true,
            has_audio: false,
            is_adjustment: true,
            out_seconds: 2.0,
            timeline_start: 1.0,
            track: 1,
            exposure: 0.5,
            ..Default::default()
        };
        let clips = vec![base, adjustment];
        let mut command = Command::new("ffmpeg");
        let (indices, titles) = push_render_inputs(
            &mut command,
            &clips,
            (1920, 1080),
            false,
            Timebase::default(),
        );
        let filters = build_render_filters(
            &clips,
            &indices,
            &titles,
            (1920, 1080),
            true,
            false,
            &[],
            0.0,
            false,
            Timebase::default(),
            None,
        )
        .unwrap();
        let joined = filters.join(";");
        // La capa de ajuste nunca referencia su propio input de medio.
        assert!(!joined.contains("[1:v:0]"));
        // Gradúa una copia de lo compuesto debajo con una puerta temporal.
        assert!(joined.contains("split=2"));
        assert!(joined.contains("geq=r="));
        assert!(joined.contains("between(T\\,1.000000\\,3.000000)"));
        assert!(joined.contains("eq=brightness="));
    }

    #[test]
    fn snap_time_attracts_to_edges_markers_and_playhead() {
        let clips = vec![
            RoughClip {
                timeline_start: 0.0,
                out_seconds: 5.0,
                ..Default::default()
            },
            RoughClip {
                timeline_start: 8.0,
                out_seconds: 2.0,
                ..Default::default()
            },
        ];
        let markers = vec![Marker {
            time: 3.0,
            name: "M".to_owned(),
        }];
        // Atracción a un borde cercano.
        assert!((snap_time(7.93, &clips, None, &markers, 0.0, 0.25) - 8.0).abs() < 1e-9);
        // El clip en movimiento no se ancla a sí mismo.
        assert!((snap_time(8.02, &clips, Some(1), &markers, 0.0, 0.25) - 8.02).abs() < 1e-9);
        // Atracción al cabezal.
        assert!((snap_time(5.12, &clips, None, &markers, 5.0, 0.25) - 5.0).abs() < 1e-9);
        // Atracción a un marcador.
        assert!((snap_time(3.06, &clips, None, &markers, 0.0, 0.25) - 3.0).abs() < 1e-9);
        // Fuera del umbral: sin cambio.
        assert!((snap_time(6.0, &clips, None, &markers, 0.0, 0.25) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn old_windows_projects_default_to_normal_speed() {
        let json = r#"{
            "version":1,
            "name":"Anterior",
            "clips":[
                {"path":"a.mp4","in_seconds":0.0,"out_seconds":2.0,"has_audio":true},
                {"path":"b.mp4","in_seconds":1.0,"out_seconds":4.0,"has_audio":true}
            ]
        }"#;
        let mut project: RoughProject = serde_json::from_str(json).unwrap();
        project.normalize();
        assert_eq!(project.clips[0].speed, 1.0);
        assert_eq!(project.version, 2);
        assert_eq!(project.clips[0].timeline_start, 0.0);
        assert_eq!(project.clips[1].timeline_start, 2.0);
        assert_eq!(project.duration(), 5.0);
    }

    #[test]
    fn project_duration_uses_latest_track_end_not_sum() {
        let clips = vec![
            RoughClip {
                path: PathBuf::from("base.mp4"),
                in_seconds: 0.0,
                out_seconds: 10.0,
                source_duration_seconds: Some(10.0),
                source_timebase: None,
                source_vfr: false,
                source_pts: None,
                has_video: true,
                has_audio: true,
                speed: 1.0,
                timeline_start: 0.0,
                track: 0,
                gain_db: 0.0,
                muted: false,
                pan: 0.0,
                position_x: 0.0,
                fade_in_seconds: 0.0,
                fade_out_seconds: 0.0,
                title: None,
                is_adjustment: false,
                exposure: 0.0,
                contrast: 0.0,
                saturation: 0.0,
                vignette: 0.0,
                transition: None,
                transition_duration: 0.5,
                label: 0,
                blur: 0.0,
                wheels: None,
                chroma: None,
                curves: None,
                keyframes: None,
                position_y: 0.0,
                scale_percent: 100.0,
                rotation: 0.0,
                opacity: 100.0,
                fusion: Fusion::Normal,
                mask: None,
                lut: None,
                proxy: None,
                speed_ramp: None,
                fx: efectos::ClipFx::default(),
                runtime: efectos::TransitionRuntime::default(),
                nested: None,
                freeze_at: None,
                enabled: true,
            },
            RoughClip {
                path: PathBuf::from("overlay.mp4"),
                in_seconds: 0.0,
                out_seconds: 2.0,
                source_duration_seconds: Some(2.0),
                source_timebase: None,
                source_vfr: false,
                source_pts: None,
                has_video: true,
                has_audio: false,
                speed: 1.0,
                timeline_start: 3.0,
                track: 1,
                gain_db: -6.0,
                muted: true,
                pan: 0.0,
                position_x: 100.0,
                position_y: 0.0,
                scale_percent: 50.0,
                fade_in_seconds: 1.0,
                fade_out_seconds: 2.0,
                title: None,
                is_adjustment: false,
                exposure: 0.0,
                contrast: 0.0,
                saturation: 0.0,
                vignette: 0.0,
                transition: None,
                transition_duration: 0.5,
                label: 0,
                blur: 0.0,
                wheels: None,
                chroma: None,
                curves: None,
                keyframes: None,
                rotation: 0.0,
                opacity: 50.0,
                fusion: Fusion::Normal,
                mask: None,
                lut: None,
                proxy: None,
                speed_ramp: None,
                fx: efectos::ClipFx::default(),
                runtime: efectos::TransitionRuntime::default(),
                nested: None,
                freeze_at: None,
                enabled: true,
            },
        ];
        let project = RoughProject {
            clips,
            ..RoughProject::default()
        };
        assert_eq!(project.duration(), 10.0);
        assert_eq!(project.video_track_count(), 2);
    }

    #[test]
    fn audio_only_projects_use_audio_tracks() {
        let json = r#"{
            "version":2,
            "name":"Podcast",
            "clips":[
                {"path":"voz.wav","in_seconds":0.0,"out_seconds":60.0,"has_video":false,"has_audio":true,"timeline_start":0.0,"track":1},
                {"path":"musica.mp3","in_seconds":0.0,"out_seconds":30.0,"has_video":false,"has_audio":true,"timeline_start":5.0,"track":0}
            ]
        }"#;
        let project: RoughProject = serde_json::from_str(json).unwrap();
        assert_eq!(project.video_track_count(), 1);
        assert_eq!(project.audio_track_count(), 2);
        assert_eq!(project.duration(), 60.0);
    }

    #[test]
    fn escapes_drawtext_specials() {
        assert_eq!(escape_drawtext("Hola mundo"), "Hola mundo");
        assert_eq!(escape_drawtext(r"ruta\C:'"), r"ruta\\C\:\'");
        assert_eq!(escape_drawtext("a'b"), r"a\'b");
    }

    #[test]
    fn title_round_trip_keeps_text_and_color() {
        let project = RoughProject {
            version: 2,
            name: "T".to_owned(),
            clips: vec![RoughClip {
                out_seconds: 3.0,
                has_audio: false,
                title: Some(Titulo {
                    text: "Subtitulo".to_owned(),
                    position_x: 0.25,
                    position_y: 0.9,
                    size: 48.0,
                    red: 1.0,
                    green: 0.5,
                    blue: 0.0,
                    style: efectos::TitleStyle::default(),
                }),
                ..Default::default()
            }],
            markers: vec![],
            subtitles: vec![],
            subtitle_style: None,
            ..RoughProject::default()
        };
        let restored: RoughProject =
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        let title = restored.clips[0].title.as_ref().unwrap();
        assert_eq!(title.text, "Subtitulo");
        assert_eq!(title.position_y, 0.9);
        assert_eq!(hex_color(title.red, title.green, title.blue), "FF8000");
    }

    #[test]
    fn color_eq_filter_is_inert_when_neutral() {
        assert_eq!(color_eq_filter(0.0, 0.0, 0.0), "");
        assert!(color_eq_filter(0.5, 0.0, -0.5).contains("eq="));
    }

    #[test]
    fn vignette_filter_is_inert_when_zero() {
        assert_eq!(vignette_filter(0.0), "");
        assert!(vignette_filter(0.5).starts_with(",vignette=angle="));
    }

    #[test]
    fn transitions_merge_fades_on_adjacent_previous_clip() {
        let mut a = RoughClip {
            out_seconds: 2.0,
            timeline_start: 0.0,
            ..Default::default()
        };
        let mut b = RoughClip {
            out_seconds: 2.0,
            timeline_start: 2.0,
            transition: Some("negro".to_owned()),
            transition_duration: 1.0,
            ..Default::default()
        };
        a.fade_out_seconds = 0.0;
        b.fade_in_seconds = 0.0;
        let resolved = resolve_render_clips(&[a, b]);
        assert!((resolved[0].fade_out_seconds - 1.0).abs() < f64::EPSILON);
        assert!((resolved[1].fade_in_seconds - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn transition_ignores_non_adjacent_clips_and_other_tracks() {
        let far = RoughClip {
            out_seconds: 1.0,
            timeline_start: 50.0,
            ..Default::default()
        };
        let other_track = RoughClip {
            out_seconds: 2.0,
            timeline_start: 0.0,
            track: 3,
            ..Default::default()
        };
        let current = RoughClip {
            out_seconds: 2.0,
            timeline_start: 2.0,
            transition: Some("negro".to_owned()),
            transition_duration: 1.0,
            ..Default::default()
        };
        let resolved = resolve_render_clips(&[far, other_track, current]);
        assert_eq!(resolved[0].fade_out_seconds, 0.0);
        assert_eq!(resolved[1].fade_out_seconds, 0.0);
        assert_eq!(resolved[2].fade_in_seconds, 0.0);
    }

    #[test]
    fn label_round_trip_persists() {
        let project = RoughProject {
            version: 2,
            name: "L".to_owned(),
            clips: vec![RoughClip {
                out_seconds: 2.0,
                label: 4,
                ..Default::default()
            }],
            markers: vec![],
            subtitles: vec![],
            subtitle_style: None,
            ..RoughProject::default()
        };
        let restored: RoughProject =
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        assert_eq!(restored.clips[0].label, 4);
        assert_eq!(label_color(0), None);
        assert!(label_color(4).is_some());
    }

    #[test]
    fn markers_round_trip_persist() {
        let project = RoughProject {
            version: 2,
            name: "M".to_owned(),
            clips: vec![],
            markers: vec![Marker {
                time: 3.25,
                name: "Corte fuerte".to_owned(),
            }],
            subtitles: vec![],
            subtitle_style: None,
            ..RoughProject::default()
        };
        let restored: RoughProject =
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        assert_eq!(restored.markers[0].name, "Corte fuerte");
        assert!((restored.markers[0].time - 3.25).abs() < f64::EPSILON);
    }

    #[test]
    fn wheels_and_blur_filters_are_inert_when_neutral() {
        assert_eq!(wheels_filter(None), "");
        assert_eq!(wheels_filter(Some(&Wheels::default())), "");
        let warmed = Wheels {
            mid_r: 0.3,
            ..Wheels::default()
        };
        let filter = wheels_filter(Some(&warmed));
        assert!(filter.starts_with(",colorbalance="));
        assert!(filter.contains("rm=0.3000"));
        assert!(!filter.contains("rh="));
        assert_eq!(blur_filter(0.0, 1080.0), "");
        let blur = blur_filter(0.5, 1080.0);
        assert!(blur.contains("gblur=sigma=135.00"));
    }

    #[test]
    fn chroma_filter_builds_chromakey_and_despill() {
        assert_eq!(chroma_filter(None), "");
        let green = Chroma {
            red: 0.0,
            green: 1.0,
            blue: 0.0,
            tolerance: 0.4,
            smooth: 0.15,
            spill: 0.5,
        };
        let filter = chroma_filter(Some(&green));
        assert!(filter.contains("chromakey=color=0x00FF00:similarity=0.4000:blend=0.1500"));
        assert!(filter.contains("despill=type=green:mix=0.5000"));
        let blue = Chroma {
            blue: 1.0,
            green: 0.0,
            ..green
        };
        assert!(chroma_filter(Some(&blue)).contains("despill=type=blue"));
    }

    #[test]
    fn multicam_cut_creates_coverage_and_trims_on_switch() {
        let base = RoughClip {
            path: PathBuf::from("cam1.mp4"),
            out_seconds: 10.0,
            timeline_start: 0.0,
            track: 0,
            ..Default::default()
        };
        let angle2 = RoughClip {
            path: PathBuf::from("cam2.mp4"),
            out_seconds: 10.0,
            timeline_start: 0.0,
            track: 1,
            has_audio: false,
            ..Default::default()
        };

        // Corte a cámara 2 en t=4: cobertura [4,10] en V2 desde el medio de cam2.
        let cut = apply_multicam_cut(vec![base.clone(), angle2.clone()], 0, 2, 4.0).unwrap();
        let coverage = cut.iter().find(|clip| clip.track == 1).unwrap();
        assert_eq!(coverage.timeline_start, 4.0);
        assert_eq!(coverage.in_seconds, 4.0);
        assert_eq!(coverage.path, PathBuf::from("cam2.mp4"));

        // Vuelta a cámara 1 en t=6: la cobertura se recorta a [4,6].
        let cut2 = apply_multicam_cut(cut, 0, 1, 6.0).unwrap();
        let coverage2 = cut2.iter().find(|clip| clip.track == 1).unwrap();
        assert_eq!(coverage2.out_seconds, 6.0);
    }

    #[test]
    fn multicam_cut_requires_aligned_angle() {
        let base = RoughClip {
            out_seconds: 10.0,
            track: 0,
            ..Default::default()
        };
        let result = apply_multicam_cut(vec![base], 0, 2, 5.0);
        assert!(result.is_err());
    }

    #[test]
    fn srt_timestamps_and_ordering() {
        assert_eq!(srt_timestamp(0.0), "00:00:00,000");
        assert_eq!(srt_timestamp(5025.6789), "01:23:45,679");
        let subtitles = vec![
            Subtitle {
                start: 5.0,
                end: 7.0,
                text: "segunda".to_owned(),
            },
            Subtitle {
                start: 1.0,
                end: 3.0,
                text: "primera".to_owned(),
            },
        ];
        let srt = build_srt(&subtitles);
        assert!(srt.starts_with("1\n00:00:01,000 --> 00:00:03,000\nprimera"));
        assert!(srt.contains("2\n00:00:05,000 --> 00:00:07,000\nsegunda"));
    }

    #[test]
    fn subtitles_become_top_track_title_clips() {
        let base = RoughClip {
            out_seconds: 10.0,
            track: 2,
            ..Default::default()
        };
        let combined = clips_with_subtitles(
            &[base],
            &[Subtitle {
                start: 2.0,
                end: 4.5,
                text: "Hola".to_owned(),
            }],
            None,
        );
        assert_eq!(combined.len(), 2);
        let sub_clip = &combined[1];
        assert_eq!(sub_clip.track, 3);
        assert_eq!(sub_clip.timeline_start, 2.0);
        assert!((sub_clip.duration() - 2.5).abs() < f64::EPSILON);
        assert_eq!(sub_clip.title.as_ref().unwrap().text, "Hola");
    }

    #[test]
    fn curves_filter_maps_luma_and_channels() {
        assert_eq!(curves_filter(None), "");
        assert_eq!(
            curves_filter(Some(&Curves {
                luma: identity_channel(),
                red: identity_channel(),
                green: identity_channel(),
                blue: identity_channel(),
            })),
            ""
        );
        let lifted = Curves {
            luma: vec![
                CurvePoint { x: 0.0, y: 0.05 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
            ..Default::default()
        };
        let filter = curves_filter(Some(&lifted));
        assert_eq!(filter, ",curves=master='0.0000/0.0500 1.0000/1.0000'");
    }

    #[test]
    fn keyframes_interpolate_linearly_and_expand() {
        let mut clip = RoughClip {
            out_seconds: 10.0,
            position_x: 0.0,
            opacity: 100.0,
            ..Default::default()
        };
        clip.keyframes = Some(vec![
            TransformKeyframe {
                t: 0.0,
                x: 0.0,
                y: 0.0,
                scale: 100.0,
                opacity: 100.0,
            },
            TransformKeyframe {
                t: 8.0,
                x: 400.0,
                y: -100.0,
                scale: 150.0,
                opacity: 20.0,
            },
        ]);
        let (x, _, _, opacity) = clip.evaluate_transform(4.0);
        assert!((x - 200.0).abs() < f64::EPSILON);
        assert!((opacity - 60.0).abs() < f64::EPSILON);
        // Fuera de rango: extremos.
        assert_eq!(clip.evaluate_transform(-1.0).0, 0.0);
        assert_eq!(clip.evaluate_transform(99.0).0, 400.0);

        let expanded = expand_keyframes(vec![clip]);
        // Tramos: [0,8] y [8,10].
        assert_eq!(expanded.len(), 2);
        let first = &expanded[0];
        assert_eq!(first.timeline_start, 0.0);
        assert!((first.duration() - 8.0).abs() < 1e-9);
        assert!((first.position_x - 200.0).abs() < 1e-9);
        assert!(first.keyframes.is_none());
    }

    #[test]
    fn audio_offset_finds_shifted_impulse() {
        let mut base = vec![0.01f32; 3000];
        base[1000] = 1.0;
        let mut other = vec![0.01f32; 3000];
        other[1013] = 1.0; // el mismo evento, 13 buckets después
        assert_eq!(best_offset_buckets(&base, &other, 2000), Some(13));
        // Sin señal común: None.
        let noise_a = vec![0.5f32; 3000];
        let noise_b = vec![0.5f32; 3000];
        assert_eq!(best_offset_buckets(&noise_a, &noise_b, 500), None);
    }

    #[test]
    fn reads_mac_project_field_names() {
        let json = r#"{
            "nombre":"Mac",
            "medios":[{"id":"m1","ruta":"C:/plan.mp4","rutaRelativa":"plan.mp4","nombre":"plan.mp4"}],
            "montaje":{
                "timebase":{"numerador":25,"denominador":1,"dropFrame":false},
                "pistas":[{"tipo":"video","clips":[{
                    "mediaID":"m1","inicio":0,"duracion":50,"entradaEnOrigen":25,
                    "velocidad":2.0,"ganancia":-4.0,"habilitado":true,"esAjuste":false,"esTitulo":true,
                    "titulo":{"texto":"Hola mundo","posicionX":0.8,"posicionY":0.2,"tamano":72,"rojo":1.0,"verde":1.0,"azul":0.0}
                }]}]
            }
        }"#;
        let project: MacProject = serde_json::from_str(json).unwrap();
        let mac_clip = &project.montaje.pistas[0].clips[0];
        assert_eq!(mac_clip.source_in, 25);
        assert_eq!(mac_clip.velocidad, 2.0);
        let titulo = mac_clip.titulo.as_ref().unwrap();
        assert_eq!(titulo.texto, "Hola mundo");
        assert_eq!(titulo.tamano, 72.0);
    }

    #[test]
    fn mac_composition_fields_deserialize() {
        let fusion: Fusion = serde_json::from_str(r#""luzFuerte""#).unwrap();
        assert_eq!(fusion.blend_mode(), Some("hardlight"));
        let mask: Mask = serde_json::from_str(
            r#"{"forma":"elipse","posicionX":0.4,"posicionY":0.6,"tamanoX":0.8,"tamanoY":0.3,"pluma":0.2,"invertida":true}"#,
        )
        .unwrap();
        assert_eq!(mask.shape, MaskShape::Ellipse);
        assert!(mask.inverted);
        assert!(mask.alpha_expression(1920.0, 1080.0).contains("sqrt"));
    }

    #[test]
    fn parses_srt_multiline_and_dot_milliseconds() {
        let parsed = parse_srt(
            "1\r\n00:00:01,250 --> 00:00:03,000\r\nHola\r\nmundo\r\n\r\n2\n00:00:04.000 --> 00:00:05.500\nFin\n",
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "Hola\nmundo");
        assert!((parsed[1].end - 5.5).abs() < 1e-9);
    }

    #[test]
    fn finds_any_ggml_model_preferring_smaller() {
        let dir = std::env::temp_dir().join(format!("novacut-whisper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ggml-medium.en-q5_0.bin"), b"x").unwrap();
        std::fs::write(dir.join("ggml-large-v3.bin"), b"x").unwrap();
        let found = find_whisper_model(&dir).unwrap();
        assert_eq!(found.file_name().unwrap(), "ggml-medium.en-q5_0.bin");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_loudness_and_silence_logs() {
        let report = parse_loudness(
            "[Parsed_loudnorm]\n{\n\"input_i\" : \"-18.20\",\n\"input_tp\" : \"-2.10\",\n\"input_lra\" : \"5.40\",\n\"input_thresh\" : \"-28.20\",\n\"target_offset\" : \"0.15\"\n}\n",
        )
        .unwrap();
        assert!((report.integrated_lufs + 18.2).abs() < 1e-9);
        assert!((report.threshold_db + 28.2).abs() < 1e-9);
        assert!((report.target_offset_db - 0.15).abs() < 1e-9);
        let args = report.two_pass_args();
        assert!(args.contains("measured_I=-18.20"));
        assert!(args.contains("measured_thresh=-28.20"));
        assert!(args.contains("offset=0.15"));
        assert!(args.contains("linear=true"));
        let ranges = parse_silences(
            "[silencedetect] silence_start: 1.2\n[silencedetect] silence_end: 2.7 | silence_duration: 1.5\n",
        );
        assert_eq!(ranges, vec![(1.2, 2.7)]);
    }

    #[test]
    fn silence_cut_packs_audible_segments() {
        let clip = RoughClip {
            in_seconds: 10.0,
            out_seconds: 20.0,
            timeline_start: 4.0,
            ..Default::default()
        };
        let segments = without_silences(&clip, &[(2.0, 4.0), (7.0, 8.0)]);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].in_seconds, 10.0);
        assert_eq!(segments[1].in_seconds, 14.0);
        assert_eq!(segments[1].timeline_start, 6.0);
    }

    #[test]
    fn parses_scene_cut_times() {
        let cuts = parse_scene_cuts(
            "[Parsed_scdet_0] lavfi.scd.score: 15.625, lavfi.scd.time: 2\n\
             [Parsed_scdet_0] lavfi.scd.score: 40.0, lavfi.scd.time: 5.5\n",
        );
        assert_eq!(cuts, vec![2.0, 5.5]);
    }

    #[test]
    fn scene_cuts_split_without_losing_footage() {
        let clip = RoughClip {
            in_seconds: 0.0,
            out_seconds: 10.0,
            timeline_start: 3.0,
            ..Default::default()
        };
        let segments = split_by_scene_cuts(&clip, &[4.0, 7.0]);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].in_seconds, 0.0);
        assert_eq!(segments[0].out_seconds, 4.0);
        assert_eq!(segments[1].in_seconds, 4.0);
        assert_eq!(segments[1].out_seconds, 7.0);
        assert_eq!(segments[1].timeline_start, 7.0);
        assert_eq!(segments[2].out_seconds, 10.0);
        let total: f64 = segments.iter().map(RoughClip::duration).sum();
        assert!((total - clip.duration()).abs() < 1e-9);
    }

    #[test]
    fn speed_ramp_expands_and_preserves_duration() {
        let clip = RoughClip {
            out_seconds: 4.0,
            speed_ramp: Some(vec![
                SpeedPoint {
                    source_t: 0.0,
                    speed: 1.0,
                },
                SpeedPoint {
                    source_t: 4.0,
                    speed: 2.0,
                },
            ]),
            ..Default::default()
        };
        let duration = clip.duration();
        let expanded = expand_speed_ramps(vec![clip]);
        let expanded_duration: f64 = expanded.iter().map(RoughClip::duration).sum();
        assert!(duration > 2.0 && duration < 4.0);
        assert!((duration - expanded_duration).abs() < 1e-9);
        assert!(expanded.iter().all(|segment| segment.speed_ramp.is_none()));
    }

    #[test]
    fn nested_sequence_flattens_at_container_time() {
        let nested = RoughClip {
            timeline_start: 5.0,
            track: 3,
            nested: Some(vec![RoughClip {
                timeline_start: 2.0,
                out_seconds: 3.0,
                ..Default::default()
            }]),
            ..Default::default()
        };
        let flattened = flatten_nested(&[nested]);
        assert_eq!(flattened.len(), 1);
        assert_eq!(flattened[0].timeline_start, 7.0);
        assert_eq!(flattened[0].track, 3);
    }

    #[test]
    fn timecode_counts_frames_and_rolls_over() {
        assert_eq!(timecode(0.0, 25.0), "00:00:00:00");
        assert_eq!(timecode(1.0, 25.0), "00:00:01:00");
        assert_eq!(timecode(1.04, 25.0), "00:00:01:01");
        assert_eq!(timecode(3661.0, 30.0), "01:01:01:00");
        assert_eq!(
            timecode(Timebase::NTSC30.seconds(1_800), 29.97),
            "00:01:00;02"
        );
        // Un fps inválido cae al valor por defecto en vez de dividir por cero.
        assert_eq!(timecode(2.0, 0.0), "00:00:02:00");
        assert_eq!(timecode(-5.0, 30.0), "00:00:00:00");
    }

    #[test]
    fn legacy_project_fps_migrates_to_rational_timebase() {
        let mut project = RoughProject {
            fps: 23.976,
            timebase: None,
            ..RoughProject::default()
        };
        project.normalize();
        assert_eq!(project.timebase, Some(Timebase::P23_976));
        assert_eq!(project.fps, Timebase::P23_976.fps());
    }

    #[test]
    fn media_probe_rate_and_vfr_helpers_keep_real_metadata() {
        assert_eq!(
            parse_frame_rate(Some("30000/1001")),
            Some(Timebase::new(30_000, 1_001, false).unwrap())
        );
        let regular: Vec<f64> = (0..=20).map(|index| index as f64 / 30.0).collect();
        assert!(!is_variable_frame_rate(&regular));
        let vfr: Vec<f64> = (0..=20)
            .map(|index| index as f64 / 30.0 + if index >= 10 { 0.1 } else { 0.0 })
            .collect();
        assert!(is_variable_frame_rate(&vfr));
        let jitter: Vec<f64> = (0..=40)
            .scan(0.0, |time, index| {
                let delta = [1.0 / 30.0, 1.0 / 28.0, 1.0 / 32.0][index % 3];
                let current = *time;
                *time += delta;
                Some(current)
            })
            .collect();
        let summary = summarize_video_pts(&jitter).expect("jitter summary");
        assert!(summary.is_variable());
        assert_eq!(summary.frame_count, 41);
    }

    #[test]
    fn project_paths_round_trip_relative_to_project_folder() {
        let directory = std::env::temp_dir().join("novacut-portable-project");
        let project_path = directory.join("montaje.ncrough");
        let source = directory.join("media").join("plano.mp4");
        let proxy = directory.join("NovaCut Proxies").join("plano-proxy.mp4");
        let lut = directory.join("looks").join("look.cube");
        let nested_source = directory.join("media").join("detalle.mp4");
        let external = std::env::temp_dir().join("fuera-del-proyecto.mp4");
        let project = RoughProject {
            clips: vec![
                RoughClip {
                    path: source.clone(),
                    proxy: Some(proxy.clone()),
                    lut: Some(lut.clone()),
                    nested: Some(vec![RoughClip {
                        path: nested_source.clone(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                RoughClip {
                    path: external.clone(),
                    ..Default::default()
                },
            ],
            ..RoughProject::default()
        };
        let mut stored = project_for_storage(&project, &project_path);
        assert_eq!(stored.clips[0].path, PathBuf::from("media/plano.mp4"));
        assert_eq!(
            stored.clips[0].proxy,
            Some(PathBuf::from("NovaCut Proxies/plano-proxy.mp4"))
        );
        assert_eq!(stored.clips[0].lut, Some(PathBuf::from("looks/look.cube")));
        assert_eq!(
            stored.clips[0].nested.as_ref().unwrap()[0].path,
            PathBuf::from("media/detalle.mp4")
        );
        assert_eq!(stored.clips[1].path, external);

        resolve_project_paths(&mut stored, project_path.parent());
        assert_eq!(stored.clips[0].path, source);
        assert_eq!(stored.clips[0].proxy, Some(proxy));
        assert_eq!(stored.clips[0].lut, Some(lut));
        assert_eq!(
            stored.clips[0].nested.as_ref().unwrap()[0].path,
            nested_source
        );
    }

    #[test]
    fn render_filters_use_the_requested_rational_rate() {
        let clips = vec![RoughClip {
            path: PathBuf::from("plan.mp4"),
            out_seconds: 2.0,
            ..Default::default()
        }];
        let rate = Timebase::P23_976;
        let mut command = Command::new("ffmpeg");
        let (indices, titles) = push_render_inputs(&mut command, &clips, (1920, 1080), false, rate);
        let filters = build_render_filters(
            &clips,
            &indices,
            &titles,
            (1920, 1080),
            true,
            false,
            &[],
            0.0,
            false,
            rate,
            None,
        )
        .unwrap();
        let joined = filters.join(";");
        assert!(joined.contains("r=24000/1001"));
        assert!(joined.contains("fps=24000/1001"));

        let mut vfr = RoughClip::default();
        vfr.source_vfr = true;
        assert_eq!(
            conform_video_filter(&vfr, &rate.ffmpeg_rate()),
            ",fps=fps=24000/1001:start_time=0:round=near"
        );
        vfr.source_vfr = false;
        assert_eq!(
            conform_video_filter(&vfr, &rate.ffmpeg_rate()),
            ",fps=fps=24000/1001:round=near"
        );
    }

    #[test]
    fn clock_labels_stay_short_for_the_ruler() {
        assert_eq!(format_clock(2.5), "2.5s");
        assert_eq!(format_clock(45.0), "45s");
        assert_eq!(format_clock(65.0), "1:05");
        assert_eq!(format_clock(3725.0), "1:02:05");
    }

    #[test]
    fn work_range_trim_cuts_edges_and_moves_to_zero() {
        let clips = vec![
            RoughClip {
                path: PathBuf::from("a.mp4"),
                in_seconds: 0.0,
                out_seconds: 10.0,
                timeline_start: 0.0,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("b.mp4"),
                in_seconds: 0.0,
                out_seconds: 10.0,
                timeline_start: 20.0,
                ..Default::default()
            },
        ];
        let trimmed = trim_clips_to_range(&clips, 4.0, 8.0);
        assert_eq!(trimmed.len(), 1, "el clip fuera de rango se descarta");
        assert!((trimmed[0].timeline_start - 0.0).abs() < 1e-9);
        assert!((trimmed[0].in_seconds - 4.0).abs() < 1e-9);
        assert!((trimmed[0].out_seconds - 8.0).abs() < 1e-9);
        assert!((trimmed[0].duration() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn work_range_trim_respects_speed_and_keeps_ramps_whole() {
        let clips = vec![
            RoughClip {
                in_seconds: 0.0,
                out_seconds: 8.0,
                speed: 2.0,
                timeline_start: 0.0,
                ..Default::default()
            },
            RoughClip {
                in_seconds: 0.0,
                out_seconds: 4.0,
                timeline_start: 1.0,
                track: 1,
                speed_ramp: Some(vec![
                    SpeedPoint {
                        source_t: 0.0,
                        speed: 1.0,
                    },
                    SpeedPoint {
                        source_t: 4.0,
                        speed: 2.0,
                    },
                ]),
                ..Default::default()
            },
        ];
        let trimmed = trim_clips_to_range(&clips, 1.0, 3.0);
        // A doble velocidad, un segundo de montaje consume dos de origen.
        assert!((trimmed[0].in_seconds - 2.0).abs() < 1e-9);
        assert!((trimmed[0].out_seconds - 6.0).abs() < 1e-9);
        // La rampa no se recorta por dentro: solo se desplaza.
        assert!(trimmed[1].speed_ramp.is_some());
        assert!((trimmed[1].timeline_start - 0.0).abs() < 1e-9);
    }

    #[test]
    fn overwrite_splits_the_clip_underneath_in_two() {
        let mut clips = vec![RoughClip {
            path: PathBuf::from("fondo.mp4"),
            in_seconds: 0.0,
            out_seconds: 10.0,
            timeline_start: 0.0,
            ..Default::default()
        }];
        let touched = clear_track_span(&mut clips, 0, true, 4.0, 6.0, &[]);
        assert_eq!(touched, 1);
        assert_eq!(clips.len(), 2, "queda una cabeza y una cola");
        assert!((clips[0].timeline_start - 0.0).abs() < 1e-9);
        assert!((clips[0].out_seconds - 4.0).abs() < 1e-9);
        assert!((clips[1].timeline_start - 6.0).abs() < 1e-9);
        assert!((clips[1].in_seconds - 6.0).abs() < 1e-9);
        assert!((clips[1].out_seconds - 10.0).abs() < 1e-9);
    }

    #[test]
    fn overwrite_removes_fully_covered_clips_and_respects_other_lanes() {
        let mut clips = vec![
            RoughClip {
                path: PathBuf::from("tapado.mp4"),
                out_seconds: 2.0,
                timeline_start: 4.0,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("otra-pista.mp4"),
                out_seconds: 8.0,
                timeline_start: 0.0,
                track: 1,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("audio.wav"),
                out_seconds: 8.0,
                timeline_start: 0.0,
                has_video: false,
                ..Default::default()
            },
        ];
        let touched = clear_track_span(&mut clips, 0, true, 3.0, 7.0, &[]);
        assert_eq!(touched, 1);
        assert_eq!(clips.len(), 2, "solo desaparece el clip tapado de V1");
        assert!(clips
            .iter()
            .all(|clip| clip.path.as_path() != Path::new("tapado.mp4")));
    }

    #[test]
    fn overwrite_never_touches_the_protected_clip_or_ramps() {
        let mut clips = vec![
            RoughClip {
                path: PathBuf::from("nuevo.mp4"),
                out_seconds: 4.0,
                timeline_start: 0.0,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("rampa.mp4"),
                out_seconds: 4.0,
                timeline_start: 1.0,
                speed_ramp: Some(vec![
                    SpeedPoint {
                        source_t: 0.0,
                        speed: 1.0,
                    },
                    SpeedPoint {
                        source_t: 4.0,
                        speed: 2.0,
                    },
                ]),
                ..Default::default()
            },
        ];
        let touched = clear_track_span(&mut clips, 0, true, 0.0, 4.0, &[0]);
        assert_eq!(touched, 0, "ni el propio clip ni las rampas se parten");
        assert_eq!(clips.len(), 2);
    }

    #[test]
    fn overwrite_detects_complex_collisions_before_mutating() {
        let clips = vec![RoughClip {
            out_seconds: 5.0,
            nested: Some(vec![RoughClip {
                out_seconds: 5.0,
                ..Default::default()
            }]),
            ..Default::default()
        }];
        assert!(span_hits_complex_clip(&clips, 0, true, 1.0, 3.0, &[]));
        assert!(!span_hits_complex_clip(&clips, 0, true, 1.0, 3.0, &[0]));
        assert!(!span_hits_complex_clip(&clips, 1, true, 1.0, 3.0, &[]));
    }

    #[test]
    fn ripple_insert_splits_crossing_clip_and_shifts_tail_once() {
        let mut clips = vec![
            RoughClip {
                path: PathBuf::from("cruzado.mp4"),
                in_seconds: 2.0,
                out_seconds: 12.0,
                timeline_start: 0.0,
                ..Default::default()
            },
            RoughClip {
                path: PathBuf::from("siguiente.mp4"),
                out_seconds: 2.0,
                timeline_start: 10.0,
                ..Default::default()
            },
        ];
        assert!(ripple_track_at(&mut clips, 0, true, 4.0, 3.0));
        assert_eq!(clips.len(), 3);
        assert!((clips[0].out_seconds - 6.0).abs() < 1e-9);
        assert!((clips[1].timeline_start - 7.0).abs() < 1e-9);
        assert!((clips[1].in_seconds - 6.0).abs() < 1e-9);
        assert!((clips[2].timeline_start - 13.0).abs() < 1e-9);
    }

    #[test]
    fn ripple_insert_is_atomic_when_a_complex_clip_crosses() {
        let original = vec![RoughClip {
            out_seconds: 6.0,
            speed_ramp: Some(vec![SpeedPoint {
                source_t: 0.0,
                speed: 1.0,
            }]),
            ..Default::default()
        }];
        let mut clips = original.clone();
        assert!(!ripple_track_at(&mut clips, 0, true, 2.0, 1.0));
        assert_eq!(clips[0].timeline_start, original[0].timeline_start);
        assert_eq!(clips[0].in_seconds, original[0].in_seconds);
        assert_eq!(clips[0].out_seconds, original[0].out_seconds);
    }

    #[test]
    fn empty_tracks_can_be_reserved_without_clips() {
        let mut project = RoughProject {
            clips: vec![RoughClip {
                out_seconds: 1.0,
                track: 0,
                ..Default::default()
            }],
            ..RoughProject::default()
        };
        assert_eq!(project.video_track_count(), 1);
        project.min_video_tracks = 3;
        assert_eq!(project.video_track_count(), 3);
        // Al llenar una pista alta, el mínimo deja de mandar y no aparecen
        // pistas vacías de más.
        project.clips.push(RoughClip {
            out_seconds: 1.0,
            track: 4,
            ..Default::default()
        });
        assert_eq!(project.video_track_count(), 5);
    }

    #[test]
    fn disabled_clips_keep_their_place_in_the_project() {
        let clip = RoughClip {
            out_seconds: 2.0,
            ..Default::default()
        };
        assert!(clip.enabled, "los clips nacen activos");
        let restored: RoughClip = serde_json::from_str(
            r#"{"path":"a.mp4","in_seconds":0.0,"out_seconds":2.0,"has_audio":true}"#,
        )
        .expect("json valido");
        assert!(
            restored.enabled,
            "los proyectos antiguos siguen teniendo los clips activos"
        );
    }

    #[test]
    fn timecode_parsing_accepts_common_formats() {
        assert_eq!(parse_timecode("12.5", Timebase::P25), Some(12.5));
        assert_eq!(parse_timecode("01:30", Timebase::P25), Some(90.0));
        assert_eq!(parse_timecode("00:01:23", Timebase::P25), Some(83.0));
        let with_frames = parse_timecode("00:01:23:12", Timebase::P25).unwrap();
        assert!((with_frames - 83.48).abs() < 1e-9);
        assert_eq!(parse_timecode("no es tiempo", Timebase::P25), None);
        assert_eq!(parse_timecode("-5", Timebase::P25), Some(0.0));
        assert_eq!(parse_timecode("00:01:00;00", Timebase::NTSC30), None);
        assert_eq!(
            parse_timecode("00:01:00;02", Timebase::NTSC30),
            Some(Timebase::NTSC30.seconds(1_800))
        );
    }

    #[test]
    fn dissolve_extends_previous_clip_only_with_source_room() {
        let base = |out_seconds: f64, source: Option<f64>| RoughClip {
            out_seconds,
            source_duration_seconds: source,
            ..Default::default()
        };
        let incoming = |timeline_start: f64| RoughClip {
            timeline_start,
            out_seconds: 4.0,
            transition: Some("dissolve".to_owned()),
            transition_duration: 1.0,
            ..Default::default()
        };
        let resolved = resolve_render_clips(&[base(4.0, Some(6.0)), incoming(4.0)]);
        assert!((resolved[0].out_seconds - 5.0).abs() < 1e-9);
        assert_eq!(resolved[0].fade_out_seconds, 0.0);
        assert_eq!(resolved[1].fade_in_seconds, 1.0);
        // Sin reserva el medio no puede alargarse: cae a paso por negro.
        let resolved = resolve_render_clips(&[base(4.0, Some(4.0)), incoming(4.0)]);
        assert_eq!(resolved[0].out_seconds, 4.0);
        assert_eq!(resolved[0].fade_out_seconds, 1.0);
        assert_eq!(resolved[1].fade_in_seconds, 1.0);
    }

    #[test]
    fn cut_points_are_sorted_without_duplicates() {
        let project = RoughProject {
            clips: vec![
                RoughClip {
                    out_seconds: 2.0,
                    timeline_start: 0.0,
                    ..Default::default()
                },
                RoughClip {
                    out_seconds: 3.0,
                    timeline_start: 2.0,
                    ..Default::default()
                },
            ],
            ..RoughProject::default()
        };
        assert_eq!(project.cut_points(), vec![0.0, 2.0, 5.0]);
    }

    #[test]
    fn solo_track_mutes_the_others() {
        let mut project = RoughProject {
            clips: vec![
                RoughClip {
                    has_video: false,
                    has_audio: true,
                    out_seconds: 2.0,
                    track: 0,
                    ..Default::default()
                },
                RoughClip {
                    has_video: false,
                    has_audio: true,
                    out_seconds: 2.0,
                    track: 1,
                    ..Default::default()
                },
            ],
            ..RoughProject::default()
        };
        project.sync_track_state();
        assert!(project.track_audible(0) && project.track_audible(1));
        project.track_mutes[0] = true;
        assert!(!project.track_audible(0));
        project.track_solos[1] = true;
        // Con un solo activo, todo lo demás calla aunque no esté enmudecido.
        assert!(!project.track_audible(0));
        assert!(project.track_audible(1));
    }

    #[test]
    fn locked_tracks_are_reported_per_media_kind() {
        let mut project = RoughProject {
            clips: vec![RoughClip {
                out_seconds: 1.0,
                track: 0,
                ..Default::default()
            }],
            ..RoughProject::default()
        };
        project.sync_track_state();
        let clip = project.clips[0].clone();
        assert!(!project.clip_locked(&clip));
        project.video_locked[0] = true;
        assert!(project.clip_locked(&clip));
    }
}

/// Renders reales con FFmpeg: prueban que los grafos que construye el host
/// son aceptados por FFmpeg y producen la duración esperada. Si FFmpeg no
/// está instalado, cada prueba se salta en silencio.
#[cfg(test)]
mod render_real_tests {
    use super::*;

    /// Salta la prueba si falta una herramienta, salvo con
    /// NOVACUT_REQUIRE_REAL=1: entonces falla, para demostrar que se ejecutó.
    fn skip(reason: &str) -> bool {
        if std::env::var_os("NOVACUT_REQUIRE_REAL").is_some() {
            panic!("prueba real saltada: {reason}");
        }
        true
    }

    fn ffmpeg_available() -> bool {
        Command::new(tool_path("ffmpeg.exe"))
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Algunos FFmpeg (el de Homebrew, por ejemplo) se compilan sin
    /// freetype y no traen `drawtext`; entonces se omiten los títulos.
    fn has_drawtext() -> bool {
        Command::new(tool_path("ffmpeg.exe"))
            .args(["-hide_banner", "-h", "filter=drawtext"])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("fontfile"))
    }

    fn work_dir(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("novacut-render-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn generate(directory: &Path) -> (PathBuf, PathBuf) {
        let video = directory.join("camara.mp4");
        let status = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=s=640x360:r=25:d=4")
            .args(["-f", "lavfi", "-i", "sine=f=300:d=4:sample_rate=48000"])
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let music = directory.join("musica.wav");
        let status = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("sine=f=660:d=8:sample_rate=48000")
            .arg(&music)
            .status()
            .unwrap();
        assert!(status.success());
        (video, music)
    }

    fn probe_duration(path: &Path) -> f64 {
        let output = Command::new(tool_path("ffprobe.exe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0.0)
    }

    /// Un montaje que usa todo lo nuevo a la vez.
    fn showcase(video: &Path, music: &Path) -> Vec<RoughClip> {
        let camera = |start: f64| RoughClip {
            path: video.to_path_buf(),
            in_seconds: 0.5,
            out_seconds: 3.0,
            source_duration_seconds: Some(4.0),
            timeline_start: start,
            ..Default::default()
        };
        let mut first = camera(0.0);
        first.fx = efectos::ClipFx {
            flip_h: true,
            crop_left: 0.1,
            crop_top: 0.05,
            temperature: 0.6,
            tint: -0.3,
            vibrance: 0.4,
            shadows: 0.5,
            highlights: -0.4,
            sharpen: 0.5,
            denoise: 0.4,
            grain: 0.3,
            stabilize: true,
            role: efectos::AudioRole::Dialogo,
            voice_cleanup: 0.6,
            compressor: true,
            eq_low_db: -3.0,
            eq_mid_db: 2.0,
            eq_high_db: 4.0,
            volume_keys: vec![
                efectos::VolumeKey { t: 0.0, db: -30.0 },
                efectos::VolumeKey { t: 1.0, db: 0.0 },
            ],
            ..Default::default()
        };
        let mut second = camera(2.5);
        second.fx.reverse = true;
        second.transition = Some("empujar_izq".to_owned());
        second.transition_duration = 0.5;
        let mut third = camera(5.0);
        third.transition = Some("blanco".to_owned());
        third.transition_duration = 0.4;
        let mut fourth = camera(7.5);
        fourth.transition = Some("deslizar_arriba".to_owned());
        fourth.transition_duration = 0.5;
        let lower_third = RoughClip {
            out_seconds: 3.0,
            timeline_start: 1.0,
            track: 1,
            has_audio: false,
            title: Some(Titulo {
                text: "Ana García · Productora".to_owned(),
                position_x: 0.3,
                position_y: 0.84,
                size: 54.0,
                style: efectos::TitleStyle {
                    box_opacity: 0.6,
                    outline: 2.0,
                    shadow: true,
                    ..Default::default()
                },
                ..Titulo::default()
            }),
            ..Default::default()
        };
        let adjustment = RoughClip {
            out_seconds: 2.0,
            timeline_start: 5.5,
            track: 2,
            has_audio: false,
            is_adjustment: true,
            fx: efectos::ClipFx {
                temperature: -0.5,
                ..Default::default()
            },
            ..Default::default()
        };
        let score = RoughClip {
            path: music.to_path_buf(),
            out_seconds: 8.0,
            source_duration_seconds: Some(8.0),
            has_video: false,
            fx: efectos::ClipFx {
                role: efectos::AudioRole::Musica,
                duck_db: -14.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut clips = vec![first, second, third, fourth, adjustment, score];
        if has_drawtext() {
            clips.push(lower_third);
        }
        clips
    }

    fn export(clips: &[RoughClip], output: &Path, format: ExportFormat) -> Result<(), String> {
        export_with(clips, output, format, None)
    }

    fn export_with(
        clips: &[RoughClip],
        output: &Path,
        format: ExportFormat,
        hw: Option<aceleracion::HwBackend>,
    ) -> Result<(), String> {
        let progress = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        run_export(
            clips,
            output,
            &AtomicBool::new(false),
            false,
            (640, 360),
            format.is_audio_only(),
            format,
            &[],
            0.0,
            false,
            Timebase::from_fps(25.0),
            None,
            hw,
            &progress,
        )
    }

    #[test]
    fn every_format_renders_the_effects_showcase() {
        if !ffmpeg_available() && skip("sin FFmpeg") {
            return;
        }
        let directory = work_dir("formatos");
        let (video, music) = generate(&directory);
        let clips = showcase(&video, &music);
        let expected = clips
            .iter()
            .map(|clip| clip.timeline_start + clip.duration())
            .fold(0.0, f64::max);
        for format in ExportFormat::ALL {
            let output = directory.join(format!("salida-{}.{}", format.code(), format.extension()));
            export(&clips, &output, format)
                .unwrap_or_else(|error| panic!("{} falló: {error}", format.label()));
            let duration = probe_duration(&output);
            assert!(
                (duration - expected).abs() < 0.35,
                "{}: duración {duration} en vez de {expected}",
                format.label()
            );
        }
        // NOVACUT_KEEP_RENDERS=1 conserva las salidas para revisarlas a ojo.
        if std::env::var_os("NOVACUT_KEEP_RENDERS").is_none() {
            let _ = std::fs::remove_dir_all(&directory);
        }
    }

    #[test]
    fn monitor_frame_renders_mid_transition_with_effects() {
        if !ffmpeg_available() && skip("sin FFmpeg") {
            return;
        }
        let directory = work_dir("monitor");
        let (video, music) = generate(&directory);
        let clips = prepare_render_clips(&showcase(&video, &music));
        // En mitad del empuje el entrante está desplazado medio lienzo.
        let incoming = clips
            .iter()
            .find(|clip| clip.runtime.slide_in.is_some())
            .expect("la transición de empuje debe resolverse");
        assert_eq!(incoming.runtime.offset_at(2.75), (0.5, 0.0));
        let pushed = clips
            .iter()
            .find(|clip| clip.runtime.slide_out.is_some())
            .expect("el saliente debe empujarse");
        assert!(
            (pushed.out_seconds - 3.5).abs() < 1e-9,
            "el saliente usa su reserva"
        );
        let sources: Vec<(RoughClip, f64)> = clips
            .iter()
            .filter(|clip| {
                clip.has_video
                    && clip.timeline_start <= 1.5
                    && 1.5 < clip.timeline_start + clip.duration()
            })
            .map(|clip| (clip.clone(), clip.in_seconds + 1.5 - clip.timeline_start))
            .collect();
        let expected_layers = if has_drawtext() { 2 } else { 1 };
        assert_eq!(
            sources.len(),
            expected_layers,
            "cámara con efectos y rótulo inferior"
        );
        let frame = render_preview_frame(&sources, Timebase::from_fps(25.0)).unwrap();
        assert_eq!(frame.pixels.len(), frame.width * frame.height * 4);
        // El recorte izquierdo del 10 % deja ver el fondo negro.
        let left_pixel = &frame.pixels[(180 * frame.width + 20) * 4..][..3];
        assert_eq!(left_pixel, &[0, 0, 0]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    fn probe_stream(path: &Path, entry: &str) -> String {
        let output = Command::new(tool_path("ffprobe.exe"))
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                entry,
            ])
            .args(["-of", "default=nw=1:nk=1"])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn gpu_export_encodes_on_hardware_and_falls_back_to_cpu() {
        if !ffmpeg_available() && skip("sin FFmpeg") {
            return;
        }
        let directory = work_dir("gpu");
        let (video, _) = generate(&directory);
        let clips = vec![RoughClip {
            path: video,
            out_seconds: 3.0,
            source_duration_seconds: Some(4.0),
            ..Default::default()
        }];
        let detected = aceleracion::detect(&tool_path("ffmpeg.exe"));
        if detected.is_empty() {
            skip("sin GPU que codifique");
        }
        for backend in &detected {
            for (format, codec) in [
                (ExportFormat::Mp4Video, "h264"),
                (ExportFormat::Mp4Hevc, "hevc"),
            ] {
                let output = directory.join(format!("{backend:?}-{codec}.mp4"));
                export_with(&clips, &output, format, Some(*backend)).unwrap();
                assert_eq!(probe_stream(&output, "stream=codec_name"), codec);
                // El propio archivo dice qué codificador lo escribió.
                assert!(
                    probe_stream(&output, "stream_tags=encoder")
                        .contains(backend.encoder(codec == "hevc")),
                    "{backend:?} no codificó {codec}"
                );
                assert!((probe_duration(&output) - 3.0).abs() < 0.2);
            }
        }
        // Una GPU que este equipo no tiene: la exportación no falla, se
        // repite con CPU y deja el aviso.
        let absent = aceleracion::HwBackend::ALL
            .into_iter()
            .find(|backend| !detected.contains(backend))
            .expect("ningún equipo tiene las cuatro");
        let output = directory.join("fallback.mp4");
        let progress = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        run_export(
            &clips,
            &output,
            &AtomicBool::new(false),
            false,
            (640, 360),
            false,
            ExportFormat::Mp4Video,
            &[],
            0.0,
            false,
            Timebase::from_fps(25.0),
            None,
            Some(absent),
            &progress,
        )
        .unwrap();
        assert!(probe_stream(&output, "stream_tags=encoder").contains("libx264"));
        let note = progress.lock().unwrap().note.clone().unwrap_or_default();
        assert!(note.contains("se exportó con CPU"), "aviso: {note}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Whisper instalado (carpeta en NOVACUT_WHISPER_DIR) y `say` de macOS
    /// para fabricar voz; si falta algo, la prueba se salta.
    fn speech_video(directory: &Path, text: &str) -> Option<PathBuf> {
        let voice = directory.join("voz.aiff");
        let spoke = Command::new("say")
            .args(["-v", "Monica", "-o"])
            .arg(&voice)
            .arg(text)
            .status()
            .is_ok_and(|status| status.success());
        if !spoke {
            return None;
        }
        let video = directory.join("entrevista.mp4");
        let status = Command::new(tool_path("ffmpeg.exe"))
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=640x360:r=25",
            ])
            .arg("-i")
            .arg(&voice)
            .args([
                "-shortest",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&video)
            .status()
            .ok()?;
        status.success().then_some(video)
    }

    fn joined(words: &[transcripcion::Word]) -> String {
        words
            .iter()
            .map(|word| transcripcion::normalized(&word.text))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn text_based_editing_removes_fillers_from_the_real_audio() {
        if !ffmpeg_available() && skip("sin FFmpeg") {
            return;
        }
        let Some((whisper, model)) = whisper_files() else {
            skip("sin Whisper");
            return;
        };
        let directory = work_dir("texto");
        let Some(video) = speech_video(
            &directory,
            "Hola, bienvenidos al podcast. Hoy vamos a hablar de edición de vídeo. Este, o sea, es muy fácil.",
        ) else {
            skip("sin `say` para fabricar voz");
            return;
        };
        let duration = probe_duration(&video);
        let clips = vec![RoughClip {
            path: video,
            out_seconds: duration,
            source_duration_seconds: Some(duration),
            ..Default::default()
        }];
        let timebase = Timebase::from_fps(25.0);
        let transcribe = |clips: &[RoughClip]| {
            transcribe_mix(
                &prepare_render_clips(clips),
                &[],
                0.0,
                timebase,
                &whisper,
                &model,
            )
            .unwrap()
        };
        let words = transcribe(&clips);
        let before = joined(&words);
        assert!(before.contains("o sea"), "transcripción: {before}");
        let fillers = transcripcion::filler_indices(&words);
        let ranges = transcripcion::ranges_for(&words, &fillers);
        let edited = transcripcion::extract_ranges(&clips, &ranges).unwrap();
        let removed: f64 = ranges.iter().map(|(start, end)| end - start).sum();
        let total = |clips: &[RoughClip]| {
            clips
                .iter()
                .map(|clip| clip.timeline_start + clip.duration())
                .fold(0.0, f64::max)
        };
        assert!((total(&edited) - (duration - removed)).abs() < 1e-6);
        // Lo que se oye tras el corte ya no contiene las muletillas y
        // conserva el resto de la frase.
        let after = joined(&transcribe(&edited));
        assert!(!after.contains("sea"), "tras el corte: {after}");
        assert!(after.contains("fácil"), "tras el corte: {after}");
        assert!(after.contains("bienvenidos"), "tras el corte: {after}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn animated_captions_highlight_the_spoken_word() {
        if (!ffmpeg_available() || !has_drawtext()) && skip("FFmpeg sin drawtext") {
            return;
        }
        let directory = work_dir("subtitulos");
        let word = |start: f64, end: f64, text: &str| transcripcion::Word {
            start,
            end,
            text: text.to_owned(),
        };
        let words = vec![
            word(0.0, 1.0, "primera"),
            word(1.0, 2.0, "segunda"),
            word(2.0, 3.0, "tercera"),
        ];
        let caption_clip = |preset| RoughClip {
            out_seconds: 3.0,
            has_audio: false,
            title: Some(Titulo {
                position_y: 0.8,
                size: 90.0,
                style: efectos::TitleStyle {
                    captions: Some(subtitulos_animados::AnimatedCaptions {
                        words: words.clone(),
                        preset,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Titulo::default()
            }),
            ..Default::default()
        };
        // Columnas (en el monitor de 640 px) donde hay color de resaltado.
        let highlighted_columns = |clip: &RoughClip, time: f64| -> Vec<usize> {
            let frame =
                render_preview_frame(&[(clip.clone(), time)], Timebase::from_fps(25.0)).unwrap();
            let mut columns = Vec::new();
            for x in 0..frame.width {
                let hit = (frame.height / 2..frame.height).any(|y| {
                    let pixel = &frame.pixels[(y * frame.width + x) * 4..][..3];
                    pixel[0] > 200 && pixel[1] > 170 && pixel[2] < 90
                });
                if hit {
                    columns.push(x);
                }
            }
            columns
        };
        let clasico = caption_clip(subtitulos_animados::CaptionPreset::Clasico);
        let first = highlighted_columns(&clasico, 0.5);
        let third = highlighted_columns(&clasico, 2.5);
        assert!(
            !first.is_empty() && !third.is_empty(),
            "debe verse la palabra activa"
        );
        // La palabra resaltada se desplaza de izquierda a derecha.
        assert!(
            first.iter().max() < third.iter().min(),
            "{first:?} vs {third:?}"
        );
        // Karaoke: al final las tres quedan coloreadas, más ancho que una.
        let karaoke = caption_clip(subtitulos_animados::CaptionPreset::Karaoke);
        assert!(highlighted_columns(&karaoke, 2.5).len() > third.len() * 2);
        // Y la exportación acepta el grafo con `enable` por palabra.
        for preset in subtitulos_animados::CaptionPreset::ALL {
            let output = directory.join(format!("{preset:?}.mp4"));
            export(&[caption_clip(preset)], &output, ExportFormat::Mp4Video).unwrap();
            assert!((probe_duration(&output) - 3.0).abs() < 0.2);
        }
        if std::env::var_os("NOVACUT_KEEP_RENDERS").is_none() {
            let _ = std::fs::remove_dir_all(&directory);
        }
    }

    #[test]
    fn titles_leave_the_video_underneath_visible() {
        if (!ffmpeg_available() || !has_drawtext()) && skip("FFmpeg sin drawtext") {
            return;
        }
        let directory = work_dir("titulo");
        let (video, _) = generate(&directory);
        let camera = RoughClip {
            path: video,
            out_seconds: 3.0,
            source_duration_seconds: Some(4.0),
            ..Default::default()
        };
        let title = RoughClip {
            out_seconds: 3.0,
            track: 1,
            has_audio: false,
            title: Some(Titulo {
                text: "Hola".to_owned(),
                position_y: 0.9,
                ..Titulo::default()
            }),
            ..Default::default()
        };
        // Brillo medio de la mitad superior, lejos del texto.
        let brightness = |pixels: &[u8], width: usize, height: usize| -> f64 {
            let mut sum = 0.0;
            for y in 0..height / 2 {
                for x in 0..width {
                    let pixel = &pixels[(y * width + x) * 4..][..3];
                    sum += pixel.iter().map(|&value| value as f64).sum::<f64>() / 3.0;
                }
            }
            sum / (width * height / 2) as f64
        };
        // 1.013 s no cae en un fotograma del medio (25 fps) y el proyecto va
        // a 30: el caso en que el monitor se quedaba negro.
        let frame = render_preview_frame(
            &[(camera.clone(), 1.013), (title.clone(), 1.013)],
            Timebase::from_fps(30.0),
        )
        .unwrap();
        assert!(
            brightness(&frame.pixels, frame.width, frame.height) > 40.0,
            "el monitor tapa el vídeo con el título"
        );
        let output = directory.join("titulo.mp4");
        export(&[camera, title], &output, ExportFormat::Mp4Video).unwrap();
        let raw = Command::new(tool_path("ffmpeg.exe"))
            .args(["-v", "error", "-ss", "1", "-i"])
            .arg(&output)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=320:180",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(raw.len(), 320 * 180 * 4);
        assert!(
            brightness(&raw, 320, 180) > 40.0,
            "la exportación tapa el vídeo con el título"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn ducking_lowers_music_while_dialogue_plays() {
        if !ffmpeg_available() && skip("sin FFmpeg") {
            return;
        }
        let directory = work_dir("ducking");
        let (video, music) = generate(&directory);
        let dialogue = RoughClip {
            path: video.clone(),
            out_seconds: 4.0,
            source_duration_seconds: Some(4.0),
            has_video: false,
            timeline_start: 0.0,
            fx: efectos::ClipFx {
                role: efectos::AudioRole::Dialogo,
                ..Default::default()
            },
            ..Default::default()
        };
        let score = |duck_db: f64| RoughClip {
            path: music.clone(),
            out_seconds: 8.0,
            source_duration_seconds: Some(8.0),
            has_video: false,
            track: 1,
            fx: efectos::ClipFx {
                role: efectos::AudioRole::Musica,
                duck_db,
                ..Default::default()
            },
            ..Default::default()
        };
        // Nivel de la música (660 Hz) durante el diálogo, aislada con un
        // paso de banda estrecho, con y sin ducking.
        let music_level = |clips: &[RoughClip], name: &str| -> f64 {
            let output = directory.join(name);
            export(clips, &output, ExportFormat::WavAudio).unwrap();
            let stats = Command::new(tool_path("ffmpeg.exe"))
                .args(["-v", "info", "-t", "3", "-i"])
                .arg(&output)
                .args([
                    "-af",
                    "bandpass=f=660:width_type=q:w=8,astats=metadata=0",
                    "-f",
                    "null",
                    "-",
                ])
                .output()
                .unwrap();
            let log = String::from_utf8_lossy(&stats.stderr).to_string();
            log.lines()
                .rev()
                .find_map(|line| line.split("RMS level dB:").nth(1))
                .and_then(|value| value.trim().parse::<f64>().ok())
                .unwrap_or_else(|| panic!("sin RMS en: {log}"))
        };
        let plain = music_level(&[dialogue.clone(), score(0.0)], "plano.wav");
        let ducked = music_level(&[dialogue, score(-14.0)], "ducking.wav");
        assert!(
            plain - ducked > 6.0,
            "la música debería bajar bajo el diálogo: {plain} dB → {ducked} dB"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}

/// Ningún texto de la interfaz puede caer en un glifo que las fuentes de
/// egui no tienen: se vería un cuadrado vacío («tofu»), en Windows igual que
/// aquí, porque egui dibuja con sus propias fuentes.
#[cfg(test)]
mod glyph_tests {
    use ab_glyph::{Font, FontRef};

    /// Caracteres dentro de literales de cadena del código de interfaz,
    /// ignorando comentarios y documentación.
    fn ui_chars() -> Vec<(char, String)> {
        let mut found = Vec::new();
        for source in [
            include_str!("main.rs"),
            include_str!("efectos.rs"),
            include_str!("subtitulos_animados.rs"),
            include_str!("transcripcion.rs"),
            include_str!("media_browser.rs"),
            include_str!("command_center.rs"),
            include_str!("navigation.rs"),
            include_str!("batch.rs"),
        ] {
            for line in source.lines() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                let mut inside = false;
                let mut previous = ' ';
                for character in line.chars() {
                    if character == '"' && previous != '\\' {
                        inside = !inside;
                    } else if inside && (character as u32) > 0x24F {
                        found.push((character, line.trim().to_owned()));
                    }
                    previous = character;
                }
            }
        }
        found
    }

    #[test]
    fn every_ui_character_exists_in_egui_fonts() {
        // Solo la familia proporcional, que es la de la interfaz: algunos
        // glifos (▾, ⇥…) existen únicamente en la monoespaciada y salían
        // como cuadrados en los botones.
        let definitions = crate::theme::fonts();
        let fonts: Vec<FontRef> = definitions.families[&eframe::egui::FontFamily::Proportional]
            .iter()
            .filter_map(|name| definitions.font_data.get(name))
            .filter_map(|data| FontRef::try_from_slice(&data.font).ok())
            .collect();
        let mut missing: Vec<String> = ui_chars()
            .into_iter()
            .filter(|(character, _)| !fonts.iter().any(|font| font.glyph_id(*character).0 != 0))
            .map(|(character, line)| format!("U+{:04X} {character}  en: {line}", character as u32))
            .collect();
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "glifos sin fuente:\n{}",
            missing.join("\n")
        );
    }
}
