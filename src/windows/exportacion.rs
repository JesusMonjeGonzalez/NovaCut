//! Ajustes de codificación y preajustes de exportación, al estilo de Media
//! Encoder: calidad constante (CRF/CQ) o bitrate objetivo, bitrate de audio
//! y perfiles ProRes, aplicados igual con CPU que con cualquier GPU.
//!
//! Los argumentos base los decide cada formato (y cada GPU); aquí solo se
//! reescriben las opciones de control de tasa, de modo que un formato nuevo
//! hereda los ajustes sin tocar este módulo.

use super::aceleracion::HwBackend;
use serde::{Deserialize, Serialize};

/// Nivel de calidad constante. `Alta` reproduce los valores de siempre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Maxima,
    #[default]
    Alta,
    Estandar,
    Ligera,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Self::Maxima, Self::Alta, Self::Estandar, Self::Ligera];

    pub fn label(self) -> &'static str {
        match self {
            Self::Maxima => "Máxima",
            Self::Alta => "Alta",
            Self::Estandar => "Estándar",
            Self::Ligera => "Ligera",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Maxima => "Casi sin pérdida; archivos grandes. Para másters y reedición",
            Self::Alta => "Indistinguible del original a la vista. Para subir a plataformas",
            Self::Estandar => "Buena calidad y la mitad de tamaño. Para compartir",
            Self::Ligera => "Archivos pequeños para correo o mensajería",
        }
    }

    /// Desplazamiento sobre el CRF/CQ de referencia (más alto = más ligero).
    fn offset(self) -> i32 {
        match self {
            Self::Maxima => -3,
            Self::Alta => 0,
            Self::Estandar => 3,
            Self::Ligera => 7,
        }
    }

    /// Perfil ProRes: Proxy, LT, 422, HQ.
    fn prores_profile(self) -> &'static str {
        match self {
            Self::Maxima => "3",
            Self::Alta => "2",
            Self::Estandar => "1",
            Self::Ligera => "0",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncodeSettings {
    #[serde(default)]
    pub quality: Quality,
    /// Bitrate de vídeo objetivo en Mbps; `None` es calidad constante.
    #[serde(default)]
    pub bitrate_mbps: Option<f64>,
    /// Bitrate de audio comprimido (AAC, MP3, Opus) en kbps.
    #[serde(default = "default_audio_kbps")]
    pub audio_kbps: u32,
}

fn default_audio_kbps() -> u32 {
    192
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            quality: Quality::Alta,
            bitrate_mbps: None,
            audio_kbps: default_audio_kbps(),
        }
    }
}

pub const AUDIO_KBPS: [u32; 5] = [96, 128, 192, 256, 320];

fn position(args: &[String], option: &str) -> Option<usize> {
    args.iter().position(|arg| arg == option)
}

fn value_of(args: &[String], option: &str) -> Option<String> {
    position(args, option).and_then(|index| args.get(index + 1).cloned())
}

fn set_option(args: &mut Vec<String>, option: &str, value: String) {
    match position(args, option) {
        Some(index) if index + 1 < args.len() => args[index + 1] = value,
        _ => {
            args.push(option.to_owned());
            args.push(value);
        }
    }
}

fn remove_option(args: &mut Vec<String>, option: &str) {
    if let Some(index) = position(args, option) {
        args.drain(index..(index + 2).min(args.len()));
    }
}

/// Desplaza un valor numérico de calidad (CRF, CQ, QP…) si la opción está.
fn shift_quality(args: &mut Vec<String>, option: &str, offset: i32, max: i32) {
    if let Some(current) = value_of(args, option).and_then(|value| value.parse::<i32>().ok()) {
        set_option(args, option, (current + offset).clamp(0, max).to_string());
    }
}

/// Aplica los ajustes a los argumentos base de un formato. `hw` es la GPU
/// que codifica, si la hay. Los renders rápidos de previsualización no
/// pasan por aquí.
pub fn apply(args: &mut Vec<String>, settings: &EncodeSettings, hw: Option<HwBackend>) {
    let encoder = value_of(args, "-c:v").unwrap_or_default();
    // Audio comprimido: el bitrate elegido; PCM no lleva bitrate.
    if position(args, "-b:a").is_some() {
        set_option(args, "-b:a", format!("{}k", settings.audio_kbps));
    }
    if encoder == "prores_ks" {
        set_option(args, "-profile:v", settings.quality.prores_profile().to_owned());
        return;
    }
    if encoder.is_empty() || encoder == "gif" {
        return;
    }
    match settings.bitrate_mbps.filter(|mbps| *mbps > 0.05) {
        None => {
            let offset = settings.quality.offset();
            match hw {
                Some(HwBackend::Nvidia) => shift_quality(args, "-cq", offset, 51),
                Some(HwBackend::Intel) => shift_quality(args, "-global_quality", offset, 51),
                Some(HwBackend::Amd) => {
                    shift_quality(args, "-qp_i", offset, 51);
                    shift_quality(args, "-qp_p", offset, 51);
                }
                // VideoToolbox: calidad 0-100, al revés que un CRF.
                Some(HwBackend::Apple) => shift_quality(args, "-q:v", -offset * 4, 100),
                None => shift_quality(args, "-crf", offset, 63),
            }
        }
        Some(mbps) => {
            let kbps = (mbps * 1000.0).round() as u64;
            for option in ["-crf", "-cq", "-global_quality", "-qp_i", "-qp_p", "-q:v"] {
                remove_option(args, option);
            }
            match hw {
                Some(HwBackend::Nvidia) => set_option(args, "-rc", "vbr".to_owned()),
                Some(HwBackend::Amd) => set_option(args, "-rc", "vbr_peak".to_owned()),
                _ => {}
            }
            set_option(args, "-b:v", format!("{kbps}k"));
            if encoder != "libvpx-vp9" {
                set_option(args, "-maxrate", format!("{}k", kbps * 3 / 2));
                set_option(args, "-bufsize", format!("{}k", kbps * 2));
            }
        }
    }
}

/// Un preajuste: formato (por código de `ExportFormat`), tamaño y ajustes.
pub struct Preset {
    pub name: &'static str,
    pub hint: &'static str,
    pub format: u8,
    pub size: (u32, u32),
    pub settings: EncodeSettings,
}

const fn settings(quality: Quality, bitrate_mbps: Option<f64>, audio_kbps: u32) -> EncodeSettings {
    EncodeSettings {
        quality,
        bitrate_mbps,
        audio_kbps,
    }
}

/// Preajustes al estilo de Media Encoder. Los códigos de formato son los de
/// `ExportFormat::ALL`: 0 H.264, 1 WAV, 2 MP3, 3 HEVC, 4 ProRes, 5 WebM, 6 GIF.
pub const PRESETS: [Preset; 10] = [
    Preset {
        name: "YouTube 1080p",
        hint: "H.264 1920×1080, calidad alta y audio AAC 320 kbps",
        format: 0,
        size: (1920, 1080),
        settings: settings(Quality::Alta, None, 320),
    },
    Preset {
        name: "YouTube 4K",
        hint: "H.264 3840×2160, calidad alta; YouTube vuelve a comprimir mejor desde 4K",
        format: 0,
        size: (3840, 2160),
        settings: settings(Quality::Alta, None, 320),
    },
    Preset {
        name: "Reels / TikTok / Shorts",
        hint: "Vertical 1080×1920 a 16 Mbps: lo que mejor aguanta la recompresión móvil",
        format: 0,
        size: (1080, 1920),
        settings: settings(Quality::Alta, Some(16.0), 256),
    },
    Preset {
        name: "Instagram cuadrado",
        hint: "1080×1080 a 12 Mbps para el feed",
        format: 0,
        size: (1080, 1080),
        settings: settings(Quality::Alta, Some(12.0), 256),
    },
    Preset {
        name: "Vimeo / archivo HEVC",
        hint: "HEVC 1080p: la misma calidad que H.264 en unos 2/3 del tamaño",
        format: 3,
        size: (1920, 1080),
        settings: settings(Quality::Alta, None, 256),
    },
    Preset {
        name: "Web ligero 720p",
        hint: "H.264 1280×720, calidad estándar y AAC 128 kbps",
        format: 0,
        size: (1280, 720),
        settings: settings(Quality::Estandar, None, 128),
    },
    Preset {
        name: "Máster ProRes 422 HQ",
        hint: "Para color, archivo o reedición en otro programa",
        format: 4,
        size: (1920, 1080),
        settings: settings(Quality::Maxima, None, 320),
    },
    Preset {
        name: "Podcast MP3",
        hint: "Solo audio, MP3 128 kbps: el estándar de los directorios de podcasts",
        format: 2,
        size: (1920, 1080),
        settings: settings(Quality::Alta, None, 128),
    },
    Preset {
        name: "Audio máster WAV",
        hint: "PCM sin compresión para masterizar o archivar",
        format: 1,
        size: (1920, 1080),
        settings: settings(Quality::Alta, None, 320),
    },
    Preset {
        name: "GIF 480p",
        hint: "GIF animado de 854×480 con paleta optimizada",
        format: 6,
        size: (854, 480),
        settings: settings(Quality::Alta, None, 128),
    },
];

/// Tamaño aproximado en MB, o `None` con calidad constante (depende de la
/// imagen).
pub fn estimated_megabytes(settings: &EncodeSettings, seconds: f64, has_video: bool) -> Option<f64> {
    let audio = settings.audio_kbps as f64 / 8.0 / 1000.0;
    if !has_video {
        return Some(audio * seconds);
    }
    settings
        .bitrate_mbps
        .map(|mbps| (mbps / 8.0 + audio) * seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn quality_shifts_crf_and_audio_bitrate() {
        let mut x264 = args(&["-c:v", "libx264", "-crf", "18", "-c:a", "aac", "-b:a", "192k"]);
        let settings = EncodeSettings {
            quality: Quality::Ligera,
            bitrate_mbps: None,
            audio_kbps: 128,
        };
        apply(&mut x264, &settings, None);
        assert_eq!(value_of(&x264, "-crf").as_deref(), Some("25"));
        assert_eq!(value_of(&x264, "-b:a").as_deref(), Some("128k"));
    }

    #[test]
    fn bitrate_replaces_constant_quality_on_every_backend() {
        let settings = EncodeSettings {
            quality: Quality::Alta,
            bitrate_mbps: Some(16.0),
            audio_kbps: 192,
        };
        let mut x264 = args(&["-c:v", "libx264", "-crf", "18"]);
        apply(&mut x264, &settings, None);
        assert!(value_of(&x264, "-crf").is_none());
        assert_eq!(value_of(&x264, "-b:v").as_deref(), Some("16000k"));
        assert_eq!(value_of(&x264, "-maxrate").as_deref(), Some("24000k"));
        let mut nvenc = args(&["-c:v", "h264_nvenc", "-rc", "vbr", "-cq", "19", "-b:v", "0"]);
        apply(&mut nvenc, &settings, Some(HwBackend::Nvidia));
        assert!(value_of(&nvenc, "-cq").is_none());
        assert_eq!(value_of(&nvenc, "-b:v").as_deref(), Some("16000k"));
        let mut amf = args(&["-c:v", "h264_amf", "-rc", "cqp", "-qp_i", "19", "-qp_p", "21"]);
        apply(&mut amf, &settings, Some(HwBackend::Amd));
        assert_eq!(value_of(&amf, "-rc").as_deref(), Some("vbr_peak"));
        assert!(value_of(&amf, "-qp_i").is_none() && value_of(&amf, "-qp_p").is_none());
        let mut vp9 = args(&["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0"]);
        apply(&mut vp9, &settings, None);
        assert_eq!(value_of(&vp9, "-b:v").as_deref(), Some("16000k"));
        assert!(value_of(&vp9, "-maxrate").is_none());
    }

    #[test]
    fn prores_quality_picks_the_profile() {
        let mut prores = args(&["-c:v", "prores_ks", "-profile:v", "2", "-c:a", "pcm_s16le"]);
        apply(&mut prores, &EncodeSettings { quality: Quality::Maxima, ..Default::default() }, None);
        assert_eq!(value_of(&prores, "-profile:v").as_deref(), Some("3"));
    }

    #[test]
    fn default_settings_leave_the_reference_args_alone() {
        let reference = args(&["-c:v", "libx264", "-crf", "18", "-c:a", "aac", "-b:a", "192k"]);
        let mut tuned = reference.clone();
        apply(&mut tuned, &EncodeSettings::default(), None);
        assert_eq!(tuned, reference);
    }
}
