//! Exportación por tramos para montajes con muchos clips.
//!
//! Cada clip es una entrada de FFmpeg con su decodificador, y cada fotograma
//! atraviesa una superposición por clip. Una entrevista editada por texto o
//! con las pausas acortadas deja cientos de cortes: medido, 200 cortes de un
//! 1080p tardaban 9 minutos en exportar 48 s y FFmpeg llegaba a 25 GB de
//! memoria (por tramos: 18 s y 1,7 GB). Aquí la timeline se parte en
//! ventanas con pocos clips activos, cada una se compone sola (con su
//! preroll, ver `montaje::window`) con el vídeo ya en el códec final y el
//! audio en PCM, y al final se unen copiando el vídeo y codificando el audio
//! una sola vez: sin chasquidos en las uniones y con la normalización de
//! sonoridad sobre el total.

use super::{
    format_seconds, montaje, run_export_once, tool_path, ExportFormat, RenderJob, RenderProgress,
    RoughClip, BACKGROUND_PROCESS,
};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(not(windows))]
use super::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Por encima de tantos clips se exporta por tramos.
pub const UMBRAL_CLIPS: usize = 24;
/// Clips activos como mucho en cada tramo (salvo que uno solo ya los pase).
pub const MAX_CLIPS_POR_TRAMO: usize = 8;

thread_local! {
    static EN_TRAMO: Cell<bool> = const { Cell::new(false) };
}

/// Verdadero mientras se compone un tramo intermedio: el audio va en PCM y
/// sin opciones de MP4 (ver `ajustar_codec`).
pub fn en_tramo() -> bool {
    EN_TRAMO.with(Cell::get)
}

/// Argumentos de códec de un tramo: el vídeo tal cual (se copiará) y el
/// audio en PCM para unirlo sin huecos.
pub fn ajustar_codec(args: &mut Vec<String>) {
    let mut limpio = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-c:a" | "-b:a" | "-movflags" => i += 2,
            _ => {
                limpio.push(args[i].clone());
                i += 1;
            }
        }
    }
    limpio.extend(["-c:a", "pcm_s16le"].map(str::to_owned));
    *args = limpio;
}

/// ¿Conviene exportar por tramos? Solo con muchos clips y en formatos que
/// se pueden unir sin recomprimir (el GIF necesita una paleta global).
pub fn conviene(job: &RenderJob) -> bool {
    job.format != ExportFormat::Gif && super::prepare_render_clips(&job.clips).len() > UMBRAL_CLIPS
}

/// Ventanas `[a, b)` sobre `[desde, hasta)`, alineadas a fotograma, con
/// `MAX_CLIPS_POR_TRAMO` clips como mucho (contando los del preroll).
pub fn ventanas(clips: &[RoughClip], desde: f64, hasta: f64, fps: f64) -> Vec<(f64, f64)> {
    let fotograma = |t: f64| (t * fps).round() / fps;
    let mut cortes: Vec<f64> = clips
        .iter()
        .flat_map(|clip| [clip.timeline_start, clip.timeline_start + clip.duration()])
        .map(fotograma)
        .filter(|t| *t > desde + 0.5 / fps && *t < hasta - 0.5 / fps)
        .collect();
    cortes.push(hasta);
    cortes.sort_by(f64::total_cmp);
    cortes.dedup_by(|a, b| (*a - *b).abs() < 0.5 / fps);
    let mut salida = Vec::new();
    let mut inicio = desde;
    let mut siguiente = 0;
    while inicio < hasta - 0.5 / fps {
        // Al menos un corte por tramo, para avanzar siempre.
        let mut fin = cortes[siguiente];
        let mut j = siguiente + 1;
        while j < cortes.len() && montaje::window(clips, inicio, cortes[j]).0.len() <= MAX_CLIPS_POR_TRAMO {
            fin = cortes[j];
            j += 1;
        }
        salida.push((inicio, fin));
        siguiente = j;
        inicio = fin;
    }
    salida
}

fn comillas(ruta: &Path) -> String {
    ruta.to_string_lossy().replace('\'', "'\\''")
}

/// Exporta `job` por tramos y los une en `job.output`.
pub fn exportar(
    job: &RenderJob,
    hw: Option<super::aceleracion::HwBackend>,
    cancel: &AtomicBool,
    progress: &Arc<std::sync::Mutex<RenderProgress>>,
) -> Result<(), String> {
    let desde = job.skip.max(0.0);
    let hasta = desde + job.output_length();
    let tramos = ventanas(&job.clips, desde, hasta, job.timebase.fps());
    let carpeta = std::env::temp_dir().join(format!(
        "novacut-tramos-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&carpeta).map_err(|error| format!("No se pudo crear la carpeta temporal: {error}"))?;
    let resultado = exportar_en(job, hw, cancel, progress, &tramos, &carpeta);
    let _ = std::fs::remove_dir_all(&carpeta);
    resultado
}

fn exportar_en(
    job: &RenderJob,
    hw: Option<super::aceleracion::HwBackend>,
    cancel: &AtomicBool,
    progress: &Arc<std::sync::Mutex<RenderProgress>>,
    tramos: &[(f64, f64)],
    carpeta: &Path,
) -> Result<(), String> {
    let total: f64 = tramos.iter().map(|(a, b)| b - a).sum::<f64>().max(0.001);
    let mut hecho = 0.0;
    let mut archivos: Vec<PathBuf> = Vec::with_capacity(tramos.len());
    for (n, &(a, b)) in tramos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Exportación cancelada".to_owned());
        }
        let (clips, preroll) = montaje::window(&job.clips, a, b);
        let archivo = carpeta.join(format!("tramo-{n:05}.mkv"));
        let tramo = RenderJob {
            clips,
            output: archivo.clone(),
            skip: preroll,
            length: Some(b - a),
            // La sonoridad se normaliza al unir, sobre el total.
            normalize_loudness: false,
            measured_loudness: None,
            ..job.clone()
        };
        let parcial = Arc::new(std::sync::Mutex::new(RenderProgress::default()));
        let resultado = std::thread::scope(|scope| {
            let hilo = scope.spawn(|| {
                EN_TRAMO.with(|en| en.set(true));
                run_export_once(&tramo, hw, cancel, &parcial)
            });
            while !hilo.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let pct = parcial.lock().map(|p| p.pct).unwrap_or(0.0);
                if let Ok(mut estado) = progress.lock() {
                    estado.pct = ((hecho + pct * (b - a)) / total * 0.97).clamp(0.0, 1.0);
                    estado.note = Some(format!("por tramos: {} de {}", n + 1, tramos.len()));
                }
            }
            hilo.join().unwrap_or_else(|_| Err("El tramo terminó de forma inesperada".to_owned()))
        });
        resultado.map_err(|error| format!("Tramo {} de {}: {error}", n + 1, tramos.len()))?;
        hecho += b - a;
        archivos.push(archivo);
    }
    unir(job, hw, &archivos, carpeta, cancel)?;
    if let Ok(mut estado) = progress.lock() {
        estado.pct = 1.0;
    }
    Ok(())
}

/// Une los tramos copiando el vídeo y codificando el audio una vez.
fn unir(
    job: &RenderJob,
    hw: Option<super::aceleracion::HwBackend>,
    archivos: &[PathBuf],
    carpeta: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let lista = carpeta.join("tramos.txt");
    let texto: String = archivos.iter().map(|ruta| format!("file '{}'\n", comillas(ruta))).collect();
    std::fs::write(&lista, texto).map_err(|error| format!("No se pudo escribir la lista de tramos: {error}"))?;
    // El audio final: el códec y bitrate de la exportación normal.
    let mut codec = job.format.export_args(job.fast, hw);
    if !job.fast {
        let acelerado = hw.filter(|_| matches!(job.format, ExportFormat::Mp4Video | ExportFormat::Mp4Hevc));
        super::exportacion::apply(&mut codec, &job.encode, acelerado);
    }
    let opcion = |nombre: &str| {
        codec
            .iter()
            .position(|arg| arg == nombre)
            .and_then(|i| codec.get(i + 1))
            .cloned()
    };
    let destino = job.output.as_path();
    let nombre = destino.file_name().and_then(|n| n.to_str()).unwrap_or("export");
    let temporal = destino.with_file_name(format!(".{nombre}.{}.part.{}", std::process::id(), job.format.extension()));
    let mut orden = Command::new(tool_path("ffmpeg.exe"));
    orden
        .args(["-y", "-v", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&lista);
    if !job.audio_only {
        orden.args(["-map", "0:v:0", "-c:v", "copy"]);
    }
    if job.audio_only || job.format.has_audio_track() {
        orden.args(["-map", "0:a:0"]);
        if let Some(audio) = opcion("-c:a") {
            orden.args(["-c:a", &audio]);
        }
        if let Some(bitrate) = opcion("-b:a") {
            orden.args(["-b:a", &bitrate]);
        }
        if job.normalize_loudness {
            let ajuste = match &job.measured_loudness {
                Some(medida) => format!("loudnorm=I=-14:TP=-1:LRA=11:{}", medida.two_pass_args()),
                None => "loudnorm=I=-14:TP=-1:LRA=11".to_owned(),
            };
            // loudnorm remuestrea a 192 kHz por dentro: se vuelve a 48 kHz.
            orden.args(["-af", &format!("{ajuste},aresample=48000")]);
        }
    }
    if let Some(flags) = opcion("-movflags") {
        orden.args(["-movflags", &flags]);
    }
    let salida = orden
        .args(["-t", &format_seconds(job.output_length())])
        .arg(&temporal)
        .creation_flags(BACKGROUND_PROCESS)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("FFmpeg no está disponible: {error}"))?;
    let resultado = salida.wait_with_output().map_err(|error| format!("No se pudo unir los tramos: {error}"))?;
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&temporal);
        return Err("Exportación cancelada".to_owned());
    }
    if !resultado.status.success() {
        let _ = std::fs::remove_file(&temporal);
        let error = String::from_utf8_lossy(&resultado.stderr);
        return Err(format!("No se pudo unir los tramos: {}", error.lines().last().unwrap_or("").trim()));
    }
    if destino.exists() {
        std::fs::remove_file(destino).map_err(|error| format!("No se pudo sustituir el destino: {error}"))?;
    }
    std::fs::rename(&temporal, destino).map_err(|error| format!("No se pudo instalar la exportación terminada: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corte(inicio: f64, duracion: f64) -> RoughClip {
        RoughClip {
            path: PathBuf::from("entrevista.mp4"),
            timeline_start: inicio,
            in_seconds: inicio,
            out_seconds: inicio + duracion,
            source_duration_seconds: Some(1000.0),
            ..Default::default()
        }
    }

    #[test]
    fn las_ventanas_cubren_todo_sin_huecos_y_con_pocos_clips() {
        let clips: Vec<RoughClip> = (0..200).map(|i| corte(i as f64 * 0.5, 0.5)).collect();
        let tramos = ventanas(&clips, 0.0, 100.0, 25.0);
        assert!(tramos.len() >= 200 / MAX_CLIPS_POR_TRAMO);
        assert_eq!(tramos.first().unwrap().0, 0.0);
        assert_eq!(tramos.last().unwrap().1, 100.0);
        for par in tramos.windows(2) {
            assert_eq!(par[0].1, par[1].0, "contiguos");
        }
        for &(a, b) in &tramos {
            assert!(montaje::window(&clips, a, b).0.len() <= MAX_CLIPS_POR_TRAMO);
            // En fotogramas enteros de 25 fps.
            assert!(((a * 25.0) - (a * 25.0).round()).abs() < 1e-6);
        }
    }

    #[test]
    fn un_clip_largo_no_impide_avanzar() {
        // Una música bajo todo cuenta en cada tramo y no bloquea el reparto.
        let mut clips: Vec<RoughClip> = (0..40).map(|i| corte(i as f64, 1.0)).collect();
        clips.push(RoughClip { track: 2, has_video: false, ..corte(0.0, 40.0) });
        let tramos = ventanas(&clips, 0.0, 40.0, 30.0);
        assert_eq!(tramos.last().unwrap().1, 40.0);
        assert!(tramos.len() > 1);
    }

    #[test]
    fn el_audio_de_un_tramo_va_en_pcm_y_sin_opciones_de_mp4() {
        let mut args: Vec<String> = ["-c:v", "libx264", "-crf", "18", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]
            .map(str::to_owned)
            .to_vec();
        ajustar_codec(&mut args);
        assert_eq!(args, ["-c:v", "libx264", "-crf", "18", "-c:a", "pcm_s16le"]);
    }
}
