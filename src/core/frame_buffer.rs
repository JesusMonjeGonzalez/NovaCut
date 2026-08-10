use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use parking_lot::RwLock;
use crate::decode::FrameInfo;

#[derive(Debug, Clone)]
pub struct FrameEntry {
    pub frame: FrameInfo,
    pub source_path: PathBuf,
    pub last_access: u64,
}

pub struct FrameBuffer {
    ram_cache: RwLock<HashMap<(PathBuf, u64), FrameEntry>>,
    max_ram_frames: usize,
    access_counter: AtomicU64,
}

impl FrameBuffer {
    pub fn new(max_ram_frames: usize, _disk_cache_enabled: bool) -> Self {
        Self {
            ram_cache: RwLock::new(HashMap::new()),
            max_ram_frames,
            access_counter: AtomicU64::new(0),
        }
    }

    pub fn get_frame(&self, source: &str, frame_num: u64, decoder_fn: impl FnOnce() -> Result<FrameInfo, String>) -> Result<FrameInfo, String> {
        let key = Self::cache_key(source, frame_num);
        let mut cache = self.ram_cache.write();
        if let Some(entry) = cache.get_mut(&key) {
            entry.last_access = self.next_access();
            return Ok(entry.frame.clone());
        }
        drop(cache);

        let frame = decoder_fn()?;
        self.insert_frame(source, frame.clone());
        Ok(frame)
    }

    fn insert_frame(&self, source: &str, frame: FrameInfo) {
        if self.max_ram_frames == 0 || frame.pts < 0 {
            return;
        }

        let mut cache = self.ram_cache.write();
        let key = Self::cache_key(source, frame.pts as u64);

        let entry = FrameEntry {
            frame,
            source_path: PathBuf::from(source),
            last_access: self.next_access(),
        };

        cache.insert(key, entry);

        while cache.len() > self.max_ram_frames {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
    }

    pub fn warmup(&self, source: &str, frame_range: std::ops::Range<u64>, decoder_fn: impl Fn(u64) -> Result<FrameInfo, String> + Send + Sync) {
        for i in frame_range {
            if let Ok(frame) = decoder_fn(i) {
                self.insert_frame(source, frame);
            }
        }
    }

    pub fn clear_source(&self, source: &str) {
        let mut cache = self.ram_cache.write();
        cache.retain(|_, entry| entry.source_path.as_path() != Path::new(source));
    }

    pub fn clear_all(&self) {
        let mut cache = self.ram_cache.write();
        cache.clear();
    }

    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.ram_cache.read();
        let total_keys = cache.len();
        let total_frames = cache.len();
        (total_keys, total_frames)
    }

    fn cache_key(source: &str, frame: u64) -> (PathBuf, u64) {
        (PathBuf::from(source), frame)
    }

    fn next_access(&self) -> u64 {
        self.access_counter.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::PixelFormat;

    #[test]
    fn test_frame_buffer_insert_and_retrieve() {
        let buffer = FrameBuffer::new(60, false);
        let source = "test.mp4";

        let frame = FrameInfo {
            pts: 0,
            width: 1920,
            height: 1080,
            format: PixelFormat::RGB24,
            data: vec![0; 1920 * 1080 * 3],
        };

        buffer.insert_frame(source, frame);
        let (keys, frames) = buffer.cache_stats();
        assert_eq!(keys, 1);
        assert_eq!(frames, 1);
    }

    #[test]
    fn test_frame_buffer_eviction() {
        let buffer = FrameBuffer::new(5, false);
        let source = "test.mp4";

        for i in 0..10 {
            let frame = FrameInfo {
                pts: i,
                width: 1920,
                height: 1080,
                format: PixelFormat::RGB24,
                data: vec![0; 1920 * 1080 * 3],
            };
            buffer.insert_frame(source, frame);
        }

        let (_, frames) = buffer.cache_stats();
        assert!(frames <= 5);
    }

    #[test]
    fn test_frame_buffer_limit_is_global() {
        let buffer = FrameBuffer::new(3, false);

        for i in 0..6 {
            buffer.insert_frame(
                if i % 2 == 0 { "source-a" } else { "source-b" },
                FrameInfo {
                    pts: i,
                    width: 1,
                    height: 1,
                    format: PixelFormat::RGB24,
                    data: vec![0; 3],
                },
            );
        }

        assert_eq!(buffer.cache_stats(), (3, 3));
    }

    #[test]
    fn test_frame_buffer_clear() {
        let buffer = FrameBuffer::new(60, false);
        let source = "test.mp4";

        for i in 0..5 {
            let frame = FrameInfo {
                pts: i,
                width: 1920,
                height: 1080,
                format: PixelFormat::RGB24,
                data: vec![0; 1920 * 1080 * 3],
            };
            buffer.insert_frame(source, frame);
        }

        buffer.clear_all();
        let (keys, frames) = buffer.cache_stats();
        assert_eq!(keys, 0);
        assert_eq!(frames, 0);
    }
}
