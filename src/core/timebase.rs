use serde::{Deserialize, Serialize};

/// Base temporal exacta compartida por los hosts.
///
/// El decimal solo sirve para interfaces y calculos que exigen segundos. El
/// contrato persistente y los filtros de FFmpeg usan siempre la fraccion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timebase {
    #[serde(alias = "numerador")]
    pub numerator: u32,
    #[serde(alias = "denominador")]
    pub denominator: u32,
    #[serde(default, alias = "dropFrame")]
    pub drop_frame: bool,
}

impl Default for Timebase {
    fn default() -> Self {
        Self {
            numerator: 30,
            denominator: 1,
            drop_frame: false,
        }
    }
}

impl Timebase {
    pub const P23_976: Self = Self {
        numerator: 24_000,
        denominator: 1_001,
        drop_frame: false,
    };

    pub const P24: Self = Self {
        numerator: 24,
        denominator: 1,
        drop_frame: false,
    };

    pub const P25: Self = Self {
        numerator: 25,
        denominator: 1,
        drop_frame: false,
    };

    pub const NTSC30: Self = Self {
        numerator: 30_000,
        denominator: 1_001,
        drop_frame: true,
    };

    pub const P30: Self = Self {
        numerator: 30,
        denominator: 1,
        drop_frame: false,
    };

    pub const P50: Self = Self {
        numerator: 50,
        denominator: 1,
        drop_frame: false,
    };

    pub const NTSC60: Self = Self {
        numerator: 60_000,
        denominator: 1_001,
        drop_frame: true,
    };

    pub const P60: Self = Self {
        numerator: 60,
        denominator: 1,
        drop_frame: false,
    };

    /// Construye una fraccion reducida y valida.
    pub fn new(numerator: u32, denominator: u32, drop_frame: bool) -> Result<Self, String> {
        if numerator == 0 || denominator == 0 {
            return Err("Timebase numerator and denominator must be greater than zero".to_owned());
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
            drop_frame,
        })
    }

    /// Convierte FPS de interfaz a la fraccion profesional mas cercana.
    pub fn from_fps(fps: f64) -> Self {
        if !fps.is_finite() || fps <= 0.0 {
            return Self::default();
        }
        let known = [
            Self::P23_976,
            Self::P24,
            Self::P25,
            Self::NTSC30,
            Self::P30,
            Self::P50,
            Self::NTSC60,
            Self::P60,
        ];
        if let Some(rate) = known.into_iter().min_by(|left, right| {
            (left.fps() - fps)
                .abs()
                .total_cmp(&(right.fps() - fps).abs())
        }) {
            if (rate.fps() - fps).abs() < 0.01 {
                return rate;
            }
        }

        // El fallback conserva tres decimales sin convertir la fraccion en un
        // numero enorme. Los FPS de produccion habituales entran en la tabla.
        let denominator = 1_000;
        let numerator = (fps.clamp(1.0, 1_000.0) * denominator as f64).round() as u32;
        Self::new(numerator.max(1), denominator, false).unwrap_or_default()
    }

    pub fn fps(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    pub fn frame_duration(self) -> f64 {
        1.0 / self.fps().max(f64::EPSILON)
    }

    pub fn seconds(self, frames: i64) -> f64 {
        frames as f64 * self.frame_duration()
    }

    pub fn frames(self, seconds: f64) -> i64 {
        (seconds.max(0.0) * self.fps()).round() as i64
    }

    /// Forma que aceptan los filtros `fps` y `color` de FFmpeg.
    pub fn ffmpeg_rate(self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    /// Numero nominal de etiquetas por segundo para un timecode HH:MM:SS:FF.
    pub fn nominal_fps(self) -> u32 {
        self.fps().round().max(1.0) as u32
    }

    fn dropped_frames_per_minute(self) -> i64 {
        if !self.drop_frame {
            return 0;
        }
        match self.nominal_fps() {
            30 => 2,
            60 => 4,
            fps => (f64::from(fps) / 15.0).round() as i64,
        }
    }

    /// Formatea un frame del montaje como timecode HH:MM:SS:FF.
    pub fn timecode(self, frames: i64) -> String {
        let fps = i64::from(self.nominal_fps());
        let mut frame_number = frames.max(0);
        if self.drop_frame {
            let dropped = self.dropped_frames_per_minute();
            let frames_per_ten_minutes = fps * 60 * 10 - dropped * 9;
            let frames_per_minute = fps * 60 - dropped;
            let block = frame_number / frames_per_ten_minutes;
            let remainder = frame_number % frames_per_ten_minutes;
            frame_number += dropped * 9 * block;
            if remainder > dropped {
                frame_number += dropped * ((remainder - dropped) / frames_per_minute);
            }
        }
        let frame = frame_number % fps;
        let total_seconds = frame_number / fps;
        let seconds = total_seconds % 60;
        let minutes = (total_seconds / 60) % 60;
        let hours = total_seconds / 3600;
        let separator = if self.drop_frame { ';' } else { ':' };
        format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{frame:02}")
    }

    /// Convierte un timecode de uno a cuatro campos a frames del montaje.
    pub fn frames_from_timecode(self, text: &str) -> Option<i64> {
        let fields: Vec<i64> = text
            .split([':', ';'])
            .map(|field| field.trim().parse::<i64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if fields.is_empty() || fields.iter().any(|field| *field < 0) {
            return None;
        }
        let fps = i64::from(self.nominal_fps());
        let clock_frames = |seconds: i64, frame: i64| {
            if self.drop_frame {
                let dropped = self.dropped_frames_per_minute();
                let minutes = seconds / 60;
                let second = seconds % 60;
                if second == 0 && minutes % 10 != 0 && frame < dropped {
                    return None;
                }
                let labels = seconds * fps + frame;
                let skipped = dropped * (minutes - minutes / 10);
                Some(labels - skipped)
            } else {
                Some(seconds * fps + frame)
            }
        };
        match fields.as_slice() {
            [frame] => Some(*frame),
            [minutes, frame] if *frame < fps => Some(minutes * fps + frame),
            [minutes, seconds, frame] if *seconds < 60 && *frame < fps => {
                clock_frames(minutes * 60 + seconds, *frame)
            }
            [hours, minutes, seconds, frame] if *minutes < 60 && *seconds < 60 && *frame < fps => {
                clock_frames(hours * 3600 + minutes * 60 + seconds, *frame)
            }
            _ => None,
        }
    }
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_ntsc_fraction_without_float_rounding() {
        let rate = Timebase::from_fps(23.976);
        assert_eq!(rate, Timebase::P23_976);
        assert_eq!(rate.ffmpeg_rate(), "24000/1001");
        assert_eq!(rate.frames(rate.seconds(48)), 48);
    }

    #[test]
    fn reduces_custom_fraction_and_serializes_aliases() {
        let rate = Timebase::new(48_000, 2_000, false).unwrap();
        assert_eq!(rate, Timebase::P24);
        let restored: Timebase =
            serde_json::from_str(r#"{"numerador":24000,"denominador":1001,"dropFrame":false}"#)
                .unwrap();
        assert_eq!(restored, Timebase::P23_976);
    }

    #[test]
    fn invalid_timebase_falls_back_for_legacy_fps() {
        assert_eq!(Timebase::from_fps(0.0), Timebase::default());
        assert_eq!(Timebase::from_fps(f64::NAN), Timebase::default());
    }

    #[test]
    fn formats_and_parses_drop_frame_timecode() {
        assert_eq!(Timebase::NTSC30.timecode(1_798), "00:00:59;28");
        assert_eq!(Timebase::NTSC30.timecode(1_800), "00:01:00;02");
        assert_eq!(Timebase::NTSC30.timecode(17_982), "00:10:00;00");
        assert_eq!(
            Timebase::NTSC30.frames_from_timecode("00:11:00;02"),
            Some(19_782)
        );
        assert_eq!(Timebase::NTSC30.frames_from_timecode("00:01:00;00"), None);
    }
}
