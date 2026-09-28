//! Estabilizador de deformación de dos pasadas (el «Warp Stabilizer» de
//! Premiere, con libvidstab) e interpolación de tiempo para cámara lenta
//! (muestreo, mezcla o flujo óptico con `minterpolate`).
//!
//! La primera pasada analiza el tramo de origen y deja un archivo de
//! transformaciones en caché; la segunda lo aplica al renderizar. Las dos
//! deben ver exactamente los mismos fotogramas, así que ambas leen el medio
//! con el mismo `-ss`/`-t` y la transformación va la primera de la cadena.

use super::RoughClip;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stabilizer {
    /// `deshake`: un paso, sin análisis previo.
    #[default]
    Rapido,
    /// vidstab: análisis de movimiento y suavizado de la trayectoria.
    Deformacion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInterpolation {
    /// Repite o salta fotogramas (lo de siempre).
    #[default]
    Muestreo,
    /// Funde fotogramas vecinos.
    Mezcla,
    /// Inventa los fotogramas intermedios estimando el movimiento.
    FlujoOptico,
}

impl TimeInterpolation {
    pub const ALL: [Self; 3] = [Self::Muestreo, Self::Mezcla, Self::FlujoOptico];

    pub fn label(self) -> &'static str {
        match self {
            Self::Muestreo => "Muestreo de fotogramas",
            Self::Mezcla => "Mezcla de fotogramas",
            Self::FlujoOptico => "Flujo óptico",
        }
    }

    /// Filtro que convierte a la cadencia del proyecto, o `None` para el
    /// conformado habitual.
    pub fn filter(self, frame_rate: &str) -> Option<String> {
        match self {
            Self::Muestreo => None,
            Self::Mezcla => Some(format!(",minterpolate=fps={frame_rate}:mi_mode=blend")),
            Self::FlujoOptico => Some(format!(
                ",minterpolate=fps={frame_rate}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1"
            )),
        }
    }
}

/// ¿Trae este FFmpeg libvidstab? El de Windows (gyan essentials) sí; otros
/// builds no. Se pregunta una vez.
pub fn vidstab_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new(super::tool_path("ffmpeg.exe"))
            .args(["-hide_banner", "-filters"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("vidstabtransform"))
            .unwrap_or(false)
    })
}

fn cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("estabilizar"))
        .unwrap_or_else(|| std::env::temp_dir().join("novacut-estabilizar"))
}

/// Archivo de análisis de un clip: depende del archivo (ruta, tamaño,
/// fecha) y del tramo que se lee, así que recortar o reenlazar invalida el
/// análisis viejo en vez de reutilizarlo mal.
pub fn analysis_path(clip: &RoughClip) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    clip.path.hash(&mut hasher);
    if let Ok(metadata) = std::fs::metadata(&clip.path) {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified() {
            modified.hash(&mut hasher);
        }
    }
    format!("{:.4}", clip.in_seconds).hash(&mut hasher);
    format!("{:.4}", clip.source_duration()).hash(&mut hasher);
    cache_dir().join(format!("{:016x}.trf", hasher.finish()))
}

/// ¿Hay que estabilizar este clip con vidstab y está ya analizado?
pub fn ready(clip: &RoughClip) -> bool {
    wants_warp(clip) && analysis_path(clip).is_file()
}

pub fn wants_warp(clip: &RoughClip) -> bool {
    clip.fx.stabilize
        && clip.fx.stabilizer == Stabilizer::Deformacion
        && clip.freeze_at.is_none()
        && clip.title.is_none()
        && !clip.is_adjustment
        && clip.nested.is_none()
        && !clip.path.as_os_str().is_empty()
        && !super::is_image_file(&clip.path)
}

/// Pasada 1: analiza el tramo del clip. Escribe en un temporal y lo mueve al
/// terminar, para que un análisis interrumpido nunca pase por bueno.
pub fn analyze(clip: &RoughClip) -> Result<PathBuf, String> {
    let target = analysis_path(clip);
    if target.is_file() {
        return Ok(target);
    }
    std::fs::create_dir_all(cache_dir())
        .map_err(|error| format!("No se pudo crear la caché de estabilización: {error}"))?;
    let partial = target.with_extension(format!("{}.part", std::process::id()));
    let output = Command::new(super::tool_path("ffmpeg.exe"))
        .args(["-v", "error", "-y", "-ss", &super::format_seconds(clip.in_seconds)])
        .args(["-t", &super::format_seconds(clip.source_duration())])
        .arg("-i")
        .arg(&clip.path)
        .args([
            "-an",
            "-vf",
            &format!(
                "vidstabdetect=shakiness=6:accuracy=12:result={}",
                escape_filter_path(&partial)
            ),
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("FFmpeg no está disponible: {error}"))?;
    if !output.status.success() || !partial.is_file() {
        let _ = std::fs::remove_file(&partial);
        return Err(String::from_utf8_lossy(&output.stderr)
            .lines()
            .last()
            .unwrap_or("el análisis falló")
            .to_owned());
    }
    std::fs::rename(&partial, &target)
        .map_err(|error| format!("No se pudo guardar el análisis: {error}"))?;
    Ok(target)
}

/// Pasada 2, al principio de la cadena del clip. Suavizado 0 … 1 → de 5 a
/// 60 fotogramas de ventana; `optzoom` amplía lo justo para no ver bordes.
pub fn transform_filter(clip: &RoughClip) -> String {
    let smoothing = 5.0 + clip.fx.stabilize_smoothing.unwrap_or(0.5).clamp(0.0, 1.0) * 55.0;
    format!(
        "vidstabtransform=input={}:smoothing={:.0}:optzoom=1:interpol=bicubic,",
        escape_filter_path(&analysis_path(clip)),
        smoothing
    )
}

/// Ruta como valor de opción dentro de un grafo: el mismo escapado de dos
/// capas que las LUT, ya probado con rutas de Windows.
fn escape_filter_path(path: &Path) -> String {
    super::escape_lut_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_depends_on_the_trimmed_range() {
        let clip = RoughClip {
            path: PathBuf::from("C:\\Grabaciones\\toma.mp4"),
            in_seconds: 1.0,
            out_seconds: 5.0,
            ..Default::default()
        };
        let mut trimmed = clip.clone();
        trimmed.in_seconds = 2.0;
        assert_ne!(analysis_path(&clip), analysis_path(&trimmed));
        assert_eq!(analysis_path(&clip), analysis_path(&clip.clone()));
    }

    #[test]
    fn filter_paths_escape_drive_colons() {
        let escaped = escape_filter_path(Path::new("C:\\Users\\Ana\\a.trf"));
        assert_eq!(escaped, "C\\\\:/Users/Ana/a.trf");
    }

    #[test]
    fn interpolation_modes_build_minterpolate() {
        assert!(TimeInterpolation::Muestreo.filter("25").is_none());
        assert!(TimeInterpolation::Mezcla.filter("25").unwrap().contains("mi_mode=blend"));
        assert!(TimeInterpolation::FlujoOptico.filter("30000/1001").unwrap().contains("mi_mode=mci"));
    }
}
