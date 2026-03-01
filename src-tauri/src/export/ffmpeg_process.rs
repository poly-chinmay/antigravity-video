//! FFmpeg Process - Out-of-process FFmpeg execution with progress tracking.
//!
//! # Design
//!
//! FFmpegProcess wraps a child process and provides:
//! - Progress parsing from FFmpeg's `-progress` output
//! - Clean termination on cancel
//! - Error detection from stderr
//!
//! # Safety
//!
//! The app survives FFmpeg crashes because:
//! - FFmpeg runs as a separate process
//! - We use non-blocking I/O to poll progress
//! - Process is killed on drop (if still running)

use super::export_types::ExportError;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

// =============================================================================
// FFMPEG PROGRESS
// =============================================================================

/// Progress data parsed from FFmpeg output.
#[derive(Debug, Clone, Default)]
pub struct FFmpegProgressData {
    /// Current frame number
    pub frame: u64,

    /// Current encoding FPS
    pub fps: f64,

    /// Bitrate in kbits/s
    pub bitrate_kbps: f64,

    /// Total size written so far in bytes
    pub size_bytes: u64,

    /// Current time (out_time_ms from FFmpeg)
    pub time_ms: u64,

    /// Speed relative to realtime (e.g., 2.5x)
    pub speed: f64,

    /// Whether encoding is complete
    pub is_complete: bool,
}

// =============================================================================
// FFMPEG PROCESS
// =============================================================================

/// Manages a single FFmpeg child process.
pub struct FFmpegProcess {
    /// Child process handle
    child: Child,

    /// When export started
    start_time: Instant,

    /// Total expected frames
    total_frames: u64,

    /// Channel to receive progress updates
    progress_rx: Receiver<FFmpegProgressData>,

    /// Thread reading stderr (for error capture)
    stderr_thread: Option<JoinHandle<String>>,

    /// Thread reading progress from stdout
    _progress_thread: Option<JoinHandle<()>>,

    /// Last known progress
    last_progress: FFmpegProgressData,
}

impl FFmpegProcess {
    /// Spawn FFmpeg with the given arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - Command line arguments (without "ffmpeg")
    /// * `total_frames` - Expected total frames for progress calculation
    ///
    /// # Returns
    ///
    /// FFmpegProcess on success, ExportError on failure.
    pub fn spawn(args: Vec<String>, total_frames: u64) -> Result<Self, ExportError> {
        // Check if ffmpeg exists
        let ffmpeg_path = which_ffmpeg()?;

        let mut cmd = Command::new(&ffmpeg_path);
        cmd.args(&args);

        // Capture stderr for errors
        cmd.stderr(Stdio::piped());

        // -progress outputs to stdout
        cmd.stdout(Stdio::piped());

        println!("🎥 Spawning FFmpeg: {} {:?}", ffmpeg_path, args);

        let mut child = cmd
            .spawn()
            .map_err(|e| ExportError::IoError { message: e.to_string() })?;

        // Set up progress channel
        let (progress_tx, progress_rx) = mpsc::channel();

        // Spawn thread to read progress from stdout
        let stdout = child.stdout.take();
        let progress_thread = if let Some(stdout) = stdout {
            Some(spawn_progress_reader(stdout, progress_tx))
        } else {
            None
        };

        // Spawn thread to capture stderr
        let stderr = child.stderr.take();
        let stderr_thread = stderr.map(spawn_stderr_reader);

        Ok(Self {
            child,
            start_time: Instant::now(),
            total_frames,
            progress_rx,
            stderr_thread,
            _progress_thread: progress_thread,
            last_progress: FFmpegProgressData::default(),
        })
    }

    /// Poll for new progress data.
    ///
    /// Returns the latest progress if available.
    pub fn poll_progress(&mut self) -> FFmpegProgressData {
        // Drain all available progress updates
        while let Ok(progress) = self.progress_rx.try_recv() {
            self.last_progress = progress;
        }

        self.last_progress.clone()
    }

    /// Check if the process has exited.
    ///
    /// Returns:
    /// - `None` if still running
    /// - `Some(Ok(()))` if exited successfully
    /// - `Some(Err(ExportError))` if failed
    pub fn try_wait(&mut self) -> Option<Result<(), ExportError>> {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    Some(Ok(()))
                } else {
                    let stderr = self.get_stderr();
                    let exit_code = status.code().unwrap_or(-1);

                    // Check for common error patterns
                    if stderr.contains("No space left on device") {
                        Some(Err(ExportError::DiskFull { available_bytes: None }))
                    } else {
                        Some(Err(ExportError::FFmpegFailed { exit_code, stderr }))
                    }
                }
            }
            Ok(None) => None, // Still running
            Err(e) => Some(Err(ExportError::IoError { message: e.to_string() })),
        }
    }

    /// Kill the FFmpeg process.
    pub fn kill(&mut self) -> Result<(), ExportError> {
        self.child.kill().map_err(|e| ExportError::IoError {
            message: format!("Failed to kill FFmpeg: {}", e),
        })
    }

    /// Get elapsed time since export started.
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Get total expected frames.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get stderr output (waits for thread if needed).
    fn get_stderr(&mut self) -> String {
        if let Some(handle) = self.stderr_thread.take() {
            handle.join().unwrap_or_default()
        } else {
            String::new()
        }
    }
}

impl Drop for FFmpegProcess {
    fn drop(&mut self) {
        // Kill the process if still running
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Find ffmpeg in PATH.
fn which_ffmpeg() -> Result<String, ExportError> {
    // Try common locations
    for path in &["ffmpeg", "/usr/local/bin/ffmpeg", "/opt/homebrew/bin/ffmpeg"] {
        if Command::new(path).arg("-version").output().is_ok() {
            return Ok(path.to_string());
        }
    }

    Err(ExportError::FFmpegNotFound)
}

/// Spawn a thread to read progress from FFmpeg stdout.
fn spawn_progress_reader(
    stdout: std::process::ChildStdout,
    tx: Sender<FFmpegProgressData>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut current = FFmpegProgressData::default();

        for line in reader.lines().map_while(Result::ok) {
            // Parse FFmpeg progress format: key=value
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "frame" => {
                        current.frame = value.parse().unwrap_or(0);
                    }
                    "fps" => {
                        current.fps = value.parse().unwrap_or(0.0);
                    }
                    "bitrate" => {
                        // Format: "1234.5kbits/s" or "N/A"
                        if let Some(kbps) = value.strip_suffix("kbits/s") {
                            current.bitrate_kbps = kbps.parse().unwrap_or(0.0);
                        }
                    }
                    "total_size" => {
                        current.size_bytes = value.parse().unwrap_or(0);
                    }
                    "out_time_ms" => {
                        current.time_ms = value.parse().unwrap_or(0);
                    }
                    "speed" => {
                        // Format: "2.5x" or "N/A"
                        if let Some(mult) = value.strip_suffix('x') {
                            current.speed = mult.parse().unwrap_or(0.0);
                        }
                    }
                    "progress" => {
                        // "continue" or "end"
                        if value == "end" {
                            current.is_complete = true;
                        }
                        // Send progress update after each block
                        let _ = tx.send(current.clone());
                    }
                    _ => {}
                }
            }
        }
    })
}

/// Spawn a thread to read stderr from FFmpeg.
fn spawn_stderr_reader(stderr: std::process::ChildStderr) -> JoinHandle<String> {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut output = String::new();

        for line in reader.lines().map_while(Result::ok) {
            output.push_str(&line);
            output.push('\n');

            // Log FFmpeg stderr for debugging
            eprintln!("[FFmpeg] {}", line);
        }

        output
    })
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_which_ffmpeg() {
        // This test will pass if ffmpeg is installed
        let result = which_ffmpeg();
        // Don't assert - just check it doesn't panic
        println!("FFmpeg found: {:?}", result);
    }

    #[test]
    fn test_progress_data_default() {
        let data = FFmpegProgressData::default();
        assert_eq!(data.frame, 0);
        assert!(!data.is_complete);
    }
}
