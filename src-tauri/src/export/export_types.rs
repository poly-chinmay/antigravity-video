//! Export types - Data structures for export pipeline.
//!
//! # Design
//!
//! All export-related types are defined here for:
//! - Clear separation of concerns
//! - Easy serialization for Tauri events
//! - Typed error handling

use crate::timeline::TimelineState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// =============================================================================
// IDENTIFIERS
// =============================================================================

/// Unique identifier for an export job.
pub type ExportJobId = String;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for an export job.
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// Timeline state to export
    pub timeline: TimelineState,

    /// Output file path
    pub output_path: PathBuf,

    /// Export preset to use
    pub preset: ExportPreset,
}

/// Export preset defining output format.
#[derive(Debug, Clone)]
pub struct ExportPreset {
    /// Output width in pixels
    pub width: u32,

    /// Output height in pixels
    pub height: u32,

    /// Frames per second
    pub fps: f64,

    /// Video codec (e.g., "libx264")
    pub codec: String,

    /// Container format (e.g., "mp4")
    pub container: String,

    /// Encoding preset (e.g., "fast", "medium", "slow")
    pub encoding_speed: String,
}

impl ExportPreset {
    /// Standard H.264 1080p preset.
    pub fn h264_1080p() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30.0,
            codec: "libx264".to_string(),
            container: "mp4".to_string(),
            encoding_speed: "fast".to_string(),
        }
    }
}

impl Default for ExportPreset {
    fn default() -> Self {
        Self::h264_1080p()
    }
}

// =============================================================================
// PROGRESS
// =============================================================================

/// Progress update from an export job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProgress {
    /// Job this progress belongs to
    pub job_id: ExportJobId,

    /// Number of frames completed
    pub frames_completed: u64,

    /// Total number of frames to export
    pub frames_total: u64,

    /// Elapsed time in seconds
    pub elapsed_secs: f64,

    /// Estimated time remaining (if calculable)
    pub eta_secs: Option<f64>,

    /// Current status
    pub status: ExportStatus,
}

impl ExportProgress {
    /// Create initial progress for a job.
    pub fn new(job_id: ExportJobId, frames_total: u64) -> Self {
        Self {
            job_id,
            frames_completed: 0,
            frames_total,
            elapsed_secs: 0.0,
            eta_secs: None,
            status: ExportStatus::Preparing,
        }
    }

    /// Calculate percentage complete.
    pub fn percent(&self) -> f32 {
        if self.frames_total == 0 {
            0.0
        } else {
            (self.frames_completed as f32 / self.frames_total as f32) * 100.0
        }
    }
}

/// Export job status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum ExportStatus {
    /// Preparing export (building filter graph)
    Preparing,

    /// Encoding frames
    Encoding {
        /// Current frame being encoded
        current_frame: u64,
    },

    /// Finalizing output file
    Finalizing,

    /// Export completed successfully
    Complete,

    /// Export was cancelled by user
    Cancelled,

    /// Export failed
    Failed {
        /// Error message
        message: String,
    },
}

// =============================================================================
// RESULT
// =============================================================================

/// Final result of an export job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ExportResult {
    /// Export completed successfully
    Success {
        /// Path to the output file
        output_path: String,

        /// Duration of exported video in seconds
        duration_secs: f64,

        /// File size in bytes
        file_size_bytes: u64,
    },

    /// Export was cancelled
    Cancelled,

    /// Export failed
    Failed {
        /// Error details
        error: ExportError,
    },
}

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ExportError {
    /// Timeline has no clips to export
    EmptyTimeline,

    /// A source file referenced by a clip was not found
    SourceFileNotFound {
        /// Path that was not found
        path: String,
    },

    /// FFmpeg binary was not found on the system
    FFmpegNotFound,

    /// FFmpeg process exited with non-zero status
    FFmpegFailed {
        /// Exit code from FFmpeg
        exit_code: i32,

        /// stderr output from FFmpeg
        stderr: String,
    },

    /// FFmpeg process was killed (e.g., due to timeout)
    FFmpegKilled,

    /// Output path is invalid or not writable
    OutputPathInvalid {
        /// The invalid path
        path: String,

        /// Reason it's invalid
        reason: String,
    },

    /// Disk is full or quota exceeded
    DiskFull {
        /// Available space in bytes (if known)
        available_bytes: Option<u64>,
    },

    /// Generic I/O error
    IoError {
        /// Error description
        message: String,
    },

    /// An export is already running
    AlreadyRunning {
        /// ID of the currently running job
        current_job_id: ExportJobId,
    },

    /// Export was cancelled by user
    Cancelled,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTimeline => write!(f, "Timeline is empty"),
            Self::SourceFileNotFound { path } => write!(f, "Source file not found: {}", path),
            Self::FFmpegNotFound => write!(f, "FFmpeg not found in PATH"),
            Self::FFmpegFailed { exit_code, stderr } => {
                write!(f, "FFmpeg failed (exit {}): {}", exit_code, stderr)
            }
            Self::FFmpegKilled => write!(f, "FFmpeg process was killed"),
            Self::OutputPathInvalid { path, reason } => {
                write!(f, "Invalid output path '{}': {}", path, reason)
            }
            Self::DiskFull { available_bytes } => {
                if let Some(bytes) = available_bytes {
                    write!(f, "Disk full ({} bytes available)", bytes)
                } else {
                    write!(f, "Disk full")
                }
            }
            Self::IoError { message } => write!(f, "I/O error: {}", message),
            Self::AlreadyRunning { current_job_id } => {
                write!(f, "Export already running: {}", current_job_id)
            }
            Self::Cancelled => write!(f, "Export cancelled"),
        }
    }
}

impl std::error::Error for ExportError {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_h264_1080p() {
        let preset = ExportPreset::h264_1080p();
        assert_eq!(preset.width, 1920);
        assert_eq!(preset.height, 1080);
        assert_eq!(preset.codec, "libx264");
    }

    #[test]
    fn test_progress_percent() {
        let mut progress = ExportProgress::new("job1".to_string(), 100);
        assert_eq!(progress.percent(), 0.0);

        progress.frames_completed = 50;
        assert_eq!(progress.percent(), 50.0);

        progress.frames_completed = 100;
        assert_eq!(progress.percent(), 100.0);
    }

    #[test]
    fn test_progress_percent_zero_total() {
        let progress = ExportProgress::new("job1".to_string(), 0);
        assert_eq!(progress.percent(), 0.0);
    }

    #[test]
    fn test_export_error_display() {
        let err = ExportError::EmptyTimeline;
        assert_eq!(err.to_string(), "Timeline is empty");

        let err = ExportError::SourceFileNotFound {
            path: "/test.mp4".to_string(),
        };
        assert!(err.to_string().contains("/test.mp4"));
    }
}
