//! Panel Proyecto al estilo de Premiere: una biblioteca de medios que no
//! depende de la timeline, bins para ordenarla y varias secuencias por
//! proyecto.
//!
//! La secuencia activa vive en los campos de siempre de `RoughProject`
//! (clips, marcadores, pistas…), así que el resto del host no cambia y los
//! proyectos antiguos se abren como un proyecto de una sola secuencia. Las
//! demás se guardan en `sequences`; abrir otra intercambia los datos.

use super::{transcripcion, Marker, RoughClip, RoughProject, SubtitleStyle};
use editorcito::subtitles::Subtitle;
use editorcito::timebase::Timebase;
use serde::{Deserialize, Serialize};

/// Un medio del proyecto: el clip entero tal como se importó, listo para
/// colocarlo en cualquier secuencia, y el bin donde está.
#[derive(Clone, Serialize, Deserialize)]
pub struct LibraryItem {
    pub clip: RoughClip,
    /// Ruta del bin separada por `/`; vacía es la raíz.
    #[serde(default)]
    pub bin: String,
}

/// Todo lo que pertenece a una secuencia y no al proyecto.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SequenceData {
    pub clips: Vec<RoughClip>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    #[serde(default)]
    pub subtitle_style: Option<SubtitleStyle>,
    #[serde(default)]
    pub track_gains: Vec<f64>,
    #[serde(default)]
    pub master_gain_db: f64,
    #[serde(default)]
    pub normalize_loudness: bool,
    #[serde(default)]
    pub track_mutes: Vec<bool>,
    #[serde(default)]
    pub track_solos: Vec<bool>,
    #[serde(default)]
    pub video_hidden: Vec<bool>,
    #[serde(default)]
    pub video_locked: Vec<bool>,
    #[serde(default)]
    pub audio_locked: Vec<bool>,
    #[serde(default = "super::default_fps")]
    pub fps: f64,
    #[serde(default)]
    pub timebase: Option<Timebase>,
    #[serde(default)]
    pub min_video_tracks: usize,
    #[serde(default)]
    pub min_audio_tracks: usize,
    #[serde(default)]
    pub transcript: Vec<transcripcion::Word>,
}

impl SequenceData {
    pub fn duration(&self) -> f64 {
        self.clips
            .iter()
            .map(|clip| clip.timeline_start + clip.duration())
            .fold(0.0, f64::max)
    }
}

/// Una secuencia que no está abierta.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSequence {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub bin: String,
    pub data: SequenceData,
}

/// Normaliza una ruta de bin: sin barras sobrantes ni tramos vacíos.
pub fn clean_bin(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// ¿`path` está dentro de `bin` (o es él)?
pub fn is_inside(path: &str, bin: &str) -> bool {
    bin.is_empty() || path == bin || path.starts_with(&format!("{bin}/"))
}

impl RoughProject {
    /// Saca la secuencia activa dejando los campos vacíos.
    fn take_active(&mut self) -> SequenceData {
        SequenceData {
            clips: std::mem::take(&mut self.clips),
            markers: std::mem::take(&mut self.markers),
            subtitles: std::mem::take(&mut self.subtitles),
            subtitle_style: self.subtitle_style.take(),
            track_gains: std::mem::take(&mut self.track_gains),
            master_gain_db: std::mem::take(&mut self.master_gain_db),
            normalize_loudness: std::mem::take(&mut self.normalize_loudness),
            track_mutes: std::mem::take(&mut self.track_mutes),
            track_solos: std::mem::take(&mut self.track_solos),
            video_hidden: std::mem::take(&mut self.video_hidden),
            video_locked: std::mem::take(&mut self.video_locked),
            audio_locked: std::mem::take(&mut self.audio_locked),
            fps: self.fps,
            timebase: self.timebase,
            min_video_tracks: std::mem::take(&mut self.min_video_tracks),
            min_audio_tracks: std::mem::take(&mut self.min_audio_tracks),
            transcript: std::mem::take(&mut self.transcript),
        }
    }

    fn load_active(&mut self, data: SequenceData) {
        self.clips = data.clips;
        self.markers = data.markers;
        self.subtitles = data.subtitles;
        self.subtitle_style = data.subtitle_style;
        self.track_gains = data.track_gains;
        self.master_gain_db = data.master_gain_db;
        self.normalize_loudness = data.normalize_loudness;
        self.track_mutes = data.track_mutes;
        self.track_solos = data.track_solos;
        self.video_hidden = data.video_hidden;
        self.video_locked = data.video_locked;
        self.audio_locked = data.audio_locked;
        self.fps = data.fps;
        self.timebase = data.timebase;
        self.min_video_tracks = data.min_video_tracks;
        self.min_audio_tracks = data.min_audio_tracks;
        self.transcript = data.transcript;
    }

    /// Guarda la activa entre las demás y abre `id`. Devuelve si cambió.
    pub fn open_sequence(&mut self, id: u64) -> bool {
        let Some(position) = self.sequences.iter().position(|sequence| sequence.id == id) else {
            return false;
        };
        let target = self.sequences.remove(position);
        let current = StoredSequence {
            id: self.sequence_id,
            name: std::mem::take(&mut self.sequence_name),
            bin: std::mem::take(&mut self.sequence_bin),
            data: self.take_active(),
        };
        self.sequences.insert(position, current);
        self.sequence_id = target.id;
        self.sequence_name = target.name;
        self.sequence_bin = target.bin;
        self.load_active(target.data);
        true
    }

    fn next_sequence_id(&self) -> u64 {
        self.sequences
            .iter()
            .map(|sequence| sequence.id)
            .chain(std::iter::once(self.sequence_id))
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Nombre libre a partir de `base` («Secuencia 2», «Secuencia 3»…).
    pub fn unique_sequence_name(&self, base: &str) -> String {
        let taken = |name: &str| {
            self.sequence_name == name || self.sequences.iter().any(|sequence| sequence.name == name)
        };
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|number| format!("{base} {number}"))
            .find(|name| !taken(name))
            .expect("siempre hay un nombre libre")
    }

    /// Crea una secuencia vacía con la cadencia de la activa y la abre.
    pub fn new_sequence(&mut self, bin: &str) -> u64 {
        let id = self.next_sequence_id();
        let data = SequenceData {
            fps: self.fps,
            timebase: self.timebase,
            ..SequenceData::default()
        };
        self.sequences.push(StoredSequence {
            id,
            name: self.unique_sequence_name(&format!("Secuencia {}", self.sequences.len() + 2)),
            bin: clean_bin(bin),
            data,
        });
        self.open_sequence(id);
        id
    }

    /// Copia de la secuencia `id` (o de la activa) con otro nombre. No la abre.
    pub fn duplicate_sequence(&mut self, id: u64) -> Option<u64> {
        let (name, bin, data) = if id == self.sequence_id {
            let snapshot = self.clone();
            let mut copy = snapshot;
            (
                self.sequence_name.clone(),
                self.sequence_bin.clone(),
                copy.take_active(),
            )
        } else {
            let sequence = self.sequences.iter().find(|sequence| sequence.id == id)?;
            (sequence.name.clone(), sequence.bin.clone(), sequence.data.clone())
        };
        let new_id = self.next_sequence_id();
        let name = self.unique_sequence_name(&format!("{name} copia"));
        self.sequences.push(StoredSequence {
            id: new_id,
            name,
            bin,
            data,
        });
        Some(new_id)
    }

    /// Borra una secuencia que no esté abierta.
    pub fn delete_sequence(&mut self, id: u64) -> bool {
        let before = self.sequences.len();
        self.sequences.retain(|sequence| sequence.id != id);
        before != self.sequences.len()
    }

    pub fn rename_sequence(&mut self, id: u64, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if id == self.sequence_id {
            self.sequence_name = name.to_owned();
        } else if let Some(sequence) = self.sequences.iter_mut().find(|sequence| sequence.id == id) {
            sequence.name = name.to_owned();
        }
    }

    /// Clip anidado con el contenido de la secuencia `id`, listo para
    /// colocarlo en la activa. `None` si es la propia activa o está vacía.
    pub fn sequence_as_nested(&self, id: u64) -> Option<RoughClip> {
        let sequence = self.sequences.iter().find(|sequence| sequence.id == id)?;
        let children: Vec<RoughClip> = sequence
            .data
            .clips
            .iter()
            .filter(|clip| clip.enabled)
            .cloned()
            .collect();
        if children.is_empty() {
            return None;
        }
        let duration = sequence.data.duration();
        Some(RoughClip {
            path: std::path::PathBuf::new(),
            in_seconds: 0.0,
            out_seconds: duration,
            has_video: children.iter().any(|clip| clip.has_video),
            has_audio: children.iter().any(|clip| clip.has_audio),
            nested: Some(children),
            ..RoughClip::default()
        })
    }

    /// Registra en la biblioteca los medios de archivo que aún no están.
    /// Devuelve cuántos se añadieron.
    pub fn register_media(&mut self, clips: &[RoughClip], bin: &str) -> usize {
        let mut added = 0;
        for clip in clips {
            if clip.path.as_os_str().is_empty()
                || clip.title.is_some()
                || clip.nested.is_some()
                || clip.is_adjustment
                || self.library.iter().any(|item| item.clip.path == clip.path)
            {
                continue;
            }
            let mut template = clip.clone();
            let full = clip.source_duration_seconds.unwrap_or(clip.out_seconds);
            template.in_seconds = 0.0;
            template.out_seconds = full.max(clip.out_seconds);
            template.timeline_start = 0.0;
            template.track = 0;
            self.library.push(LibraryItem {
                clip: template,
                bin: clean_bin(bin),
            });
            added += 1;
        }
        added
    }

    /// Proyectos anteriores a la biblioteca: sus medios salen de la timeline
    /// de todas las secuencias.
    pub fn migrate_library(&mut self) {
        if !self.library.is_empty() {
            return;
        }
        let mut all: Vec<RoughClip> = self.clips.clone();
        for sequence in &self.sequences {
            all.extend(sequence.data.clips.iter().cloned());
        }
        self.register_media(&all, "");
    }

    /// Todos los bins que existen: los declarados y los que usan medios y
    /// secuencias, con sus antecesores. Ordenados.
    pub fn all_bins(&self) -> Vec<String> {
        let mut bins: Vec<String> = self
            .bins
            .iter()
            .cloned()
            .chain(self.library.iter().map(|item| item.bin.clone()))
            .chain(self.sequences.iter().map(|sequence| sequence.bin.clone()))
            .chain(std::iter::once(self.sequence_bin.clone()))
            .filter(|bin| !bin.is_empty())
            .collect();
        let mut with_parents = Vec::new();
        for bin in &bins {
            let parts: Vec<&str> = bin.split('/').collect();
            for depth in 1..parts.len() {
                with_parents.push(parts[..depth].join("/"));
            }
        }
        bins.extend(with_parents);
        bins.sort();
        bins.dedup();
        bins
    }

    pub fn add_bin(&mut self, path: &str) -> Option<String> {
        let path = clean_bin(path);
        if path.is_empty() || self.all_bins().contains(&path) {
            return None;
        }
        self.bins.push(path.clone());
        Some(path)
    }

    /// Renombra o mueve un bin con todo su contenido.
    pub fn rename_bin(&mut self, from: &str, to: &str) {
        let (from, to) = (clean_bin(from), clean_bin(to));
        if from.is_empty() || to.is_empty() || from == to {
            return;
        }
        let rewrite = |path: &mut String| {
            if *path == from {
                *path = to.clone();
            } else if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
                *path = format!("{to}/{rest}");
            }
        };
        self.bins.iter_mut().for_each(rewrite);
        self.library.iter_mut().for_each(|item| rewrite(&mut item.bin));
        self.sequences.iter_mut().for_each(|sequence| rewrite(&mut sequence.bin));
        rewrite(&mut self.sequence_bin);
    }

    /// Borra un bin: su contenido sube al bin padre.
    pub fn delete_bin(&mut self, path: &str) {
        let path = clean_bin(path);
        let parent = path.rsplit_once('/').map(|(parent, _)| parent.to_owned()).unwrap_or_default();
        let lift = |bin: &mut String| {
            if is_inside(bin, &path) && !path.is_empty() {
                let rest = bin[path.len()..].trim_start_matches('/');
                *bin = clean_bin(&format!("{parent}/{rest}"));
            }
        };
        self.bins.retain(|bin| *bin != path);
        self.bins.iter_mut().for_each(lift);
        self.library.iter_mut().for_each(|item| lift(&mut item.bin));
        self.sequences.iter_mut().for_each(|sequence| lift(&mut sequence.bin));
        lift(&mut self.sequence_bin);
    }

    /// ¿Usa alguna secuencia este archivo?
    pub fn media_in_use(&self, path: &std::path::Path) -> bool {
        fn uses(clips: &[RoughClip], path: &std::path::Path) -> bool {
            clips.iter().any(|clip| {
                clip.path == path || clip.nested.as_deref().is_some_and(|nested| uses(nested, path))
            })
        }
        uses(&self.clips, path)
            || self
                .sequences
                .iter()
                .any(|sequence| uses(&sequence.data.clips, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn media(name: &str, start: f64) -> RoughClip {
        RoughClip {
            path: PathBuf::from(name),
            in_seconds: 1.0,
            out_seconds: 4.0,
            source_duration_seconds: Some(10.0),
            timeline_start: start,
            ..Default::default()
        }
    }

    fn project() -> RoughProject {
        let mut project: RoughProject =
            serde_json::from_str(r#"{"version":2,"name":"p","clips":[]}"#).unwrap();
        project.normalize();
        project
    }

    #[test]
    fn old_projects_open_as_a_single_sequence_with_their_media() {
        let mut project = project();
        project.clips = vec![media("a.mp4", 0.0), media("a.mp4", 3.0), media("b.wav", 6.0)];
        project.migrate_library();
        assert_eq!(project.library.len(), 2, "un medio por archivo");
        let item = &project.library[0];
        assert_eq!((item.clip.in_seconds, item.clip.out_seconds), (0.0, 10.0));
        assert_eq!(project.sequence_name, "Secuencia 1");
    }

    #[test]
    fn switching_sequences_keeps_each_edit_intact() {
        let mut project = project();
        project.clips = vec![media("a.mp4", 0.0)];
        project.markers.push(Marker {
            time: 1.0,
            name: "uno".into(),
        });
        let first = project.sequence_id;
        let second = project.new_sequence("Cortes");
        assert!(project.clips.is_empty() && project.markers.is_empty());
        assert_eq!(project.sequence_name, "Secuencia 2");
        assert_eq!(project.sequence_bin, "Cortes");
        project.clips = vec![media("b.mp4", 0.0), media("b.mp4", 3.0)];
        assert!(project.open_sequence(first));
        assert_eq!(project.clips.len(), 1);
        assert_eq!(project.markers[0].name, "uno");
        assert!(project.open_sequence(second));
        assert_eq!(project.clips.len(), 2);
        // Guardar y abrir conserva ambas.
        let json = serde_json::to_string(&project).unwrap();
        let mut restored: RoughProject = serde_json::from_str(&json).unwrap();
        restored.normalize();
        assert_eq!(restored.sequences.len(), 1);
        assert!(restored.open_sequence(first));
        assert_eq!(restored.clips.len(), 1);
    }

    #[test]
    fn nesting_a_sequence_builds_a_clip_of_its_length() {
        let mut project = project();
        let id = project.new_sequence("");
        project.clips = vec![media("a.mp4", 0.0), media("b.mp4", 5.0)];
        let outer = project.sequences[0].id;
        project.open_sequence(outer);
        let nested = project.sequence_as_nested(id).unwrap();
        assert!((nested.duration() - 8.0).abs() < 1e-9);
        assert_eq!(nested.nested.as_ref().unwrap().len(), 2);
        assert!(project.sequence_as_nested(project.sequence_id).is_none());
    }

    #[test]
    fn duplicate_rename_and_delete_sequences() {
        let mut project = project();
        project.clips = vec![media("a.mp4", 0.0)];
        let copy = project.duplicate_sequence(project.sequence_id).unwrap();
        assert_eq!(project.sequences[0].name, "Secuencia 1 copia");
        assert_eq!(project.sequences[0].data.clips.len(), 1);
        assert_eq!(project.clips.len(), 1, "duplicar no vacía la activa");
        project.rename_sequence(copy, "Versión corta");
        assert_eq!(project.sequences[0].name, "Versión corta");
        assert!(project.delete_sequence(copy));
        assert!(!project.delete_sequence(project.sequence_id), "la abierta no se borra");
    }

    #[test]
    fn bins_rename_and_delete_with_their_content() {
        let mut project = project();
        project.register_media(&[media("a.mp4", 0.0)], "Entrevistas/Día 1");
        project.add_bin("Música");
        assert_eq!(project.all_bins(), vec!["Entrevistas", "Entrevistas/Día 1", "Música"]);
        project.rename_bin("Entrevistas", "Brutos");
        assert_eq!(project.library[0].bin, "Brutos/Día 1");
        project.delete_bin("Brutos/Día 1");
        assert_eq!(project.library[0].bin, "Brutos");
        assert!(project.add_bin(" / ").is_none());
        assert!(project.media_in_use(std::path::Path::new("a.mp4")) == false);
    }
}
