//! Presupuesto de disco para la caché de proxies.
//!
//! El host de Windows escribe los proxies junto al proyecto, así que la carpeta
//! puede contener material del usuario. Todo lo de aquí se limita al sufijo que
//! escribe el generador (`-proxy.mp4`): un archivo que no lo lleve no se cuenta
//! ni se borra nunca, aunque esté en la misma carpeta.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Sufijo exacto que produce el generador de proxies.
pub const PROXY_SUFFIX: &str = "-proxy.mp4";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheUsage {
    pub files: usize,
    pub total_bytes: u64,
    /// Ocupado por proxies que el proyecto abierto tiene enlazados.
    pub in_use_bytes: u64,
}

impl CacheUsage {
    pub fn evictable_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.in_use_bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Eviction {
    pub removed: usize,
    pub freed_bytes: u64,
    pub remaining_bytes: u64,
    /// El proyecto abierto ocupa por sí solo más que el límite. No se toca:
    /// desalojarlo obligaría a recodificar lo que la preview pide ahora mismo.
    pub over_budget: bool,
}

struct Entry {
    path: PathBuf,
    bytes: u64,
    accessed: SystemTime,
}

fn is_proxy(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.len() > PROXY_SUFFIX.len() && name.ends_with(PROXY_SUFFIX))
}

fn scan(dir: &Path) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !is_proxy(&path) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            // NTFS puede tener el atime desactivado; entonces la fecha de
            // escritura es la mejor señal de antigüedad disponible.
            let accessed = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some(Entry {
                path,
                bytes: metadata.len(),
                accessed,
            })
        })
        .collect()
}

pub fn usage(dir: &Path, in_use: &HashSet<PathBuf>) -> CacheUsage {
    let entries = scan(dir);
    CacheUsage {
        files: entries.len(),
        total_bytes: entries.iter().map(|entry| entry.bytes).sum(),
        in_use_bytes: entries
            .iter()
            .filter(|entry| in_use.contains(&entry.path))
            .map(|entry| entry.bytes)
            .sum(),
    }
}

/// Recorta la caché al presupuesto desalojando primero lo que hace más tiempo
/// que no se abre. Un límite de cero desactiva el recorte. Los proxies
/// enlazados por el proyecto abierto nunca se desalojan.
pub fn enforce_budget(dir: &Path, limit_bytes: u64, in_use: &HashSet<PathBuf>) -> Eviction {
    let mut entries = scan(dir);
    let mut remaining: u64 = entries.iter().map(|entry| entry.bytes).sum();
    if limit_bytes == 0 {
        return Eviction {
            remaining_bytes: remaining,
            ..Eviction::default()
        };
    }
    // Más viejo primero; la ruta desempata para que dos marcas iguales no den
    // desalojos distintos en cada pasada.
    entries.sort_by(|a, b| {
        a.accessed
            .cmp(&b.accessed)
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut removed = 0;
    let mut freed = 0;
    for entry in entries {
        if remaining <= limit_bytes {
            break;
        }
        if in_use.contains(&entry.path) || std::fs::remove_file(&entry.path).is_err() {
            continue;
        }
        removed += 1;
        freed += entry.bytes;
        remaining -= entry.bytes;
    }
    Eviction {
        removed,
        freed_bytes: freed,
        remaining_bytes: remaining,
        over_budget: remaining > limit_bytes,
    }
}

/// Huella del medio de origen: tamaño y fecha de escritura. Si el usuario
/// reemplaza el archivo por otra versión, la huella cambia y el proxy anterior
/// deja de considerarse válido en vez de mostrar contenido que ya no existe.
pub fn fingerprint(source: &Path) -> Option<u64> {
    let metadata = std::fs::metadata(source).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |delta| delta.as_secs());
    // Las rutas de Windows no distinguen mayúsculas: normalizarlas evita que el
    // mismo archivo escrito de dos formas genere dos proxies del mismo vídeo.
    let path = source.to_string_lossy();
    let path = if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.into_owned()
    };
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    mix(path.as_bytes());
    mix(&metadata.len().to_le_bytes());
    mix(&modified.to_le_bytes());
    Some(hash)
}

/// Deja solo caracteres que cualquier sistema de archivos acepta y acota el
/// largo, porque el nombre del medio puede ser una frase entera.
fn safe_stem(source: &Path) -> String {
    let stem: String = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("clip")
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let stem = stem.trim().trim_end_matches('.').to_owned();
    if stem.is_empty() {
        "clip".to_owned()
    } else {
        stem
    }
}

/// Nombre del proxy de un medio. Depende del medio, no del clip que lo usa: dos
/// clips del mismo archivo comparten proxy y reordenarlos no reasigna nombres.
pub fn proxy_file_name(source: &Path, fingerprint: u64) -> String {
    format!("{}-{fingerprint:016x}{PROXY_SUFFIX}", safe_stem(source))
}

/// Ruta que le toca al proxy de este medio, o `None` si el medio no se puede leer.
pub fn proxy_target(dir: &Path, source: &Path) -> Option<PathBuf> {
    Some(dir.join(proxy_file_name(source, fingerprint(source)?)))
}

/// ¿El proxy enlazado corresponde al medio tal y como está hoy en disco? Los
/// proxies con el nombre antiguo, por índice de clip, dan `false`: siguen
/// sirviendo, pero la interfaz puede ofrecer regenerarlos.
pub fn proxy_matches_source(proxy: &Path, source: &Path) -> bool {
    match (
        proxy.file_name().and_then(|name| name.to_str()),
        fingerprint(source),
    ) {
        (Some(name), Some(fingerprint)) => name == proxy_file_name(source, fingerprint),
        _ => false,
    }
}

/// Refresca la marca de acceso al reutilizar un proxy, para que el presupuesto
/// no lo desaloje por parecer antiguo. NTFS suele no actualizarla al leer.
pub fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_times(std::fs::FileTimes::new().set_accessed(SystemTime::now()));
    }
}

pub fn format_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb < 0.1 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{gb:.2} GB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("novacut-proxy-cache-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Cuatro proxies de 1000 bytes, del más antiguo (0) al más reciente (3),
    /// más un archivo del usuario que nunca debe entrar en el cálculo.
    fn populate(dir: &Path) -> Vec<PathBuf> {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
        std::fs::write(dir.join("notas del montaje.mp4"), b"ajeno").unwrap();
        (0..4)
            .map(|index| {
                let path = dir.join(format!("clip{index}-{index}{PROXY_SUFFIX}"));
                std::fs::write(&path, vec![7u8; 1000]).unwrap();
                let stamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + index * 60);
                filetime_set(&path, stamp);
                path
            })
            .collect()
    }

    fn filetime_set(path: &Path, stamp: SystemTime) {
        // Sin dependencias externas: se ajusta el atime/mtime con `utimensat`
        // vía `std::fs::File::set_times`, disponible desde Rust 1.75.
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_times(
            std::fs::FileTimes::new()
                .set_accessed(stamp)
                .set_modified(stamp),
        )
        .unwrap();
    }

    #[test]
    fn usage_counts_only_generated_proxies_and_splits_the_open_project() {
        let dir = TempDir::new("usage");
        let proxies = populate(&dir.0);
        let in_use: HashSet<PathBuf> = [proxies[3].clone()].into_iter().collect();
        let usage = usage(&dir.0, &in_use);
        assert_eq!(usage.files, 4);
        assert_eq!(usage.total_bytes, 4000);
        assert_eq!(usage.in_use_bytes, 1000);
        assert_eq!(usage.evictable_bytes(), 3000);
        assert!(dir.0.join("notas del montaje.mp4").is_file());
    }

    #[test]
    fn eviction_removes_the_least_recently_used_and_stops_once_it_fits() {
        let dir = TempDir::new("lru");
        let proxies = populate(&dir.0);
        let eviction = enforce_budget(&dir.0, 2500, &HashSet::new());
        assert_eq!(eviction.removed, 2);
        assert_eq!(eviction.freed_bytes, 2000);
        assert_eq!(eviction.remaining_bytes, 2000);
        assert!(!eviction.over_budget);
        assert!(!proxies[0].exists() && !proxies[1].exists());
        assert!(proxies[2].is_file() && proxies[3].is_file());
        assert!(dir.0.join("notas del montaje.mp4").is_file());
    }

    #[test]
    fn linked_proxies_survive_and_the_shortfall_is_reported() {
        let dir = TempDir::new("linked");
        let proxies = populate(&dir.0);
        let in_use: HashSet<PathBuf> = proxies[..2].iter().cloned().collect();
        let eviction = enforce_budget(&dir.0, 1500, &in_use);
        assert!(proxies[0].is_file() && proxies[1].is_file());
        assert_eq!(eviction.removed, 2);
        assert_eq!(eviction.remaining_bytes, 2000);
        assert!(eviction.over_budget);
    }

    #[test]
    fn a_zero_budget_a_fitting_cache_and_a_missing_folder_leave_everything_alone() {
        let dir = TempDir::new("noop");
        let proxies = populate(&dir.0);
        assert_eq!(
            enforce_budget(&dir.0, 0, &HashSet::new()),
            Eviction {
                remaining_bytes: 4000,
                ..Eviction::default()
            }
        );
        assert_eq!(enforce_budget(&dir.0, 4000, &HashSet::new()).removed, 0);
        assert!(proxies.iter().all(|path| path.is_file()));

        let missing = dir.0.join("sin-crear");
        assert_eq!(usage(&missing, &HashSet::new()), CacheUsage::default());
        assert_eq!(
            enforce_budget(&missing, 10, &HashSet::new()),
            Eviction::default()
        );
    }

    #[test]
    fn refreshing_a_reused_proxy_moves_it_out_of_the_eviction_queue() {
        let dir = TempDir::new("refresh");
        let proxies = populate(&dir.0);
        filetime_set(
            &proxies[0],
            SystemTime::UNIX_EPOCH + Duration::from_secs(9_999),
        );
        let eviction = enforce_budget(&dir.0, 3500, &HashSet::new());
        assert_eq!(eviction.removed, 1);
        assert!(proxies[0].is_file());
        assert!(!proxies[1].exists());
    }

    #[test]
    fn the_proxy_name_follows_the_media_not_the_clip_that_uses_it() {
        let dir = TempDir::new("identity");
        let source = dir.0.join("Entrevista final.MOV");
        std::fs::write(&source, vec![1u8; 4096]).unwrap();
        filetime_set(&source, SystemTime::UNIX_EPOCH + Duration::from_secs(5_000));

        let target = proxy_target(&dir.0, &source).unwrap();
        let name = target.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("Entrevista final-"), "{name}");
        assert!(name.ends_with(PROXY_SUFFIX), "{name}");
        // Estable entre llamadas: reordenar clips no reasigna proxies.
        assert_eq!(proxy_target(&dir.0, &source).unwrap(), target);
        assert!(proxy_matches_source(&target, &source));

        // Un archivo distinto con el mismo nombre no comparte proxy.
        let otra = dir.0.join("otra").join("Entrevista final.MOV");
        std::fs::create_dir_all(otra.parent().unwrap()).unwrap();
        std::fs::write(&otra, vec![1u8; 4096]).unwrap();
        filetime_set(&otra, SystemTime::UNIX_EPOCH + Duration::from_secs(5_000));
        assert_ne!(proxy_target(&dir.0, &otra).unwrap(), target);
    }

    #[test]
    fn replacing_the_media_invalidates_the_proxy_that_described_it() {
        let dir = TempDir::new("stale");
        let source = dir.0.join("toma.mp4");
        std::fs::write(&source, vec![1u8; 4096]).unwrap();
        filetime_set(&source, SystemTime::UNIX_EPOCH + Duration::from_secs(5_000));
        let antes = proxy_target(&dir.0, &source).unwrap();

        // Misma ruta, otro contenido y otra fecha: el proxy anterior ya no vale.
        std::fs::write(&source, vec![2u8; 8192]).unwrap();
        filetime_set(&source, SystemTime::UNIX_EPOCH + Duration::from_secs(9_000));
        let despues = proxy_target(&dir.0, &source).unwrap();
        assert_ne!(antes, despues);
        assert!(!proxy_matches_source(&antes, &source));
        assert!(proxy_matches_source(&despues, &source));

        // El nombre antiguo por índice de clip sigue existiendo, pero no se
        // confunde con uno al día.
        assert!(!proxy_matches_source(
            &dir.0.join("toma-0-proxy.mp4"),
            &source
        ));
        // Un medio que ya no está en disco no puede validar ni nombrar nada.
        std::fs::remove_file(&source).unwrap();
        assert!(fingerprint(&source).is_none());
        assert!(proxy_target(&dir.0, &source).is_none());
        assert!(!proxy_matches_source(&despues, &source));
    }

    #[test]
    fn awkward_media_names_still_produce_a_usable_file_name() {
        let hash = 0x0123_4567_89ab_cdef;
        assert_eq!(
            // El separador de Windows lo resuelve `file_stem` en su sistema; aquí
            // se comprueba el saneado de caracteres que NTFS no admite.
            proxy_file_name(Path::new("a:b*c?.mov"), hash),
            format!("a_b_c_-0123456789abcdef{PROXY_SUFFIX}")
        );
        let largo = proxy_file_name(Path::new(&format!("{}.mov", "n".repeat(200))), hash);
        assert_eq!(largo.len(), 48 + 1 + 16 + PROXY_SUFFIX.len());
        assert_eq!(
            proxy_file_name(Path::new("...mov"), hash),
            format!("clip-0123456789abcdef{PROXY_SUFFIX}")
        );
        assert_eq!(
            proxy_file_name(Path::new("Toma 3.final.mov"), hash),
            format!("Toma 3.final-0123456789abcdef{PROXY_SUFFIX}")
        );
        // Sigue siendo un proxy a ojos del presupuesto.
        assert!(is_proxy(Path::new(&largo)));
    }

    #[test]
    fn sizes_read_in_gigabytes_only_when_they_are_large_enough() {
        assert_eq!(format_size(2_500_000_000), "2.50 GB");
        assert_eq!(format_size(37_000_000), "37 MB");
        assert_eq!(format_size(0), "0 MB");
    }
}
