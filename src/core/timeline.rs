use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TrackType {
    Video,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub source_path: String,
    pub start_frame: u64,
    pub end_frame: u64,
    pub track_index: usize,
    pub enabled: bool,
    pub opacity: f32,
    pub volume: f32,
    pub speed: f32,
    pub transform: Transform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub position_x: f64,
    pub position_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position_x: 0.0,
            position_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub track_type: TrackType,
    pub clips: Vec<Clip>,
    pub muted: bool,
    pub locked: bool,
    pub sync_lock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_frames: u64,
    pub tracks: Vec<Track>,
    pub nested_sequences: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Timeline {
    pub sequences: HashMap<String, Sequence>,
    pub active_sequence: String,
    pub playhead_frame: u64,
    pub zoom_level: f32,
    pub snap_enabled: bool,
    pub snap_threshold: u32,
    pub undo_stack: Vec<TimelineCommand>,
    pub redo_stack: Vec<TimelineCommand>,
    pub max_undo_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineCommand {
    AddClip { clip: Clip, sequence_id: String },
    RemoveClip { clip_id: String, sequence_id: String, track_index: usize },
    MoveClip { clip_id: String, from_frame: u64, to_frame: u64 },
    SplitClip { clip_id: String, split_frame: u64 },
    RippleDelete { clip_id: String, sequence_id: String },
    SetTransform { clip_id: String, transform: Transform },
    SetSpeed { clip_id: String, speed: f32 },
    ToggleMute { track_id: String },
    ToggleLock { track_id: String },
}

impl Timeline {
    pub fn new(sequence_id: &str, name: &str, width: u32, height: u32, fps: f64) -> Self {
        let sequence = Sequence {
            id: sequence_id.to_string(),
            name: name.to_string(),
            width,
            height,
            fps,
            duration_frames: (fps * 3600.0) as u64,
            tracks: Vec::new(),
            nested_sequences: Vec::new(),
        };

        let mut sequences = HashMap::new();
        sequences.insert(sequence_id.to_string(), sequence);

        Self {
            sequences,
            active_sequence: sequence_id.to_string(),
            playhead_frame: 0,
            zoom_level: 1.0,
            snap_enabled: true,
            snap_threshold: 2,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_undo_entries: 200,
        }
    }

    pub fn add_video_track(&mut self) -> String {
        let track_id = format!("vtrack_{}", self.next_track_id());
        self.add_track_inner(&track_id, TrackType::Video);
        track_id
    }

    pub fn add_audio_track(&mut self) -> String {
        let track_id = format!("atrack_{}", self.next_track_id());
        self.add_track_inner(&track_id, TrackType::Audio);
        track_id
    }

    fn add_track_inner(&mut self, id: &str, track_type: TrackType) {
        if let Some(seq) = self.sequences.get_mut(&self.active_sequence) {
            let track = Track {
                id: id.to_string(),
                track_type,
                clips: Vec::new(),
                muted: false,
                locked: false,
                sync_lock: false,
            };
            seq.tracks.push(track);
        }
    }

    pub fn add_clip(&mut self, clip: Clip) -> Result<(), String> {
        let cmd = TimelineCommand::AddClip {
            clip: clip.clone(),
            sequence_id: self.active_sequence.clone(),
        };

        if let Some(seq) = self.sequences.get_mut(&self.active_sequence) {
            if clip.track_index < seq.tracks.len() {
                seq.tracks[clip.track_index].clips.push(clip);
            } else {
                return Err(format!("Track index {} out of range", clip.track_index));
            }
        } else {
            return Err("Active sequence not found".to_string());
        }

        self.push_undo(cmd);
        Ok(())
    }

    pub fn remove_clip(&mut self, clip_id: &str) -> Result<(), String> {
        let track_index = if let Some(seq) = self.sequences.get_mut(&self.active_sequence) {
            let mut removed_track = None;
            for track in &mut seq.tracks {
                if let Some(pos) = track.clips.iter().position(|c| c.id == clip_id) {
                    removed_track = Some(track.clips.remove(pos).track_index);
                    break;
                }
            }
            removed_track.ok_or_else(|| format!("Clip '{}' not found", clip_id))?
        } else {
            return Err("Active sequence not found".to_string());
        };

        self.push_undo(TimelineCommand::RemoveClip {
            clip_id: clip_id.to_string(),
            sequence_id: self.active_sequence.clone(),
            track_index,
        });
        Ok(())
    }

    pub fn ripple_delete(&mut self, clip_id: &str) -> Result<(), String> {
        let removed = if let Some(seq) = self.sequences.get_mut(&self.active_sequence) {
            let mut removed = false;
            for track in &mut seq.tracks {
                if let Some(pos) = track.clips.iter().position(|c| c.id == clip_id) {
                    let clip = track.clips[pos].clone();
                    let duration = clip.end_frame.saturating_sub(clip.start_frame);

                    track.clips.remove(pos);

                    for remaining in &mut track.clips {
                        if remaining.start_frame >= clip.end_frame {
                            remaining.start_frame -= duration;
                            remaining.end_frame -= duration;
                        }
                    }
                    removed = true;
                    break;
                }
            }
            removed
        } else {
            return Err("Active sequence not found".to_string());
        };

        if !removed {
            return Err(format!("Clip '{}' not found", clip_id));
        }

        self.push_undo(TimelineCommand::RippleDelete {
            clip_id: clip_id.to_string(),
            sequence_id: self.active_sequence.clone(),
        });
        Ok(())
    }

    pub fn split_clip(&mut self, clip_id: &str, split_frame: u64) -> Result<Clip, String> {
        let new_clip = if let Some(seq) = self.sequences.get_mut(&self.active_sequence) {
            let mut split = None;
            for track in &mut seq.tracks {
                if let Some(pos) = track.clips.iter().position(|c| c.id == clip_id) {
                    let clip = track.clips[pos].clone();

                    if split_frame <= clip.start_frame || split_frame >= clip.end_frame {
                        return Err("Split frame outside clip range".to_string());
                    }

                    let new_clip = Clip {
                        id: format!("{}_split_{}", clip.id, split_frame),
                        source_path: clip.source_path.clone(),
                        start_frame: split_frame,
                        end_frame: clip.end_frame,
                        track_index: clip.track_index,
                        enabled: clip.enabled,
                        opacity: clip.opacity,
                        volume: clip.volume,
                        speed: clip.speed,
                        transform: clip.transform.clone(),
                    };

                    track.clips[pos].end_frame = split_frame;
                    track.clips.insert(pos + 1, new_clip.clone());
                    split = Some(new_clip);
                    break;
                }
            }
            split.ok_or_else(|| format!("Clip '{}' not found", clip_id))?
        } else {
            return Err("Active sequence not found".to_string());
        };

        self.push_undo(TimelineCommand::SplitClip {
            clip_id: clip_id.to_string(),
            split_frame,
        });
        Ok(new_clip)
    }

    pub fn set_playhead(&mut self, frame: u64) {
        self.playhead_frame = frame;
    }

    pub fn playhead(&self) -> u64 {
        self.playhead_frame
    }

    pub fn active_sequence(&self) -> Option<&Sequence> {
        self.sequences.get(&self.active_sequence)
    }

    pub fn active_sequence_mut(&mut self) -> Option<&mut Sequence> {
        self.sequences.get_mut(&self.active_sequence)
    }

    pub fn undo(&mut self) -> Option<TimelineCommand> {
        self.undo_stack.pop().inspect(|cmd| {
            self.redo_stack.push(cmd.clone());
        })
    }

    pub fn redo(&mut self) -> Option<TimelineCommand> {
        self.redo_stack.pop().inspect(|cmd| {
            self.undo_stack.push(cmd.clone());
        })
    }

    fn push_undo(&mut self, cmd: TimelineCommand) {
        self.undo_stack.push(cmd);
        if self.undo_stack.len() > self.max_undo_entries {
            self.undo_stack.drain(0..self.undo_stack.len() - self.max_undo_entries);
        }
        self.redo_stack.clear();
    }

    fn next_track_id(&self) -> usize {
        self.sequences.values()
            .flat_map(|s| &s.tracks)
            .filter_map(|t| t.id.rsplit_once('_')?.1.parse::<usize>().ok())
            .max()
            .map(|id| id + 1)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_creation() {
        let timeline = Timeline::new("seq1", "Main Sequence", 1920, 1080, 30.0);
        assert_eq!(timeline.active_sequence, "seq1");
        assert_eq!(timeline.playhead_frame, 0);
        assert!(timeline.snap_enabled);
    }

    #[test]
    fn test_add_tracks() {
        let mut timeline = Timeline::new("seq1", "Main", 1920, 1080, 24.0);
        let vtrack = timeline.add_video_track();
        let atrack = timeline.add_audio_track();
        assert!(vtrack.starts_with("vtrack_"));
        assert!(atrack.starts_with("atrack_"));
        assert_eq!(vtrack, "vtrack_0");
        assert_eq!(atrack, "atrack_1");
    }

    #[test]
    fn test_add_and_remove_clip() {
        let mut timeline = Timeline::new("seq1", "Main", 1920, 1080, 30.0);
        timeline.add_video_track();

        let clip = Clip {
            id: "clip1".to_string(),
            source_path: "test.mp4".to_string(),
            start_frame: 0,
            end_frame: 300,
            track_index: 0,
            enabled: true,
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            transform: Transform::default(),
        };

        timeline.add_clip(clip).unwrap();
        assert_eq!(timeline.undo_stack.len(), 1);

        timeline.remove_clip("clip1").unwrap();
        assert_eq!(timeline.undo_stack.len(), 2);
    }

    #[test]
    fn test_split_clip() {
        let mut timeline = Timeline::new("seq1", "Main", 1920, 1080, 30.0);
        timeline.add_video_track();

        let clip = Clip {
            id: "clip1".to_string(),
            source_path: "test.mp4".to_string(),
            start_frame: 0,
            end_frame: 300,
            track_index: 0,
            enabled: true,
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            transform: Transform::default(),
        };

        timeline.add_clip(clip).unwrap();
        let new_clip = timeline.split_clip("clip1", 150).unwrap();
        assert_eq!(new_clip.start_frame, 150);
        assert_eq!(new_clip.end_frame, 300);
    }

    #[test]
    fn test_ripple_delete() {
        let mut timeline = Timeline::new("seq1", "Main", 1920, 1080, 30.0);
        timeline.add_video_track();

        let clip1 = Clip {
            id: "clip1".to_string(),
            source_path: "test.mp4".to_string(),
            start_frame: 0,
            end_frame: 100,
            track_index: 0,
            enabled: true,
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            transform: Transform::default(),
        };

        let clip2 = Clip {
            id: "clip2".to_string(),
            source_path: "test2.mp4".to_string(),
            start_frame: 100,
            end_frame: 200,
            track_index: 0,
            enabled: true,
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            transform: Transform::default(),
        };

        timeline.add_clip(clip1).unwrap();
        timeline.add_clip(clip2).unwrap();
        timeline.ripple_delete("clip1").unwrap();

        if let Some(seq) = timeline.active_sequence() {
            assert_eq!(seq.tracks[0].clips.len(), 1);
            assert_eq!(seq.tracks[0].clips[0].start_frame, 0);
            assert_eq!(seq.tracks[0].clips[0].end_frame, 100);
        }
    }

    #[test]
    fn test_undo_redo() {
        let mut timeline = Timeline::new("seq1", "Main", 1920, 1080, 30.0);
        timeline.add_video_track();

        let clip = Clip {
            id: "clip1".to_string(),
            source_path: "test.mp4".to_string(),
            start_frame: 0,
            end_frame: 100,
            track_index: 0,
            enabled: true,
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            transform: Transform::default(),
        };

        timeline.add_clip(clip).unwrap();
        assert_eq!(timeline.undo_stack.len(), 1);
        assert_eq!(timeline.redo_stack.len(), 0);

        timeline.undo();
        assert_eq!(timeline.undo_stack.len(), 0);
        assert_eq!(timeline.redo_stack.len(), 1);

        timeline.redo();
        assert_eq!(timeline.undo_stack.len(), 1);
        assert_eq!(timeline.redo_stack.len(), 0);
    }
}
