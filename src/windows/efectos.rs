//! Efectos por clip al estilo del panel «Controles de efectos» / Lumetri /
//! «Sonido esencial» de Premiere, expresados como cadenas de filtros FFmpeg.
//!
//! Todo lo que hay aquí es lógica pura: cada función devuelve el fragmento de
//! filtro (vacío cuando el efecto está en su valor neutro) y el host lo
//! intercala en la cadena de cada clip. Un proyecto sin efectos produce
//! exactamente el mismo grafo que antes de existir este módulo.

use eframe::egui;
use serde::{Deserialize, Serialize};

/// Papel de un clip en la mezcla, como las etiquetas de «Sonido esencial».
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioRole {
    #[default]
    Ninguno,
    Dialogo,
    Musica,
}

/// Punto de la «banda elástica» de volumen: tiempo local al clip (segundos
/// de timeline desde su inicio) y ganancia relativa en dB.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolumeKey {
    pub t: f64,
    pub db: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipFx {
    // --- Transformar / distorsionar
    pub flip_h: bool,
    pub flip_v: bool,
    /// Recorte por borde como fracción del lado (0 = nada, 0.5 = mitad).
    /// Como el efecto «Recortar» de Premiere, deja transparente lo recortado.
    pub crop_left: f64,
    pub crop_right: f64,
    pub crop_top: f64,
    pub crop_bottom: f64,
    /// Reproduce el clip hacia atrás (vídeo y audio).
    pub reverse: bool,
    /// Estabilizador de un paso (el «Warp Stabilizer» barato).
    pub stabilize: bool,
    // --- Lumetri básico
    /// -1 frío … +1 cálido.
    pub temperature: f64,
    /// -1 verde … +1 magenta.
    pub tint: f64,
    /// -1 … +1, satura sobre todo los tonos poco saturados.
    pub vibrance: f64,
    /// -1 … +1, levanta o hunde las sombras sin tocar las luces.
    pub shadows: f64,
    /// -1 … +1, recupera o empuja las altas luces.
    pub highlights: f64,
    // --- Detalle y textura
    /// 0 … 1, máscara de enfoque.
    pub sharpen: f64,
    /// 0 … 1, reducción de ruido espacio-temporal.
    pub denoise: f64,
    /// 0 … 1, grano de película animado.
    pub grain: f64,
    // --- Audio
    pub eq_low_db: f64,
    pub eq_mid_db: f64,
    pub eq_high_db: f64,
    /// 0 … 1, «Mejorar voz»: paso alto + reducción de ruido de fondo.
    pub voice_cleanup: f64,
    /// Compresor suave para igualar el nivel de la voz.
    pub compressor: bool,
    pub role: AudioRole,
    /// Atenuación deseada (dB, negativa) de la música bajo el diálogo.
    pub duck_db: f64,
    /// Banda elástica de volumen; vacía = ganancia fija.
    pub volume_keys: Vec<VolumeKey>,
}

impl ClipFx {
    pub fn has_video_effects(&self) -> bool {
        self.flip_h
            || self.flip_v
            || self.crop_left > 0.0
            || self.crop_right > 0.0
            || self.crop_top > 0.0
            || self.crop_bottom > 0.0
            || self.reverse
            || self.stabilize
            || !self.video_color_chain().is_empty()
    }

    /// Filtros temporales que deben ir al principio de la cadena, justo tras
    /// leer el medio y antes de `setpts`. `reverse` necesita la entrada
    /// acotada (el host siempre la lee con `-ss`/`-t`), y no tiene sentido
    /// sobre un fotograma congelado.
    pub fn input_prefix(&self, frozen: bool) -> String {
        if self.reverse && !frozen {
            "reverse,".to_owned()
        } else {
            String::new()
        }
    }

    /// Geometría previa al escalado: estabilizar y voltear.
    pub fn geometry_chain(&self, allow_temporal: bool) -> String {
        let mut out = String::new();
        if self.stabilize && allow_temporal {
            out.push_str(",deshake=rx=32:ry=32:edge=mirror");
        }
        if self.flip_h {
            out.push_str(",hflip");
        }
        if self.flip_v {
            out.push_str(",vflip");
        }
        out
    }

    /// Lumetri básico, detalle y textura. Va después de ruedas/curvas/LUT/eq
    /// del host y antes de pasar a RGBA.
    pub fn video_color_chain(&self) -> String {
        let mut out = String::new();
        if self.denoise > 0.001 {
            let luma = self.denoise.clamp(0.0, 1.0) * 8.0;
            out.push_str(&format!(
                ",hqdn3d={luma:.3}:{:.3}:{:.3}:{:.3}",
                luma * 0.75,
                luma * 1.5,
                luma * 1.1
            ));
        }
        if self.temperature.abs() > 0.001 {
            let kelvin = 6500.0 - self.temperature.clamp(-1.0, 1.0) * 2500.0;
            out.push_str(&format!(",colortemperature=temperature={kelvin:.0}"));
        }
        if self.tint.abs() > 0.001 {
            let tint = self.tint.clamp(-1.0, 1.0);
            let green = 1.0 - tint * 0.15;
            let other = 1.0 + tint * 0.05;
            out.push_str(&format!(
                ",colorchannelmixer=rr={other:.4}:gg={green:.4}:bb={other:.4}"
            ));
        }
        if self.vibrance.abs() > 0.001 {
            out.push_str(&format!(
                ",vibrance=intensity={:.4}",
                self.vibrance.clamp(-1.0, 1.0)
            ));
        }
        if self.shadows.abs() > 0.001 || self.highlights.abs() > 0.001 {
            let shadow = (0.25 + self.shadows.clamp(-1.0, 1.0) * 0.12).clamp(0.02, 0.6);
            let light = (0.75 + self.highlights.clamp(-1.0, 1.0) * 0.12).clamp(0.4, 0.98);
            out.push_str(&format!(
                ",curves=all='0/0 0.25/{shadow:.4} 0.75/{light:.4} 1/1'"
            ));
        }
        if self.sharpen > 0.001 {
            out.push_str(&format!(
                ",unsharp=5:5:{:.3}:5:5:0",
                self.sharpen.clamp(0.0, 1.0) * 2.0
            ));
        }
        if self.grain > 0.001 {
            out.push_str(&format!(
                ",noise=alls={:.0}:allf=t+u",
                self.grain.clamp(0.0, 1.0) * 40.0
            ));
        }
        out
    }

    /// Recorte con transparencia; requiere que la cadena ya esté en RGBA.
    pub fn alpha_chain(&self) -> String {
        let mut out = String::new();
        let edges = [
            (self.crop_left, "0", "0", "iw*{v}", "ih"),
            (self.crop_right, "iw-iw*{v}", "0", "iw*{v}", "ih"),
            (self.crop_top, "0", "0", "iw", "ih*{v}"),
            (self.crop_bottom, "0", "ih-ih*{v}", "iw", "ih*{v}"),
        ];
        for (value, x, y, w, h) in edges {
            if value <= 0.0005 {
                continue;
            }
            let v = format!("{:.4}", value.clamp(0.0, 1.0));
            out.push_str(&format!(
                ",drawbox=x={}:y={}:w={}:h={}:color=black@0:t=fill:replace=1",
                x.replace("{v}", &v),
                y.replace("{v}", &v),
                w.replace("{v}", &v),
                h.replace("{v}", &v)
            ));
        }
        out
    }

    /// Filtros de audio que van justo tras leer la pista del clip.
    pub fn audio_prefix(&self) -> String {
        if self.reverse {
            "areverse,".to_owned()
        } else {
            String::new()
        }
    }

    /// Reparación, EQ y dinámica («Sonido esencial»), antes de la ganancia.
    pub fn audio_chain(&self) -> String {
        let mut out = String::new();
        if self.voice_cleanup > 0.001 {
            let reduction = 6.0 + self.voice_cleanup.clamp(0.0, 1.0) * 24.0;
            out.push_str(&format!(
                ",highpass=f=80,afftdn=nr={reduction:.1}:nf=-40:tn=1"
            ));
        }
        if self.eq_low_db.abs() > 0.05 {
            out.push_str(&format!(
                ",bass=g={:.2}:f=120",
                self.eq_low_db.clamp(-24.0, 24.0)
            ));
        }
        if self.eq_mid_db.abs() > 0.05 {
            out.push_str(&format!(
                ",equalizer=f=1000:t=q:w=1:g={:.2}",
                self.eq_mid_db.clamp(-24.0, 24.0)
            ));
        }
        if self.eq_high_db.abs() > 0.05 {
            out.push_str(&format!(
                ",treble=g={:.2}:f=6000",
                self.eq_high_db.clamp(-24.0, 24.0)
            ));
        }
        if self.compressor {
            out.push_str(",acompressor=threshold=0.1:ratio=4:attack=5:release=120:makeup=2");
        }
        out
    }

    /// Banda elástica de volumen evaluada por fotograma de audio. `t` es el
    /// tiempo local del clip ya retemporizado, igual que en la timeline.
    pub fn volume_envelope(&self) -> String {
        let mut keys = self.volume_keys.clone();
        keys.retain(|key| key.t.is_finite() && key.db.is_finite());
        if keys.is_empty() {
            return String::new();
        }
        keys.sort_by(|left, right| left.t.total_cmp(&right.t));
        keys.dedup_by(|later, earlier| (later.t - earlier.t).abs() < 1e-6);
        let db = |key: &VolumeKey| key.db.clamp(-96.0, 24.0);
        // Se construye de derecha a izquierda: tras el último punto se
        // mantiene su valor; antes del primero, el del primero.
        let mut expression = format!("{:.3}", db(&keys[keys.len() - 1]));
        for pair in keys.windows(2).rev() {
            let (a, b) = (pair[0], pair[1]);
            let span = (b.t - a.t).max(1e-6);
            expression = format!(
                "if(lt(t,{bt:.4}),{ad:.3}+({bd:.3}-({ad:.3}))*(t-{at:.4})/{span:.4},{expression})",
                at = a.t,
                bt = b.t,
                ad = db(&a),
                bd = db(&b),
            );
        }
        expression = format!(
            "if(lt(t,{:.4}),{:.3},{expression})",
            keys[0].t,
            db(&keys[0])
        );
        format!(",volume=volume='pow(10,({expression})/20)':eval=frame")
    }

    /// Valor de la banda elástica en un instante, para dibujarla y para el
    /// monitor. Devuelve dB relativos.
    pub fn volume_at(&self, t: f64) -> f64 {
        let mut keys = self.volume_keys.clone();
        if keys.is_empty() {
            return 0.0;
        }
        keys.sort_by(|left, right| left.t.total_cmp(&right.t));
        if t <= keys[0].t {
            return keys[0].db;
        }
        for pair in keys.windows(2) {
            if t < pair[1].t {
                let mix = (t - pair[0].t) / (pair[1].t - pair[0].t).max(1e-6);
                return pair[0].db + (pair[1].db - pair[0].db) * mix;
            }
        }
        keys[keys.len() - 1].db
    }
}

/// Estilo de «Gráficos esenciales» de un título: caja de fondo, contorno,
/// sombra y relleno a pantalla completa (mate de color).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleStyle {
    /// Opacidad de la caja negra tras el texto (0 = sin caja).
    pub box_opacity: f64,
    /// Grosor del contorno negro en píxeles a 1080p (0 = sin contorno).
    pub outline: f64,
    pub shadow: bool,
    /// Rellena todo el lienzo con este color (RGB 0…1) bajo el texto.
    pub matte: Option<[f64; 3]>,
    /// Subtítulos animados palabra a palabra en lugar del texto fijo.
    pub captions: Option<super::subtitulos_animados::AnimatedCaptions>,
}

fn hex(rgb: [f64; 3]) -> String {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "{:02X}{:02X}{:02X}",
        channel(rgb[0]),
        channel(rgb[1]),
        channel(rgb[2])
    )
}

/// Filtros que dibujan un título sobre su lienzo transparente. `text` y
/// `font` llegan ya escapados; `scale` pasa medidas de 1080p al lienzo real.
pub fn title_drawing(
    style: &TitleStyle,
    text: &str,
    font: &str,
    fontsize: f64,
    fontcolor: &str,
    position: (f64, f64),
    scale: f64,
) -> String {
    let mut parts = Vec::new();
    if let Some(matte) = style.matte {
        parts.push(format!(
            "drawbox=x=0:y=0:w=iw:h=ih:color=0x{}@1:t=fill:replace=1",
            hex(matte)
        ));
    }
    if !text.trim().is_empty() {
        let mut extra = String::new();
        if style.box_opacity > 0.001 {
            extra.push_str(&format!(
                ":box=1:boxcolor=black@{:.3}:boxborderw={:.0}",
                style.box_opacity.clamp(0.0, 1.0),
                (fontsize * 0.35).max(2.0)
            ));
        }
        if style.outline > 0.01 {
            extra.push_str(&format!(
                ":borderw={:.0}:bordercolor=black",
                (style.outline * scale).max(1.0)
            ));
        }
        if style.shadow {
            let offset = (4.0 * scale).max(1.0);
            extra.push_str(&format!(
                ":shadowx={offset:.0}:shadowy={offset:.0}:shadowcolor=black@0.65"
            ));
        }
        parts.push(format!(
            "drawtext=fontfile='{font}':text='{text}':fontsize={fontsize}:fontcolor=0x{fontcolor}:x=W*{:.4}-text_w/2:y=H*{:.4}-text_h/2{extra}",
            position.0.clamp(0.0, 1.0),
            position.1.clamp(0.0, 1.0),
        ));
    }
    if parts.is_empty() {
        "null".to_owned()
    } else {
        parts.join(",")
    }
}

/// Catálogo de transiciones: identificador persistido y nombre visible.
/// `negro` y `dissolve` son las históricas y conservan su identificador.
pub const TRANSITIONS: &[(&str, &str)] = &[
    ("negro", "Pasar a negro"),
    ("blanco", "Pasar a blanco"),
    ("dissolve", "Disolución cruzada"),
    ("deslizar_izq", "Deslizar desde la derecha"),
    ("deslizar_der", "Deslizar desde la izquierda"),
    ("deslizar_arriba", "Deslizar desde abajo"),
    ("deslizar_abajo", "Deslizar desde arriba"),
    ("empujar_izq", "Empujar a la izquierda"),
    ("empujar_der", "Empujar a la derecha"),
];

pub fn transition_label(id: &str) -> &'static str {
    TRANSITIONS
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, label)| *label)
        .unwrap_or("Pasar a negro")
}

/// Dirección de entrada de una transición de movimiento: el clip entrante
/// parte desplazado `(dx, dy)` lienzos y llega a su sitio. `push` indica que
/// el saliente también se desplaza (empujar) en vez de quedarse debajo.
pub fn motion_of(id: &str) -> Option<(f64, f64, bool)> {
    match id {
        "deslizar_izq" => Some((1.0, 0.0, false)),
        "deslizar_der" => Some((-1.0, 0.0, false)),
        "deslizar_arriba" => Some((0.0, 1.0, false)),
        "deslizar_abajo" => Some((0.0, -1.0, false)),
        "empujar_izq" => Some((1.0, 0.0, true)),
        "empujar_der" => Some((-1.0, 0.0, true)),
        _ => None,
    }
}

/// Estado derivado de las transiciones para un render concreto. No se
/// guarda en el proyecto: `resolve_render_clips` lo recalcula siempre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitionRuntime {
    /// Los fundidos del clip van a blanco en vez de a negro.
    pub white_in: bool,
    pub white_out: bool,
    /// Entrada en movimiento: (dx, dy, inicio en timeline, duración).
    pub slide_in: Option<(f64, f64, f64, f64)>,
    /// Salida empujada: (dx, dy, inicio en timeline, duración).
    pub slide_out: Option<(f64, f64, f64, f64)>,
    /// Fundidos solo de audio (la imagen entra en movimiento, sin alfa).
    pub audio_fade_in: f64,
    pub audio_fade_out: f64,
}

impl TransitionRuntime {
    /// Expresión de desplazamiento para `overlay` (en píxeles del lienzo),
    /// evaluada por fotograma sobre el tiempo de la composición.
    pub fn offset_expressions(&self) -> Option<(String, String)> {
        let term = |horizontal: bool| -> String {
            let pick = |dx: f64, dy: f64| if horizontal { dx } else { dy };
            let canvas = if horizontal { "W" } else { "H" };
            let mut parts = Vec::new();
            if let Some((dx, dy, start, d)) = self.slide_in {
                let delta = pick(dx, dy);
                if delta != 0.0 {
                    parts.push(format!(
                        "({delta:.1})*{canvas}*max(0,1-(t-{start:.6})/{d:.6})"
                    ));
                }
            }
            if let Some((dx, dy, start, d)) = self.slide_out {
                let delta = -pick(dx, dy);
                if delta != 0.0 {
                    parts.push(format!(
                        "({delta:.1})*{canvas}*min(1,max(0,(t-{start:.6})/{d:.6}))"
                    ));
                }
            }
            parts.join("+")
        };
        let (x, y) = (term(true), term(false));
        if x.is_empty() && y.is_empty() {
            None
        } else {
            Some((x, y))
        }
    }

    /// Desplazamiento en fracciones de lienzo en un instante de timeline,
    /// para el monitor (que compone un único fotograma).
    pub fn offset_at(&self, time: f64) -> (f64, f64) {
        let mut offset = (0.0, 0.0);
        if let Some((dx, dy, start, d)) = self.slide_in {
            let remaining = (1.0 - (time - start) / d.max(1e-6)).clamp(0.0, 1.0);
            offset.0 += dx * remaining;
            offset.1 += dy * remaining;
        }
        if let Some((dx, dy, start, d)) = self.slide_out {
            let done = ((time - start) / d.max(1e-6)).clamp(0.0, 1.0);
            offset.0 -= dx * done;
            offset.1 -= dy * done;
        }
        offset
    }
}

/// Una entrada de la mezcla: etiqueta del filtro, papel y atenuación.
pub struct MixInput {
    pub label: String,
    pub role: AudioRole,
    pub duck_db: f64,
}

/// Mezcla final con «ducking» automático: si hay diálogo y alguna música
/// pide atenuarse, la música pasa por un compresor cuya cadena lateral es el
/// propio diálogo. Sin eso, es el `amix` de siempre.
pub fn mix_audio(inputs: &[MixInput], mastering: &str, total: f64) -> Vec<String> {
    let mut filters = Vec::new();
    let mix = |labels: &[&str], output: &str| -> String {
        if labels.len() == 1 {
            format!("{}anull[{output}]", labels[0])
        } else {
            format!(
                "{}amix=inputs={}:duration=longest:normalize=0[{output}]",
                labels.concat(),
                labels.len()
            )
        }
    };
    if inputs.is_empty() {
        filters.push(format!(
            "anullsrc=r=48000:cl=stereo:d={total:.6}{mastering}[aout]"
        ));
        return filters;
    }
    let labels: Vec<String> = inputs
        .iter()
        .map(|input| format!("[{}]", input.label))
        .collect();
    let dialog: Vec<&str> = inputs
        .iter()
        .zip(&labels)
        .filter(|(input, _)| input.role == AudioRole::Dialogo)
        .map(|(_, label)| label.as_str())
        .collect();
    let ducked: Vec<(&MixInput, &str)> = inputs
        .iter()
        .zip(&labels)
        .filter(|(input, _)| input.role == AudioRole::Musica && input.duck_db < -0.5)
        .map(|(input, label)| (input, label.as_str()))
        .collect();
    if dialog.is_empty() || ducked.is_empty() {
        if labels.len() == 1 {
            filters.push(format!("{}anull{mastering}[aout]", labels[0]));
        } else {
            filters.push(format!(
                "{}amix=inputs={}:duration=longest:normalize=0{mastering}[aout]",
                labels.concat(),
                labels.len()
            ));
        }
        return filters;
    }
    let depth = ducked
        .iter()
        .map(|(input, _)| -input.duck_db)
        .fold(0.0, f64::max)
        .clamp(1.0, 40.0);
    // Con umbral bajo el diálogo siempre supera la rodilla; la relación fija
    // cuánto se hunde la música (≈ profundidad pedida para voz normal).
    let ratio = (1.0 + depth / 2.0).clamp(1.5, 20.0);
    let music_labels: Vec<&str> = ducked.iter().map(|(_, label)| *label).collect();
    filters.push(mix(&dialog, "duckdlg"));
    filters.push("[duckdlg]asplit=2[duckvoice][duckkey]".to_owned());
    filters.push(mix(&music_labels, "duckmus"));
    filters.push(format!(
        "[duckmus][duckkey]sidechaincompress=threshold=0.015:ratio={ratio:.2}:attack=40:release=450:makeup=1[duckout]"
    ));
    let mut rest: Vec<&str> = labels
        .iter()
        .map(String::as_str)
        .filter(|label| !dialog.contains(label) && !music_labels.contains(label))
        .collect();
    rest.push("[duckvoice]");
    rest.push("[duckout]");
    filters.push(format!(
        "{}amix=inputs={}:duration=longest:normalize=0{mastering}[aout]",
        rest.concat(),
        rest.len()
    ));
    filters
}

fn slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
) -> bool {
    let changed = ui
        .horizontal(|ui| {
            let slider = ui.add(egui::Slider::new(value, range).text(text));
            let reset = ui.small_button("↺").on_hover_text("Restablecer").clicked();
            if reset {
                *value = 0.0;
            }
            slider.changed() || reset
        })
        .inner;
    changed
}

/// Secciones de vídeo del inspector. Devuelve si algo cambió.
pub fn inspector_video(ui: &mut egui::Ui, fx: &mut ClipFx, is_adjustment: bool) -> bool {
    let mut changed = false;
    ui.collapsing("Lumetri básico", |ui| {
        changed |= slider(ui, &mut fx.temperature, -1.0..=1.0, "Temperatura");
        changed |= slider(ui, &mut fx.tint, -1.0..=1.0, "Tinte");
        changed |= slider(ui, &mut fx.vibrance, -1.0..=1.0, "Intensidad");
        changed |= slider(ui, &mut fx.shadows, -1.0..=1.0, "Sombras");
        changed |= slider(ui, &mut fx.highlights, -1.0..=1.0, "Iluminaciones");
    });
    ui.collapsing("Detalle y textura", |ui| {
        changed |= slider(ui, &mut fx.sharpen, 0.0..=1.0, "Enfocar");
        changed |= slider(ui, &mut fx.denoise, 0.0..=1.0, "Reducir ruido");
        changed |= slider(ui, &mut fx.grain, 0.0..=1.0, "Grano");
    });
    if !is_adjustment {
        ui.collapsing("Transformar y recortar", |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut fx.flip_h, "Voltear horizontal")
                    .on_hover_text("Espejo izquierda-derecha")
                    .changed();
                changed |= ui
                    .checkbox(&mut fx.flip_v, "Voltear vertical")
                    .on_hover_text("Espejo arriba-abajo")
                    .changed();
            });
            let mut percent = |ui: &mut egui::Ui, value: &mut f64, text: &str| {
                let mut shown = *value * 100.0;
                let response = ui.add(
                    egui::Slider::new(&mut shown, 0.0..=50.0)
                        .suffix(" %")
                        .text(text),
                );
                if response.changed() {
                    *value = shown / 100.0;
                    changed = true;
                }
            };
            percent(ui, &mut fx.crop_left, "Recorte izq.");
            percent(ui, &mut fx.crop_right, "Recorte der.");
            percent(ui, &mut fx.crop_top, "Recorte sup.");
            percent(ui, &mut fx.crop_bottom, "Recorte inf.");
        });
        ui.collapsing("Tiempo y estabilización", |ui| {
            changed |= ui
                .checkbox(&mut fx.reverse, "Reproducir hacia atrás")
                .on_hover_text("Invierte imagen y sonido del recorte. Conviene en clips cortos: FFmpeg carga el tramo entero en memoria.")
                .changed();
            changed |= ui
                .checkbox(&mut fx.stabilize, "Estabilizar (deshake)")
                .on_hover_text("Compensa la vibración de cámara en mano. Se aplica en la exportación y en la reproducción; el fotograma fijo del monitor no puede mostrarlo.")
                .changed();
        });
    }
    changed
}

/// Sección «Sonido esencial» del inspector. `duration` y `local_playhead`
/// permiten añadir puntos de volumen en el cabezal.
pub fn inspector_audio(
    ui: &mut egui::Ui,
    fx: &mut ClipFx,
    duration: f64,
    local_playhead: Option<f64>,
) -> bool {
    let mut changed = false;
    ui.collapsing("Sonido esencial", |ui| {
        ui.horizontal(|ui| {
            ui.label("Tipo");
            changed |= ui.radio_value(&mut fx.role, AudioRole::Ninguno, "—").changed();
            changed |= ui.radio_value(&mut fx.role, AudioRole::Dialogo, "Diálogo").changed();
            changed |= ui.radio_value(&mut fx.role, AudioRole::Musica, "Música").changed();
        });
        match fx.role {
            AudioRole::Dialogo => {
                changed |= slider(ui, &mut fx.voice_cleanup, 0.0..=1.0, "Reducir ruido de fondo");
                changed |= ui.checkbox(&mut fx.compressor, "Igualar nivel (compresor)").on_hover_text("Sube las partes flojas de la voz y baja las fuertes").changed();
            }
            AudioRole::Musica => {
                let mut ducking = fx.duck_db < -0.5;
                if ui
                    .checkbox(&mut ducking, "Atenuar bajo el diálogo (ducking)").on_hover_text("La música baja sola mientras hay voz")
                    .changed()
                {
                    fx.duck_db = if ducking { -12.0 } else { 0.0 };
                    changed = true;
                }
                if ducking {
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut fx.duck_db, -30.0..=-2.0)
                                .suffix(" dB")
                                .text("Atenuación"),
                        )
                        .changed();
                }
            }
            AudioRole::Ninguno => {
                ui.label(
                    egui::RichText::new(
                        "Marca los clips de voz como Diálogo y la música como Música para mezclar con ducking.",
                    )
                    .size(11.5)
                    .weak(),
                );
            }
        }
    });
    ui.collapsing("Ecualizador y dinámica", |ui| {
        changed |= slider(ui, &mut fx.eq_low_db, -18.0..=18.0, "Graves (dB)");
        changed |= slider(ui, &mut fx.eq_mid_db, -18.0..=18.0, "Medios (dB)");
        changed |= slider(ui, &mut fx.eq_high_db, -18.0..=18.0, "Agudos (dB)");
        if fx.role != AudioRole::Dialogo {
            changed |= slider(ui, &mut fx.voice_cleanup, 0.0..=1.0, "Reducir ruido");
            changed |= ui
                .checkbox(&mut fx.compressor, "Compresor")
                .on_hover_text("Iguala el nivel: sube lo flojo y baja lo fuerte")
                .changed();
        }
    });
    ui.collapsing(
        format!("Volumen animado ({} puntos)", fx.volume_keys.len()),
        |ui| {
            if let Some(local) = local_playhead.filter(|local| *local >= 0.0 && *local <= duration)
            {
                let current = fx.volume_at(local);
                if ui
                    .button(format!("+ Punto en el cabezal ({local:.2} s)"))
                    .clicked()
                {
                    fx.volume_keys.retain(|key| (key.t - local).abs() > 0.01);
                    fx.volume_keys.push(VolumeKey {
                        t: local,
                        db: current,
                    });
                    fx.volume_keys.sort_by(|a, b| a.t.total_cmp(&b.t));
                    changed = true;
                }
            } else {
                ui.label(
                    egui::RichText::new("Coloca el cabezal sobre el clip para añadir puntos.")
                        .size(11.5)
                        .weak(),
                );
            }
            let mut remove = None;
            for (index, key) in fx.volume_keys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut key.t)
                                .speed(0.02)
                                .range(0.0..=duration.max(0.0))
                                .suffix(" s"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut key.db)
                                .speed(0.2)
                                .range(-60.0..=12.0)
                                .suffix(" dB"),
                        )
                        .changed();
                    if ui
                        .small_button("×")
                        .on_hover_text("Quita este punto de volumen")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                fx.volume_keys.remove(index);
                changed = true;
            }
            if !fx.volume_keys.is_empty()
                && ui
                    .button("Quitar animación")
                    .on_hover_text("Borra todos los puntos de volumen")
                    .clicked()
            {
                fx.volume_keys.clear();
                changed = true;
            }
        },
    );
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_fx_adds_nothing() {
        let fx = ClipFx::default();
        assert!(!fx.has_video_effects());
        assert_eq!(fx.input_prefix(false), "");
        assert_eq!(fx.geometry_chain(true), "");
        assert_eq!(fx.video_color_chain(), "");
        assert_eq!(fx.alpha_chain(), "");
        assert_eq!(fx.audio_prefix(), "");
        assert_eq!(fx.audio_chain(), "");
        assert_eq!(fx.volume_envelope(), "");
    }

    #[test]
    fn old_projects_deserialize_without_fx() {
        let fx: ClipFx = serde_json::from_str("{}").unwrap();
        assert_eq!(fx, ClipFx::default());
        let partial: ClipFx = serde_json::from_str(r#"{"flip_h":true}"#).unwrap();
        assert!(partial.flip_h && !partial.flip_v);
    }

    #[test]
    fn reverse_skips_frozen_clips() {
        let fx = ClipFx {
            reverse: true,
            ..Default::default()
        };
        assert_eq!(fx.input_prefix(false), "reverse,");
        assert_eq!(fx.input_prefix(true), "");
        assert_eq!(fx.audio_prefix(), "areverse,");
    }

    #[test]
    fn stabilize_only_when_temporal_allowed() {
        let fx = ClipFx {
            stabilize: true,
            flip_h: true,
            ..Default::default()
        };
        assert_eq!(
            fx.geometry_chain(true),
            ",deshake=rx=32:ry=32:edge=mirror,hflip"
        );
        assert_eq!(fx.geometry_chain(false), ",hflip");
    }

    #[test]
    fn temperature_warms_by_lowering_kelvin() {
        let warm = ClipFx {
            temperature: 1.0,
            ..Default::default()
        };
        assert_eq!(
            warm.video_color_chain(),
            ",colortemperature=temperature=4000"
        );
        let cool = ClipFx {
            temperature: -1.0,
            ..Default::default()
        };
        assert_eq!(
            cool.video_color_chain(),
            ",colortemperature=temperature=9000"
        );
    }

    #[test]
    fn crop_edges_become_transparent_boxes() {
        let fx = ClipFx {
            crop_left: 0.1,
            crop_bottom: 0.25,
            ..Default::default()
        };
        assert_eq!(
            fx.alpha_chain(),
            ",drawbox=x=0:y=0:w=iw*0.1000:h=ih:color=black@0:t=fill:replace=1,drawbox=x=0:y=ih-ih*0.2500:w=iw:h=ih*0.2500:color=black@0:t=fill:replace=1"
        );
    }

    #[test]
    fn volume_envelope_is_piecewise_linear() {
        let fx = ClipFx {
            volume_keys: vec![
                VolumeKey { t: 2.0, db: -20.0 },
                VolumeKey { t: 1.0, db: 0.0 },
            ],
            ..Default::default()
        };
        assert_eq!(fx.volume_at(0.0), 0.0);
        assert_eq!(fx.volume_at(1.5), -10.0);
        assert_eq!(fx.volume_at(9.0), -20.0);
        assert_eq!(
            fx.volume_envelope(),
            ",volume=volume='pow(10,(if(lt(t,1.0000),0.000,if(lt(t,2.0000),0.000+(-20.000-(0.000))*(t-1.0000)/1.0000,-20.000)))/20)':eval=frame"
        );
    }

    #[test]
    fn mix_without_roles_is_plain_amix() {
        let inputs = vec![
            MixInput {
                label: "a0".into(),
                role: AudioRole::Ninguno,
                duck_db: 0.0,
            },
            MixInput {
                label: "a1".into(),
                role: AudioRole::Musica,
                duck_db: 0.0,
            },
        ];
        assert_eq!(
            mix_audio(&inputs, ",volume=1", 5.0),
            vec!["[a0][a1]amix=inputs=2:duration=longest:normalize=0,volume=1[aout]"]
        );
        assert_eq!(
            mix_audio(&[], "", 2.0),
            vec!["anullsrc=r=48000:cl=stereo:d=2.000000[aout]"]
        );
    }

    #[test]
    fn mix_ducks_music_under_dialogue() {
        let inputs = vec![
            MixInput {
                label: "a0".into(),
                role: AudioRole::Dialogo,
                duck_db: 0.0,
            },
            MixInput {
                label: "a1".into(),
                role: AudioRole::Musica,
                duck_db: -12.0,
            },
            MixInput {
                label: "a2".into(),
                role: AudioRole::Ninguno,
                duck_db: 0.0,
            },
        ];
        let filters = mix_audio(&inputs, "", 5.0);
        assert_eq!(filters[0], "[a0]anull[duckdlg]");
        assert_eq!(filters[1], "[duckdlg]asplit=2[duckvoice][duckkey]");
        assert_eq!(filters[2], "[a1]anull[duckmus]");
        assert!(filters[3].starts_with("[duckmus][duckkey]sidechaincompress="));
        assert!(filters[3].contains("ratio=7.00"));
        assert_eq!(
            filters[4],
            "[a2][duckvoice][duckout]amix=inputs=3:duration=longest:normalize=0[aout]"
        );
    }

    #[test]
    fn slide_offsets_reach_rest_at_end() {
        let runtime = TransitionRuntime {
            slide_in: Some((1.0, 0.0, 10.0, 1.0)),
            ..Default::default()
        };
        assert_eq!(runtime.offset_at(10.0), (1.0, 0.0));
        assert_eq!(runtime.offset_at(10.5), (0.5, 0.0));
        assert_eq!(runtime.offset_at(12.0), (0.0, 0.0));
        let (x, y) = runtime.offset_expressions().unwrap();
        assert_eq!(x, "(1.0)*W*max(0,1-(t-10.000000)/1.000000)");
        assert_eq!(y, "");
        let push = TransitionRuntime {
            slide_out: Some((1.0, 0.0, 10.0, 1.0)),
            ..Default::default()
        };
        assert_eq!(push.offset_at(10.5), (-0.5, 0.0));
    }

    #[test]
    fn title_drawing_keeps_plain_titles_identical() {
        let plain = title_drawing(
            &TitleStyle::default(),
            "Hola",
            "f.ttf",
            72.0,
            "FFFFFF",
            (0.5, 0.5),
            1.0,
        );
        assert_eq!(
            plain,
            "drawtext=fontfile='f.ttf':text='Hola':fontsize=72:fontcolor=0xFFFFFF:x=W*0.5000-text_w/2:y=H*0.5000-text_h/2"
        );
        let matte_only = TitleStyle {
            matte: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        };
        assert_eq!(
            title_drawing(&matte_only, "", "f.ttf", 72.0, "FFFFFF", (0.5, 0.5), 1.0),
            "drawbox=x=0:y=0:w=iw:h=ih:color=0xFF0000@1:t=fill:replace=1"
        );
        assert_eq!(
            title_drawing(&TitleStyle::default(), " ", "f", 9.0, "0", (0.0, 0.0), 1.0),
            "null"
        );
        let styled = TitleStyle {
            box_opacity: 0.5,
            outline: 3.0,
            shadow: true,
            ..Default::default()
        };
        let drawn = title_drawing(&styled, "A", "f", 40.0, "FFFFFF", (0.5, 0.9), 0.5);
        assert!(drawn.ends_with(":box=1:boxcolor=black@0.500:boxborderw=14:borderw=2:bordercolor=black:shadowx=2:shadowy=2:shadowcolor=black@0.65"));
    }

    #[test]
    fn transitions_catalog_keeps_legacy_ids() {
        assert_eq!(transition_label("negro"), "Pasar a negro");
        assert_eq!(transition_label("dissolve"), "Disolución cruzada");
        assert_eq!(motion_of("empujar_der"), Some((-1.0, 0.0, true)));
        assert_eq!(motion_of("dissolve"), None);
    }
}
