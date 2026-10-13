//! Vídeo HDR de móvil (HLG del iPhone y de Android, PQ/HDR10) a Rec.709.
//! Sin conversión, FFmpeg trata esa señal como si fuera SDR y la imagen sale
//! gris y lavada en el monitor y en la exportación. Como Premiere y Resolve,
//! se pasa a SDR al decodificar: el blanco de referencia de BT.2408 (203
//! nits) queda en torno al 90 % y las altas luces se comprimen con una curva
//! suave en vez de quemarse. La conversión es una LUT 3D calculada aquí, así
//! que no depende de que el FFmpeg traiga zscale o libplacebo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(not(windows))]
use super::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transferencia {
    /// ARIB STD-B67: lo que graban por defecto el iPhone y muchos Android.
    Hlg,
    /// SMPTE ST 2084: HDR10 y Dolby Vision de cámaras y descargas.
    Pq,
}

impl Transferencia {
    pub fn nombre(self) -> &'static str {
        match self {
            Transferencia::Hlg => "HDR (HLG)",
            Transferencia::Pq => "HDR (PQ)",
        }
    }

    fn desde_ffprobe(valor: &str) -> Option<Self> {
        match valor.trim() {
            "arib-std-b67" => Some(Transferencia::Hlg),
            "smpte2084" => Some(Transferencia::Pq),
            _ => None,
        }
    }
}

/// Blanco de referencia HDR (BT.2408), en nits.
const BLANCO_REFERENCIA: f64 = 203.0;
/// Pico supuesto de un HLG (y de un PQ sin metadatos): 1000 nits.
const PICO_HLG: f64 = 1000.0;
/// Hasta aquí (lineal SDR) la curva es recta; por encima, comprime.
const RODILLA: f64 = 0.7;
/// Pendiente del tramo recto: el blanco de referencia queda cerca de 0,79
/// lineal, un 90 % de señal.
const PENDIENTE: f64 = 0.82;

fn hlg_a_lineal(v: f64) -> f64 {
    const A: f64 = 0.178_832_77;
    const B: f64 = 0.284_668_92;
    const C: f64 = 0.559_910_73;
    let v = v.clamp(0.0, 1.0);
    if v <= 0.5 {
        v * v / 3.0
    } else {
        (((v - C) / A).exp() + B) / 12.0
    }
}

fn pq_a_nits(v: f64) -> f64 {
    const M1: f64 = 2610.0 / 16384.0;
    const M2: f64 = 2523.0 / 4096.0 * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = 2413.0 / 4096.0 * 32.0;
    const C3: f64 = 2392.0 / 4096.0 * 32.0;
    let p = v.clamp(0.0, 1.0).powf(1.0 / M2);
    ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1) * 10_000.0
}

fn luminancia_2020([r, g, b]: [f64; 3]) -> f64 {
    0.2627 * r + 0.6780 * g + 0.0593 * b
}

/// Curva de tono: recta hasta la rodilla y luego se acerca a 1 sin
/// tocarlo, con la misma pendiente en el empalme (sin escalón visible).
/// `w` es la luminancia en múltiplos del blanco de referencia.
fn curva(w: f64) -> f64 {
    let rodilla = RODILLA / PENDIENTE;
    if w <= rodilla {
        w * PENDIENTE
    } else {
        let d = (w - rodilla) * PENDIENTE;
        RODILLA + d / (1.0 + d / (1.0 - RODILLA))
    }
}

/// Un color de la señal HDR (RGB no lineal de BT.2020, 0-1) a Rec.709 SDR
/// no lineal (gamma 2,4 de BT.1886), 0-1.
pub fn convertir(transferencia: Transferencia, rgb: [f64; 3]) -> [f64; 3] {
    let nits = match transferencia {
        Transferencia::Hlg => {
            // OOTF de BT.2100 con gamma 1,2 (pantalla de 1000 nits): la
            // luz de escena pasa a luz de pantalla.
            let escena = rgb.map(hlg_a_lineal);
            let factor = luminancia_2020(escena).max(0.0).powf(0.2) * PICO_HLG;
            escena.map(|c| c * factor)
        }
        Transferencia::Pq => rgb.map(pq_a_nits),
    };
    let y = luminancia_2020(nits) / BLANCO_REFERENCIA;
    if y <= 1e-9 {
        return [0.0; 3];
    }
    let escala = curva(y) / y / BLANCO_REFERENCIA;
    let [r, g, b] = nits.map(|c| c * escala);
    let lineal = [
        1.6605 * r - 0.5876 * g - 0.0728 * b,
        -0.1246 * r + 1.1329 * g - 0.0083 * b,
        -0.0182 * r - 0.1006 * g + 1.1187 * b,
    ];
    // Lo que cae fuera de la gama de Rec.709 se recorta por canal, como
    // zscale: desaturar hacia el gris volvía rosados los rojos de BT.2020.
    lineal.map(|c| c.clamp(0.0, 1.0).powf(1.0 / 2.4))
}

fn cube(transferencia: Transferencia, n: usize) -> String {
    let mut texto = format!("TITLE \"NovaCut {}\"\nLUT_3D_SIZE {n}\n", transferencia.nombre());
    texto.reserve(n * n * n * 27);
    let paso = 1.0 / (n - 1) as f64;
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let [ro, go, bo] = convertir(transferencia, [r as f64 * paso, g as f64 * paso, b as f64 * paso]);
                texto.push_str(&format!("{ro:.6} {go:.6} {bo:.6}\n"));
            }
        }
    }
    texto
}

/// 65 puntos por lado: la curva PQ es muy empinada en las sombras y con 33
/// se notaban escalones en degradados oscuros.
const LADO: usize = 65;
/// Sube si cambia la conversión, para no reutilizar LUT viejas.
const VERSION: u32 = 1;

fn archivo_lut(transferencia: Transferencia) -> Option<PathBuf> {
    let carpeta = std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("hdr"))
        .unwrap_or_else(|| std::env::temp_dir().join("novacut-hdr"));
    let nombre = match transferencia {
        Transferencia::Hlg => "hlg",
        Transferencia::Pq => "pq",
    };
    let ruta = carpeta.join(format!("{nombre}-a-709-v{VERSION}-{LADO}.cube"));
    if !ruta.is_file() {
        std::fs::create_dir_all(&carpeta).ok()?;
        // Se escribe aparte y se renombra: FFmpeg nunca lee uno a medias.
        let parcial = ruta.with_extension(format!("{}.part", std::process::id()));
        std::fs::write(&parcial, cube(transferencia, LADO)).ok()?;
        std::fs::rename(&parcial, &ruta).ok()?;
    }
    Some(ruta)
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Option<Transferencia>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<Transferencia>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Transferencia HDR del primer vídeo del archivo; `None` si es SDR, una
/// imagen o no se puede leer. Se pregunta a FFprobe una vez por archivo.
pub fn detectar(ruta: &Path) -> Option<Transferencia> {
    if ruta.as_os_str().is_empty() || super::is_image_file(ruta) {
        return None;
    }
    if let Some(conocida) = cache().lock().ok()?.get(ruta) {
        return *conocida;
    }
    let salida = std::process::Command::new(super::tool_path("ffprobe.exe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=color_transfer",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(ruta)
        .creation_flags(super::CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let detectada = if salida.status.success() {
        Transferencia::desde_ffprobe(&String::from_utf8_lossy(&salida.stdout))
    } else {
        None
    };
    cache().lock().ok()?.insert(ruta.to_path_buf(), detectada);
    detectada
}

/// Filtros que van tras escalar al tamaño de salida y antes de cualquier
/// ajuste de color, con la coma delante; vacío si el archivo es SDR. Los proxies conservan la señal
/// del original, así que se decide siempre por el original.
pub fn cadena(ruta: &Path) -> String {
    let Some(transferencia) = detectar(ruta) else {
        return String::new();
    };
    let Some(lut) = archivo_lut(transferencia) else {
        return String::new();
    };
    // La matriz de BT.2020 hay que pedirla: FFmpeg usa la de BT.601 si el
    // archivo no la declara. `setparams` marca el resultado como Rec.709
    // para que el codificador no lo etiquete como HDR.
    format!(
        ",scale=in_color_matrix=bt2020:in_range=auto,format=gbrpf32le,lut3d=file={}:interp=tetrahedral,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709",
        super::escape_lut_path(&lut)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_negro_sigue_negro_y_el_blanco_de_referencia_queda_hacia_el_90() {
        assert_eq!(convertir(Transferencia::Hlg, [0.0; 3]), [0.0; 3]);
        // HLG al 75 % es el blanco de referencia (203 nits).
        let blanco = convertir(Transferencia::Hlg, [0.75; 3]);
        assert!(blanco.iter().all(|c| (c - 0.9).abs() < 0.03), "{blanco:?}");
        // En PQ, 203 nits son el 58 % de la señal.
        let pq = convertir(Transferencia::Pq, [0.5807; 3]);
        assert!(pq.iter().all(|c| (c - 0.9).abs() < 0.03), "{pq:?}");
        // Los grises siguen grises.
        assert!((blanco[0] - blanco[2]).abs() < 1e-3);
    }

    #[test]
    fn las_altas_luces_se_comprimen_sin_quemarse_ni_invertirse() {
        let mut anterior = -1.0;
        for paso in 0..=100 {
            let v = paso as f64 / 100.0;
            for transferencia in [Transferencia::Hlg, Transferencia::Pq] {
                let c = convertir(transferencia, [v; 3]);
                assert!(c.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)));
                if transferencia == Transferencia::Hlg {
                    assert!(c[1] >= anterior, "no monótona en {v}");
                    anterior = c[1];
                }
            }
        }
        // El pico de HLG queda por debajo de 1: aún hay detalle en las luces.
        let pico = convertir(Transferencia::Hlg, [1.0; 3])[1];
        let casi = convertir(Transferencia::Hlg, [0.95; 3])[1];
        assert!(pico < 1.0 && pico > casi, "{casi} → {pico}");
    }

    #[test]
    fn un_rojo_saturado_sigue_rojo_dentro_de_rec709() {
        let rojo = convertir(Transferencia::Hlg, [0.8, 0.1, 0.1]);
        assert!(rojo[0] > 0.6 && rojo[1] < 0.3 && rojo[2] < 0.3, "{rojo:?}");
        let verde_2020 = convertir(Transferencia::Pq, [0.2, 0.6, 0.2]);
        assert!(verde_2020.iter().all(|c| (0.0..=1.0).contains(c)));
        assert!(verde_2020[1] > verde_2020[0] && verde_2020[1] > verde_2020[2]);
    }

    #[test]
    fn la_curva_empalma_sin_escalon() {
        let rodilla = RODILLA / PENDIENTE;
        let (antes, despues) = (curva(rodilla - 1e-6), curva(rodilla + 1e-6));
        assert!((despues - antes).abs() < 1e-5);
        assert!(curva(1000.0) < 1.0);
    }
}
