use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

#[derive(Debug)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum PreviewError {
    FFmpegNotFound,
    FFmpegFailed(String),
    IOError(std::io::Error),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreviewError::FFmpegNotFound => write!(f, "FFmpeg binary not found"),
            PreviewError::FFmpegFailed(msg) => write!(f, "FFmpeg failed: {}", msg),
            PreviewError::IOError(e) => write!(f, "IO Error: {}", e),
        }
    }
}

/// Render a single frame at the specific time instant using FFmpeg.
/// Outputs a temporary PNG file.
pub fn render_frame_at_time(
    source_path: &Path,
    time_secs: f64,
) -> Result<PreviewFrame, PreviewError> {
    // Create a temp file path
    let mut temp_path = env::temp_dir();
    temp_path.push(format!("ghost_preview_{}.png", Uuid::new_v4()));

    // FFmpeg command:
    // ffmpeg -ss <time> -i <source> -frames:v 1 -vf scale=1280:-1 -f image2 <out_path> -y

    // We put -ss before -i for fast seeking
    println!(
        "[PREVIEW] clip={}, source_time={}, output={}",
        source_path.display(),
        time_secs,
        temp_path.display()
    );

    let status = Command::new("ffmpeg")
        .args([
            "-y", // Overwrite output
            "-ss",
            &time_secs.to_string(), // Seek first! (Correct order checked)
            "-i",
            source_path.to_str().ok_or(PreviewError::FFmpegFailed(
                "Invalid source path".to_string(),
            ))?,
            "-frames:v",
            "1", // Exact one frame
            "-vf",
            "scale=1280:-1", // Scale to HD width, preserve aspect ratio
            "-f",
            "image2", // Force image format
            temp_path
                .to_str()
                .ok_or(PreviewError::FFmpegFailed("Invalid temp path".to_string()))?,
        ])
        .output()
        .map_err(PreviewError::IOError)?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(PreviewError::FFmpegFailed(stderr.to_string()));
    }

    if !temp_path.exists() {
        println!("[PREVIEW] ❌ PNG NOT CREATED at {}", temp_path.display());
    } else {
        println!(
            "[PREVIEW] ✅ PNG CREATED size={} bytes at {}",
            std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0),
            temp_path.display()
        );
    }

    Ok(PreviewFrame {
        width: 1280, // Approximate, determined by scale filter
        height: 720, // Approximate
        path: temp_path,
    })
}
