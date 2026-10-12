//! Media import - FFprobe-based metadata extraction and validation.
//!
//! # Design
//!
//! 1. Validate file path (exists, is file, is absolute)
//! 2. Find FFprobe binary (bundled first, then PATH)
//! 3. Run FFprobe with JSON output
//! 4. Parse and validate metadata
//! 5. Return MediaSource or error

use super::media_source::MediaSource;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Result type for media import operations.
pub type MediaImportResult = Result<MediaSource, MediaImportError>;

/// Errors that can occur during media import.
#[derive(Debug, Clone)]
pub enum MediaImportError {
    /// File does not exist at the given path
    FileNotFound { path: String },

    /// Path exists but is not a file (e.g., directory)
    NotAFile { path: String },

    /// Path is not absolute
    NotAbsolutePath { path: String },

    /// FFprobe binary not found
    FFprobeNotFound,

    /// FFprobe execution failed
    FFprobeFailed { message: String },

    /// FFprobe timed out
    FFprobeTimeout { timeout_secs: u64 },

    /// Failed to parse FFprobe output
    ParseError { message: String },

    /// No video stream found in file
    NoVideoStream { path: String },

    /// Invalid duration (zero, negative, or missing)
    InvalidDuration { path: String, value: f64 },

    /// Invalid resolution (zero or negative dimensions)
    InvalidResolution {
        path: String,
        width: i64,
        height: i64,
    },

    /// Invalid frame rate
    InvalidFrameRate { path: String, value: f64 },

    /// File read error
    IoError { message: String },
}

impl std::fmt::Display for MediaImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound { path } => write!(f, "File not found: {}", path),
            Self::NotAFile { path } => write!(f, "Not a file: {}", path),
            Self::NotAbsolutePath { path } => write!(f, "Path must be absolute: {}", path),
            Self::FFprobeNotFound => write!(f, "FFprobe not found in bundled location or PATH"),
            Self::FFprobeFailed { message } => write!(f, "FFprobe failed: {}", message),
            Self::FFprobeTimeout { timeout_secs } => {
                write!(f, "FFprobe timed out after {} seconds", timeout_secs)
            }
            Self::ParseError { message } => {
                write!(f, "Failed to parse FFprobe output: {}", message)
            }
            Self::NoVideoStream { path } => write!(f, "No video stream found in: {}", path),
            Self::InvalidDuration { path, value } => {
                write!(f, "Invalid duration {} for: {}", value, path)
            }
            Self::InvalidResolution {
                path,
                width,
                height,
            } => {
                write!(f, "Invalid resolution {}x{} for: {}", width, height, path)
            }
            Self::InvalidFrameRate { path, value } => {
                write!(f, "Invalid frame rate {} for: {}", value, path)
            }
            Self::IoError { message } => write!(f, "IO error: {}", message),
        }
    }
}

impl std::error::Error for MediaImportError {}

// Serialize for Tauri command return
impl serde::Serialize for MediaImportError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// =============================================================================
// FFPROBE OUTPUT STRUCTURES
// =============================================================================

/// FFprobe JSON output structure (partial, only what we need).
#[derive(Debug, Deserialize)]
struct FFprobeOutput {
    streams: Option<Vec<FFprobeStream>>,
    format: Option<FFprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FFprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    r_frame_rate: Option<String>, // e.g., "30000/1001" or "30/1"
    duration: Option<String>,     // Stream duration (may differ from format)
}

#[derive(Debug, Deserialize)]
struct FFprobeFormat {
    duration: Option<String>,
    size: Option<String>,
}

// =============================================================================
// IMPORT FUNCTION
// =============================================================================

/// Import a media file and produce a verified MediaSource.
///
/// # Arguments
///
/// * `path` - Absolute path to the media file
///
/// # Returns
///
/// * `Ok(MediaSource)` - Verified media source with metadata
/// * `Err(MediaImportError)` - Detailed error if import failed
///
/// # Thread Safety
///
/// This function is synchronous and should be called from a worker thread,
/// not the main Tauri thread.
pub fn import_media<P: AsRef<Path>>(path: P) -> MediaImportResult {
    let path = path.as_ref();

    // Step 1: Validate path
    validate_path(path)?;

    // Step 2: Find FFprobe
    let ffprobe_path = find_ffprobe()?;

    // Step 3: Run FFprobe
    let output = run_ffprobe(&ffprobe_path, path)?;

    // Step 4: Parse output
    let probe_data = parse_ffprobe_output(&output)?;

    // Step 5: Extract and validate metadata
    let media_source = build_media_source(path, probe_data)?;

    Ok(media_source)
}

// =============================================================================
// VALIDATION
// =============================================================================

fn validate_path(path: &Path) -> Result<(), MediaImportError> {
    // Must be absolute
    if !path.is_absolute() {
        return Err(MediaImportError::NotAbsolutePath {
            path: path.display().to_string(),
        });
    }

    // Must exist
    if !path.exists() {
        return Err(MediaImportError::FileNotFound {
            path: path.display().to_string(),
        });
    }

    // Must be a file
    if !path.is_file() {
        return Err(MediaImportError::NotAFile {
            path: path.display().to_string(),
        });
    }

    Ok(())
}

// =============================================================================
// FFPROBE DISCOVERY
// =============================================================================

/// Find FFprobe binary: bundled first, then PATH.
fn find_ffprobe() -> Result<PathBuf, MediaImportError> {
    // Try bundled locations first
    let bundled_paths = get_bundled_ffprobe_paths();
    for bundled in bundled_paths {
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // Fallback to PATH
    if let Ok(output) = Command::new("which")
        .arg("ffprobe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(path_str.trim());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    // Windows fallback
    if let Ok(output) = Command::new("where")
        .arg("ffprobe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            // 'where' can return multiple lines, take first
            if let Some(first_line) = path_str.lines().next() {
                let path = PathBuf::from(first_line.trim());
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    Err(MediaImportError::FFprobeNotFound)
}

/// Get potential bundled FFprobe locations.
fn get_bundled_ffprobe_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // macOS: Check in app bundle Resources
    #[cfg(target_os = "macos")]
    {
        if let Ok(exe) = std::env::current_exe() {
            // In development: check relative to binary
            if let Some(parent) = exe.parent() {
                paths.push(parent.join("ffprobe"));
                paths.push(parent.join("bin").join("ffprobe"));
            }
            // In production: check in Resources
            if let Some(parent) = exe.parent() {
                if let Some(macos_dir) = parent.parent() {
                    paths.push(macos_dir.join("Resources").join("ffprobe"));
                    paths.push(macos_dir.join("Resources").join("bin").join("ffprobe"));
                }
            }
        }
    }

    // Linux: Similar structure
    #[cfg(target_os = "linux")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                paths.push(parent.join("ffprobe"));
                paths.push(parent.join("bin").join("ffprobe"));
            }
        }
    }

    // Windows
    #[cfg(target_os = "windows")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                paths.push(parent.join("ffprobe.exe"));
                paths.push(parent.join("bin").join("ffprobe.exe"));
            }
        }
    }

    paths
}

// =============================================================================
// FFPROBE EXECUTION
// =============================================================================

/// Timeout for FFprobe execution.
const FFPROBE_TIMEOUT_SECS: u64 = 30;

fn run_ffprobe(ffprobe_path: &Path, media_path: &Path) -> Result<String, MediaImportError> {
    let mut cmd = Command::new(ffprobe_path);
    cmd.args([
        "-v",
        "quiet",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
    ]);
    cmd.arg(media_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| MediaImportError::FFprobeFailed {
        message: format!("Failed to spawn FFprobe: {}", e),
    })?;

    // Wait with timeout
    let output = wait_with_timeout(child, Duration::from_secs(FFPROBE_TIMEOUT_SECS))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaImportError::FFprobeFailed {
            message: format!("FFprobe exited with error: {}", stderr),
        });
    }

    String::from_utf8(output.stdout).map_err(|e| MediaImportError::ParseError {
        message: format!("Invalid UTF-8 in FFprobe output: {}", e),
    })
}

/// Wait for a child process with timeout.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, MediaImportError> {
    // Simple polling approach - no async needed
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process finished
                let stdout = child.stdout.take().map_or(Vec::new(), |mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                });
                let stderr = child.stderr.take().map_or(Vec::new(), |mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                });
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                // Still running
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    return Err(MediaImportError::FFprobeTimeout {
                        timeout_secs: timeout.as_secs(),
                    });
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                return Err(MediaImportError::FFprobeFailed {
                    message: format!("Error waiting for FFprobe: {}", e),
                });
            }
        }
    }
}

// =============================================================================
// OUTPUT PARSING
// =============================================================================

fn parse_ffprobe_output(json_str: &str) -> Result<FFprobeOutput, MediaImportError> {
    serde_json::from_str(json_str).map_err(|e| MediaImportError::ParseError {
        message: format!("Invalid JSON: {}", e),
    })
}

// =============================================================================
// MEDIA SOURCE CONSTRUCTION
// =============================================================================

fn build_media_source(path: &Path, data: FFprobeOutput) -> Result<MediaSource, MediaImportError> {
    let path_str = path.display().to_string();

    // Find video stream
    let streams = data
        .streams
        .ok_or_else(|| MediaImportError::NoVideoStream {
            path: path_str.clone(),
        })?;

    let video_stream = streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| MediaImportError::NoVideoStream {
            path: path_str.clone(),
        })?;

    // Find audio stream (optional)
    let audio_stream = streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));

    // Extract duration: prefer format duration, fallback to stream duration
    let duration_secs = extract_duration(&data.format, video_stream, &path_str)?;

    // Extract resolution
    let (width, height) = extract_resolution(video_stream, &path_str)?;

    // Extract frame rate
    let frame_rate = extract_frame_rate(video_stream, &path_str)?;

    // Extract codecs
    let video_codec = video_stream
        .codec_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let audio_codec = audio_stream.and_then(|s| s.codec_name.clone());

    // Get file size
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    // Generate ID
    let id = uuid::Uuid::new_v4().to_string();

    Ok(MediaSource::new(
        id,
        path.to_path_buf(),
        duration_secs,
        width as u32,
        height as u32,
        frame_rate,
        video_codec,
        audio_codec,
        file_size,
    ))
}

fn extract_duration(
    format: &Option<FFprobeFormat>,
    video_stream: &FFprobeStream,
    path: &str,
) -> Result<f64, MediaImportError> {
    // Try format duration first
    if let Some(fmt) = format {
        if let Some(dur_str) = &fmt.duration {
            if let Ok(dur) = dur_str.parse::<f64>() {
                if dur > 0.0 {
                    return Ok(dur);
                }
            }
        }
    }

    // Fallback to stream duration
    if let Some(dur_str) = &video_stream.duration {
        if let Ok(dur) = dur_str.parse::<f64>() {
            if dur > 0.0 {
                return Ok(dur);
            }
        }
    }

    Err(MediaImportError::InvalidDuration {
        path: path.to_string(),
        value: 0.0,
    })
}

fn extract_resolution(
    video_stream: &FFprobeStream,
    path: &str,
) -> Result<(i64, i64), MediaImportError> {
    let width = video_stream.width.unwrap_or(0);
    let height = video_stream.height.unwrap_or(0);

    if width <= 0 || height <= 0 {
        return Err(MediaImportError::InvalidResolution {
            path: path.to_string(),
            width,
            height,
        });
    }

    Ok((width, height))
}

fn extract_frame_rate(video_stream: &FFprobeStream, path: &str) -> Result<f64, MediaImportError> {
    let rate_str = video_stream.r_frame_rate.as_deref().unwrap_or("0/1");

    // Parse fraction like "30000/1001" or "30/1"
    let parts: Vec<&str> = rate_str.split('/').collect();
    let frame_rate = if parts.len() == 2 {
        let num: f64 = parts[0].parse().unwrap_or(0.0);
        let den: f64 = parts[1].parse().unwrap_or(1.0);
        if den > 0.0 {
            num / den
        } else {
            0.0
        }
    } else {
        rate_str.parse().unwrap_or(0.0)
    };

    if frame_rate <= 0.0 {
        return Err(MediaImportError::InvalidFrameRate {
            path: path.to_string(),
            value: frame_rate,
        });
    }

    Ok(frame_rate)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_nonexistent() {
        let result = validate_path(Path::new("/nonexistent/file.mp4"));
        assert!(matches!(result, Err(MediaImportError::FileNotFound { .. })));
    }

    #[test]
    fn test_validate_path_relative() {
        let result = validate_path(Path::new("relative/path.mp4"));
        assert!(matches!(
            result,
            Err(MediaImportError::NotAbsolutePath { .. })
        ));
    }

    #[test]
    fn test_parse_frame_rate_fraction() {
        let stream = FFprobeStream {
            codec_type: Some("video".to_string()),
            codec_name: Some("h264".to_string()),
            width: Some(1920),
            height: Some(1080),
            r_frame_rate: Some("30000/1001".to_string()),
            duration: Some("10.0".to_string()),
        };
        let rate = extract_frame_rate(&stream, "test.mp4").unwrap();
        assert!((rate - 29.97).abs() < 0.01);
    }

    #[test]
    fn test_parse_frame_rate_integer() {
        let stream = FFprobeStream {
            codec_type: Some("video".to_string()),
            codec_name: Some("h264".to_string()),
            width: Some(1920),
            height: Some(1080),
            r_frame_rate: Some("30/1".to_string()),
            duration: Some("10.0".to_string()),
        };
        let rate = extract_frame_rate(&stream, "test.mp4").unwrap();
        assert!((rate - 30.0).abs() < 0.01);
    }

    /// Integration test: Import a real video file.
    /// Run with: cargo test test_import_real_file -- --ignored
    #[test]
    #[ignore] // Ignored by default - requires video files to exist
    fn test_import_real_file() {
        use std::path::PathBuf;

        // Look for test videos in the project
        let project_root = std::env::current_dir()
            .unwrap()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap());

        let videos_dir = project_root.join("videos").join("uploads");

        // Find first .mp4 file
        if let Ok(entries) = std::fs::read_dir(&videos_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "mp4") {
                    println!("Testing import of: {:?}", path);

                    let result = super::import_media(&path);

                    match result {
                        Ok(source) => {
                            println!("✅ Import successful:");
                            println!("   ID: {}", source.id);
                            println!("   Duration: {:.2}s", source.duration_secs);
                            println!("   Resolution: {}x{}", source.width, source.height);
                            println!("   Frame Rate: {:.2} fps", source.frame_rate);
                            println!("   Video Codec: {}", source.video_codec);
                            println!("   Audio Codec: {:?}", source.audio_codec);
                            println!("   File Size: {} bytes", source.file_size);

                            // Validate invariants
                            assert!(source.duration_secs > 0.0, "Duration must be positive");
                            assert!(source.width > 0, "Width must be positive");
                            assert!(source.height > 0, "Height must be positive");
                            assert!(source.frame_rate > 0.0, "Frame rate must be positive");
                            assert!(!source.video_codec.is_empty(), "Codec must not be empty");
                        }
                        Err(e) => {
                            panic!("Import failed: {:?}", e);
                        }
                    }
                    return; // Test one file
                }
            }
        }

        println!("⚠️ No video files found in {:?}, skipping test", videos_dir);
    }
}
