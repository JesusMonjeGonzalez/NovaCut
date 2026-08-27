#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Serialize, Deserialize)]
struct RoughClip {
    path: PathBuf,
    in_seconds: f64,
    out_seconds: f64,
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
    /// Secuencia compuesta; sus clips usan tiempos relativos al contenedor.
    #[serde(default)]
    nested: Option<Vec<RoughClip>>,
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
const MONITOR_FPS: f64 = 30.0;

/// Reproducción en curso del monitor: proceso FFmpeg + reloj local.
struct Playback {
    child: std::process::Child,
    audio_child: std::process::Child,
    rx: Receiver<Option<PreviewFrame>>,
    start_playhead: f64,
    last_consumed: u64,
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

/// Extrae un fotograma pequeño del medio para la miniatura de la timeline.
fn generate_thumbnail(path: &Path, time: f64) -> Result<PreviewFrame, String> {
    let result = Command::new(tool_path("ffmpeg.exe"))
        .args(["-v", "error", "-ss", &format_seconds(time), "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!(
                "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=decrease,pad={THUMB_WIDTH}:{THUMB_HEIGHT}:(ow-iw)/2:(oh-ih)/2,format=rgba"
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
    if result.stdout.len() != THUMB_WIDTH * THUMB_HEIGHT * 4 {
        return Err("Miniatura incompleta".to_owned());
    }
    Ok(PreviewFrame {
        pixels: result.stdout,
        width: THUMB_WIDTH,
        height: THUMB_HEIGHT,
    })
}

/// Evento de gesto de timeline emitido por el bucle de pintado.
enum TimelineDragEvent {
    Move(usize, f64, usize),
    TrimStart(usize, f64),
    TrimEnd(usize, f64),
    Commit(usize),
    Select(usize),
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
    candidates.into_iter().find(|path| path.is_file())
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

fn expand_speed_ramps(clips: Vec<RoughClip>) -> Vec<RoughClip> {
    let mut out = Vec::new();
    for clip in clips {
        if clip
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
            timeline_offset += timeline_duration;
            segment.fade_out_seconds = if source_end >= clip.source_duration() - 0.001 {
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

/// Materializa las transiciones por negro sobre copias de los clips: al clip
/// anterior de la misma pista se le fija el fundido de salida y al actual el
/// de entrada, ambos iguales a la duración pedida.
fn resolve_render_clips(clips: &[RoughClip]) -> Vec<RoughClip> {
    let mut out = clips.to_vec();
    for i in 0..out.len() {
        if !out[i].has_video || out[i].transition.as_deref() != Some("negro") {
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
            out[j].fade_out_seconds = d;
            out[i].fade_in_seconds = d;
        }
    }
    out
}

/// Cadena `eq` si algún ajuste está activo; vacío cuando todo es neutro.
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
        }
    }
}

impl Default for RoughClip {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            in_seconds: 0.0,
            out_seconds: 1.0,
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
            nested: None,
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

/// Subtítulo con nombre anclado a un intervalo de la timeline.
#[derive(Clone, Serialize, Deserialize)]
struct Subtitle {
    start: f64,
    end: f64,
    text: String,
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

/// Convierte segundos a marca de tiempo SRT (HH:MM:SS,mmm).
fn srt_timestamp(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let (rest, ms) = (total_ms / 1000, total_ms % 1000);
    let (h, rem) = (rest / 3600, rest % 3600);
    let (m, s) = (rem / 60, rem % 60);
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

/// Genera un archivo .srt a partir de los subtítulos ordenados.
fn build_srt(subtitles: &[Subtitle]) -> String {
    let mut ordered: Vec<&Subtitle> = subtitles.iter().collect();
    ordered.sort_by(|left, right| left.start.total_cmp(&right.start));
    let mut out = String::new();
    for (index, subtitle) in ordered.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            srt_timestamp(subtitle.start),
            srt_timestamp(subtitle.end),
            subtitle.text.trim()
        ));
    }
    out
}

fn parse_srt_timestamp(value: &str) -> Option<f64> {
    let normalized = value.trim().replace(',', ".");
    let mut parts = normalized.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_srt(content: &str) -> Result<Vec<Subtitle>, String> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut subtitles = Vec::new();
    for block in normalized.split("\n\n") {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let Some(first) = lines.next() else {
            continue;
        };
        let timing = if first.contains("-->") {
            first
        } else {
            lines
                .next()
                .ok_or_else(|| "Bloque SRT sin tiempos".to_owned())?
        };
        let (start, end) = timing
            .split_once("-->")
            .ok_or_else(|| format!("Tiempo SRT no válido: {timing}"))?;
        let start =
            parse_srt_timestamp(start).ok_or_else(|| format!("Inicio SRT no válido: {start}"))?;
        let end = parse_srt_timestamp(end.split_whitespace().next().unwrap_or(end))
            .ok_or_else(|| format!("Final SRT no válido: {end}"))?;
        let text = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
        if end > start && !text.is_empty() {
            subtitles.push(Subtitle { start, end, text });
        }
    }
    if subtitles.is_empty() {
        Err("El archivo SRT no contiene subtítulos válidos".to_owned())
    } else {
        subtitles.sort_by(|left, right| left.start.total_cmp(&right.start));
        Ok(subtitles)
    }
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
    for root in roots {
        let executable = ["whisper-cli.exe", "main.exe"]
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
            }),
            ..Default::default()
        });
    }
    out
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
        for clip in &mut self.clips {
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

    fn video_track_count(&self) -> usize {
        self.clips
            .iter()
            .filter(|clip| clip.has_video)
            .map(|clip| clip.track + 1)
            .max()
            .unwrap_or(1)
    }

    fn audio_track_count(&self) -> usize {
        self.clips
            .iter()
            .filter(|clip| clip.has_audio)
            .map(|clip| clip.track + 1)
            .max()
            .unwrap_or(1)
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
            name: "Montaje sin titulo".to_owned(),
            clips: Vec::new(),
            markers: Vec::new(),
            subtitles: Vec::new(),
            subtitle_style: None,
            track_gains: Vec::new(),
            master_gain_db: 0.0,
            normalize_loudness: false,
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
    pending_silence_cut: Option<(ClipJobKey, Vec<(f64, f64)>)>,
    scene_cut_result: Option<Receiver<SceneCutResult>>,
    pending_scene_cut: Option<(ClipJobKey, Vec<f64>)>,
    show_waveform: bool,
    show_vectorscope: bool,
    pending_document_action: Option<DocumentAction>,
    document_generation: u64,
    /// Edición en vivo (texto/arrastre) aún sin comprometer a undo/disco:
    /// (estado previo al gesto, instante del último cambio).
    pending_edit: Option<(RoughProject, std::time::Instant)>,
    /// Progreso del render compartido con el hilo de FFmpeg.
    render_progress: Arc<std::sync::Mutex<RenderProgress>>,
}

#[derive(Clone)]
struct ClipJobKey {
    generation: u64,
    index: usize,
    path: PathBuf,
    in_seconds: f64,
    out_seconds: f64,
}

type ProxyResult = (ClipJobKey, Result<PathBuf, String>);
type SilenceResult = (ClipJobKey, Result<Vec<(f64, f64)>, String>);
type SceneCutResult = (ClipJobKey, Result<Vec<f64>, String>);
type LoudnessJobResult = (u64, Result<LoudnessReport, String>);
type TranscriptionResult = (u64, Result<Vec<Subtitle>, String>);

#[derive(Clone, Copy)]
enum DocumentAction {
    New,
    Open,
}

#[derive(Default)]
struct RenderProgress {
    pct: f64,
    eta_secs: f64,
}

/// Qué produce la exportación: vídeo H.264 o solo audio.
#[derive(Clone, Copy, PartialEq)]
enum ExportFormat {
    Mp4Video,
    WavAudio,
    Mp3Audio,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Mp4Video => "mp4",
            Self::WavAudio => "wav",
            Self::Mp3Audio => "mp3",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mp4Video => "MP4 (video)",
            Self::WavAudio => "WAV (solo audio)",
            Self::Mp3Audio => "MP3 (solo audio)",
        }
    }
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
        self.stop_playback();
        self.project = RoughProject::default();
        self.project_path = None;
        self.selected = None;
        self.playhead = 0.0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.preview_texture = None;
        self.preview_result = None;
        self.preview_refresh_pending = false;
        self.dirty = false;
        self.pending_edit = None;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.status = "Proyecto nuevo".to_owned();
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
        let Some(action) = self.pending_document_action else {
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
                    save = ui.button("Guardar").clicked();
                    discard = ui.button("Descartar").clicked();
                    cancel = ui.button("Cancelar").clicked();
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
    fn relink_selected(&mut self) {
        let Some(index) = self.selected else {
            self.status = "Selecciona un clip offline primero".to_owned();
            return;
        };
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
        self.project.clips[index].path = new_path.clone();
        self.finish_edit(before);
        self.status = format!("Medio revinculado: {}", new_path.display());
    }

    fn new(_context: &eframe::CreationContext<'_>) -> Self {
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
            proxy_result: None,
            loudness_result: None,
            loudness_report: None,
            loudness_report_generation: None,
            silence_result: None,
            transcription_result: None,
            pending_silence_cut: None,
            scene_cut_result: None,
            pending_scene_cut: None,
            show_waveform: false,
            show_vectorscope: false,
            pending_document_action: None,
            document_generation: 0,
            pending_edit: None,
            render_progress: Arc::new(std::sync::Mutex::new(RenderProgress::default())),
        };
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
            app.project = project;
            app.status = "Sesion anterior recuperada automaticamente".to_owned();
        } else if !app.ffmpeg_ready {
            app.status = "Falta el motor multimedia. Pulsa Instalar FFmpeg.".to_owned();
        }
        app
    }

    fn import_media(&mut self) {
        let Some(paths) = FileDialog::new()
            .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm", "m4v"])
            .add_filter("Audio", &["wav", "mp3", "m4a", "aac", "flac", "ogg"])
            .pick_files()
        else {
            return;
        };

        self.import_paths(paths);
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
        let root = self
            .project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| source.parent().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir)
            .join("NovaCut Proxies");
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("clip");
        let target = root.join(format!("{stem}-{index}-proxy.mp4"));
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
        let generation = self.document_generation;
        let (sender, receiver) = mpsc::channel();
        self.loudness_result = Some(receiver);
        self.status = "Analizando sonoridad del montaje...".to_owned();
        std::thread::spawn(move || {
            let mut command = Command::new(tool_path("ffmpeg.exe"));
            command.args(["-v", "info"]);
            let (indices, titles) = push_render_inputs(&mut command, &prepared, (640, 360), false);
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
                    apply = ui.button("Aplicar corte").clicked();
                    cancel = ui.button("Cancelar").clicked();
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
            self.selected = self.project.clips.get(key.index).map(|_| key.index);
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
                    apply = ui.button("Partir en las escenas").clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        if apply {
            let before = self.project.clone();
            let segments = split_by_scene_cuts(&self.project.clips[key.index], &cuts);
            let segment_count = segments.len();
            self.project.clips.splice(key.index..=key.index, segments);
            self.selected = self.project.clips.get(key.index).map(|_| key.index);
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
        self.selected = Some(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = "Pista convertida en secuencia anidada".to_owned();
    }

    fn unnest_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
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
        self.selected = self.project.clips.get(index).map(|_| index);
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
                self.selected = Some(self.project.clips.len() - 1);
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
        let generation = self.document_generation;
        let (sender, receiver) = mpsc::channel();
        self.transcription_result = Some(receiver);
        self.status = "Preparando audio para Whisper...".to_owned();
        std::thread::spawn(move || {
            let prefix =
                std::env::temp_dir().join(format!("novacut-whisper-{}", std::process::id()));
            let wav = prefix.with_extension("wav");
            let srt = prefix.with_extension("srt");
            let _ = std::fs::remove_file(&wav);
            let _ = std::fs::remove_file(&srt);
            let mut ffmpeg = Command::new(tool_path("ffmpeg.exe"));
            ffmpeg.args(["-y", "-v", "error"]);
            let (indices, titles) = push_render_inputs(&mut ffmpeg, &prepared, (640, 360), false);
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
                let output = Command::new(&whisper)
                    .arg("-m")
                    .arg(&model)
                    .arg("-f")
                    .arg(&wav)
                    .args(["-osrt", "-of"])
                    .arg(&prefix)
                    .args(["-l", "auto"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .map_err(|error| format!("No se pudo iniciar Whisper: {error}"))?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
                }
                let content = std::fs::read_to_string(&srt)
                    .map_err(|error| format!("Whisper no generó SRT: {error}"))?;
                parse_srt(&content)
            });
            let _ = std::fs::remove_file(wav);
            let _ = std::fs::remove_file(srt);
            let _ = sender.send((generation, result));
        });
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
            Ok(subtitles) => {
                let count = subtitles.len();
                let before = self.project.clone();
                self.project.subtitles = subtitles;
                self.finish_edit(before);
                self.status = format!("Whisper generó {count} subtítulos");
            }
            Err(error) => self.status = format!("Fallo de transcripción: {error}"),
        }
        self.transcription_result = None;
    }

    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        let before = self.project.clone();
        let mut append_at = self.project.duration();
        let mut imported = 0;
        for path in paths {
            match probe_media(&path) {
                Ok((duration, has_video, has_audio))
                    if duration > 0.0 && (has_video || has_audio) =>
                {
                    self.project.clips.push(RoughClip {
                        path,
                        in_seconds: 0.0,
                        out_seconds: duration,
                        has_video,
                        has_audio,
                        speed: 1.0,
                        timeline_start: append_at,
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
                        nested: None,
                    });
                    append_at += duration;
                    imported += 1;
                }
                Ok(_) => self.status = "FFprobe no devolvio una duracion valida".to_owned(),
                Err(error) => self.status = error,
            }
        }
        if imported > 0 {
            self.finish_edit(before);
            self.selected = Some(self.project.clips.len() - 1);
            self.status = format!("{imported} medio(s) importado(s)");
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

        match serde_json::to_string_pretty(&self.project)
            .map_err(|error| error.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|error| error.to_string()))
        {
            Ok(()) => {
                save_backup(&path, &self.project);
                self.project_path = Some(path);
                self.dirty = false;
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
                project.normalize();
                self.project = project;
                self.project_path = Some(path);
                self.selected = None;
                self.playhead = 0.0;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.preview_texture = None;
                self.preview_result = None;
                self.preview_refresh_pending = false;
                self.dirty = false;
                self.pending_edit = None;
                self.document_generation = self.document_generation.wrapping_add(1);
                save_recovery(&self.project);
                self.status = "Proyecto abierto".to_owned();
            }
            Ok(_) => self.status = "Version de proyecto no compatible".to_owned(),
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
        let project_directory = path.parent();
        let media: HashMap<String, PathBuf> = mac
            .medios
            .iter()
            .filter_map(|item| {
                resolve_mac_media(item, project_directory).map(|path| (item.id.clone(), path))
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
                let (media_path, source_in, has_audio) = if clip.es_titulo {
                    (PathBuf::new(), 0.0, false)
                } else {
                    let Some(media_path) = media.get(&clip.media_id) else {
                        offline += 1;
                        continue;
                    };
                    let has_audio = probe_media(media_path)
                        .map(|(_, _, audio)| audio)
                        .unwrap_or(true);
                    (media_path.clone(), clip.source_in as f64 / fps, has_audio)
                };
                clips.push(RoughClip {
                    path: media_path,
                    in_seconds: source_in,
                    out_seconds: source_in + clip.duracion as f64 / fps * speed,
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
                    nested: None,
                    transition: None,
                    transition_duration: default_transition_duration(),
                    label: 0,
                });
            }
        }
        if clips.is_empty() {
            self.status = "No se localizaron clips compatibles del proyecto Mac".to_owned();
            return;
        }
        self.project = RoughProject {
            version: 2,
            name: mac
                .nombre
                .unwrap_or_else(|| "Proyecto importado de Mac".to_owned()),
            clips,
            markers: Vec::new(),
            subtitles: Vec::new(),
            subtitle_style: None,
            track_gains: Vec::new(),
            master_gain_db: 0.0,
            normalize_loudness: false,
        };
        self.project_path = None;
        self.selected = None;
        self.playhead = 0.0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = true;
        self.pending_edit = None;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.preview_texture = None;
        self.preview_result = None;
        self.preview_refresh_pending = false;
        save_recovery(&self.project);
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
    fn snap_tolerance(&self) -> f64 {
        if self.snap_enabled {
            3.0 / self.zoom.max(0.05) as f64
        } else {
            0.0
        }
    }

    fn request_preview(&mut self) {
        if self.preview_result.is_some() {
            self.preview_refresh_pending = true;
            return;
        }
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
                (
                    index,
                    clip.clone(),
                    clip.in_seconds
                        + (self.playhead - clip.timeline_start).clamp(0.0, clip.duration())
                            * clip.speed.clamp(0.1, 8.0),
                )
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
            let result = render_preview_frame(&sources);
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
            let before = self.project.clone();
            let local = self.playhead - self.project.clips[index].timeline_start;
            let source_split = self.project.clips[index].in_seconds
                + local * self.project.clips[index].speed.clamp(0.1, 8.0);
            let mut right = self.project.clips[index].clone();
            self.project.clips[index].out_seconds = source_split;
            right.in_seconds = source_split;
            right.timeline_start = self.playhead;
            self.project.clips.insert(index + 1, right);
            self.selected = Some(index + 1);
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
        let audio_only = self.export_format != ExportFormat::Mp4Video;
        let Some(output) = FileDialog::new()
            .add_filter(
                match self.export_format {
                    ExportFormat::Mp4Video => "MP4 H.264",
                    ExportFormat::WavAudio => "WAV audio",
                    ExportFormat::Mp3Audio => "MP3 audio",
                },
                &[self.export_format.extension()],
            )
            .set_file_name(match self.export_format {
                ExportFormat::Mp4Video => "NovaCut Export.mp4",
                ExportFormat::WavAudio => "NovaCut Audio.wav",
                ExportFormat::Mp3Audio => "NovaCut Audio.mp3",
            })
            .save_file()
        else {
            return;
        };

        let clips = self.effective_clips();
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
        let measured_loudness = self.current_loudness_measurement().cloned();
        let (sender, receiver) = mpsc::channel();
        self.export_result = Some(receiver);
        self.export_cancel = Some(cancel);
        self.status = if audio_only {
            format!("Exportando audio {}...", format.extension().to_uppercase())
        } else {
            format!("Exportando H.264 {}x{}...", size.0, size.1)
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
                measured_loudness.as_ref(),
                &progress,
            )
            .map(|()| output);
            let _ = sender.send(result);
        });
    }

    fn poll_export(&mut self) {
        let Some(receiver) = &self.export_result else {
            return;
        };
        if let Ok(result) = receiver.try_recv() {
            self.status = match result {
                Ok(path) => format!("Exportado: {}", path.display()),
                Err(error) if error == "Exportacion cancelada" => error,
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
        self.selected = Some(self.project.clips.len() - 1);
        self.finish_edit(before);
        self.status = "Titulo creado; edita su texto en el inspector".to_owned();
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
        self.selected = Some(self.project.clips.len() - 1);
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
        let before = self.project.clone();
        self.project.subtitles.push(Subtitle {
            start: self.playhead,
            end: self.playhead + 3.0,
            text: String::new(),
        });
        self.finish_edit(before);
        self.status = "Subtitulo añadido; escribe su texto en la lista".to_owned();
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

    /// Clips del proyecto más los subtítulos como títulos superiores.
    fn effective_clips(&self) -> Vec<RoughClip> {
        if self.burn_subtitles {
            clips_with_subtitles(
                &self.project.clips,
                &self.project.subtitles,
                self.project.subtitle_style.as_ref(),
            )
        } else {
            self.project.clips.clone()
        }
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
        let (input_indices, is_title_input) =
            push_render_inputs(&mut command, &prepared, size, self.use_proxies);
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

    /// Cierra el hueco entre el clip seleccionado y el clip anterior de su pista.
    fn close_gap(&mut self) {
        let Some(index) = self.selected else {
            self.status = "Selecciona un clip para cerrar el hueco".to_owned();
            return;
        };
        let clip = &self.project.clips[index];
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
        let prepared = prepare_render_clips(&self.effective_clips());
        let mut command = Command::new(tool_path("ffmpeg.exe"));
        command.args(["-v", "error"]);
        let (input_indices, is_title_input) = push_render_inputs(
            &mut command,
            &prepared,
            (MONITOR_WIDTH as u32, MONITOR_HEIGHT as u32),
            self.use_proxies,
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
        let start_playhead = self.playhead;
        let skip_frames = (start_playhead * MONITOR_FPS).max(0.0) as u64;
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
                        (index - skip_frames - 1) as f64 / MONITOR_FPS,
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
            meter,
            _stream: stream,
            sink,
        });
        self.status = "Reproduciendo el montaje".to_owned();
    }

    /// Consume vídeo con el reloj de audio como maestro para evitar deriva.
    fn poll_playback(&mut self, context: &egui::Context) {
        let total = self.project.duration();
        let Some(playback) = &mut self.playback else {
            return;
        };
        let audio_time = playback.sink.get_pos().as_secs_f64();
        let expected = (audio_time * MONITOR_FPS) as u64;
        let mut reached_end = false;
        while playback.last_consumed < expected {
            match playback.rx.try_recv() {
                Ok(Some(frame)) => {
                    playback.last_consumed += 1;
                    self.playhead = (playback.start_playhead
                        + playback.last_consumed as f64 / MONITOR_FPS)
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
            self.playback = None;
            self.status = "Reproduccion terminada".to_owned();
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
        let measured_loudness = self.current_loudness_measurement().cloned();
        let (sender, receiver) = mpsc::channel();
        self.montage_render = Some(receiver);
        self.status = "Renderizando previsualizacion del montaje...".to_owned();
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
                measured_loudness.as_ref(),
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
                Err(error) => self.status = format!("Fallo la previsualizacion: {error}"),
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
        // Cualquier edición en vivo pendiente queda superada por este
        // commit explícito (sus mutaciones ya están dentro de `previous`
        // o del proyecto actual); se cancela sin perder datos.
        self.pending_edit = None;
        self.stop_playback();
        self.undo_stack.push(previous);
        if self.undo_stack.len() > 100 {
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
            self.selected = None;
            self.playhead = self.playhead.min(self.project.duration());
            save_recovery(&self.project);
            self.status = "Deshacer".to_owned();
            self.dirty = true;
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
            self.selected = None;
            save_recovery(&self.project);
            self.status = "Rehacer".to_owned();
            self.dirty = true;
            self.document_generation = self.document_generation.wrapping_add(1);
            self.preview_texture = None;
            self.request_preview();
        }
    }
}

impl eframe::App for NovaCutWindows {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_export();
        self.poll_setup();
        self.poll_preview(context);
        self.poll_montage_render();
        self.poll_playback(context);
        self.pump_thumbnails(context);
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
            self.import_paths(dropped_paths);
        }
        let keyboard_shortcuts = !context.wants_keyboard_input();
        let undo_shortcut = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
        let redo_shortcut = keyboard_shortcuts
            && context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Y));
        let save_shortcut =
            context.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::S));
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
        if undo_shortcut {
            self.undo();
        }
        if redo_shortcut {
            self.redo();
        }
        if save_shortcut {
            self.save_project(false);
        }
        if split_shortcut || nle_split {
            self.split_at_playhead();
        }
        if delete_selected || ripple_delete {
            if let Some(index) = self.selected {
                if index < self.project.clips.len() {
                    let before = self.project.clone();
                    let removed = self.project.clips.remove(index);
                    if ripple_delete {
                        // Ripple en la misma pista: los clips posteriores se
                        // desplazan para cerrar el hueco.
                        let removed_end = removed.timeline_start + removed.duration();
                        for clip in &mut self.project.clips {
                            if clip.track == removed.track
                                && clip.has_video == removed.has_video
                                && clip.timeline_start >= removed_end - 0.001
                            {
                                clip.timeline_start -= removed.duration();
                            }
                        }
                    }
                    self.selected = None;
                    self.finish_edit(before);
                    self.status = if ripple_delete {
                        "Clip eliminado con ripple".to_owned()
                    } else {
                        "Clip eliminado".to_owned()
                    };
                }
            }
        }
        if go_home {
            self.playhead = 0.0;
            self.request_preview();
        }
        if go_end {
            self.playhead = self.project.duration();
            self.request_preview();
        }
        if nudge_left {
            self.playhead = (self.playhead - 0.5).max(0.0);
            self.request_preview();
        }
        if nudge_right {
            self.playhead = (self.playhead + 0.5).min(self.project.duration());
            self.request_preview();
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
                    // Marca de la app + nombre del proyecto + punto de estado.
                    theme::logo_mark(ui);
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("EDITORCITO")
                            .strong()
                            .size(15.0)
                            .color(theme::TEXT),
                    );
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

                    // Documento
                    if theme::bar_button(ui, "Nuevo").clicked() {
                        self.request_document_action(DocumentAction::New);
                    }
                    if theme::bar_button(ui, "Abrir").clicked() {
                        self.request_document_action(DocumentAction::Open);
                    }
                    if theme::bar_button(ui, "Guardar").clicked() {
                        self.save_project(false);
                    }
                    if theme::bar_button(ui, "Guardar como").clicked() {
                        self.save_project(true);
                    }
                    theme::bar_separator(ui);

                    // Historial
                    if ui
                        .add_enabled(
                            !self.undo_stack.is_empty(),
                            egui::Button::new(egui::RichText::new("Deshacer").size(11.0)),
                        )
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(
                            !self.redo_stack.is_empty(),
                            egui::Button::new(egui::RichText::new("Rehacer").size(11.0)),
                        )
                        .clicked()
                    {
                        self.redo();
                    }
                    theme::bar_separator(ui);

                    // Medio
                    if ui
                        .add_enabled(
                            self.ffmpeg_ready,
                            egui::Button::new(egui::RichText::new("+ Importar").size(11.0)),
                        )
                        .clicked()
                    {
                        self.import_media();
                    }
                    if theme::bar_button(ui, "Importar secuencia").clicked() {
                        self.import_nested_project();
                    }
                    theme::bar_separator(ui);

                    // Timeline
                    if theme::bar_button(ui, "Partir en cabezal").clicked() {
                        self.split_at_playhead();
                    }
                    if self.ffmpeg_ready && theme::bar_button(ui, "+ Título").clicked() {
                        self.add_title_at_playhead();
                    }
                    if self.ffmpeg_ready
                        && ui
                            .add(egui::Button::new(egui::RichText::new("+ Ajuste").size(11.0)))
                            .on_hover_text(
                                "Capa de ajuste: gradúa todo lo que hay debajo en la pista superior",
                            )
                            .clicked()
                    {
                        self.add_adjustment_layer_at_playhead();
                    }
                    if theme::bar_button(ui, "+ Marcador").clicked() {
                        self.add_marker();
                    }
                    ui.toggle_value(&mut self.snap_enabled, egui::RichText::new("Imán").size(11.0))
                        .on_hover_text("Ajuste magnético");
                    ui.toggle_value(
                        &mut self.use_proxies,
                        egui::RichText::new("Proxies").size(11.0),
                    );
                    theme::bar_separator(ui);

                    // Exportación: presets + botón principal en acento.
                    egui::ComboBox::from_id_salt("preset-export")
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
                            ui.selectable_value(&mut chosen, (1280, 720), "720p");
                            ui.selectable_value(&mut chosen, (1920, 1080), "1080p (YouTube)");
                            ui.selectable_value(&mut chosen, (3840, 2160), "4K");
                            ui.selectable_value(
                                &mut chosen,
                                (1080, 1920),
                                "Shorts/Reels/TikTok 1080x1920",
                            );
                            ui.selectable_value(
                                &mut chosen,
                                (1080, 1080),
                                "Cuadrado (Instagram) 1080x1080",
                            );
                            self.export_size = chosen;
                        });
                    egui::ComboBox::from_id_salt("formato-export")
                        .selected_text(self.export_format.label())
                        .show_ui(ui, |ui| {
                            let mut chosen = self.export_format;
                            ui.selectable_value(&mut chosen, ExportFormat::Mp4Video, "MP4 (video)");
                            ui.selectable_value(
                                &mut chosen,
                                ExportFormat::WavAudio,
                                "WAV (solo audio)",
                            );
                            ui.selectable_value(
                                &mut chosen,
                                ExportFormat::Mp3Audio,
                                "MP3 (solo audio)",
                            );
                            self.export_format = chosen;
                        });
                    if ui
                        .add_enabled(
                            self.ffmpeg_ready && self.export_result.is_none(),
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "Exportar {}",
                                    self.export_format.extension().to_uppercase()
                                ))
                                .size(11.0)
                                .strong()
                                .color(egui::Color32::from_rgb(8, 24, 27)),
                            )
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(0.0, 24.0)),
                        )
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
                        if theme::bar_button(ui, "Cancelar").clicked() {
                            if let Some(cancel) = &self.export_cancel {
                                cancel.store(true, Ordering::Relaxed);
                                self.status = "Cancelando exportacion...".to_owned();
                            }
                        }
                    }
                });
            });
        self.show_unsaved_dialog(context);
        self.show_silence_review(context);
        self.show_scene_cut_review(context);

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
                            if theme::accent_button(ui, "Instalar FFmpeg automaticamente")
                                .clicked()
                            {
                                self.install_ffmpeg();
                            }
                            ui.add_space(6.0);
                            if theme::bar_button(ui, "Descargar FFmpeg manualmente").clicked() {
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
                            ))
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
                                .size(10.5)
                                .color(theme::TEXT_DIM),
                        );
                    });
                });
            return;
        }

        let project_before_inspector = self.project.clone();
        let mut trim_changed = false;
        let mut relink_requested = false;
        let mut proxy_requested = false;
        let mut nest_requested = false;
        let mut unnest_requested = false;
        // Panel de medios a la izquierda, como el "MEDIOS" de la app macOS:
        // lista de clips del proyecto; un clic selecciona y centra el cabezal.
        egui::SidePanel::left("medios")
            .default_width(250.0)
            .min_width(170.0)
            .frame(egui::Frame::new().fill(theme::BG))
            .show(context, |ui| {
                theme::panel_header(ui, "Medios", Some(self.project.clips.len()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for index in 0..self.project.clips.len() {
                        let clip = self.project.clips[index].clone();
                        let selected = self.selected == Some(index);
                        let full_name = clip.name();
                        let name = if full_name.chars().count() > 22 {
                            let mut trimmed: String = full_name.chars().take(22).collect();
                            trimmed.push('…');
                            trimmed
                        } else {
                            full_name
                        };
                        let duration = format!("{:.1} s", clip.duration());
                        let row_h = 36.0;
                        let (rect, response) = ui.allocate_exact_size(
                            egui::Vec2::new(ui.available_width(), row_h),
                            egui::Sense::click(),
                        );
                        let response =
                            response.on_hover_text(clip.path.display().to_string());
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
                        if let Some(texture) = self.thumbnails.get(&clip.path) {
                            if clip.has_video {
                                painter.image(
                                    texture.id(),
                                    egui::Rect::from_min_size(
                                        egui::pos2(thumb_x, rect.top() + 5.0),
                                        egui::vec2(thumb_size, thumb_size),
                                    ),
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                                text_x += thumb_size + 6.0;
                            }
                        }
                        painter.text(
                            egui::pos2(text_x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &name,
                            egui::FontId::proportional(12.0),
                            if selected { theme::ACCENT } else { theme::TEXT },
                        );
                        painter.text(
                            egui::pos2(rect.right() - 8.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            duration,
                            egui::FontId::proportional(10.0),
                            theme::TEXT_FAINT,
                        );
                        if response.clicked() {
                            self.selected = Some(index);
                            self.playhead = clip.timeline_start.max(0.0);
                            self.preview_texture = None;
                            self.request_preview();
                        }
                        ui.add_space(1.0);
                    }
                });
            });
        egui::SidePanel::right("inspector")
            .default_width(280.0)
            .frame(egui::Frame::new().fill(theme::BG))
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    theme::panel_header(ui, "Inspector", None);
                    if let Some(clip) = self
                        .selected
                        .and_then(|index| self.project.clips.get_mut(index))
                    {
                        ui.label(clip.name());
                        if clip.nested.is_some() {
                            ui.colored_label(
                                egui::Color32::from_rgb(130, 190, 255),
                                "Secuencia compuesta: edita sus clips después de desanidarla.",
                            );
                            unnest_requested |= ui.button("Desanidar secuencia").clicked();
                            ui.separator();
                            ui.disable();
                        }
                        if clip.title.is_some() {
                            let Some(title) = clip.title.as_mut() else {
                                unreachable!()
                            };
                            ui.label("Texto del titulo");
                            trim_changed |= ui.text_edit_singleline(&mut title.text).changed();
                            ui.label("Tamano");
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
                            if !clip.path.as_os_str().is_empty()
                                && ui.button("Quitar titulo").clicked()
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
                            ui.small(clip.path.display().to_string());
                            if !clip.path.as_os_str().is_empty() && !clip.path.exists() {
                                ui.colored_label(
                                    egui::Color32::from_rgb(246, 83, 83),
                                    "MEDIO OFFLINE",
                                );
                                relink_requested |= ui.button("Revincular...").clicked();
                            }
                            if let Some(proxy) = &clip.proxy {
                                ui.small(format!("Proxy: {}", proxy.display()));
                                if ui.button("Quitar proxy").clicked() {
                                    clip.proxy = None;
                                    trim_changed = true;
                                }
                            } else if clip.has_video && clip.nested.is_none() {
                                proxy_requested |= ui.button("Crear proxy 540p").clicked();
                            }
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
                                    clip.label = option;
                                    trim_changed = true;
                                }
                            }
                        });
                        let trim_allowed = clip
                            .speed_ramp
                            .as_ref()
                            .is_none_or(|points| points.is_empty());
                        if !trim_allowed {
                            ui.small("Entrada/salida bloqueadas mientras exista una rampa.");
                        }
                        theme::section_label(ui, "Entrada (segundos)");
                        trim_changed |= ui
                            .add_enabled(
                                trim_allowed,
                                egui::DragValue::new(&mut clip.in_seconds)
                                    .speed(0.04)
                                    .range(0.0..=clip.out_seconds),
                            )
                            .changed();
                        theme::section_label(ui, "Salida (segundos)");
                        trim_changed |= ui
                            .add_enabled(
                                trim_allowed,
                                egui::DragValue::new(&mut clip.out_seconds)
                                    .speed(0.04)
                                    .range(clip.in_seconds..=86_400.0),
                            )
                            .changed();
                        ui.label(format!("Duracion: {:.2} s", clip.duration()));
                        ui.label("Inicio en timeline");
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.timeline_start)
                                    .speed(0.04)
                                    .range(0.0..=86_400.0)
                                    .suffix(" s"),
                            )
                            .changed();
                        ui.label(if clip.has_video {
                            "Pista de video"
                        } else {
                            "Pista de audio"
                        });
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.track)
                                    .speed(0.1)
                                    .range(0..=15)
                                    .prefix(if clip.has_video { "V" } else { "A" }),
                            )
                            .changed();
                        theme::section_label(ui, "Velocidad");
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.speed)
                                    .speed(0.05)
                                    .range(0.1..=8.0)
                                    .suffix("x"),
                            )
                            .changed();
                        ui.collapsing("Rampa de velocidad", |ui| {
                            if clip.speed_ramp.is_none() {
                                if ui.button("Activar rampa").clicked() {
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
                                    if can_remove && ui.button("×").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            }
                            if let Some(index) = remove {
                                points.remove(index);
                                trim_changed = true;
                            }
                            if ui.button("Añadir punto en cabezal").clicked() {
                                points.push(SpeedPoint {
                                    source_t: source_here,
                                    speed: clip.speed,
                                });
                                points.sort_by(|left, right| {
                                    left.source_t.total_cmp(&right.source_t)
                                });
                                trim_changed = true;
                            }
                            if ui.button("Quitar rampa").clicked() {
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
                                        .text("Exposicion"),
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
                                        .text("Saturacion"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.vignette, -1.0..=1.0)
                                        .text("Vineta"),
                                )
                                .changed();
                            trim_changed |= ui
                                .add(
                                    egui::Slider::new(&mut clip.blur, 0.0..=1.0).text("Desenfoque"),
                                )
                                .changed();
                            if !clip.is_adjustment {
                                ui.collapsing("Croma (pantalla verde/azul)", |ui| {
                                if clip.chroma.is_none() {
                                    if ui.button("Activar croma").clicked() {
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
                                    if ui.button("Desactivar croma").clicked() {
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
                                if ui.button("Neutras").clicked() {
                                    clip.wheels = Some(Wheels::default());
                                    trim_changed = true;
                                }
                            });
                            ui.collapsing("Curvas", |ui| {
                                if clip.curves.is_none() {
                                    if ui.button("Activar curvas").clicked() {
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
                                                if point_count > 2 && ui.button("−").clicked() {
                                                    remove_point = Some(point_index);
                                                }
                                            });
                                        }
                                        if let Some(index) = remove_point {
                                            points.remove(index);
                                            trim_changed = true;
                                        }
                                        if point_count < 8 && ui.button("+ punto").clicked() {
                                            let last_x = points.last().map(|p| p.x).unwrap_or(1.0);
                                            points.push(CurvePoint {
                                                x: ((last_x + 1.0) / 2.0).clamp(0.0, 1.0),
                                                y: 0.5,
                                            });
                                            trim_changed = true;
                                        }
                                    }
                                    if ui.button("Identidad").clicked() {
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
                                    if ui.button("Activar máscara").clicked() {
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
                                            )
                                            .changed();
                                        trim_changed |= ui
                                            .selectable_value(
                                                &mut mask.shape,
                                                MaskShape::Ellipse,
                                                "Elipse",
                                            )
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
                                    ui.checkbox(&mut mask.inverted, "Invertida").changed();
                                if ui.button("Desactivar máscara").clicked() {
                                    clip.mask = None;
                                    trim_changed = true;
                                }
                            });
                            ui.collapsing("LUT 3D", |ui| {
                                if let Some(path) = &clip.lut {
                                    ui.small(path.display().to_string());
                                    if ui.button("Quitar LUT").clicked() {
                                        clip.lut = None;
                                        trim_changed = true;
                                    }
                                } else if ui.button("Cargar .cube...").clicked() {
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
                                if ui.button("Añadir keyframe aquí").clicked() {
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
                                        if ui.button("×").clicked() {
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
                                if ui.button("Quitar animación").clicked() {
                                    clip.keyframes = None;
                                    trim_changed = true;
                                }
                            } else {
                                ui.separator();
                                ui.label("Transformacion");
                                ui.horizontal(|ui| {
                                    ui.label("X");
                                    trim_changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut clip.position_x)
                                                .speed(1.0)
                                                .range(-7680.0..=7680.0)
                                                .suffix(" px"),
                                        )
                                        .changed();
                                    ui.label("Y");
                                    trim_changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut clip.position_y)
                                                .speed(1.0)
                                                .range(-4320.0..=4320.0)
                                                .suffix(" px"),
                                        )
                                        .changed();
                                });
                                ui.label("Escala");
                                trim_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut clip.scale_percent)
                                            .speed(0.5)
                                            .range(1.0..=800.0)
                                            .suffix(" %"),
                                    )
                                    .changed();
                                ui.label("Rotacion");
                                trim_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut clip.rotation)
                                            .speed(0.25)
                                            .range(-3600.0..=3600.0)
                                            .suffix(" deg"),
                                    )
                                    .changed();
                                ui.label("Opacidad");
                                trim_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut clip.opacity, 0.0..=100.0)
                                            .suffix(" %"),
                                    )
                                    .changed();
                                if ui.button("Animar (keyframes)").clicked() {
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
                        ui.label(if clip.has_audio {
                            "Audio detectado"
                        } else {
                            "Sin audio"
                        });
                        ui.label("Ganancia de audio");
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.gain_db)
                                    .speed(0.2)
                                    .range(-96.0..=24.0)
                                    .suffix(" dB"),
                            )
                            .changed();
                        trim_changed |= ui.checkbox(&mut clip.muted, "Silenciar clip").changed();
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
                        ui.separator();
                        ui.label("Fundidos");
                        let half = (clip.duration() / 2.0).max(0.0);
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.fade_in_seconds)
                                    .speed(0.05)
                                    .range(0.0..=half.max(0.01))
                                    .suffix(" s"),
                            )
                            .changed();
                        trim_changed |= ui
                            .add(
                                egui::DragValue::new(&mut clip.fade_out_seconds)
                                    .speed(0.05)
                                    .range(0.0..=half.max(0.01))
                                    .suffix(" s"),
                            )
                            .changed();
                        let mut is_transition = clip.transition.as_deref() == Some("negro");
                        if ui
                            .checkbox(&mut is_transition, "Transicion con el clip anterior")
                            .changed()
                        {
                            clip.transition = if is_transition {
                                Some("negro".to_owned())
                            } else {
                                None
                            };
                            trim_changed = true;
                        }
                        if is_transition {
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
                        ui.add_space(12.0);
                        let busy_render =
                            self.montage_render.is_some() || self.export_result.is_some();
                        let replay_clip = ui
                            .add_enabled(
                                self.preview_result.is_none(),
                                egui::Button::new("Reproducir recorte"),
                            )
                            .clicked();
                        let to_title = clip.has_video
                            && clip.title.is_none()
                            && ui.button("Convertir en titulo").clicked();
                        let watch_edit = clip.has_video
                            && ui
                                .add_enabled(
                                    !busy_render,
                                    egui::Button::new(if busy_render {
                                        "Renderizando..."
                                    } else {
                                        "Ver montaje completo"
                                    }),
                                )
                                .clicked();
                        let close_gap = ui.button("Cerrar hueco").clicked();
                        let sync_audio =
                            clip.has_audio && ui.button("Sincronizar ángulos por audio").clicked();
                        let cut_silences = clip.has_audio
                            && ui
                                .add_enabled(
                                    self.silence_result.is_none()
                                        && self.pending_silence_cut.is_none(),
                                    egui::Button::new("Detectar y cortar silencios"),
                                )
                                .clicked();
                        let detect_scenes = clip.has_video
                            && clip.title.is_none()
                            && clip.nested.is_none()
                            && ui
                                .add_enabled(
                                    self.scene_cut_result.is_none()
                                        && self.pending_scene_cut.is_none(),
                                    egui::Button::new("Detectar cortes de escena"),
                                )
                                .clicked();
                        if clip.nested.is_some() {
                            unnest_requested |= ui.button("Desanidar secuencia").clicked();
                        } else {
                            nest_requested |= ui.button("Anidar pista").clicked();
                        }
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
                        ui.label("Selecciona un clip del montaje.");
                    }
                });
            });
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
        if proxy_requested {
            self.create_proxy_for_selected();
        }
        if nest_requested {
            self.nest_selected_track();
        }
        if unnest_requested {
            self.unnest_selected();
        }

        let metadata_before = self.project.clone();
        let mut metadata_changed = false;
        let mut burn_changed = false;
        let mut scopes_changed = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(12)))
            .show(context, |ui| {
            // Barra de título del monitor, al estilo de las cabeceras de panel.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("MONITOR")
                        .strong()
                        .size(10.0)
                        .color(theme::TEXT),
                );
                if self.preview_result.is_some() {
                    ui.spinner();
                }
                let playing = self.playback.is_some();
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(if playing { "■ Detener" } else { "▶ Reproducir" })
                                .size(11.0),
                        ),
                    )
                    .clicked()
                {
                    self.toggle_playback();
                }
                if ui
                    .add_enabled(
                        self.frame_result.is_none(),
                        egui::Button::new(
                            egui::RichText::new(if self.frame_result.is_some() {
                                "Exportando fotograma..."
                            } else {
                                "Exportar fotograma"
                            })
                            .size(11.0),
                        ),
                    )
                    .clicked()
                {
                    self.export_frame();
                }
                scopes_changed |=
                    ui.checkbox(&mut self.show_waveform, egui::RichText::new("Waveform").size(11.0))
                        .changed();
                scopes_changed |= ui
                    .checkbox(
                        &mut self.show_vectorscope,
                        egui::RichText::new("Vectorscope").size(11.0),
                    )
                    .changed();
                ui.separator();
                ui.label(egui::RichText::new("Vol").size(11.0).color(theme::TEXT_DIM));
                ui.add_sized(
                    [80.0, 16.0],
                    egui::Slider::new(&mut self.monitor_volume, 0.0..=1.0).show_value(false),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.2} s", self.playhead))
                            .monospace()
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    );
                });
            });
            ui.add_space(8.0);
            let monitor_size = egui::vec2(
                ui.available_width().min(640.0),
                ui.available_width().min(640.0) * 9.0 / 16.0,
            );
            ui.vertical_centered(|ui| {
                egui::Frame::new()
                    .fill(egui::Color32::BLACK)
                    .corner_radius(6.0)
                    .stroke(egui::Stroke::new(1.0_f32, theme::STROKE))
                    .show(ui, |ui| {
                        ui.set_min_size(monitor_size);
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
            let total = self.project.duration();
            let video_tracks = self.project.video_track_count();
            let audio_tracks = self.project.audio_track_count();
            let track_count = video_tracks + audio_tracks;
            let timeline_height = track_count as f32 * 54.0 + 12.0;
            // --- Cabecera del timeline: título a la izquierda, zoom a la derecha ---
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("TIMELINE")
                        .strong()
                        .size(10.0)
                        .color(theme::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("Ajustar").size(10.5)),
                        )
                        .clicked()
                    {
                        self.zoom = 1.0;
                        self.hscroll = 0.0;
                    }
                    if ui
                        .add(egui::Button::new(egui::RichText::new("+").size(11.0)))
                        .clicked()
                    {
                        let view_old = total / self.zoom as f64;
                        self.zoom = (self.zoom * 1.4).min(200.0);
                        let view_new = total / self.zoom as f64;
                        let anchor = self.playhead;
                        self.hscroll =
                            (anchor - (anchor - self.hscroll) * (view_new / view_old)).max(0.0);
                    }
                    ui.label(
                        egui::RichText::new(format!("{:.0}%", self.zoom * 100.0))
                            .monospace()
                            .size(10.5)
                            .color(theme::TEXT_DIM),
                    );
                    if ui
                        .add(egui::Button::new(egui::RichText::new("−").size(11.0)))
                        .clicked()
                    {
                        self.zoom = (self.zoom / 1.4).max(1.0);
                        self.hscroll = self
                            .hscroll
                            .min((total - total / self.zoom as f64).max(0.0));
                    }
                    if self.zoom > 1.0 {
                        let view_seconds = total / self.zoom as f64;
                        let max_off = (total - view_seconds).max(0.0);
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
            let view_seconds = total / self.zoom as f64;
            let (timeline_rect, timeline_response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), timeline_height),
                egui::Sense::click_and_drag(),
            );
            let painter = ui.painter().with_clip_rect(timeline_rect);
            let pps = timeline_rect.width().max(1.0) as f64 / view_seconds.max(0.001);
            let hs = self.hscroll;
            let to_x = |t: f64| timeline_rect.left() + ((t - hs) * pps) as f32;
            let pointer_time = |x: f32| {
                (hs + ((x - timeline_rect.left()) / timeline_rect.width().max(1.0)) as f64
                    * view_seconds)
                    .clamp(0.0, total.max(0.0))
            };
            // Rueda del ratón: Ctrl+rueda = zoom hacia el cabezal; rueda = desplazar.
            if timeline_response.hovered() {
                let (wheel_x, wheel_y, ctrl) = context.input(|input| {
                    (
                        input.raw_scroll_delta.x,
                        input.raw_scroll_delta.y,
                        input.modifiers.ctrl,
                    )
                });
                if ctrl && wheel_y.abs() > 0.0 {
                    let factor = (1.0 - wheel_y / 400.0).clamp(0.7, 1.4);
                    let anchor = self.playhead;
                    let view_old = view_seconds;
                    self.zoom = (self.zoom * factor).clamp(1.0, 200.0);
                    let view_new = total / self.zoom as f64;
                    self.hscroll =
                        (anchor - (anchor - self.hscroll) * (view_new / view_old)).max(0.0);
                } else if wheel_x.abs() > 0.0 || wheel_y.abs() > 0.0 {
                    let max_off = (total - view_seconds).max(0.0);
                    self.hscroll =
                        (self.hscroll - (wheel_x + wheel_y) as f64 / pps).clamp(0.0, max_off);
                }
            }
            let max_off = (total - view_seconds).max(0.0);
            self.hscroll = self.hscroll.clamp(0.0, max_off);

            let tl_bg = egui::Rect::from_min_size(
                timeline_rect.min,
                egui::vec2(timeline_rect.width(), timeline_rect.height()),
            );
            painter.rect_filled(tl_bg, 6.0, egui::Color32::from_rgb(13, 14, 16));
            painter.rect_stroke(
                tl_bg,
                6.0,
                egui::Stroke::new(1.0_f32, theme::STROKE_SOFT),
                egui::StrokeKind::Inside,
            );
            // Regla de tiempo: marcas cada 1/5/10/30/60/300 s según el zoom.
            {
                let steps: [f64; 7] = [0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0];
                let step = steps
                    .iter()
                    .find(|&&step| step * pps >= 70.0)
                    .unwrap_or(&steps[6]);
                let first = (hs / step).floor() * step;
                let mut t = first;
                while t <= hs + view_seconds {
                    let x = to_x(t);
                    if x >= timeline_rect.left() {
                        painter.line_segment(
                            [
                                egui::pos2(x, timeline_rect.top()),
                                egui::pos2(x, timeline_rect.top() + 6.0),
                            ],
                            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(60)),
                        );
                        if step * pps >= 90.0 {
                            painter.text(
                                egui::pos2(x + 4.0, timeline_rect.top() + 4.0),
                                egui::Align2::LEFT_TOP,
                                format_seconds(t),
                                egui::FontId::monospace(9.0),
                                egui::Color32::from_gray(110),
                            );
                        }
                    }
                    t += step;
                }
            }
            let row_height = (timeline_rect.height() - 12.0) / track_count as f32;
            for row in 0..track_count {
                let y = timeline_rect.bottom() - 6.0 - row as f32 * row_height;
                painter.line_segment(
                    [
                        egui::pos2(timeline_rect.left(), y),
                        egui::pos2(timeline_rect.right(), y),
                    ],
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(30, 32, 36)),
                );
                painter.text(
                    egui::pos2(timeline_rect.left() + 5.0, y - row_height / 2.0),
                    egui::Align2::LEFT_CENTER,
                    if row < audio_tracks {
                        format!("A{}", audio_tracks - row)
                    } else {
                        format!("V{}", row - audio_tracks + 1)
                    },
                    egui::FontId::monospace(10.0),
                    egui::Color32::from_gray(120),
                );
            }
            const EDGE_GRAB: f32 = 6.0;
            let mut timeline_drag = None;
            for (index, clip) in self.project.clips.iter().enumerate() {
                let left = to_x(clip.timeline_start);
                let right = to_x(clip.timeline_start + clip.duration());
                let row = if clip.has_video {
                    audio_tracks + clip.track
                } else {
                    audio_tracks.saturating_sub(clip.track + 1)
                };
                let bottom = timeline_rect.bottom() - 6.0 - row as f32 * row_height;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(left + 1.0, bottom - row_height + 4.0),
                    egui::pos2((right - 1.0).max(left + 3.0), bottom - 4.0),
                );
                let color = if self.selected == Some(index) {
                    egui::Color32::from_rgb(0, 132, 150)
                } else if clip.has_video {
                    egui::Color32::from_rgb(44, 78, 124)
                } else {
                    egui::Color32::from_rgb(38, 100, 82)
                };
                painter.rect_filled(rect, 4.0, color);
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(
                        1.0_f32,
                        if self.selected == Some(index) {
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
                        painter.image(
                            texture_id,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                }
                // Franja de la etiqueta de color.
                if let Some(label) = label_color(clip.label) {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left() + 1.0, rect.top()),
                            egui::pos2(rect.right() - 1.0, rect.top() + 4.0),
                        ),
                        2.0,
                        label,
                    );
                }
                let response = ui.interact(
                    rect,
                    ui.id().with(("timeline-clip", index)),
                    egui::Sense::click_and_drag(),
                );
                if response.drag_started() {
                    let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                    let kind = if (pointer.x - rect.left()).abs() <= EDGE_GRAB {
                        DragKind::TrimStart
                    } else if (rect.right() - pointer.x).abs() <= EDGE_GRAB {
                        DragKind::TrimEnd
                    } else {
                        DragKind::Move
                    };
                    self.drag_edit = Some((index, kind, self.project.clone()));
                    self.selected = Some(index);
                } else if response.dragged() {
                    if let Some((_, kind, _)) = self.drag_edit.as_ref().filter(|s| s.0 == index) {
                        let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
                        match *kind {
                            DragKind::Move => {
                                let target_row =
                                    (((timeline_rect.bottom() - 6.0 - pointer.y) / row_height)
                                        .floor()
                                        .max(0.0) as usize)
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
                            DragKind::TrimStart => {
                                timeline_drag = Some(TimelineDragEvent::TrimStart(
                                    index,
                                    pointer_time(pointer.x),
                                ));
                            }
                            DragKind::TrimEnd => {
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
                    timeline_drag = Some(TimelineDragEvent::Select(index));
                }
                if rect.width() > 55.0 && rect.intersects(timeline_rect) {
                    if thumb.is_some() {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(rect.left(), rect.center().y - 9.0),
                                egui::pos2(rect.right(), rect.center().y + 9.0),
                            ),
                            0.0,
                            egui::Color32::from_black_alpha(150),
                        );
                    }
                    painter.text(
                        rect.center() + egui::vec2(1.0, 1.0),
                        egui::Align2::CENTER_CENTER,
                        clip.name(),
                        egui::FontId::proportional(11.5),
                        egui::Color32::from_black_alpha(160),
                    );
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        clip.name(),
                        egui::FontId::proportional(11.5),
                        egui::Color32::WHITE,
                    );
                }
            }
            let playhead_x = to_x(self.playhead);
            if playhead_x >= timeline_rect.left() && playhead_x <= timeline_rect.right() {
                painter.line_segment(
                    [
                        egui::pos2(playhead_x, timeline_rect.top()),
                        egui::pos2(playhead_x, timeline_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(246, 83, 83)),
                );
            }
            // Marcadores: triangulos verdes; clic para saltar al instante.
            let mut marker_jump: Option<f64> = None;
            for (marker_index, marker) in self.project.markers.iter().enumerate() {
                let x = to_x(marker.time);
                if x < timeline_rect.left() || x > timeline_rect.right() {
                    continue;
                }
                let triangle = [
                    egui::pos2(x - 5.0, timeline_rect.top() + 1.0),
                    egui::pos2(x + 5.0, timeline_rect.top() + 1.0),
                    egui::pos2(x, timeline_rect.top() + 9.0),
                ];
                painter.add(egui::Shape::convex_polygon(
                    triangle.to_vec(),
                    egui::Color32::from_rgb(90, 220, 120),
                    egui::Stroke::NONE,
                ));
                let hit = ui.interact(
                    egui::Rect::from_min_size(
                        egui::pos2(x - 7.0, timeline_rect.top()),
                        egui::vec2(14.0, 12.0),
                    ),
                    ui.id().with(("marker", marker_index)),
                    egui::Sense::click(),
                );
                if hit.clicked() {
                    marker_jump = Some(marker.time);
                }
                if marker.time >= self.hscroll
                    && (marker.time - self.hscroll) * pps < timeline_rect.width() as f64 - 30.0
                    && timeline_rect.width() > 120.0
                {
                    painter.text(
                        egui::pos2(x + 7.0, timeline_rect.top() + 5.0),
                        egui::Align2::LEFT_CENTER,
                        &marker.name,
                        egui::FontId::monospace(10.0),
                        egui::Color32::from_rgb(140, 240, 160),
                    );
                }
            }
            if let Some(time) = marker_jump {
                self.playhead = time;
                self.request_preview();
            }
            if let Some(pointer) = timeline_response.interact_pointer_pos() {
                self.playhead = pointer_time(pointer.x);
                if timeline_response.drag_stopped() || timeline_response.clicked() {
                    self.request_preview();
                }
            }
            match timeline_drag {
                Some(TimelineDragEvent::Move(index, delta, track)) => {
                    if self
                        .drag_edit
                        .as_ref()
                        .is_some_and(|(active, _, _)| *active == index)
                    {
                        let target = (self.project.clips[index].timeline_start + delta).max(0.0);
                        self.project.clips[index].timeline_start = snap_time(
                            target,
                            &self.project.clips,
                            Some(index),
                            &self.project.markers,
                            self.playhead,
                            self.snap_tolerance(),
                        );
                        self.project.clips[index].track = track;
                        self.selected = Some(index);
                        self.request_preview();
                    }
                }
                Some(TimelineDragEvent::TrimStart(index, target_time)) => {
                    let Some((_, DragKind::TrimStart, before)) = self
                        .drag_edit
                        .as_ref()
                        .map(|s| (s.0, s.1, &s.2))
                        .filter(|s| s.0 == index)
                    else {
                        return;
                    };
                    let orig = &before.clips[index];
                    if orig
                        .speed_ramp
                        .as_ref()
                        .is_some_and(|points| !points.is_empty())
                    {
                        self.status =
                            "Quita la rampa de velocidad antes de recortar bordes".to_owned();
                        return;
                    }
                    let speed = orig.speed.clamp(0.1, 8.0);
                    let start_min = (orig.timeline_start - orig.in_seconds / speed).max(0.0);
                    let max_start = (orig.timeline_start + orig.duration() - 0.04).max(start_min);
                    let new_start = snap_time(
                        target_time,
                        &before.clips,
                        Some(index),
                        &before.markers,
                        self.playhead,
                        self.snap_tolerance(),
                    )
                    .clamp(start_min, max_start);
                    let delta_t = new_start - orig.timeline_start;
                    let clip_out = &mut self.project.clips[index];
                    clip_out.timeline_start = new_start;
                    clip_out.in_seconds = (orig.in_seconds + delta_t * speed).max(0.0);
                    self.request_preview();
                }
                Some(TimelineDragEvent::TrimEnd(index, target_time)) => {
                    let Some((_, DragKind::TrimEnd, before)) = self
                        .drag_edit
                        .as_ref()
                        .map(|s| (s.0, s.1, &s.2))
                        .filter(|s| s.0 == index)
                    else {
                        return;
                    };
                    let orig = &before.clips[index];
                    if orig
                        .speed_ramp
                        .as_ref()
                        .is_some_and(|points| !points.is_empty())
                    {
                        self.status =
                            "Quita la rampa de velocidad antes de recortar bordes".to_owned();
                        return;
                    }
                    let speed = orig.speed.clamp(0.1, 8.0);
                    let min_end = orig.timeline_start + 0.04;
                    let new_end = snap_time(
                        target_time,
                        &before.clips,
                        Some(index),
                        &before.markers,
                        self.playhead,
                        self.snap_tolerance(),
                    )
                    .max(min_end);
                    let clip_out = &mut self.project.clips[index];
                    clip_out.in_seconds = orig.in_seconds;
                    clip_out.out_seconds =
                        orig.in_seconds + (new_end - orig.timeline_start) * speed;
                    self.request_preview();
                }
                Some(TimelineDragEvent::Commit(index)) => {
                    if let Some((active, kind, before)) = self.drag_edit.take() {
                        if active != index {
                            return;
                        }
                        let now = &self.project.clips[index];
                        let orig = &before.clips[index];
                        let changed = now.timeline_start != orig.timeline_start
                            || now.track != orig.track
                            || now.in_seconds != orig.in_seconds
                            || now.out_seconds != orig.out_seconds;
                        if changed {
                            self.finish_edit(before);
                            self.status = match kind {
                                DragKind::Move => "Clip movido".to_owned(),
                                _ => "Clip recortado".to_owned(),
                            };
                        }
                    }
                }
                Some(TimelineDragEvent::Select(index)) => self.selected = Some(index),
                None => {}
            }
            ui.add_space(10.0);
            if !self.project.markers.is_empty() {
                ui.collapsing("Marcadores", |ui| {
                    let mut marker_delete: Option<usize> = None;
                    let mut marker_jump: Option<f64> = None;
                    for (marker_index, marker) in self.project.markers.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{:02}", marker_index + 1));
                            metadata_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut marker.time)
                                        .speed(0.05)
                                        .suffix(" s"),
                                )
                                .changed();
                            metadata_changed |= ui.text_edit_singleline(&mut marker.name).changed();
                            if ui.button("Ir").clicked() {
                                marker_jump = Some(marker.time);
                            }
                            if ui.button("×").clicked() {
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
                });
                ui.add_space(6.0);
            }
            ui.collapsing("Mezclador de audio", |ui| {
                let tracks = self.project.audio_track_count().max(1);
                self.project.track_gains.resize(tracks, 0.0);
                for (track, gain) in self.project.track_gains.iter_mut().enumerate() {
                    metadata_changed |= ui
                        .add(
                            egui::Slider::new(gain, -60.0..=12.0)
                                .text(format!("Bus {}", track + 1))
                                .suffix(" dB"),
                        )
                        .changed();
                }
                ui.separator();
                metadata_changed |= ui
                    .add(
                        egui::Slider::new(&mut self.project.master_gain_db, -60.0..=12.0)
                            .text("Master")
                            .suffix(" dB"),
                    )
                    .changed();
                metadata_changed |= ui
                    .checkbox(
                        &mut self.project.normalize_loudness,
                        "Normalizar exportación a -14 LUFS",
                    )
                    .changed();
                if ui
                    .add_enabled(
                        self.loudness_result.is_none(),
                        egui::Button::new("Medir LUFS"),
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
                        ui.small(
                            "Medición vigente: la exportación normalizará con precisión (dos pasos).",
                        );
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
            });
            ui.collapsing("Subtítulos", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("+ Subtítulo aquí").clicked() {
                        self.add_subtitle();
                    }
                    if ui.button("Exportar SRT").clicked() {
                        self.export_srt();
                    }
                    if ui.button("Importar SRT").clicked() {
                        self.import_srt();
                    }
                    if ui
                        .add_enabled(
                            self.transcription_result.is_none(),
                            egui::Button::new("Transcribir con Whisper"),
                        )
                        .clicked()
                    {
                        self.transcribe_with_whisper();
                    }
                    burn_changed |= ui
                        .checkbox(&mut self.burn_subtitles, "Quemar en vídeo")
                        .changed();
                });
                ui.collapsing("Estilo", |ui| {
                    let style = self
                        .project
                        .subtitle_style
                        .get_or_insert_with(SubtitleStyle::default);
                    metadata_changed |= ui
                        .add(egui::Slider::new(&mut style.size, 12.0..=160.0).text("Tamaño"))
                        .changed();
                    metadata_changed |= ui
                        .add(
                            egui::Slider::new(&mut style.position_y, 0.0..=1.0)
                                .text("Posición vertical"),
                        )
                        .changed();
                    metadata_changed |= ui
                        .add(egui::Slider::new(&mut style.red, 0.0..=1.0).text("Rojo"))
                        .changed();
                    metadata_changed |= ui
                        .add(egui::Slider::new(&mut style.green, 0.0..=1.0).text("Verde"))
                        .changed();
                    metadata_changed |= ui
                        .add(egui::Slider::new(&mut style.blue, 0.0..=1.0).text("Azul"))
                        .changed();
                });
                let mut subtitle_delete: Option<usize> = None;
                let mut subtitle_jump: Option<f64> = None;
                for (subtitle_index, subtitle) in self.project.subtitles.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:02}", subtitle_index + 1));
                        metadata_changed |= ui
                            .add(
                                egui::DragValue::new(&mut subtitle.start)
                                    .speed(0.05)
                                    .suffix(" s"),
                            )
                            .changed();
                        ui.label("→");
                        metadata_changed |= ui
                            .add(
                                egui::DragValue::new(&mut subtitle.end)
                                    .speed(0.05)
                                    .suffix(" s"),
                            )
                            .changed();
                        metadata_changed |= ui
                            .add_sized(
                                [180.0, 18.0],
                                egui::TextEdit::singleline(&mut subtitle.text)
                                    .hint_text("Texto del subtítulo"),
                            )
                            .changed();
                        subtitle.start = subtitle.start.max(0.0);
                        subtitle.end = subtitle.end.max(subtitle.start + 0.04);
                        if ui.button("Ir").clicked() {
                            subtitle_jump = Some(subtitle.start);
                        }
                        if ui.button("×").clicked() {
                            subtitle_delete = Some(subtitle_index);
                        }
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
            });
            ui.label("Planos del montaje");
            ui.add_space(10.0);

            enum Action {
                Up(usize),
                Down(usize),
                Remove(usize),
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
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        selected,
                                        format!("{:02}  {}", index + 1, clip.name()),
                                    )
                                    .clicked()
                                {
                                    self.selected = Some(index);
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Eliminar").clicked() {
                                            action = Some(Action::Remove(index));
                                        }
                                        if ui
                                            .add_enabled(
                                                clip.track > 0,
                                                egui::Button::new("▼ Bajar pista"),
                                            )
                                            .on_hover_text("Mueve el clip a la pista inferior")
                                            .clicked()
                                        {
                                            action = Some(Action::Down(index));
                                        }
                                        if ui
                                            .add_enabled(
                                                clip.track < 15,
                                                egui::Button::new("▲ Subir pista"),
                                            )
                                            .on_hover_text("Mueve el clip a la pista superior")
                                            .clicked()
                                        {
                                            action = Some(Action::Up(index));
                                        }
                                        ui.label(format!(
                                            "{}{} @ {:.2}s | {:.2}s",
                                            if clip.has_video { "V" } else { "A" },
                                            clip.track + 1,
                                            clip.timeline_start,
                                            clip.duration()
                                        ));
                                    },
                                );
                            });
                        });
                    ui.add_space(5.0);
                }
            });
            match action {
                Some(Action::Up(index)) if self.project.clips[index].track < 15 => {
                    let before = self.project.clone();
                    self.project.clips[index].track += 1;
                    self.selected = Some(index);
                    self.finish_edit(before);
                }
                Some(Action::Down(index)) if self.project.clips[index].track > 0 => {
                    let before = self.project.clone();
                    self.project.clips[index].track -= 1;
                    self.selected = Some(index);
                    self.finish_edit(before);
                }
                Some(Action::Remove(index)) => {
                    let before = self.project.clone();
                    self.project.clips.remove(index);
                    self.selected = None;
                    self.finish_edit(before);
                }
                _ => {}
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
                                    format_seconds((eta * 10.0).round() / 10.0)
                                ))
                                .size(10.0)
                                .color(theme::TEXT_DIM),
                            );
                        }
                        if theme::bar_button(ui, "Cancelar").clicked() {
                            if let Some(cancel) = &self.export_cancel {
                                cancel.store(true, Ordering::Relaxed);
                                self.status = "Cancelando exportacion...".to_owned();
                            }
                        }
                        ui.separator();
                    }
                    let failed = self.status.contains("FALLO") || self.status.contains("error");
                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(10.5)
                            .color(if failed { theme::WARN } else { theme::TEXT_DIM }),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let total = self.project.duration();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} clips · {:.1} s",
                                self.project.clips.len(),
                                total
                            ))
                            .size(10.0)
                            .color(theme::TEXT_FAINT),
                        );
                        ui.label(
                            egui::RichText::new("FFmpeg · Windows x64")
                                .size(9.0)
                                .color(theme::TEXT_FAINT),
                        );
                        let offline = self.missing_media_indices().len();
                        if offline > 0 {
                            ui.label(
                                egui::RichText::new(format!("{offline} medio(s) OFFLINE"))
                                    .size(10.0)
                                    .strong()
                                    .color(theme::DANGER),
                            );
                        }
                    });
                });
            });
    }
}

fn probe_media(path: &Path) -> Result<(f64, bool, bool), String> {
    let duration = Command::new(tool_path("ffprobe.exe"))
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("FFprobe no esta disponible: {error}"))?;
    if !duration.status.success() {
        return Err(String::from_utf8_lossy(&duration.stderr).trim().to_owned());
    }
    let seconds = String::from_utf8_lossy(&duration.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| "FFprobe no pudo leer la duracion".to_owned())?;
    let has_stream = |selector: &str| -> Result<bool, String> {
        let output = Command::new(tool_path("ffprobe.exe"))
            .args([
                "-v",
                "error",
                "-select_streams",
                selector,
                "-show_entries",
                "stream=index",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| error.to_string())?;
        Ok(output.status.success() && !output.stdout.is_empty())
    };
    Ok((seconds, has_stream("v:0")?, has_stream("a:0")?))
}

fn render_preview_frame(sources: &[(RoughClip, f64)]) -> Result<PreviewFrame, String> {
    const WIDTH: usize = 640;
    const HEIGHT: usize = 360;
    let mut command = Command::new(tool_path("ffmpeg.exe"));
    command.args(["-v", "error"]);
    let mut is_title_input = Vec::with_capacity(sources.len());
    for (clip, source_time) in sources {
        if clip.title.is_some() || clip.is_adjustment {
            command.args(["-f", "lavfi", "-i", "color=c=black@0.0:s=640x360:r=30"]);
            is_title_input.push(true);
        } else {
            command
                .args(["-ss", &format_seconds(*source_time), "-i"])
                .arg(&clip.path);
            is_title_input.push(false);
        }
    }
    let mut filters = vec![format!("color=c=black:s={WIDTH}x{HEIGHT}:r=30:d=0.1[base]")];
    for (index, (clip, _)) in sources.iter().enumerate() {
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
            let text = escape_drawtext(&title.text);
            // El tamano se define sobre 1080p y se escala al monitor.
            let fontsize = title.size.max(8.0) * HEIGHT as f64 / 1080.0;
            let fontcolor = hex_color(title.red, title.green, title.blue);
            filters.push(format!(
                "[{index}:v:0]drawtext=fontfile='{font}':text='{text}':fontsize={fontsize}:fontcolor=0x{fontcolor}:x=W*{px:.4}-text_w/2:y=H*{py:.4}-text_h/2,format=rgba,rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none,colorchannelmixer=aa={opacity:.6}[pv{index}]",
                font = escape_filter_path(&font),
                px = title.position_x.clamp(0.0, 1.0),
                py = title.position_y.clamp(0.0, 1.0),
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
                "[{index}:v:0]scale={width}:{height}:force_original_aspect_ratio={scale_mode}{crop}{wheels}{curves}{lut}{eq}{vig}{blur},format=rgba{chroma}{mask},rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none{blend_canvas},colorchannelmixer=aa={opacity:.6}[pv{index}]"
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
            filters.push(format!(
                "[padjsrc{index}]null{wheels}{curves}{lut}{eq}{vig}{blur},format=rgba,geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='{alpha_expr}'[padjfx{index}]"
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
                &format!("color=c=black@0.0:s={}x{}:r=30", size.0, size.1),
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
            command
                .args([
                    "-ss",
                    &format_seconds(clip.in_seconds),
                    "-t",
                    &format_seconds(clip.source_duration()),
                    "-i",
                ])
                .arg(path);
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
    measured_loudness: Option<&LoudnessReport>,
) -> Result<Vec<String>, String> {
    let (out_w, out_h) = size;
    let mut filters = Vec::new();
    let total = clips
        .iter()
        .map(|clip| clip.timeline_start + clip.duration())
        .fold(0.0, f64::max);
    if include_video {
        filters.push(format!(
            "color=c=black:s={out_w}x{out_h}:r=30:d={total:.6}[base]"
        ));
    }
    let mut audio_inputs = String::new();
    let mut audio_count = 0;
    for (index, media) in input_indices.iter().enumerate() {
        let speed = clips[index].speed.clamp(0.1, 8.0);
        let start = clips[index].timeline_start.max(0.0);
        if clips[index].has_video && include_video {
            let angle = clips[index].rotation.to_radians();
            let opacity = (clips[index].opacity / 100.0).clamp(0.0, 1.0);
            let (fade_in, fade_out) = clips[index].effective_fades();
            let duration = clips[index].duration();
            let mut fade_filters = String::new();
            if fade_in > 0.004 {
                fade_filters.push_str(&format!(",fade=t=in:st=0:d={fade_in:.3}"));
            }
            if fade_out > 0.004 {
                fade_filters.push_str(&format!(
                    ",fade=t=out:st={:.3}:d={fade_out:.3}",
                    (duration - fade_out)
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
                let text = escape_drawtext(&title.text);
                let fontsize = title.size.max(8.0);
                let fontcolor = hex_color(title.red, title.green, title.blue);
                filters.push(format!(
                    "[{media}:v:0]drawtext=fontfile='{font}':text='{text}':fontsize={fontsize}:fontcolor=0x{fontcolor}:x=W*{px:.4}-text_w/2:y=H*{py:.4}-text_h/2,format=rgba,setpts=(PTS-STARTPTS)+{start:.6}/TB,rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none,colorchannelmixer=aa={opacity:.6}{fade_filters}[v{index}]",
                    font = escape_filter_path(&font),
                    px = title.position_x.clamp(0.0, 1.0),
                    py = title.position_y.clamp(0.0, 1.0),
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
                filters.push(format!(
                    "[{media}:v:0]setpts=(PTS-STARTPTS)/{speed:.6}+{start:.6}/TB,scale={width}:{height}:force_original_aspect_ratio={scale_mode}{crop},setsar=1,fps=30{wheels}{curves}{lut}{eq}{vig}{blur},format=rgba{chroma}{mask},rotate={angle:.8}:ow=rotw(iw):oh=roth(ih):c=none{blend_canvas},colorchannelmixer=aa={opacity:.6}{fade_filters}[v{index}]"
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
            filters.push(format!(
                "[{media}:a:0]asetpts=PTS-STARTPTS,{},aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo{pan_filter},volume={volume:.8},adelay={delay_ms}|{delay_ms}{afade_filters}[a{index}]",
                atempo_filter(speed)
            ));
            audio_inputs.push_str(&format!("[a{index}]"));
            audio_count += 1;
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
            filters.push(format!(
                "[adjsrc{layer}]null{wheels}{curves}{lut}{eq}{vig}{blur},format=rgba,geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='{alpha_expr}'[adjfx{layer}]"
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
        if audio_count == 0 {
            filters.push(format!(
                "anullsrc=r=48000:cl=stereo:d={total:.6}{mastering}[aout]"
            ));
        } else if audio_count == 1 {
            filters.push(format!("{audio_inputs}anull{mastering}[aout]"));
        } else {
            filters.push(format!(
                "{audio_inputs}amix=inputs={audio_count}:duration=longest:normalize=0{mastering}[aout]"
            ));
        }
    }
    Ok(filters)
}

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
    measured_loudness: Option<&LoudnessReport>,
    progress: &Arc<std::sync::Mutex<RenderProgress>>,
) -> Result<(), String> {
    let prepared = prepare_render_clips(clips);
    let clips: &[RoughClip] = &prepared;
    if let Some(missing) = clips
        .iter()
        .find_map(|clip| clip.lut.as_deref().filter(|path| !path.is_file()))
    {
        return Err(format!(
            "LUT no encontrada, exportacion cancelada: {}",
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
    let (input_indices, is_title_input) = push_render_inputs(&mut command, clips, size, false);
    let filters = build_render_filters(
        clips,
        &input_indices,
        &is_title_input,
        size,
        !audio_only,
        true,
        track_gains,
        master_gain_db,
        normalize_loudness,
        measured_loudness,
    )?;
    let total = clips
        .iter()
        .map(|clip| clip.timeline_start + clip.duration())
        .fold(0.0, f64::max);

    let log_file = std::fs::File::create(&error_log)
        .map_err(|error| format!("No se pudo crear el registro de exportacion: {error}"))?;
    let child = command.args(["-filter_complex", &filters.join(";")]);
    if !audio_only {
        child.args(["-map", "[vout]"]);
    }
    child
        .args(["-map", "[aout]"])
        .args(["-progress", "pipe:1", "-nostats"]);
    if audio_only {
        match format {
            ExportFormat::WavAudio => child.args(["-c:a", "pcm_s16le"]),
            ExportFormat::Mp3Audio => child.args(["-c:a", "libmp3lame", "-b:a", "192k"]),
            ExportFormat::Mp4Video => unreachable!(),
        };
    } else {
        child.args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            if fast { "ultrafast" } else { "medium" },
            "-crf",
            if fast { "28" } else { "18" },
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
        ]);
    }
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
            return Err("Exportacion cancelada".to_owned());
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
            .map_err(|error| format!("No se pudo instalar la exportacion terminada: {error}"))
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
            path.is_absolute()
                || Command::new("where.exe")
                    .arg(name)
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

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NovaCut Windows")
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([900.0, 560.0]),
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
    pub const TEXT_DIM: Color32 = Color32::from_rgb(160, 160, 160);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(120, 120, 120);

    pub const OK: Color32 = Color32::from_rgb(90, 220, 120);
    pub const WARN: Color32 = Color32::from_rgb(246, 140, 40);
    pub const DANGER: Color32 = Color32::from_rgb(246, 83, 83);

    pub const R: CornerRadius = CornerRadius::same(5);

    /// Aplica el tema completo al contexto. Sustituye al antiguo
    /// `Visuals::dark()` con retoques sueltos.
    pub fn apply(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = Color32::from_rgb(26, 26, 26);
        v.extreme_bg_color = Color32::from_rgb(14, 14, 15);
        v.faint_bg_color = Color32::from_rgb(28, 28, 30);
        v.hyperlink_color = ACCENT;
        v.window_stroke = Stroke::NONE;
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
        s.button_padding = Vec2::new(10.0, 4.0);
        s.interact_size = Vec2::new(40.0, 22.0);
        s.indent = 18.0;
        s.slider_width = 130.0;
        s.slider_rail_height = 4.0;
        s.window_margin = Margin::same(10);
        s.menu_margin = Margin::same(6);
        s.combo_width = 140.0;
        style.animation_time = 0.1;

        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::proportional(15.0),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(12.5));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(10.5));
        ctx.set_style(style);
    }

    /// Marca de la app: cuadrado redondeado con la N, sustituye al icono real.
    pub fn logo_mark(ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, ACCENT_DIM);
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, ACCENT), egui::StrokeKind::Inside);
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
            egui::Button::new(RichText::new(label).size(11.0).color(Color32::from_rgb(8, 24, 27)))
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
            [Pos2::new(rect.left(), rect.bottom()), Pos2::new(rect.right(), rect.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(14, 14, 14)),
        );
        painter.text(
            Pos2::new(rect.left() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            title.to_uppercase(),
            FontId::proportional(10.0),
            TEXT,
        );
        if let Some(count) = count {
            let title_width = painter
                .layout_no_wrap(title.to_uppercase(), FontId::proportional(10.0), TEXT)
                .size()
                .x;
            painter.text(
                Pos2::new(rect.left() + 12.0 + title_width + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                count.to_string(),
                FontId::proportional(10.0),
                TEXT_FAINT,
            );
        }
        ui.add_space(6.0);
    }

    /// Etiqueta de sección dentro del inspector, al estilo de los títulos
    /// pequeños de la app Mac.
    pub fn section_label(ui: &mut Ui, title: &str) {
        ui.add_space(2.0);
        ui.label(RichText::new(title.to_uppercase()).size(9.5).strong().color(TEXT_FAINT));
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

    #[test]
    fn clip_duration_never_becomes_negative() {
        let clip = RoughClip {
            path: PathBuf::from("plan.mp4"),
            in_seconds: 8.0,
            out_seconds: 3.0,
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
            nested: None,
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
                nested: None,
            }],
            markers: vec![],
            subtitles: vec![],
            subtitle_style: None,
            track_gains: vec![],
            master_gain_db: 0.0,
            normalize_loudness: false,
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
    }

    #[test]
    fn fades_never_exceed_half_clip() {
        let mut clip = RoughClip {
            path: PathBuf::from("plan.mp4"),
            in_seconds: 0.0,
            out_seconds: 4.0,
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
            nested: None,
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
            nested: None,
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
        let (indices, titles) =
            push_render_inputs(&mut command, &[clip.clone()], (1920, 1080), false);
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
        let (indices, titles) = push_render_inputs(&mut command, &clips, (1920, 1080), false);
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
        let mut project = RoughProject::default();
        project.clips = vec![
            RoughClip {
                path: PathBuf::from("base.mp4"),
                in_seconds: 0.0,
                out_seconds: 10.0,
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
                nested: None,
            },
            RoughClip {
                path: PathBuf::from("overlay.mp4"),
                in_seconds: 0.0,
                out_seconds: 2.0,
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
                nested: None,
            },
        ];
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
                }),
                ..Default::default()
            }],
            markers: vec![],
            subtitles: vec![],
            subtitle_style: None,
            track_gains: vec![],
            master_gain_db: 0.0,
            normalize_loudness: false,
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
            track_gains: vec![],
            master_gain_db: 0.0,
            normalize_loudness: false,
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
            track_gains: vec![],
            master_gain_db: 0.0,
            normalize_loudness: false,
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
        assert!(filter.contains("master='0.0000/0.0500 1.0000/1.0000'"));
        assert!(!filter.contains("r='"));
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
}
