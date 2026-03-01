//! Export module - Video export pipeline.
//!
//! # Overview
//!
//! This module provides a minimal, reliable export pipeline:
//! - Takes timeline state as input
//! - Renders via FFmpeg (out-of-process)
//! - Produces H.264 MP4 output
//!
//! # Usage
//!
//! ```ignore
//! let mut service = ExportService::new();
//!
//! let config = ExportConfig {
//!     timeline: engine.state(),
//!     output_path: PathBuf::from("/path/to/output.mp4"),
//!     preset: ExportPreset::h264_1080p(),
//! };
//!
//! let job_id = service.start_export(config)?;
//!
//! // Poll for progress
//! loop {
//!     if let Some(progress) = service.poll_progress(&job_id) {
//!         println!("{}%", progress.percent());
//!     }
//!
//!     if let Some(result) = service.check_complete(&job_id) {
//!         match result {
//!             ExportResult::Success { output_path, .. } => {
//!                 println!("Exported to: {}", output_path);
//!             }
//!             ExportResult::Failed { error } => {
//!                 println!("Export failed: {}", error);
//!             }
//!             ExportResult::Cancelled => {
//!                 println!("Export cancelled");
//!             }
//!         }
//!         break;
//!     }
//!
//!     std::thread::sleep(std::time::Duration::from_millis(100));
//! }
//! ```
//!
//! # Error Handling
//!
//! The export pipeline handles these failure modes:
//! - FFmpeg crash: Detected via non-zero exit, returns `FFmpegFailed`
//! - User cancel: Kill process, return `Cancelled`
//! - Disk full: Parse FFmpeg stderr, return `DiskFull`
//! - Source missing: Pre-check before spawn, return `SourceFileNotFound`

mod export_service;
mod export_types;
mod ffmpeg_process;

pub use export_service::ExportService;
pub use export_types::*;
