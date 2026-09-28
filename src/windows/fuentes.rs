//! Fuentes instaladas para los títulos: listado por familia y estilo (con
//! el nombre que lleva la propia fuente, no el del archivo) y resolución de
//! la fuente de un título cuando el proyecto viaja a otro equipo.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct FontEntry {
    pub family: String,
    pub style: String,
    pub path: PathBuf,
}

impl FontEntry {
    pub fn label(&self) -> String {
        if self.style.is_empty() || self.style.eq_ignore_ascii_case("regular") {
            self.family.clone()
        } else {
            format!("{} {}", self.family, self.style)
        }
    }
}

/// Carpetas donde el sistema guarda fuentes, las del usuario incluidas.
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("fonts")))
    {
        dirs.push(directory);
    }
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
    }
    for path in ["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"] {
        dirs.push(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Library").join("Fonts"));
    }
    dirs.push(PathBuf::from("/usr/share/fonts/truetype"));
    dirs
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ttf" | "otf")
    )
}

/// Familia y estilo leídos de la tabla `name` (prefiere los nombres
/// tipográficos, que agrupan negritas y cursivas bajo la misma familia).
pub fn read_names(path: &Path) -> Option<(String, String)> {
    let data = std::fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let pick = |ids: &[u16]| {
        ids.iter().find_map(|id| {
            face.names()
                .into_iter()
                .filter(|name| name.name_id == *id && name.is_unicode())
                .find_map(|name| name.to_string())
                .filter(|text| !text.trim().is_empty())
        })
    };
    let family = pick(&[16, 1])?;
    let style = pick(&[17, 2]).unwrap_or_default();
    Some((family, style))
}

fn scan() -> Vec<FontEntry> {
    let mut entries = Vec::new();
    for directory in font_dirs() {
        let Ok(listing) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in listing.flatten() {
            let path = entry.path();
            // Las fuentes enormes (CJK) tardan en leerse y no se usan en
            // rótulos occidentales; se omiten del listado.
            let small = entry.metadata().is_ok_and(|meta| meta.len() < 40 * 1024 * 1024);
            if !is_font_file(&path) || !small {
                continue;
            }
            if let Some((family, style)) = read_names(&path) {
                entries.push(FontEntry { family, style, path });
            }
        }
    }
    entries.sort_by(|left, right| {
        left.family
            .to_lowercase()
            .cmp(&right.family.to_lowercase())
            .then_with(|| left.style.cmp(&right.style))
    });
    entries.dedup_by(|later, earlier| later.family == earlier.family && later.style == earlier.style);
    entries
}

static INSTALLED: OnceLock<Vec<FontEntry>> = OnceLock::new();

/// Empieza a leer las fuentes en segundo plano (tarda un momento en un
/// Windows con cientos de fuentes); `installed` devuelve `None` mientras.
pub fn start_loading() {
    if INSTALLED.get().is_none() {
        std::thread::spawn(|| {
            let _ = INSTALLED.set(scan());
        });
    }
}

pub fn installed() -> Option<&'static [FontEntry]> {
    INSTALLED.get().map(Vec::as_slice)
}

/// Fuente con la que dibujar un título: la elegida si existe; si el
/// proyecto viene de otro equipo, la del mismo nombre de archivo en las
/// carpetas de fuentes; si no, la predeterminada.
pub fn resolve(requested: Option<&Path>) -> Option<PathBuf> {
    if let Some(requested) = requested {
        if requested.is_file() {
            return Some(requested.to_path_buf());
        }
        if let Some(name) = requested.file_name() {
            if let Some(found) = font_dirs()
                .into_iter()
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
            {
                return Some(found);
            }
        }
    }
    super::find_font()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_has_a_readable_family_name() {
        let Some(font) = super::super::find_font() else {
            return;
        };
        let (family, _) = read_names(&font).expect("la fuente por defecto debe leerse");
        assert!(!family.is_empty());
    }

    #[test]
    fn missing_font_falls_back_by_file_name_or_default() {
        let Some(default) = super::super::find_font() else {
            return;
        };
        let moved = Path::new("Z:/otro-equipo/Fonts").join(default.file_name().unwrap());
        assert!(resolve(Some(&moved)).is_some_and(|path| path.is_file()));
        assert!(resolve(Some(Path::new("Z:/nada/inexistente.ttf"))).is_some());
    }
}
