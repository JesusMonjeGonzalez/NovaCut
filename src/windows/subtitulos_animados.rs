//! Subtítulos animados al estilo CapCut / TikTok: páginas de pocas palabras
//! con la palabra que suena resaltada.
//!
//! La maquetación se hace aquí, midiendo con la misma fuente TTF que usará
//! `drawtext`, y cada palabra se dibuja con su propio `drawtext` centrado en
//! su hueco. Así el estado activo y el normal de una palabra ocupan
//! exactamente el mismo sitio y nunca se ven dobles; un error de medida solo
//! movería el espaciado.

use super::transcripcion::{pages, Word};
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Cuánto crece la palabra activa en el estilo Pop.
const POP_GROWTH: f64 = 1.18;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionPreset {
    /// Texto blanco con contorno; la palabra activa, en color.
    #[default]
    Clasico,
    /// Las palabras se van coloreando al decirse y así se quedan.
    Karaoke,
    /// La palabra activa lleva una caja de color detrás.
    Caja,
    /// La palabra activa crece y cambia de color.
    Pop,
}

impl CaptionPreset {
    pub const ALL: [CaptionPreset; 4] = [Self::Clasico, Self::Karaoke, Self::Caja, Self::Pop];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clasico => "Clásico (palabra en color)",
            Self::Karaoke => "Karaoke (se van coloreando)",
            Self::Caja => "Caja tras la palabra",
            Self::Pop => "Pop (la palabra crece)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimatedCaptions {
    /// Palabras en tiempo local del clip.
    pub words: Vec<Word>,
    pub preset: CaptionPreset,
    pub max_words: usize,
    pub uppercase: bool,
    /// Color de resaltado (RGB 0…1).
    pub highlight: [f64; 3],
    /// Se regenera sola desde la transcripción del proyecto tras cada
    /// edición por texto.
    pub follow_transcript: bool,
}

impl Default for AnimatedCaptions {
    fn default() -> Self {
        Self {
            words: Vec::new(),
            preset: CaptionPreset::Clasico,
            max_words: 4,
            uppercase: false,
            highlight: [1.0, 0.85, 0.1],
            follow_transcript: true,
        }
    }
}

/// Palabras del proyecto que caen en `[start, end)`, pasadas a tiempo local.
pub fn local_words(transcript: &[Word], start: f64, end: f64) -> Vec<Word> {
    transcript
        .iter()
        .filter(|word| word.start >= start - 0.001 && word.start < end)
        .map(|word| Word {
            start: word.start - start,
            end: (word.end - start).min(end - start),
            text: word.text.clone(),
        })
        .collect()
}

fn load_font(path: &Path) -> Option<FontArc> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, FontArc>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().ok()?;
    if let Some(font) = cache.get(path) {
        return Some(font.clone());
    }
    let bytes = std::fs::read(path).ok()?;
    let font = FontArc::try_from_vec(bytes).ok()?;
    cache.insert(path.to_path_buf(), font.clone());
    Some(font)
}

/// Ancho en píxeles de `text` con `drawtext` a `fontsize`, con kerning.
///
/// FreeType (el de drawtext) toma el tamaño como el em en píxeles, mientras
/// que `PxScale` de ab_glyph es el alto ascendente-descendente; sin esta
/// conversión, en Arial los anchos salían un 15 % cortos y las palabras se
/// pisaban.
fn text_width(font: &FontArc, size: f64, text: &str) -> f64 {
    let scale = match font.units_per_em() {
        Some(units) => PxScale::from(size as f32 * font.height_unscaled() / units),
        None => PxScale::from(size as f32),
    };
    let scaled = font.as_scaled(scale);
    let mut width = 0.0_f32;
    let mut previous = None;
    for character in text.chars() {
        let glyph = scaled.glyph_id(character);
        if let Some(previous) = previous {
            width += scaled.kern(previous, glyph);
        }
        width += scaled.h_advance(glyph);
        previous = Some(glyph);
    }
    width as f64
}

/// Una palabra ya colocada: centro horizontal y parte superior de su línea.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedWord {
    pub text: String,
    pub center_x: f64,
    pub top_y: f64,
    pub start: f64,
    pub end: f64,
    pub page_start: f64,
    pub page_end: f64,
}

/// Coloca todas las palabras: páginas de `max_words`, como mucho dos líneas
/// que no pasan del 86 % del ancho, centradas en `position_y` del lienzo.
pub fn layout(
    captions: &AnimatedCaptions,
    measure: &dyn Fn(&str) -> f64,
    space: f64,
    fontsize: f64,
    canvas: (u32, u32),
    position_y: f64,
) -> Vec<PlacedWord> {
    let (width, height) = (canvas.0 as f64, canvas.1 as f64);
    let max_line = width * 0.86;
    let line_height = fontsize * 1.25;
    let shown: Vec<String> = captions
        .words
        .iter()
        .map(|word| {
            if captions.uppercase {
                word.text.to_uppercase()
            } else {
                word.text.clone()
            }
        })
        .collect();
    let mut placed = Vec::new();
    let page_list = pages(&captions.words, 64, captions.max_words.clamp(1, 12));
    for (page_index, range) in page_list.iter().enumerate() {
        let page_start = captions.words[range.start].start;
        // La página se mantiene hasta que empieza la siguiente (si está
        // cerca), para que el texto no parpadee entre frases seguidas.
        let natural_end = captions.words[range.end - 1].end;
        let page_end = page_list
            .get(page_index + 1)
            .map(|next| captions.words[next.start].start)
            .filter(|next_start| next_start - natural_end < 0.6)
            .unwrap_or(natural_end + 0.15)
            .max(natural_end);
        let mut lines: Vec<Vec<(usize, f64)>> = vec![Vec::new()];
        let mut line_width = 0.0;
        for index in range.clone() {
            let word_width = measure(&shown[index]);
            let line_count = lines.len();
            let current = lines.last_mut().expect("siempre hay una línea");
            let extra = if current.is_empty() {
                word_width
            } else {
                space + word_width
            };
            if !current.is_empty() && line_width + extra > max_line && line_count < 2 {
                lines.push(vec![(index, word_width)]);
                line_width = word_width;
            } else {
                current.push((index, word_width));
                line_width += extra;
            }
        }
        let block_top = height * position_y - line_height * lines.len() as f64 / 2.0;
        for (line_index, line) in lines.iter().enumerate() {
            let total: f64 = line.iter().map(|(_, w)| w).sum::<f64>()
                + space * line.len().saturating_sub(1) as f64;
            let mut x = (width - total) / 2.0;
            for (index, word_width) in line {
                placed.push(PlacedWord {
                    text: shown[*index].clone(),
                    center_x: x + word_width / 2.0,
                    top_y: block_top + line_height * line_index as f64,
                    start: captions.words[*index].start,
                    end: captions.words[*index]
                        .end
                        .max(captions.words[*index].start + 0.05),
                    page_start,
                    page_end,
                });
                x += word_width + space;
            }
        }
    }
    placed
}

/// Estilo de un estado de palabra.
struct Look {
    color: String,
    size: f64,
    boxed: Option<String>,
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

/// Tramos de tiempo (inicio, fin).
type Spans = Vec<(f64, f64)>;

/// Ventanas en las que la palabra se ve normal y resaltada.
fn windows(word: &PlacedWord, preset: CaptionPreset) -> (Spans, Spans) {
    let active_end = word.end.min(word.page_end);
    match preset {
        CaptionPreset::Karaoke => (
            vec![(word.page_start, word.start)],
            vec![(word.start, word.page_end)],
        ),
        _ => (
            vec![(word.page_start, word.start), (active_end, word.page_end)],
            vec![(word.start, active_end)],
        ),
    }
}

/// Cadena de `drawtext` para todas las palabras. `text_escape` escapa el
/// texto para drawtext y `font` llega escapado para el grafo. Con
/// `preview_time` solo se dibuja lo visible en ese instante, sin `enable`.
#[allow(clippy::too_many_arguments)]
pub fn caption_filters(
    captions: &AnimatedCaptions,
    font_file: &Path,
    font: &str,
    text_escape: &dyn Fn(&str) -> String,
    fontsize: f64,
    base_color: &str,
    canvas: (u32, u32),
    position_y: f64,
    preview_time: Option<f64>,
) -> String {
    let Some(face) = load_font(font_file) else {
        return "null".to_owned();
    };
    let space = text_width(&face, fontsize, " ");
    // Pop agranda la palabra activa: se reserva ya su hueco ampliado para
    // que al crecer no pise a las vecinas.
    let reserve = if captions.preset == CaptionPreset::Pop {
        POP_GROWTH
    } else {
        1.0
    };
    let measure = |text: &str| text_width(&face, fontsize, text) * reserve;
    let placed = layout(captions, &measure, space, fontsize, canvas, position_y);
    let scale = canvas.1 as f64 / 1080.0;
    let outline = (3.0 * scale).max(1.0);
    let highlight = hex(captions.highlight);
    let normal = Look {
        color: base_color.to_owned(),
        size: fontsize,
        boxed: None,
    };
    let active = match captions.preset {
        CaptionPreset::Clasico | CaptionPreset::Karaoke => Look {
            color: highlight.clone(),
            size: fontsize,
            boxed: None,
        },
        CaptionPreset::Caja => Look {
            color: "FFFFFF".to_owned(),
            size: fontsize,
            boxed: Some(highlight.clone()),
        },
        CaptionPreset::Pop => Look {
            color: highlight.clone(),
            size: fontsize * POP_GROWTH,
            boxed: None,
        },
    };
    let mut filters = Vec::new();
    for word in &placed {
        let (normal_windows, active_windows) = windows(word, captions.preset);
        for (look, spans) in [(&normal, normal_windows), (&active, active_windows)] {
            let spans: Vec<(f64, f64)> = spans
                .into_iter()
                .filter(|(from, to)| to - from > 0.001)
                .collect();
            if spans.is_empty() {
                continue;
            }
            let enable = match preview_time {
                Some(time) => {
                    if !spans.iter().any(|(from, to)| time >= *from && time < *to) {
                        continue;
                    }
                    String::new()
                }
                None => format!(
                    ":enable='{}'",
                    spans
                        .iter()
                        .map(|(from, to)| format!("between(t,{from:.3},{:.3})", to - 0.001))
                        .collect::<Vec<_>>()
                        .join("+")
                ),
            };
            // Pop: la palabra crece desde su centro, sin mover la línea.
            let grow = (look.size - fontsize) / 2.0;
            let boxed = look
                .boxed
                .as_ref()
                .map(|color| {
                    format!(
                        ":box=1:boxcolor=0x{color}@0.95:boxborderw={:.0}",
                        (fontsize * 0.12).max(2.0)
                    )
                })
                .unwrap_or_default();
            filters.push(format!(
                "drawtext=fontfile='{font}':text='{}':fontsize={:.1}:fontcolor=0x{}:x={:.1}-text_w/2:y={:.1}:borderw={outline:.0}:bordercolor=black{boxed}{enable}",
                text_escape(&word.text),
                look.size,
                look.color,
                word.center_x,
                word.top_y - grow,
            ));
        }
    }
    if filters.is_empty() {
        "null".to_owned()
    } else {
        filters.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: f64, end: f64, text: &str) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
        }
    }

    fn captions(words: Vec<Word>) -> AnimatedCaptions {
        AnimatedCaptions {
            words,
            max_words: 3,
            ..Default::default()
        }
    }

    /// Medida fija para probar la maquetación sin fuente: 10 px por letra.
    fn fixed(text: &str) -> f64 {
        text.chars().count() as f64 * 10.0
    }

    #[test]
    fn layout_centers_each_page() {
        let words = vec![
            word(0.0, 0.5, "hola"),
            word(0.5, 1.0, "que"),
            word(1.0, 1.5, "tal"),
            word(1.5, 2.0, "amigos"),
        ];
        let placed = layout(&captions(words), &fixed, 10.0, 50.0, (1000, 1000), 0.5);
        // Página 1: "hola que tal" = 40+10+30+10+30 = 120 px centrados.
        assert_eq!(placed[0].center_x, 440.0 + 20.0);
        assert_eq!(placed[1].center_x, 490.0 + 15.0);
        assert_eq!(placed[2].center_x, 530.0 + 15.0);
        // Página 2 sola y centrada; su texto empieza donde acaba la primera.
        assert_eq!(placed[3].center_x, 500.0);
        assert_eq!(placed[0].page_end, 1.5);
        assert_eq!(placed[3].page_start, 1.5);
        assert!((placed[0].top_y - (500.0 - 62.5 / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn long_pages_wrap_to_two_lines() {
        let words = vec![
            word(0.0, 0.5, "supercalifragilistico"),
            word(0.5, 1.0, "espialidoso"),
        ];
        let placed = layout(&captions(words), &fixed, 10.0, 40.0, (300, 1000), 0.5);
        assert!(
            placed[1].top_y > placed[0].top_y,
            "la segunda palabra baja de línea"
        );
        assert_eq!(placed[1].center_x, 150.0);
    }

    #[test]
    fn karaoke_keeps_words_colored_after_spoken() {
        let placed = PlacedWord {
            text: "a".into(),
            center_x: 0.0,
            top_y: 0.0,
            start: 1.0,
            end: 2.0,
            page_start: 0.0,
            page_end: 3.0,
        };
        assert_eq!(
            windows(&placed, CaptionPreset::Karaoke),
            (vec![(0.0, 1.0)], vec![(1.0, 3.0)])
        );
        assert_eq!(
            windows(&placed, CaptionPreset::Clasico),
            (vec![(0.0, 1.0), (2.0, 3.0)], vec![(1.0, 2.0)])
        );
    }

    #[test]
    fn local_words_are_clipped_to_the_clip() {
        let transcript = vec![
            word(1.0, 2.0, "a"),
            word(5.0, 6.0, "b"),
            word(9.0, 11.0, "c"),
        ];
        assert_eq!(
            local_words(&transcript, 4.0, 10.0),
            vec![word(1.0, 2.0, "b"), word(5.0, 6.0, "c")]
        );
    }

    #[test]
    fn preview_draws_only_visible_states() {
        let Some(font) = crate::find_font() else {
            return;
        };
        let captions = captions(vec![word(0.0, 1.0, "uno"), word(1.0, 2.0, "dos")]);
        let escape = |text: &str| text.to_owned();
        let at = |time| {
            caption_filters(
                &captions,
                &font,
                "f",
                &escape,
                60.0,
                "FFFFFF",
                (1920, 1080),
                0.8,
                Some(time),
            )
        };
        let early = at(0.5);
        assert_eq!(early.matches("drawtext").count(), 2);
        assert!(early.contains("text='uno'") && early.contains("fontcolor=0xFFD91A"));
        assert!(!early.contains("enable"));
        let full = caption_filters(
            &captions,
            &font,
            "f",
            &escape,
            60.0,
            "FFFFFF",
            (1920, 1080),
            0.8,
            None,
        );
        assert_eq!(full.matches("drawtext").count(), 4);
        assert!(full.contains(":enable='between(t,1.000,1.999)'"));
    }
}
