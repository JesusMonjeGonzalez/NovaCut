//! Codificación por hardware: NVENC (NVIDIA), Quick Sync (Intel), AMF (AMD)
//! y VideoToolbox (Apple, solo en el host de desarrollo).
//!
//! Que FFmpeg liste un codificador no significa que haya GPU capaz: los
//! builds de Windows traen los tres aunque el equipo no tenga ninguna. Por
//! eso se prueba de verdad codificando unos fotogramas, y si la exportación
//! falla con GPU se repite con CPU.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwBackend {
    Nvidia,
    Intel,
    Amd,
    Apple,
}

impl HwBackend {
    /// Orden de preferencia cuando hay varias: la GPU dedicada primero.
    pub const ALL: [HwBackend; 4] = [Self::Nvidia, Self::Amd, Self::Intel, Self::Apple];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA NVENC",
            Self::Intel => "Intel Quick Sync",
            Self::Amd => "AMD AMF",
            Self::Apple => "Apple VideoToolbox",
        }
    }

    pub fn encoder(self, hevc: bool) -> &'static str {
        match (self, hevc) {
            (Self::Nvidia, false) => "h264_nvenc",
            (Self::Nvidia, true) => "hevc_nvenc",
            (Self::Intel, false) => "h264_qsv",
            (Self::Intel, true) => "hevc_qsv",
            (Self::Amd, false) => "h264_amf",
            (Self::Amd, true) => "hevc_amf",
            (Self::Apple, false) => "h264_videotoolbox",
            (Self::Apple, true) => "hevc_videotoolbox",
        }
    }

    fn pixel_format(self) -> &'static str {
        match self {
            Self::Intel => "nv12",
            _ => "yuv420p",
        }
    }

    /// Argumentos de vídeo equivalentes en calidad a los de CPU (CRF 18 en
    /// H.264, 22 en HEVC). `fast` es para los renders de previsualización.
    pub fn video_args(self, hevc: bool, fast: bool) -> Vec<String> {
        let mut args: Vec<&str> = vec!["-c:v", self.encoder(hevc), "-pix_fmt", self.pixel_format()];
        match self {
            Self::Nvidia => args.extend([
                "-preset",
                if fast { "p1" } else { "p5" },
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                match (hevc, fast) {
                    (_, true) => "28",
                    (false, false) => "19",
                    (true, false) => "23",
                },
                "-b:v",
                "0",
            ]),
            Self::Intel => args.extend([
                "-preset",
                if fast { "veryfast" } else { "medium" },
                "-global_quality",
                match (hevc, fast) {
                    (_, true) => "28",
                    (false, false) => "20",
                    (true, false) => "23",
                },
            ]),
            Self::Amd => {
                let (qi, qp) = match (hevc, fast) {
                    (_, true) => ("28", "30"),
                    (false, false) => ("19", "21"),
                    (true, false) => ("22", "24"),
                };
                args.extend([
                    "-quality",
                    if fast { "speed" } else { "quality" },
                    "-rc",
                    "cqp",
                    "-qp_i",
                    qi,
                    "-qp_p",
                    qp,
                ]);
            }
            Self::Apple => args.extend(["-q:v", if fast { "50" } else { "65" }]),
        }
        if hevc {
            args.extend(["-tag:v", "hvc1"]);
        }
        args.into_iter().map(str::to_owned).collect()
    }
}

/// Backends cuyo codificador H.264 aparece en `ffmpeg -encoders`.
pub fn listed_backends(encoders: &str) -> Vec<HwBackend> {
    HwBackend::ALL
        .into_iter()
        .filter(|backend| {
            let name = backend.encoder(false);
            encoders
                .lines()
                .any(|line| line.split_whitespace().nth(1) == Some(name))
        })
        .collect()
}

fn hide_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000)
    }
    #[cfg(not(windows))]
    {
        command
    }
}

/// ¿Codifica de verdad? Unos fotogramas pequeños a `null`, en H.264 y HEVC
/// por separado no hace falta: si falla uno, el otro también.
fn works(ffmpeg: &Path, backend: HwBackend) -> bool {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=gray:s=320x240:r=25:d=0.2")
        .args(backend.video_args(false, true))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command)
        .status()
        .is_ok_and(|status| status.success())
}

/// Detecta las GPUs utilizables, en orden de preferencia. Tarda unos cientos
/// de milisegundos por backend listado: llamarlo fuera del hilo de interfaz.
pub fn detect(ffmpeg: &Path) -> Vec<HwBackend> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-encoders"])
        .stderr(Stdio::null());
    let Ok(output) = hide_console(&mut command).output() else {
        return Vec::new();
    };
    let listed = listed_backends(&String::from_utf8_lossy(&output.stdout));
    listed
        .into_iter()
        .filter(|backend| works(ffmpeg, *backend))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODERS: &str = " V....D libx264              libx264 H.264
 V....D h264_amf             AMD AMF H.264 Encoder (codec h264)
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 V....D h264_qsv             H.264 / AVC (Intel Quick Sync Video acceleration) (codec h264)
 V....D hevc_nvenc           NVIDIA NVENC hevc encoder (codec hevc)";

    #[test]
    fn lists_backends_in_preference_order() {
        assert_eq!(
            listed_backends(ENCODERS),
            vec![HwBackend::Nvidia, HwBackend::Amd, HwBackend::Intel]
        );
        assert!(listed_backends(" V....D libx264  x").is_empty());
    }

    #[test]
    fn video_args_pick_encoder_and_quality() {
        assert_eq!(
            HwBackend::Nvidia.video_args(false, false).join(" "),
            "-c:v h264_nvenc -pix_fmt yuv420p -preset p5 -tune hq -rc vbr -cq 19 -b:v 0"
        );
        assert_eq!(
            HwBackend::Intel.video_args(true, false).join(" "),
            "-c:v hevc_qsv -pix_fmt nv12 -preset medium -global_quality 23 -tag:v hvc1"
        );
        assert_eq!(
            HwBackend::Amd.video_args(false, true).join(" "),
            "-c:v h264_amf -pix_fmt yuv420p -quality speed -rc cqp -qp_i 28 -qp_p 30"
        );
    }
}
