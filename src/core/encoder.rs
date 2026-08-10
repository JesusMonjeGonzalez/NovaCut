//! Declarative export configuration.
//!
//! Platform backends consume this API. This crate does not currently encode media.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Mp4,
    Mov,
    WebM,
    Wav,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderPreset {
    Preview,
    Balanced,
    Master,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Encoder {
    pub format: ExportFormat,
    pub preset: EncoderPreset,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub include_audio: bool,
}

impl Encoder {
    pub fn new(
        format: ExportFormat,
        preset: EncoderPreset,
        width: u32,
        height: u32,
        fps: f64,
        include_audio: bool,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Export dimensions must be greater than zero".to_string());
        }
        if !fps.is_finite() || fps <= 0.0 {
            return Err("Export frame rate must be finite and greater than zero".to_string());
        }

        Ok(Self {
            format,
            preset,
            width,
            height,
            fps,
            include_audio,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_export_description() {
        assert!(Encoder::new(
            ExportFormat::Mp4,
            EncoderPreset::Balanced,
            1920,
            1080,
            0.0,
            true,
        )
        .is_err());
    }
}
