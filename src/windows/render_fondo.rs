//! Render en segundo plano por tramos, como las barras de Premiere: el
//! montaje se parte por sus cortes; cada tramo se clasifica según lo que
//! cuesta reproducirlo (nada, amarillo o rojo) y se identifica por una firma
//! de su contenido. Un tramo ya renderizado sigue valiendo mientras su
//! contenido no cambie, aunque se edite otra parte o se desplace.

use super::estabilizar::{Stabilizer, TimeInterpolation};
use super::{montaje, RoughClip};
use std::hash::{Hash, Hasher};

/// Lo que cuesta reproducir un tramo en tiempo real.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Coste {
    /// Un plano limpio: se reproduce tal cual, no hace falta render.
    Ligero,
    /// Efectos, títulos, transiciones o dos capas: amarillo.
    Medio,
    /// Flujo óptico, estabilización con análisis, rampas, anidados o tres
    /// capas o más: rojo.
    Pesado,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tramo {
    pub inicio: f64,
    pub fin: f64,
    pub coste: Coste,
    /// Firma del contenido (clips relativos al tramo, lienzo, subtítulos).
    pub firma: u64,
}

/// ¿Este clip, solo, se reproduce sin componer nada?
fn limpio(clip: &RoughClip) -> bool {
    clip.title.is_none()
        && !clip.is_adjustment
        && clip.nested.is_none()
        && clip.transition.is_none()
        && (clip.speed - 1.0).abs() < 1e-6
        && clip.speed_ramp.as_ref().is_none_or(|puntos| puntos.is_empty())
        && clip.keyframes.as_ref().is_none_or(|claves| claves.is_empty())
        && clip.anim.is_empty()
        && clip.wheels.is_none()
        && clip.curves.is_none()
        && clip.chroma.is_none()
        && clip.mask.is_none()
        && clip.lut.is_none()
        && clip.fusion.blend_mode().is_none()
        && clip.exposure.abs() < 1e-6
        && clip.contrast.abs() < 1e-6
        && clip.saturation.abs() < 1e-6
        && clip.vignette.abs() < 1e-6
        && clip.blur.abs() < 1e-6
        && clip.rotation.abs() < 1e-6
        && (clip.opacity - 100.0).abs() < 1e-6
        && (clip.scale_percent - 100.0).abs() < 1e-6
        && clip.position_x.abs() < 1e-6
        && clip.position_y.abs() < 1e-6
        && clip.fade_in_seconds <= 0.0
        && clip.fade_out_seconds <= 0.0
        && !clip.fx.has_video_effects()
}

/// Pesado aunque esté solo: lo que FFmpeg no compone en tiempo real.
fn pesado(clip: &RoughClip) -> bool {
    clip.nested.is_some()
        || clip.speed_ramp.as_ref().is_some_and(|puntos| !puntos.is_empty())
        || clip.fx.interpolation == TimeInterpolation::FlujoOptico
        || (clip.fx.stabilize && clip.fx.stabilizer == Stabilizer::Deformacion)
        || clip.fx.reverse
        || clip.fx.denoise > 0.001
}

/// Coste de lo que se ve en un tramo: los clips de vídeo activos en él.
pub fn coste(activos: &[&RoughClip]) -> Coste {
    let capas = activos.len();
    if capas == 0 {
        return Coste::Ligero;
    }
    if capas >= 3 || activos.iter().any(|clip| pesado(clip)) {
        return Coste::Pesado;
    }
    if capas == 2 || activos.iter().any(|clip| !limpio(clip)) {
        return Coste::Medio;
    }
    Coste::Ligero
}

/// Tramos del montaje entre cortes consecutivos de cualquier pista. Solo
/// cuentan los clips que se ven (vídeo, títulos, capas de ajuste).
/// `contexto` entra en la firma: lienzo, subtítulos quemados…
pub fn tramos(clips: &[RoughClip], contexto: u64) -> Vec<Tramo> {
    let visibles: Vec<&RoughClip> = clips.iter().filter(|clip| clip.enabled && clip.has_video).collect();
    let mut cortes: Vec<f64> = visibles
        .iter()
        .flat_map(|clip| [clip.timeline_start, clip.timeline_start + clip.duration()])
        .filter(|t| t.is_finite())
        .collect();
    cortes.sort_by(f64::total_cmp);
    cortes.dedup_by(|a, b| (*a - *b).abs() < montaje::MIN_CLIP);
    let mut salida = Vec::new();
    for par in cortes.windows(2) {
        let (inicio, fin) = (par[0], par[1]);
        let medio = (inicio + fin) / 2.0;
        let activos: Vec<&RoughClip> = visibles
            .iter()
            .copied()
            .filter(|clip| clip.timeline_start <= medio && medio < clip.timeline_start + clip.duration())
            .collect();
        if activos.is_empty() {
            continue;
        }
        let (ventana, preroll) = montaje::window(clips, inicio, fin);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&ventana).unwrap_or_default().hash(&mut hasher);
        preroll.to_bits().hash(&mut hasher);
        (fin - inicio).to_bits().hash(&mut hasher);
        contexto.hash(&mut hasher);
        salida.push(Tramo {
            inicio,
            fin,
            coste: coste(&activos),
            firma: hasher.finish(),
        });
    }
    salida
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn plano(ruta: &str, inicio: f64, duracion: f64) -> RoughClip {
        RoughClip {
            path: PathBuf::from(ruta),
            timeline_start: inicio,
            out_seconds: duracion,
            source_duration_seconds: Some(duracion),
            has_video: true,
            ..Default::default()
        }
    }

    #[test]
    fn un_plano_limpio_no_necesita_render_y_los_efectos_si() {
        let mut clips = vec![plano("a.mp4", 0.0, 2.0), plano("b.mp4", 2.0, 2.0)];
        clips[1].exposure = 0.3;
        let tramos = tramos(&clips, 0);
        assert_eq!(tramos.len(), 2);
        assert_eq!(tramos[0].coste, Coste::Ligero);
        assert_eq!(tramos[1].coste, Coste::Medio);
        clips[1].fx.interpolation = TimeInterpolation::FlujoOptico;
        assert_eq!(super::tramos(&clips, 0)[1].coste, Coste::Pesado);
    }

    #[test]
    fn las_capas_superpuestas_parten_el_montaje_y_suben_el_coste() {
        let mut encima = plano("titulo", 1.0, 1.0);
        encima.track = 1;
        let tercera = RoughClip { track: 2, ..encima.clone() };
        let clips = vec![plano("a.mp4", 0.0, 3.0), encima, tercera];
        let tramos = tramos(&clips, 0);
        let costes: Vec<Coste> = tramos.iter().map(|t| t.coste).collect();
        assert_eq!(costes, vec![Coste::Ligero, Coste::Pesado, Coste::Ligero]);
        assert_eq!((tramos[1].inicio, tramos[1].fin), (1.0, 2.0));
    }

    #[test]
    fn editar_otra_parte_o_desplazar_conserva_la_firma() {
        let mut clips = vec![plano("a.mp4", 0.0, 2.0), plano("b.mp4", 2.0, 2.0)];
        clips[1].exposure = 0.5;
        let antes = tramos(&clips, 7);
        // Cambiar el primer plano no toca la firma del segundo…
        let mut editado = clips.clone();
        editado[0].saturation = 0.4;
        assert_eq!(tramos(&editado, 7)[1].firma, antes[1].firma);
        // …desplazarlo tampoco…
        let mut movido = clips.clone();
        movido[1].timeline_start = 5.0;
        let ultimo = tramos(&movido, 7).last().cloned().unwrap();
        assert_eq!((ultimo.inicio, ultimo.firma), (5.0, antes[1].firma));
        // …y cambiar su contenido o el lienzo sí.
        let mut otro = clips.clone();
        otro[1].exposure = 0.6;
        assert_ne!(tramos(&otro, 7)[1].firma, antes[1].firma);
        assert_ne!(tramos(&clips, 8)[1].firma, antes[1].firma);
    }
}
