use serde::{Deserialize, Serialize};

/// Subtítulo con nombre anclado a un intervalo de la timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subtitle {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Indices into the original document, ordered by time even while filtering.
pub fn search_subtitles(subtitles: &[Subtitle], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    let mut indices: Vec<usize> = subtitles
        .iter()
        .enumerate()
        .filter(|(_, cue)| needle.is_empty() || cue.text.to_lowercase().contains(&needle))
        .map(|(index, _)| index)
        .collect();
    indices.sort_by(|a, b| subtitles[*a].start.total_cmp(&subtitles[*b].start));
    indices
}

/// Validate the entire edit before returning a replacement document value.
pub fn shift_subtitles(subtitles: &[Subtitle], offset: f64) -> Result<Vec<Subtitle>, String> {
    if !offset.is_finite() {
        return Err("Introduce un desplazamiento finito en segundos".into());
    }
    subtitles.iter().map(|cue| {
        let start = cue.start + offset;
        let end = cue.end + offset;
        if !cue.start.is_finite() || !cue.end.is_finite() || cue.start < 0.0 || cue.end <= cue.start
            || !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Err("El ajuste dejaría tiempos inválidos o subtítulos antes del inicio. No se ha movido ninguno.".into());
        }
        Ok(Subtitle { start, end, text: cue.text.clone() })
    }).collect()
}

/// Convierte segundos a marca de tiempo SRT (HH:MM:SS,mmm).
pub fn srt_timestamp(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let (rest, ms) = (total_ms / 1000, total_ms % 1000);
    let (h, rem) = (rest / 3600, rest % 3600);
    let (m, s) = (rem / 60, rem % 60);
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

/// Genera un archivo .srt a partir de los subtítulos ordenados.
pub fn build_srt(subtitles: &[Subtitle]) -> String {
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
    let hours = parts.next()?;
    let minutes = parts.next()?;
    let seconds = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, "0"));
    if !(2..=8).contains(&hours.len())
        || !digits(hours)
        || minutes.len() != 2
        || !digits(minutes)
        || whole.len() != 2
        || !digits(whole)
        || fraction.len() > 3
        || !digits(fraction)
    {
        return None;
    }
    let hours = hours.parse::<u64>().ok()?;
    let minutes = minutes.parse::<u64>().ok()?;
    let whole = whole.parse::<u64>().ok()?;
    if minutes >= 60 || whole >= 60 {
        return None;
    }
    let millis = fraction.parse::<u64>().ok()? * 10_u64.pow(3 - fraction.len() as u32);
    Some((hours * 3_600_000 + minutes * 60_000 + whole * 1000 + millis) as f64 / 1000.0)
}

/// Texto de un archivo de subtítulos o intercambio en la codificación en que
/// suele llegar: UTF-8 (con o sin BOM), UTF-16 con BOM (Bloc de notas
/// «Unicode») o, si no es UTF-8 válido, Windows-1252, la de casi todos los
/// SRT en español descargados. Nunca falla: lo indescifrable queda como «�».
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], little: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| if little { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    // 0x80–0x9F de Windows-1252; el resto coincide con Latin-1.
    const CP1252: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|&byte| match byte {
            0x80..=0x9F => CP1252[(byte - 0x80) as usize],
            _ => byte as char,
        })
        .collect()
}

/// Lee un archivo de texto con `decode_text`.
pub fn read_text_file(path: &std::path::Path) -> std::io::Result<String> {
    std::fs::read(path).map(|bytes| decode_text(&bytes))
}

pub fn parse_srt(content: &str) -> Result<Vec<Subtitle>, String> {
    let normalized = content
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    // Whitespace-only separators are common in hand-edited SRTs. Normalize
    // just these lines so text within a multiline cue remains intact.
    let normalized = normalized
        .lines()
        .map(|line| if line.trim().is_empty() { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cues() -> Vec<Subtitle> {
        vec![
            Subtitle {
                start: 4.0,
                end: 6.0,
                text: "Hola\nmundo".into(),
            },
            Subtitle {
                start: 1.0,
                end: 2.0,
                text: "HOLA otra vez".into(),
            },
        ]
    }

    #[test]
    fn decodes_windows_1252_utf16_and_bom() {
        let latin = b"1\r\n00:00:01,000 --> 00:00:02,000\r\n\xbfQu\xe9 tal? \x93ni\xf1o\x94 \x80\r\n";
        let text = decode_text(latin);
        assert!(text.contains("¿Qué tal? “niño” €"), "{text}");
        assert_eq!(parse_srt(&text).unwrap()[0].text, "¿Qué tal? “niño” €");
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "Año ñ".encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        assert_eq!(decode_text(&utf16), "Año ñ");
        let mut utf16be = vec![0xFE, 0xFF];
        for unit in "Año".encode_utf16() {
            utf16be.extend(unit.to_be_bytes());
        }
        assert_eq!(decode_text(&utf16be), "Año");
        assert_eq!(decode_text("\u{feff}Año".as_bytes()), "Año");
        assert_eq!(decode_text("Año".as_bytes()), "Año");
    }

    #[test]
    fn search_returns_original_indices_in_time_order() {
        let cues = sample_cues();
        assert_eq!(search_subtitles(&cues, "  hola  "), vec![1, 0]);
        assert_eq!(search_subtitles(&cues, "\n "), vec![1, 0]);
        assert_eq!(search_subtitles(&cues, "MUNDO"), vec![0]);
        assert!(search_subtitles(&cues, "[a-z]").is_empty());
        assert!(search_subtitles(&cues, "ausente").is_empty());
        assert_eq!(cues, sample_cues());
    }

    #[test]
    fn shift_preserves_durations_text_and_order_and_can_be_reversed() {
        let cues = sample_cues();
        let shifted = shift_subtitles(&cues, -1.0).unwrap();
        assert_eq!(shifted[0].start, 3.0);
        assert_eq!((shifted[1].start, shifted[1].end), (0.0, 1.0));
        assert_eq!(shifted[0].text, cues[0].text);
        assert_eq!(shift_subtitles(&shifted, 1.0).unwrap(), cues);
        assert_eq!(shift_subtitles(&cues, 0.0).unwrap(), cues);
        assert!(shift_subtitles(&[], 1.0).unwrap().is_empty());
    }

    #[test]
    fn shift_rejects_negative_timing_nonfinite_values_and_lost_precision_atomically() {
        let cues = sample_cues();
        for offset in [-1.001, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert!(shift_subtitles(&cues, offset).is_err());
            assert_eq!(cues, sample_cues());
        }
        for (start, end) in [
            (f64::NAN, 2.0),
            (1.0, f64::INFINITY),
            (-1.0, 1.0),
            (3.0, 2.0),
        ] {
            assert!(shift_subtitles(
                &[Subtitle {
                    start,
                    end,
                    text: "Invalid".into()
                }],
                1.0
            )
            .is_err());
        }
    }

    #[test]
    fn imports_bom_whitespace_separators_and_position_metadata() {
        let cues = parse_srt("\u{feff}00:00:01,250-->00:00:03,000 X1:10 X2:20\r\nHola\r\nmundo\r\n \t\r\n2\r\n00:00:04.000 --> 00:00:05.500\r\nFin\r\n").unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hola\nmundo");
        assert_eq!((cues[0].start, cues[0].end), (1.25, 3.0));
        assert_eq!(cues[1].end, 5.5);
    }

    #[test]
    fn rejects_invalid_or_unbounded_timestamps() {
        for value in [
            "-1:00:00,000",
            "00:60:00,000",
            "00:00:60,000",
            "00:00:NaN",
            "00:00:inf",
            "1e9:00:00,000",
            "00:00:01.2.3",
            "00:00:01,0000",
            "999999999999999999999999:00:00,000",
        ] {
            assert!(parse_srt_timestamp(value).is_none(), "{value}");
            assert!(parse_srt(&format!("1\n00:00:00,000 --> {value}\nInvalid")).is_err());
        }
        assert!(parse_srt("1\n00:00:02,000 --> 00:00:01,000\nReversed").is_err());
    }

    #[test]
    fn export_orders_cues_and_carries_millisecond_rounding() {
        assert_eq!(srt_timestamp(599.9995), "00:10:00,000");
        let cues = vec![
            Subtitle {
                start: 5.0,
                end: 7.0,
                text: "Fin".into(),
            },
            Subtitle {
                start: 1.25,
                end: 3.0,
                text: "Hola\nmundo".into(),
            },
        ];
        assert_eq!(
            parse_srt(&build_srt(&cues)).unwrap(),
            vec![cues[1].clone(), cues[0].clone()]
        );
    }

    #[test]
    fn persisted_subtitles_keep_the_existing_windows_schema() {
        let cue: Subtitle =
            serde_json::from_str(r#"{"start":1.25,"end":3.0,"text":"Hola"}"#).unwrap();
        assert_eq!(cue.start, 1.25);
        assert_eq!(serde_json::to_value(&cue).unwrap()["text"], "Hola");
    }
}
