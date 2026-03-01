//! MediaSource - Verified media file metadata.
//!
//! A MediaSource represents a media file that has been:
//! - Verified to exist on disk
//! - Probed for metadata via FFprobe
//! - Validated for sane values
//!
//! It is safe to use for creating timeline clips.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A verified media source ready for timeline use.
///
/// # Invariants
///
/// - `path` points to an existing file
/// - `duration_secs` > 0
/// - `width` > 0 and `height` > 0
/// - `frame_rate` > 0
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSource {
    /// Unique identifier for this source
    pub id: String,

    /// Absolute path to the media file
    pub path: PathBuf,

    /// Duration in seconds
    pub duration_secs: f64,

    /// Video width in pixels
    pub width: u32,

    /// Video height in pixels
    pub height: u32,

    /// Frame rate (frames per second)
    pub frame_rate: f64,

    /// Video codec name (e.g., "h264", "hevc")
    pub video_codec: String,

    /// Audio codec name (e.g., "aac", "mp3"), if present
    pub audio_codec: Option<String>,

    /// File size in bytes
    pub file_size: u64,

    /// Human-readable file name (for display)
    pub display_name: String,
}

impl MediaSource {
    /// Create a new MediaSource with the given parameters.
    ///
    /// This is an internal constructor. Use `import_media()` to create
    /// MediaSource objects from actual files.
    pub(crate) fn new(
        id: String,
        path: PathBuf,
        duration_secs: f64,
        width: u32,
        height: u32,
        frame_rate: f64,
        video_codec: String,
        audio_codec: Option<String>,
        file_size: u64,
    ) -> Self {
        let display_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        Self {
            id,
            path,
            duration_secs,
            width,
            height,
            frame_rate,
            video_codec,
            audio_codec,
            file_size,
            display_name,
        }
    }

    /// Get the duration as an integer (microseconds) for frame-accurate work.
    pub fn duration_micros(&self) -> i64 {
        (self.duration_secs * 1_000_000.0) as i64
    }

    /// Get total frame count at the source's native frame rate.
    pub fn frame_count(&self) -> u64 {
        (self.duration_secs * self.frame_rate).round() as u64
    }
}
