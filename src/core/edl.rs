//! EDL CMX 3600: el formato de lista de cortes que Premiere, Resolve y
//! cualquier sala de máster siguen leyendo hoy.
//!
//! Aquí vive solo el formato, en frames del montaje y sin saber nada del
//! modelo de proyecto de cada host. Así la ida y vuelta —exportar y volver a
//! importar— se ejecuta en las pruebas del core, que es la única comprobación
//! que no depende de que el formato esté escrito como yo creo que se escribe.

use crate::timebase::Timebase;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Video,
    /// `A1` es `Audio(1)`. El canal cero no existe en un EDL.
    Audio(usize),
    /// `B`: vídeo y audio del mismo plano, que viajan enlazados.
    Both,
}

impl Channel {
    fn label(self) -> String {
        match self {
            Channel::Video => "V".to_owned(),
            Channel::Audio(index) => format!("A{}", index.max(1)),
            Channel::Both => "B".to_owned(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "V" => Some(Channel::Video),
            "B" | "VA" | "AV" => Some(Channel::Both),
            "A" | "A1" => Some(Channel::Audio(1)),
            "AA" | "A1A2" => Some(Channel::Audio(1)),
            "NONE" => None,
            other => other
                .strip_prefix('A')
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| (1..=16).contains(index))
                .map(Channel::Audio),
        }
    }
}

/// Un corte, en frames del montaje. Es lo que un EDL sabe decir.
#[derive(Debug, Clone, PartialEq)]
pub struct EdlClip {
    /// Archivo o cinta de origen. Al exportar decide el reel; al importar
    /// recoge el reel tal cual venía, porque un EDL no trae rutas.
    pub source: String,
    pub name: String,
    pub channel: Channel,
    pub source_in: i64,
    pub record_in: i64,
    pub duration: i64,
    /// 1.0 es velocidad normal.
    pub speed: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EdlDocument {
    pub title: Option<String>,
    pub clips: Vec<EdlClip>,
    /// Lo que el formato no puede representar y no se debe fingir.
    pub warnings: Vec<String>,
    /// Cadencia final, con el drop frame que declare la cabecera `FCM`.
    pub timebase: Option<Timebase>,
}

/// Reel de siete caracteres por archivo, estable y sin repetir: el mismo medio
/// usa el mismo reel en todos sus eventos, de vídeo y de audio.
fn assign_reels(clips: &[EdlClip]) -> HashMap<String, String> {
    let mut reels: HashMap<String, String> = HashMap::new();
    let mut used: Vec<String> = Vec::new();
    for clip in clips {
        if reels.contains_key(&clip.source) {
            continue;
        }
        let stem = std::path::Path::new(&clip.source)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&clip.source);
        let raw: String = stem
            .to_uppercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let mut candidate = if raw.is_empty() {
            "AX".to_owned()
        } else {
            raw.chars().take(7).collect()
        };
        if used.contains(&candidate) {
            let base: String = candidate.chars().take(6).collect();
            let mut suffix = 2;
            while used.contains(&format!("{base}{suffix}")) {
                suffix += 1;
            }
            candidate = format!("{base}{suffix}");
        }
        used.push(candidate.clone());
        reels.insert(clip.source.clone(), candidate);
    }
    reels
}

/// Escribe un EDL CMX 3600. Las líneas de evento no pasan de 80 columnas, que
/// es el ancho que las salas de máster siguen esperando.
pub fn to_edl(title: &str, timebase: Timebase, clips: &[EdlClip]) -> String {
    let reels = assign_reels(clips);
    let mut lines = vec![
        format!("TITLE: {title}"),
        format!(
            "FCM: {}",
            if timebase.drop_frame {
                "DROP FRAME"
            } else {
                "NON-DROP FRAME"
            }
        ),
        String::new(),
    ];

    let mut ordered: Vec<&EdlClip> = clips.iter().collect();
    ordered.sort_by(|left, right| {
        left.record_in
            .cmp(&right.record_in)
            .then_with(|| left.channel.label().cmp(&right.channel.label()))
    });

    for (index, clip) in ordered.iter().enumerate() {
        let reel = reels
            .get(&clip.source)
            .cloned()
            .unwrap_or_else(|| "AX".into());
        // La duración en el origen se estira o encoge con la velocidad: dos
        // segundos de montaje al doble consumen cuatro del medio.
        let source_span = source_span(clip);
        lines.push(format!(
            "{:03}  {:<7}   {:<3}  C      {} {} {} {}",
            index + 1,
            reel,
            clip.channel.label(),
            timebase.timecode(clip.source_in),
            timebase.timecode(clip.source_in + source_span),
            timebase.timecode(clip.record_in),
            timebase.timecode(clip.record_in + clip.duration)
        ));
        lines.push(format!(
            "* FROM CLIP NAME: {}",
            if clip.name.trim().is_empty() {
                reel.as_str()
            } else {
                clip.name.trim()
            }
        ));
        if clip.speed != 1.0 && clip.speed.is_finite() {
            // El M2 lleva la velocidad en fotogramas por segundo del origen,
            // que es como lo leen las salas; el comentario es para humanos.
            lines.push(format!(
                "M2   {:<7} {:>9.1}    {}",
                reel,
                clip.speed * f64::from(timebase.nominal_fps()),
                timebase.timecode(clip.source_in)
            ));
            lines.push(format!(
                "* SPEED CHANGE RATE: {}",
                (clip.speed * 100.0).round() as i64
            ));
        }
        lines.push(String::new());
    }
    lines.join("\n") + "\n"
}

fn source_span(clip: &EdlClip) -> i64 {
    if !clip.speed.is_finite() || clip.speed == 0.0 {
        return clip.duration;
    }
    let span = (clip.duration as f64 * clip.speed.abs()).round() as i64;
    span.max(1)
}

/// Lee un EDL CMX 3600.
///
/// Lo que el formato no lleva no se inventa: las disoluciones entran como corte
/// con aviso, y los medios no existen —cada reel es una cinta, no un archivo—.
pub fn from_edl(text: &str, timebase: Timebase) -> EdlDocument {
    let mut document = EdlDocument {
        timebase: Some(timebase),
        ..EdlDocument::default()
    };
    let mut declared_drop_frame: Option<bool> = None;
    let mut pending: Vec<(EdlClip, i32)> = Vec::new();

    for raw in text.replace("\r\n", "\n").split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_uppercase();
        if let Some(rest) = upper.strip_prefix("TITLE:") {
            let start = line.len() - rest.len();
            document.title = Some(line[start..].trim().to_owned());
            continue;
        }
        if upper.starts_with("FCM:") {
            declared_drop_frame = Some(!upper.contains("NON-DROP") && upper.contains("DROP"));
            continue;
        }
        if let Some(comment) = line.strip_prefix('*') {
            apply_comment(comment.trim(), &mut pending);
            continue;
        }
        if upper.starts_with("M2") {
            apply_motion(line, timebase, &mut pending, &mut document.warnings);
            continue;
        }
        match parse_event(line, timebase) {
            Some((clip, number, kind)) => {
                if kind != 'C' {
                    document.warnings.push(format!(
                        "Evento {number}: «{kind}» entra como corte; un EDL no lleva la forma de la transición"
                    ));
                }
                pending.push((clip, number));
            }
            None if line.starts_with(|c: char| c.is_ascii_digit()) => document.warnings.push(
                format!("Línea ilegible, omitida: {}", &line[..line.len().min(40)]),
            ),
            None => {}
        }
    }

    if let Some(drop_frame) = declared_drop_frame {
        if drop_frame != timebase.drop_frame {
            match Timebase::new(timebase.numerator, timebase.denominator, drop_frame) {
                Ok(adjusted) if adjusted.drop_frame == drop_frame => {
                    document.timebase = Some(adjusted)
                }
                _ => document.warnings.push(format!(
                    "El EDL declara {}, que no aplica a {:.3} fps",
                    if drop_frame {
                        "DROP FRAME"
                    } else {
                        "NON-DROP FRAME"
                    },
                    timebase.fps()
                )),
            }
        }
    }

    for (clip, number) in pending {
        if clip.duration <= 0 {
            document.warnings.push(format!(
                "Evento {number}: duración no positiva en el montaje, omitido"
            ));
            continue;
        }
        document.clips.push(clip);
    }
    document
}

fn parse_event(line: &str, timebase: Timebase) -> Option<(EdlClip, i32, char)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 8 {
        return None;
    }
    let number = fields[0].parse::<i32>().ok()?;
    let times: Vec<i64> = fields[fields.len() - 4..]
        .iter()
        .map(|field| timebase.frames_from_timecode(field))
        .collect::<Option<Vec<_>>>()?;
    let kind = fields[3]
        .to_uppercase()
        .chars()
        .next()
        .filter(char::is_ascii_alphabetic)?;
    let channel = Channel::parse(&fields[2].to_uppercase())?;
    Some((
        EdlClip {
            source: fields[1].to_owned(),
            name: fields[1].to_owned(),
            channel,
            source_in: times[0].max(0),
            record_in: times[2].max(0),
            duration: times[3] - times[2],
            speed: 1.0,
        },
        number,
        kind,
    ))
}

fn apply_comment(comment: &str, pending: &mut [(EdlClip, i32)]) {
    let Some((clip, _)) = pending.last_mut() else {
        return;
    };
    let upper = comment.to_uppercase();
    if let Some(rest) = upper.strip_prefix("FROM CLIP NAME:") {
        let name = comment[comment.len() - rest.len()..].trim();
        if !name.is_empty() {
            clip.name = name.to_owned();
        }
    } else if let Some(rest) = upper.strip_prefix("SPEED CHANGE RATE:") {
        if let Ok(percent) = rest.trim().trim_end_matches('%').parse::<f64>() {
            if percent != 0.0 && percent.is_finite() {
                clip.speed = percent / 100.0;
            }
        }
    }
}

/// `M2  REEL  048.0  01:00:00:00`: la velocidad va en fotogramas por segundo
/// del origen, así que se compara con la cadencia del montaje.
fn apply_motion(
    line: &str,
    timebase: Timebase,
    pending: &mut [(EdlClip, i32)],
    warnings: &mut Vec<String>,
) {
    let Some((clip, number)) = pending.last_mut() else {
        return;
    };
    let fields: Vec<&str> = line.split_whitespace().collect();
    let Some(rate) = fields.get(2).and_then(|field| field.parse::<f64>().ok()) else {
        return;
    };
    if !rate.is_finite() {
        return;
    }
    if rate == 0.0 {
        warnings.push(format!(
            "Evento {number}: congelado (M2 a 0 fps); entra a velocidad normal"
        ));
        return;
    }
    clip.speed = rate / f64::from(timebase.nominal_fps());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<EdlClip> {
        vec![
            EdlClip {
                source: "/medios/Entrevista.mov".into(),
                name: "Entrevista".into(),
                channel: Channel::Both,
                source_in: 0,
                record_in: 0,
                duration: 120,
                speed: 1.0,
            },
            EdlClip {
                source: "/medios/B roll.mov".into(),
                name: "B-roll".into(),
                channel: Channel::Video,
                source_in: 50,
                record_in: 120,
                duration: 80,
                speed: 1.0,
            },
            EdlClip {
                source: "/medios/Entrevista.mov".into(),
                name: "Rampa".into(),
                channel: Channel::Audio(2),
                source_in: 10,
                record_in: 200,
                duration: 100,
                speed: 2.0,
            },
        ]
    }

    #[test]
    fn a_timeline_survives_the_round_trip_through_the_format() {
        let written = to_edl("Ida y vuelta", Timebase::P25, &sample());
        let read = from_edl(&written, Timebase::P25);
        assert_eq!(read.title.as_deref(), Some("Ida y vuelta"));
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
        assert_eq!(read.clips.len(), 3);
        for (before, after) in sample().iter().zip(read.clips.iter()) {
            assert_eq!(after.record_in, before.record_in);
            assert_eq!(after.duration, before.duration);
            assert_eq!(after.source_in, before.source_in);
            assert_eq!(after.channel, before.channel);
            assert_eq!(after.name, before.name);
            assert!((after.speed - before.speed).abs() < 1e-9, "{after:?}");
        }
        // El mismo archivo usa el mismo reel en todos sus eventos.
        assert_eq!(read.clips[0].source, read.clips[2].source);
        assert_ne!(read.clips[0].source, read.clips[1].source);
    }

    #[test]
    fn the_written_file_keeps_the_shape_a_master_room_expects() {
        let written = to_edl("Formato", Timebase::P25, &sample());
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines[0], "TITLE: Formato");
        assert_eq!(lines[1], "FCM: NON-DROP FRAME");
        let events: Vec<&&str> = lines
            .iter()
            .filter(|line| line.starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|line| line.len() <= 80), "{events:?}");
        assert!(
            events[0].starts_with("001  ENTREVIS"[..12].trim_end()),
            "{}",
            events[0]
        );
        assert!(written.contains("M2   "), "{written}");
        assert!(written.contains("* SPEED CHANGE RATE: 200"));
        // A doble velocidad, cien frames de montaje consumen doscientos del medio.
        assert!(written.contains("00:00:00:10 00:00:08:10"), "{written}");
    }

    #[test]
    fn an_edl_from_another_room_is_read_with_its_own_spacing() {
        let text = "TITLE: MONTAJE AJENO\nFCM: NON-DROP FRAME\n\n\
001  CINTA01 V     C        01:00:00:00 01:00:04:00 00:00:00:00 00:00:04:00\n\
* FROM CLIP NAME: Plano general\n\
002  CINTA02 B     D    025 02:00:00:00 02:00:02:00 00:00:04:00 00:00:06:00\n\
003  CINTA01 A2    C        01:00:10:00 01:00:12:00 00:00:04:00 00:00:06:00\n\
M2   CINTA01       050.0    01:00:10:00\n\
004  CINTA03 V     C        00:00:00:00 00:00:00:00 00:00:06:00 00:00:06:00\n\
ESTO NO ES UN EVENTO\n";
        let read = from_edl(text, Timebase::P25);
        assert_eq!(read.title.as_deref(), Some("MONTAJE AJENO"));
        assert_eq!(read.clips.len(), 3, "el evento de duración cero se omite");
        assert_eq!(read.clips[0].name, "Plano general");
        assert_eq!(read.clips[0].source_in, 90_000);
        assert_eq!(read.clips[1].channel, Channel::Both);
        assert_eq!(read.clips[2].channel, Channel::Audio(2));
        assert_eq!(read.clips[2].speed, 2.0);
        assert!(read.warnings.iter().any(|w| w.contains("«D»")));
        assert!(read
            .warnings
            .iter()
            .any(|w| w.contains("duración no positiva")));
        assert!(!read.warnings.iter().any(|w| w.contains("ESTO NO ES")));
    }

    #[test]
    fn drop_frame_is_honoured_where_it_exists_and_refused_where_it_does_not() {
        let ntsc = from_edl(
            "FCM: DROP FRAME\n001  A       V    C      00:00:00;00 00:00:01;00 00:00:00;00 00:00:01;00\n",
            Timebase::NTSC30,
        );
        assert!(ntsc.timebase.unwrap().drop_frame);
        assert!(ntsc.warnings.is_empty(), "{:?}", ntsc.warnings);
        assert_eq!(ntsc.clips.len(), 1);

        // NTSC sin drop frame es legítimo y la cabecera manda.
        let ndf = from_edl(
            "FCM: NON-DROP FRAME\n001  A       V    C      00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00\n",
            Timebase::NTSC30,
        );
        assert!(!ndf.timebase.unwrap().drop_frame);
        assert!(ndf.warnings.is_empty(), "{:?}", ndf.warnings);

        let impossible = from_edl(
            "FCM: DROP FRAME\n001  A       V    C      00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00\n",
            Timebase::P25,
        );
        assert!(!impossible.timebase.unwrap().drop_frame);
        assert!(impossible.warnings.iter().any(|w| w.contains("DROP FRAME")));
        assert_eq!(impossible.clips.len(), 1);
    }

    #[test]
    fn empty_and_meaningless_input_is_harmless() {
        assert_eq!(from_edl("", Timebase::P25).clips.len(), 0);
        assert!(from_edl("", Timebase::P25).warnings.is_empty());
        assert_eq!(from_edl("basura sin formato", Timebase::P25).clips.len(), 0);
        assert_eq!(to_edl("Vacío", Timebase::P25, &[]).lines().count(), 3);
    }
}
