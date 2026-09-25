//! Edición por texto: la transcripción palabra a palabra es otra vista del
//! montaje. Borrar palabras quita ese tramo de tiempo de todas las pistas y
//! cierra el hueco, como «Extraer» en Premiere o borrar texto en Descript.
//!
//! Todo es lógica pura sobre tiempos de timeline; el host solo la llama y
//! registra el paso de deshacer.

use super::RoughClip;
use editorcito::subtitles::Subtitle;
use serde::{Deserialize, Serialize};

/// Una palabra reconocida, en segundos de timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Deserialize)]
struct WhisperJson {
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: i64,
    to: i64,
}

/// Lee el JSON de `whisper-cli -ml 1 -sow -oj`: un segmento por palabra con
/// sus tiempos en milisegundos. Se descartan los segmentos vacíos y las
/// anotaciones que no son habla (`[Música]`, `(risas)`).
pub fn parse_whisper_words(json: &str) -> Result<Vec<Word>, String> {
    let document: WhisperJson = serde_json::from_str(json)
        .map_err(|error| format!("JSON de Whisper no válido: {error}"))?;
    let mut words = Vec::new();
    for segment in document.transcription {
        let text = segment.text.trim();
        if text.is_empty() || is_annotation(text) {
            continue;
        }
        let start = segment.offsets.from.max(0) as f64 / 1000.0;
        let end = (segment.offsets.to as f64 / 1000.0).max(start);
        words.push(Word {
            start,
            end,
            text: text.to_owned(),
        });
    }
    Ok(words)
}

fn is_annotation(text: &str) -> bool {
    (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with('(') && text.ends_with(')'))
        || text.chars().all(|character| !character.is_alphanumeric())
}

/// Texto normalizado para comparar: minúsculas y sin puntuación.
pub fn normalized(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric() || character.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Muletillas de una palabra. Solo se proponen: el usuario revisa antes de
/// borrar, porque «este» también es un demostrativo legítimo.
const FILLERS: &[&str] = &[
    "eh", "ehh", "ehm", "em", "emm", "erm", "mm", "mmm", "hm", "hmm", "ah", "ahh", "uh", "uhm",
    "um", "umm", "este",
];

/// Índices de palabras que parecen muletillas, incluidas las de dos
/// palabras («o sea»).
pub fn filler_indices(words: &[Word]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let current = normalized(&words[index].text);
        if index + 1 < words.len() && current == "o" && normalized(&words[index + 1].text) == "sea"
        {
            found.push(index);
            found.push(index + 1);
            index += 2;
            continue;
        }
        if FILLERS.contains(&current.as_str()) {
            found.push(index);
        }
        index += 1;
    }
    found
}

/// Tramos de tiempo que ocupan las palabras elegidas, fusionando las
/// consecutivas para que borrar una frase sea un solo corte.
pub fn ranges_for(words: &[Word], indices: &[usize]) -> Vec<(f64, f64)> {
    let mut sorted: Vec<usize> = indices
        .iter()
        .copied()
        .filter(|index| *index < words.len())
        .collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges: Vec<(f64, f64)> = Vec::new();
    let mut previous: Option<usize> = None;
    for index in sorted {
        let word = &words[index];
        match (ranges.last_mut(), previous) {
            (Some(last), Some(prev)) if prev + 1 == index => last.1 = last.1.max(word.end),
            _ => ranges.push((word.start, word.end)),
        }
        previous = Some(index);
    }
    merge_ranges(ranges)
}

fn merge_ranges(mut ranges: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ranges.retain(|(start, end)| end - start > 0.001);
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 + 0.001 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Posición de `time` tras quitar `ranges` (ordenados y sin solaparse).
/// `None` si el instante cae dentro de un tramo quitado.
pub fn remap_time(time: f64, ranges: &[(f64, f64)]) -> Option<f64> {
    let mut removed = 0.0;
    for (start, end) in ranges {
        if time >= *end - 1e-9 {
            removed += end - start;
        } else if time > *start + 1e-9 {
            return None;
        } else {
            break;
        }
    }
    Some(time - removed)
}

/// Desplaza palabras, subtítulos o marcadores tras un corte. Lo que caía
/// dentro de lo quitado desaparece; lo que lo cruzaba se recorta.
pub fn remap_span(start: f64, end: f64, ranges: &[(f64, f64)]) -> Option<(f64, f64)> {
    let clamp_start = |time: f64| {
        remap_time(time, ranges).unwrap_or_else(|| {
            let range = ranges
                .iter()
                .find(|(from, to)| time > *from && time < *to)
                .expect("remap_time solo falla dentro de un tramo");
            remap_time(range.1, ranges).unwrap_or(0.0)
        })
    };
    let clamp_end = |time: f64| {
        remap_time(time, ranges).unwrap_or_else(|| {
            let range = ranges
                .iter()
                .find(|(from, to)| time > *from && time < *to)
                .expect("remap_time solo falla dentro de un tramo");
            remap_time(range.0, ranges).unwrap_or(0.0)
        })
    };
    let (new_start, new_end) = (clamp_start(start), clamp_end(end));
    (new_end > new_start + 0.001).then_some((new_start, new_end))
}

pub fn remap_words(words: &[Word], ranges: &[(f64, f64)]) -> Vec<Word> {
    words
        .iter()
        .filter_map(|word| {
            // Una palabra con el centro dentro del corte se considera borrada.
            remap_time((word.start + word.end) / 2.0, ranges)?;
            let (start, end) = remap_span(word.start, word.end, ranges)?;
            Some(Word {
                start,
                end,
                text: word.text.clone(),
            })
        })
        .collect()
}

pub fn remap_subtitles(subtitles: &[Subtitle], ranges: &[(f64, f64)]) -> Vec<Subtitle> {
    subtitles
        .iter()
        .filter_map(|subtitle| {
            let (start, end) = remap_span(subtitle.start, subtitle.end, ranges)?;
            Some(Subtitle {
                start,
                end,
                text: subtitle.text.clone(),
            })
        })
        .collect()
}

fn is_complex(clip: &RoughClip) -> bool {
    clip.nested.is_some()
        || clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
}

/// El tramo `[from, to]` (tiempo local de timeline) de un clip como clip
/// independiente, respetando velocidad, clips invertidos, keyframes y la
/// banda de volumen. Su `timeline_start` queda en el inicio del tramo.
pub fn clip_portion(clip: &RoughClip, from: f64, to: f64) -> Option<RoughClip> {
    let duration = clip.duration();
    let from = from.clamp(0.0, duration);
    let to = to.clamp(from, duration);
    if to - from < 0.001 || is_complex(clip) {
        return None;
    }
    let speed = clip.speed.clamp(0.1, 8.0);
    let mut part = clip.clone();
    if clip.fx.reverse {
        part.in_seconds = clip.out_seconds - to * speed;
        part.out_seconds = clip.out_seconds - from * speed;
    } else {
        part.in_seconds = clip.in_seconds + from * speed;
        part.out_seconds = clip.in_seconds + to * speed;
    }
    part.timeline_start = clip.timeline_start + from;
    let at_start = from < 0.001;
    let at_end = to > duration - 0.001;
    if !at_start {
        part.fade_in_seconds = 0.0;
        part.transition = None;
    }
    if !at_end {
        part.fade_out_seconds = 0.0;
    }
    if let Some(keyframes) = &clip.keyframes {
        if !at_start {
            let (x, y, scale, opacity) = clip.evaluate_transform(from);
            let mut shifted: Vec<_> = keyframes
                .iter()
                .filter(|keyframe| keyframe.t > from + 0.001 && keyframe.t <= to)
                .cloned()
                .map(|mut keyframe| {
                    keyframe.t -= from;
                    keyframe
                })
                .collect();
            shifted.insert(
                0,
                super::TransformKeyframe {
                    t: 0.0,
                    x,
                    y,
                    scale,
                    opacity,
                },
            );
            part.keyframes = Some(shifted);
        }
    }
    if !clip.fx.volume_keys.is_empty() && !at_start {
        let level = clip.fx.volume_at(from);
        let mut keys: Vec<_> = clip
            .fx
            .volume_keys
            .iter()
            .filter(|key| key.t > from + 0.001 && key.t <= to)
            .map(|key| super::efectos::VolumeKey {
                t: key.t - from,
                db: key.db,
            })
            .collect();
        keys.insert(0, super::efectos::VolumeKey { t: 0.0, db: level });
        part.fx.volume_keys = keys;
    }
    Some(part)
}

/// Quita `ranges` de todas las pistas y cierra los huecos. Falla sin tocar
/// nada si un corte cae dentro de una secuencia anidada o una rampa.
pub fn extract_ranges(
    clips: &[RoughClip],
    ranges: &[(f64, f64)],
) -> Result<Vec<RoughClip>, String> {
    let ranges = merge_ranges(ranges.to_vec());
    let mut current = clips.to_vec();
    // De atrás hacia delante: cada corte no mueve los tramos anteriores.
    for (start, end) in ranges.iter().rev().copied() {
        let length = end - start;
        let mut next = Vec::with_capacity(current.len() + 1);
        for clip in &current {
            let clip_start = clip.timeline_start;
            let clip_end = clip_start + clip.duration();
            if clip_end <= start + 0.001 {
                next.push(clip.clone());
            } else if clip_start >= end - 0.001 {
                let mut shifted = clip.clone();
                shifted.timeline_start -= length;
                next.push(shifted);
            } else {
                if is_complex(clip) {
                    return Err(format!(
                        "El corte cruza «{}», que es una secuencia anidada o tiene rampa de velocidad",
                        clip.name()
                    ));
                }
                if let Some(head) = clip_portion(clip, 0.0, start - clip_start) {
                    next.push(head);
                }
                if let Some(mut tail) = clip_portion(clip, end - clip_start, clip.duration()) {
                    tail.timeline_start = start;
                    next.push(tail);
                }
            }
        }
        current = next;
    }
    Ok(current)
}

/// Agrupa palabras en páginas de subtítulo: se corta al llenar la línea, al
/// acabar una frase o tras una pausa. Devuelve rangos de índices.
pub fn pages(words: &[Word], max_chars: usize, max_words: usize) -> Vec<std::ops::Range<usize>> {
    let mut result = Vec::new();
    let mut page_start = 0;
    let mut chars = 0;
    for (index, word) in words.iter().enumerate() {
        let length = word.text.chars().count();
        let count = index - page_start;
        let gap = index > page_start && word.start - words[index - 1].end > 0.7;
        if count > 0 && (count >= max_words.max(1) || chars + 1 + length > max_chars || gap) {
            result.push(page_start..index);
            page_start = index;
            chars = 0;
        }
        chars += if chars == 0 { length } else { length + 1 };
        let sentence_end = word.text.ends_with(['.', '?', '!', '…']);
        if sentence_end {
            result.push(page_start..index + 1);
            page_start = index + 1;
            chars = 0;
        }
    }
    if page_start < words.len() {
        result.push(page_start..words.len());
    }
    result
}

/// Subtítulos clásicos desde las palabras: una página por cue.
pub fn subtitles_from_words(words: &[Word]) -> Vec<Subtitle> {
    pages(words, 42, 12)
        .into_iter()
        .map(|range| Subtitle {
            start: words[range.start].start,
            end: words[range.end - 1].end,
            text: words[range.clone()]
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
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

    #[test]
    fn parses_word_level_whisper_json() {
        let json = r#"{"transcription":[
            {"offsets":{"from":0,"to":0},"text":""},
            {"offsets":{"from":0,"to":490},"text":" Hola,"},
            {"offsets":{"from":490,"to":900},"text":" [Música]"},
            {"offsets":{"from":900,"to":1280},"text":" bienvenidos"}
        ]}"#;
        let words = parse_whisper_words(json).unwrap();
        assert_eq!(
            words,
            vec![word(0.0, 0.49, "Hola,"), word(0.9, 1.28, "bienvenidos")]
        );
        assert!(parse_whisper_words("{").is_err());
    }

    #[test]
    fn fillers_include_two_word_o_sea() {
        let words = vec![
            word(0.0, 0.2, "Eh,"),
            word(0.2, 0.5, "hoy"),
            word(0.5, 0.6, "o"),
            word(0.6, 0.8, "sea,"),
            word(0.8, 1.0, "o"),
            word(1.0, 1.2, "no"),
        ];
        assert_eq!(filler_indices(&words), vec![0, 2, 3]);
    }

    #[test]
    fn consecutive_words_become_one_range() {
        let words = vec![
            word(0.0, 1.0, "a"),
            word(1.0, 2.0, "b"),
            word(2.5, 3.0, "c"),
            word(3.0, 4.0, "d"),
        ];
        assert_eq!(ranges_for(&words, &[1, 0, 3]), vec![(0.0, 2.0), (3.0, 4.0)]);
    }

    #[test]
    fn remap_shifts_and_drops() {
        let ranges = [(1.0, 2.0), (5.0, 6.0)];
        assert_eq!(remap_time(0.5, &ranges), Some(0.5));
        assert_eq!(remap_time(1.5, &ranges), None);
        assert_eq!(remap_time(3.0, &ranges), Some(2.0));
        assert_eq!(remap_time(7.0, &ranges), Some(5.0));
        // Un subtítulo que cruza el corte se recorta, no desaparece.
        assert_eq!(remap_span(0.5, 1.8, &ranges), Some((0.5, 1.0)));
        assert_eq!(remap_span(1.1, 1.9, &ranges), None);
        let words = vec![
            word(0.0, 1.0, "a"),
            word(1.0, 2.0, "b"),
            word(2.0, 3.0, "c"),
        ];
        assert_eq!(
            remap_words(&words, &ranges),
            vec![word(0.0, 1.0, "a"), word(1.0, 2.0, "c")]
        );
    }

    #[test]
    fn extract_cuts_every_track_and_closes_gap() {
        let video = RoughClip {
            out_seconds: 10.0,
            ..Default::default()
        };
        let music = RoughClip {
            out_seconds: 10.0,
            has_video: false,
            track: 1,
            ..Default::default()
        };
        let late = RoughClip {
            in_seconds: 2.0,
            out_seconds: 4.0,
            timeline_start: 10.0,
            ..Default::default()
        };
        let result = extract_ranges(&[video, music, late], &[(3.0, 5.0)]).unwrap();
        assert_eq!(result.len(), 5);
        let spans: Vec<(f64, f64, f64)> = result
            .iter()
            .map(|clip| (clip.timeline_start, clip.in_seconds, clip.out_seconds))
            .collect();
        assert_eq!(
            spans,
            vec![
                (0.0, 0.0, 3.0),
                (3.0, 5.0, 10.0),
                (0.0, 0.0, 3.0),
                (3.0, 5.0, 10.0),
                (8.0, 2.0, 4.0)
            ]
        );
    }

    #[test]
    fn extract_respects_speed_reverse_and_volume() {
        let mut clip = RoughClip {
            out_seconds: 8.0,
            speed: 2.0,
            ..Default::default()
        };
        clip.fx.volume_keys = vec![
            crate::efectos::VolumeKey { t: 0.0, db: 0.0 },
            crate::efectos::VolumeKey { t: 4.0, db: -20.0 },
        ];
        // 4 s de timeline a 2x: quitar [1, 2] quita fuente [2, 4].
        let result = extract_ranges(&[clip.clone()], &[(1.0, 2.0)]).unwrap();
        assert_eq!((result[0].in_seconds, result[0].out_seconds), (0.0, 2.0));
        assert_eq!((result[1].in_seconds, result[1].out_seconds), (4.0, 8.0));
        assert_eq!(result[1].timeline_start, 1.0);
        assert_eq!(result[1].fx.volume_keys[0].db, -10.0);
        assert_eq!(result[1].fx.volume_keys[1].t, 2.0);
        clip.fx.reverse = true;
        let result = extract_ranges(&[clip], &[(1.0, 2.0)]).unwrap();
        // Invertido: el principio en timeline es el final del medio.
        assert_eq!((result[0].in_seconds, result[0].out_seconds), (6.0, 8.0));
        assert_eq!((result[1].in_seconds, result[1].out_seconds), (0.0, 4.0));
    }

    #[test]
    fn extract_refuses_complex_clips_atomically() {
        let nested = RoughClip {
            out_seconds: 5.0,
            nested: Some(vec![RoughClip {
                out_seconds: 5.0,
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert!(extract_ranges(&[nested.clone()], &[(1.0, 2.0)]).is_err());
        // Si el corte no lo toca, se desplaza entero sin problema.
        let mut later = nested;
        later.timeline_start = 3.0;
        let result = extract_ranges(&[later], &[(1.0, 2.0)]).unwrap();
        assert_eq!(result[0].timeline_start, 2.0);
    }

    #[test]
    fn pages_break_on_sentence_pause_and_length() {
        let words = vec![
            word(0.0, 0.3, "Hola."),
            word(0.3, 0.6, "Esto"),
            word(0.6, 0.9, "es"),
            word(2.0, 2.3, "una"),
            word(2.3, 2.6, "prueba"),
        ];
        assert_eq!(pages(&words, 42, 12), vec![0..1, 1..3, 3..5]);
        assert_eq!(pages(&words[1..], 42, 1), vec![0..1, 1..2, 2..3, 3..4]);
        let subtitles = subtitles_from_words(&words);
        assert_eq!(subtitles[1].text, "Esto es");
        assert_eq!((subtitles[2].start, subtitles[2].end), (2.0, 2.6));
    }
}
