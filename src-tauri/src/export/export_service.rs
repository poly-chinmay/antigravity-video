//! ExportService - Coordinates export operations.
//!
//! # Design
//!
//! ExportService is the main entry point for exports:
//! - Validates timeline and output path
//! - Builds FFmpeg command from timeline state
//! - Manages active export process
//! - Reports progress and final result
//!
//! # Single Export Constraint
//!
//! Only one export can run at a time. This simplifies resource management
//! and matches expected user behavior.

use super::export_types::*;
use super::ffmpeg_process::FFmpegProcess;
use crate::timeline::TimelineState;
use std::path::{Path, PathBuf};
use std::time::Instant;

// =============================================================================
// EXPORT SERVICE
// =============================================================================

/// Service for managing video exports.
#[derive(Default)]
pub struct ExportService {
    /// Currently active export (if any)
    active_export: Option<ActiveExport>,
}

/// State of an active export.
struct ActiveExport {
    /// Unique job ID
    job_id: ExportJobId,

    /// FFmpeg child process
    process: FFmpegProcess,

    /// Output path
    output_path: PathBuf,

    /// Total expected duration
    total_duration_secs: f64,

    /// When export started
    start_time: Instant,
}

impl ExportService {
    /// Create a new export service.
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // PUBLIC API
    // =========================================================================

    /// Start an export.
    ///
    /// Returns the job ID for tracking progress.
    pub fn start_export(&mut self, config: ExportConfig) -> Result<ExportJobId, ExportError> {
        // Check if export already running
        if let Some(active) = &self.active_export {
            return Err(ExportError::AlreadyRunning {
                current_job_id: active.job_id.clone(),
            });
        }

        // Validate timeline
        if config.timeline.clips.is_empty() {
            return Err(ExportError::EmptyTimeline);
        }

        // Validate output path
        validate_output_path(&config.output_path)?;

        // Validate source files exist
        for clip in &config.timeline.clips {
            if !Path::new(&clip.source_file).exists() {
                return Err(ExportError::SourceFileNotFound {
                    path: clip.source_file.clone(),
                });
            }
        }

        // Build FFmpeg command
        let (args, total_duration_secs, total_frames) =
            build_ffmpeg_args(&config.timeline, &config.output_path, &config.preset)?;

        // Spawn FFmpeg process
        let process = FFmpegProcess::spawn(args, total_frames)?;

        // Generate job ID
        let job_id = uuid::Uuid::new_v4().to_string();

        // Store active export
        self.active_export = Some(ActiveExport {
            job_id: job_id.clone(),
            process,
            output_path: config.output_path,
            total_duration_secs,
            start_time: Instant::now(),
        });

        println!("📹 Export started: {}", job_id);

        Ok(job_id)
    }

    /// Poll for progress updates.
    ///
    /// Returns None if no export is active or job_id doesn't match.
    pub fn poll_progress(&mut self, job_id: &ExportJobId) -> Option<ExportProgress> {
        let active = self.active_export.as_mut()?;

        if &active.job_id != job_id {
            return None;
        }

        // Poll FFmpeg for progress
        let ffmpeg_progress = active.process.poll_progress();
        let elapsed_secs = active.start_time.elapsed().as_secs_f64();

        // Convert to ExportProgress
        let frames_total = active.process.total_frames();
        let frames_completed = ffmpeg_progress.frame;

        // Calculate ETA
        let eta_secs = if frames_completed > 0 && frames_total > 0 {
            let frames_remaining = frames_total.saturating_sub(frames_completed);
            let frames_per_sec = frames_completed as f64 / elapsed_secs;
            if frames_per_sec > 0.0 {
                Some(frames_remaining as f64 / frames_per_sec)
            } else {
                None
            }
        } else {
            None
        };

        // Determine status
        let status = if ffmpeg_progress.is_complete {
            ExportStatus::Complete
        } else if frames_completed > 0 {
            ExportStatus::Encoding {
                current_frame: frames_completed,
            }
        } else {
            ExportStatus::Preparing
        };

        Some(ExportProgress {
            job_id: job_id.clone(),
            frames_completed,
            frames_total,
            elapsed_secs,
            eta_secs,
            status,
        })
    }

    /// Check if export has completed and get result.
    ///
    /// If complete, removes the active export and returns the result.
    pub fn check_complete(&mut self, job_id: &ExportJobId) -> Option<ExportResult> {
        // Check if this job is active
        if self.active_export.as_ref().map(|a| &a.job_id) != Some(job_id) {
            return None;
        }

        // Try to get exit status
        let exit_result = {
            let active = self.active_export.as_mut()?;
            active.process.try_wait()
        };

        // If process hasn't exited, return None
        let exit_result = exit_result?;

        // Process has exited, remove active export
        let active = self.active_export.take().unwrap();

        match exit_result {
            Ok(()) => {
                // Success! Get file info
                let file_size_bytes = std::fs::metadata(&active.output_path)
                    .map(|m| m.len())
                    .unwrap_or(0);

                println!(
                    "✅ Export complete: {} ({} bytes)",
                    active.output_path.display(),
                    file_size_bytes
                );

                Some(ExportResult::Success {
                    output_path: active.output_path.to_string_lossy().to_string(),
                    duration_secs: active.total_duration_secs,
                    file_size_bytes,
                })
            }
            Err(error) => {
                println!("❌ Export failed: {}", error);
                Some(ExportResult::Failed { error })
            }
        }
    }

    /// Cancel an active export.
    pub fn cancel(&mut self, job_id: &ExportJobId) -> Result<(), ExportError> {
        let active = self.active_export.as_mut().ok_or(ExportError::Cancelled)?;

        if &active.job_id != job_id {
            return Err(ExportError::Cancelled);
        }

        // Kill the process
        active.process.kill()?;

        // Remove active export
        let active = self.active_export.take().unwrap();

        // Try to delete partial output file
        let _ = std::fs::remove_file(&active.output_path);

        println!("🛑 Export cancelled: {}", job_id);

        Ok(())
    }

    /// Check if an export is currently running.
    pub fn is_running(&self) -> bool {
        self.active_export.is_some()
    }

    /// Get the currently active job ID.
    pub fn active_job_id(&self) -> Option<&ExportJobId> {
        self.active_export.as_ref().map(|a| &a.job_id)
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Validate that the output path is usable.
fn validate_output_path(path: &Path) -> Result<(), ExportError> {
    // Check parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(ExportError::OutputPathInvalid {
                path: path.to_string_lossy().to_string(),
                reason: "Parent directory does not exist".to_string(),
            });
        }

        // Check if directory is writable by trying to create a temp file
        let test_path = parent.join(".export_test");
        match std::fs::write(&test_path, b"test") {
            Ok(()) => {
                let _ = std::fs::remove_file(&test_path);
            }
            Err(e) => {
                return Err(ExportError::OutputPathInvalid {
                    path: path.to_string_lossy().to_string(),
                    reason: format!("Directory not writable: {}", e),
                });
            }
        }
    }

    Ok(())
}

/// Build FFmpeg arguments from timeline state.
fn build_ffmpeg_args(
    timeline: &TimelineState,
    output_path: &Path,
    preset: &ExportPreset,
) -> Result<(Vec<String>, f64, u64), ExportError> {
    let mut args = Vec::new();

    // Overwrite output
    args.push("-y".to_string());

    // Sort clips by start time
    let mut clips = timeline.clips.clone();
    clips.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Add inputs
    for clip in &clips {
        args.push("-i".to_string());
        args.push(clip.source_file.clone());
    }

    // Build filter complex
    let mut filter_complex = String::new();
    let mut concat_inputs = String::new();
    let mut total_duration_secs = 0.0;

    for (i, clip) in clips.iter().enumerate() {
        // For timeline::Clip, we use the full clip duration (no source_in/source_out support yet)
        // Start at 0 in the source and use clip.duration
        let start_secs = 0.0; // Start from beginning of source for now
        let duration_secs = clip.duration;
        total_duration_secs += duration_secs;

        // Video filter chain:
        // 1. Trim to duration
        // 2. Scale to fit within output dimensions
        // 3. Pad to exact output dimensions (centering)
        // 4. Reset timestamps
        filter_complex.push_str(&format!(
            "[{}:v]trim=start={:.6}:duration={:.6},setpts=PTS-STARTPTS,\
             scale={}:{}:force_original_aspect_ratio=decrease,\
             pad={}:{}:(ow-iw)/2:(oh-ih)/2:black[v{}];",
            i,
            start_secs,
            duration_secs,
            preset.width,
            preset.height,
            preset.width,
            preset.height,
            i
        ));

        concat_inputs.push_str(&format!("[v{}]", i));
    }

    // Concat filter
    filter_complex.push_str(&format!(
        "{}concat=n={}:v=1:a=0[outv]",
        concat_inputs,
        clips.len()
    ));

    args.push("-filter_complex".to_string());
    args.push(filter_complex);
    args.push("-map".to_string());
    args.push("[outv]".to_string());

    // Output codec settings
    args.push("-c:v".to_string());
    args.push(preset.codec.clone());
    args.push("-preset".to_string());
    args.push(preset.encoding_speed.clone());
    args.push("-pix_fmt".to_string());
    args.push("yuv420p".to_string());

    // Progress output
    args.push("-progress".to_string());
    args.push("pipe:1".to_string());

    // Output file
    args.push(output_path.to_string_lossy().to_string());

    // Calculate total frames
    let total_frames = (total_duration_secs * preset.fps).ceil() as u64;

    Ok((args, total_duration_secs, total_frames))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;

    fn make_test_timeline() -> TimelineState {
        let mut timeline = TimelineState::default();
        timeline.clips.push(Clip::new(
            "track0".to_string(),
            0.0,
            5.0,
            "/tmp/test.mp4".to_string(),
        ));
        timeline
    }

    #[test]
    fn test_export_service_new() {
        let service = ExportService::new();
        assert!(!service.is_running());
        assert!(service.active_job_id().is_none());
    }

    #[test]
    fn test_empty_timeline_rejected() {
        let mut service = ExportService::new();

        let config = ExportConfig {
            timeline: TimelineState::default(),
            output_path: PathBuf::from("/tmp/test.mp4"),
            preset: ExportPreset::h264_1080p(),
        };

        let result = service.start_export(config);
        assert!(matches!(result, Err(ExportError::EmptyTimeline)));
    }

    #[test]
    fn test_build_ffmpeg_args() {
        let timeline = make_test_timeline();
        let output_path = PathBuf::from("/tmp/output.mp4");
        let preset = ExportPreset::h264_1080p();

        let result = build_ffmpeg_args(&timeline, &output_path, &preset);
        assert!(result.is_ok());

        let (args, duration, frames) = result.unwrap();

        // Check key arguments are present
        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"-filter_complex".to_string()));
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"-progress".to_string()));

        // Duration should be ~5 seconds
        assert!((duration - 5.0).abs() < 0.1);

        // Frames at 30fps should be ~150
        assert!(frames > 0);
    }
}
