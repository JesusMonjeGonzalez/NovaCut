//! Localiza en una carpeta los medios que un proyecto ha perdido.
//!
//! Mover un rodaje entero de disco es la forma normal de romper un proyecto, y
//! revincular archivo por archivo no es una respuesta cuando son doscientos.
//! El árbol se recorre una sola vez y se resuelve todo junto.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Un medio que el proyecto no encuentra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingMedia {
    pub key: usize,
    pub name: String,
    /// Tamaño conocido del original. Cero cuando no se sabe.
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelinkOutcome {
    pub found: HashMap<usize, PathBuf>,
    /// Nombres que no aparecieron, en el orden en que se pidieron.
    pub missing: Vec<String>,
    pub scanned: usize,
    /// El recorrido se detuvo en el tope: la carpeta elegida es enorme y el
    /// archivo podría estar más allá.
    pub truncated: bool,
}

struct Candidate {
    path: PathBuf,
    bytes: u64,
    depth: usize,
}

/// Recorre `root` en anchura buscando los nombres pedidos.
///
/// Entre varios archivos con el mismo nombre gana el que además coincide en
/// tamaño; después, el menos profundo; y en último término el orden alfabético,
/// para que dos búsquedas sobre la misma carpeta den siempre lo mismo.
pub fn find_missing_media(wanted: &[MissingMedia], root: &Path, max_files: usize) -> RelinkOutcome {
    if wanted.is_empty() {
        return RelinkOutcome::default();
    }
    let mut by_name: HashMap<String, Vec<Candidate>> = HashMap::new();
    for media in wanted {
        by_name.entry(media.name.to_lowercase()).or_default();
    }

    let mut scanned = 0;
    let mut truncated = false;
    let mut pending = vec![root.to_path_buf()];
    'search: while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        // Ordenar el nivel deja el recorrido reproducible entre sistemas: el
        // orden de `read_dir` depende del sistema de archivos.
        let mut level: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        level.sort();
        for path in level {
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            // Los enlaces no se siguen: un enlace a un directorio padre haría
            // el recorrido infinito.
            if !metadata.is_file() {
                continue;
            }
            if scanned >= max_files {
                truncated = true;
                break 'search;
            }
            scanned += 1;
            let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_lowercase)
            else {
                continue;
            };
            if let Some(candidates) = by_name.get_mut(&name) {
                candidates.push(Candidate {
                    depth: path.components().count(),
                    bytes: metadata.len(),
                    path,
                });
            }
        }
    }

    let mut found = HashMap::new();
    let mut missing = Vec::new();
    for media in wanted {
        let candidates = by_name
            .get(&media.name.to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let best = candidates.iter().min_by(|left, right| {
            let matches = |candidate: &Candidate| media.bytes > 0 && candidate.bytes == media.bytes;
            matches(right)
                .cmp(&matches(left))
                .then_with(|| left.depth.cmp(&right.depth))
                .then_with(|| left.path.cmp(&right.path))
        });
        match best {
            Some(candidate) => {
                found.insert(media.key, candidate.path.clone());
            }
            None => missing.push(media.name.clone()),
        }
    }
    RelinkOutcome {
        found,
        missing,
        scanned,
        truncated,
    }
}

pub fn summarize(outcome: &RelinkOutcome) -> String {
    let found = outcome.found.len();
    let mut text = format!(
        "{found} medio{} revinculado{}",
        plural(found),
        plural(found)
    );
    if !outcome.missing.is_empty() {
        let sample = outcome.missing.iter().take(3).cloned().collect::<Vec<_>>();
        text.push_str(&format!(
            " · {} sin localizar: {}",
            outcome.missing.len(),
            sample.join(", ")
        ));
    }
    if outcome.truncated {
        text.push_str(" · carpeta demasiado grande, la búsqueda se detuvo antes de terminar");
    }
    text
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("novacut-relink-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn write(&self, relative: &str, bytes: usize) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, vec![9u8; bytes]).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_moved_shoot_is_resolved_in_one_pass() {
        let dir = TempDir::new("moved");
        let good = dir.write("rodaje/dia 2/A001.mov", 5000);
        dir.write("rodaje/comprimidos/A001.mov", 120);
        let audio = dir.write("rodaje/audio/ENTREVISTA.WAV", 800);

        let wanted = vec![
            MissingMedia {
                key: 0,
                name: "A001.mov".into(),
                bytes: 5000,
            },
            MissingMedia {
                key: 1,
                name: "entrevista.wav".into(),
                bytes: 0,
            },
            MissingMedia {
                key: 2,
                name: "B009.mov".into(),
                bytes: 100,
            },
        ];
        let outcome = find_missing_media(&wanted, &dir.0, 10_000);
        assert_eq!(outcome.found.get(&0), Some(&good));
        assert_eq!(outcome.found.get(&1), Some(&audio));
        assert_eq!(outcome.found.get(&2), None);
        assert_eq!(outcome.missing, vec!["B009.mov".to_owned()]);
        assert_eq!(outcome.scanned, 3);
        assert!(!outcome.truncated);
        assert!(
            summarize(&outcome).starts_with("2 medios revinculados · 1 sin localizar: B009.mov")
        );
    }

    #[test]
    fn ties_are_broken_the_same_way_every_time() {
        let dir = TempDir::new("ties");
        let shallow = dir.write("A001.mov", 10);
        dir.write("a/b/A001.mov", 10);
        let wanted = |bytes| {
            vec![MissingMedia {
                key: 7,
                name: "A001.mov".into(),
                bytes,
            }]
        };
        // Sin tamaño útil manda la profundidad; con tamaño útil, la coincidencia.
        assert_eq!(
            find_missing_media(&wanted(0), &dir.0, 10_000).found.get(&7),
            Some(&shallow)
        );
        assert_eq!(
            find_missing_media(&wanted(999), &dir.0, 10_000)
                .found
                .get(&7),
            Some(&shallow)
        );
        let deep = dir.write("a/b/B002.mov", 42);
        let wanted_deep = vec![MissingMedia {
            key: 8,
            name: "B002.mov".into(),
            bytes: 42,
        }];
        assert_eq!(
            find_missing_media(&wanted_deep, &dir.0, 10_000)
                .found
                .get(&8),
            Some(&deep)
        );
    }

    #[test]
    fn nothing_to_look_for_a_missing_folder_and_a_capped_walk_are_all_safe() {
        let dir = TempDir::new("edges");
        dir.write("rodaje/A001.mov", 10);
        assert_eq!(
            find_missing_media(&[], &dir.0, 10_000),
            RelinkOutcome::default()
        );

        let wanted = vec![MissingMedia {
            key: 0,
            name: "A001.mov".into(),
            bytes: 10,
        }];
        let absent = find_missing_media(&wanted, &dir.0.join("no-existe"), 10_000);
        assert!(absent.found.is_empty());
        assert_eq!(absent.missing, vec!["A001.mov".to_owned()]);
        assert!(!absent.truncated);

        let capped = find_missing_media(&wanted, &dir.0, 0);
        assert!(capped.truncated);
        assert_eq!(capped.scanned, 0);
        assert!(capped.found.is_empty());
        assert!(summarize(&capped).contains("demasiado grande"));
    }
}
