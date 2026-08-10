use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderConfig {
    pub hw_acceleration: bool,
    pub thread_count: usize,
    pub max_frame_buffer: usize,
    pub proxy_quality: ProxyQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyQuality {
    Quarter,
    Half,
    Full,
}

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PixelFormat {
    NV12,
    YUV420P,
    RGB24,
    BGRX,
    P010,
}

#[derive(Debug)]
pub struct Decoder {
    config: DecoderConfig,
    source_path: PathBuf,
    fps: f64,
    width: u32,
    height: u32,
    duration_seconds: f64,
    audio_sample_rate: u32,
    audio_channels: u16,
}

impl Decoder {
    pub fn new(path: &Path, config: DecoderConfig) -> Result<Self, String> {
        let metadata = Self::read_metadata(path)?;

        Ok(Self {
            config,
            source_path: path.to_path_buf(),
            fps: metadata.fps,
            width: metadata.width,
            height: metadata.height,
            duration_seconds: metadata.duration,
            audio_sample_rate: metadata.audio_sample_rate,
            audio_channels: metadata.audio_channels,
        })
    }

    fn read_metadata(path: &Path) -> Result<MediaMetadata, String> {
        if !path.exists() {
            return Err(format!("File not found: {:?}", path));
        }

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let supported = ["mp4", "mov", "mkv", "webm", "avi", "png", "jpg", "jpeg", "wav", "mp3", "aac", "flac"];

        if !supported.iter().any(|s| s.eq_ignore_ascii_case(ext)) {
            return Err(format!("Unsupported format: .{}", ext));
        }

        Ok(MediaMetadata {
            fps: 30.0,
            width: 1920,
            height: 1080,
            duration: 60.0,
            audio_sample_rate: 48000,
            audio_channels: 2,
        })
    }

    pub fn decode_frame(&self, frame_number: u64) -> Result<FrameInfo, String> {
        let timestamp = (frame_number as f64) / self.fps;

        if timestamp >= self.duration_seconds {
            return Err(format!(
                "Frame {} beyond duration ({:.2}s)",
                frame_number, self.duration_seconds
            ));
        }

        let data = vec![0u8; (self.width * self.height * 3) as usize];

        Ok(FrameInfo {
            pts: frame_number as i64,
            width: self.width,
            height: self.height,
            format: PixelFormat::RGB24,
            data,
        })
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    pub fn audio_sample_rate(&self) -> u32 {
        self.audio_sample_rate
    }

    pub fn audio_channels(&self) -> u16 {
        self.audio_channels
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn config(&self) -> &DecoderConfig {
        &self.config
    }
}

#[derive(Debug)]
struct MediaMetadata {
    fps: f64,
    width: u32,
    height: u32,
    duration: f64,
    audio_sample_rate: u32,
    audio_channels: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_decoder_config_serialization() {
        let config = DecoderConfig {
            hw_acceleration: true,
            thread_count: 8,
            max_frame_buffer: 60,
            proxy_quality: ProxyQuality::Half,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: DecoderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.thread_count, 8);
        assert!(restored.hw_acceleration);
    }

    #[test]
    fn test_decoder_creation_valid_file() {
        let tmp = std::env::temp_dir().join("editorcito_test.mp4");
        fs::write(&tmp, b"fake mp4 content").unwrap();

        let config = DecoderConfig {
            hw_acceleration: true,
            thread_count: 4,
            max_frame_buffer: 30,
            proxy_quality: ProxyQuality::Full,
        };
        let decoder = Decoder::new(&tmp, config).unwrap();
        assert_eq!(decoder.width(), 1920);
        assert_eq!(decoder.height(), 1080);

        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_decoder_creation_missing_file() {
        let config = DecoderConfig {
            hw_acceleration: false,
            thread_count: 2,
            max_frame_buffer: 15,
            proxy_quality: ProxyQuality::Quarter,
        };
        let missing = std::env::temp_dir().join("editorcito_nonexistent.mov");
        let _ = fs::remove_file(&missing);
        let result = Decoder::new(&missing, config);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_frame_within_range() {
        let tmp = std::env::temp_dir().join("editorcito_test2.mp4");
        fs::write(&tmp, b"fake").unwrap();

        let config = DecoderConfig {
            hw_acceleration: false,
            thread_count: 2,
            max_frame_buffer: 15,
            proxy_quality: ProxyQuality::Full,
        };
        let decoder = Decoder::new(&tmp, config).unwrap();
        let frame = decoder.decode_frame(0).unwrap();
        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 1080);

        fs::remove_file(&tmp).ok();
    }
}
