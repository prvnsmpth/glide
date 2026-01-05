use crate::cursor::CursorEvent;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceType {
    Display,
    Window,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    pub source_type: SourceType,
    pub source_index: usize,
    pub width: u32,
    pub height: u32,
    /// Window offset on screen (for translating cursor coordinates)
    #[serde(default)]
    pub window_offset: (i32, i32),
    pub cursor_events: Vec<CursorEvent>,
}

impl RecordingMetadata {
    pub fn new_display(index: usize, width: u32, height: u32) -> Self {
        Self {
            source_type: SourceType::Display,
            source_index: index,
            width,
            height,
            window_offset: (0, 0),
            cursor_events: Vec::new(),
        }
    }

    pub fn new_window(window_id: u32, width: u32, height: u32, offset_x: i32, offset_y: i32) -> Self {
        Self {
            source_type: SourceType::Window,
            source_index: window_id as usize,
            width,
            height,
            window_offset: (offset_x, offset_y),
            cursor_events: Vec::new(),
        }
    }

    pub fn save(&self, video_path: &Path) -> Result<()> {
        let metadata_path = metadata_path_for_video(video_path);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&metadata_path, json)
            .with_context(|| format!("Failed to write metadata to {:?}", metadata_path))?;
        Ok(())
    }

    pub fn load(video_path: &Path) -> Result<Self> {
        let metadata_path = metadata_path_for_video(video_path);
        let json = fs::read_to_string(&metadata_path)
            .with_context(|| format!("Failed to read metadata from {:?}", metadata_path))?;
        let metadata: Self = serde_json::from_str(&json)?;
        Ok(metadata)
    }
}

/// Get the metadata file path for a video file (same name with .json extension)
pub fn metadata_path_for_video(video_path: &Path) -> std::path::PathBuf {
    video_path.with_extension("json")
}
