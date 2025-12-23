// src-tauri/src/commands.rs
use crate::timeline::{Clip, TimelineEngine, TimelineState};
use tauri::{AppHandle, Emitter, Manager, State};
// We use uuid to generate unique IDs for new clips
use uuid::Uuid;

// --- COMMAND 1: Get Current State ---
// The frontend calls this to know what to draw.
#[tauri::command]
pub fn get_timeline_state(engine: State<'_, TimelineEngine>) -> Result<TimelineState, String> {
    // Lock the state to read it safely
    let state = engine.state.lock().map_err(|_| "Failed to lock state")?;
    // Return a copy of the state to the UI
    Ok(state.clone())
}

// --- COMMAND 2: Add a Clip (Simulated for now) ---
// This is what the UI will call when a file is dropped.
// In the future, this will involve FFmpeg to get real duration.
#[tauri::command]
pub fn add_clip(
    engine: State<'_, TimelineEngine>,
    file_path: String,
    duration: f64, // Frontend tells us duration for now
) -> Result<TimelineState, String> {
    println!("➡️ Received Add Clip Command for: {}", file_path);

    // Lock the state to modify it
    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;

    // Create the new clip struct
    let new_clip = Clip {
        id: Uuid::new_v4().to_string(),        // Generate a unique ID
        track_id: "video_track_1".to_string(), // Hardcoded for V1
        start: state.duration,                 // Append to the end
        duration: duration,
        source_file: file_path,
    };

    // Add clip to state
    state.clips.push(new_clip);
    // Update total duration
    state.duration += duration;

    println!("✅ Clip Added. New State Duration: {:.2}s", state.duration);

    // Return the updated state so UI can redraw instantly
    Ok(state.clone())
}

// --- COMMAND 3: Add Test Clips (Fixture) ---
// Generates synthetic clips for testing purposes.
#[tauri::command]
pub fn add_test_clips(
    _app: AppHandle,
    engine: State<'_, TimelineEngine>,
    count: usize,
) -> Result<TimelineState, String> {
    println!("🧪 Generating {} test clips...", count);

    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;

    // Call the helper logic
    add_test_clips_logic(&mut state, count);

    // Emit update
    _app.emit("STATE_UPDATE", &*state)
        .map_err(|e| e.to_string())?;

    println!(
        "✅ Added {} test clips. Total duration: {:.2}s",
        count, state.duration
    );
    Ok(state.clone())
}

// Helper function for testing logic without Tauri types
fn add_test_clips_logic(state: &mut TimelineState, count: usize) {
    // Determine uploads dir (hacky for this helper, but works for now)
    let current_dir = std::env::current_dir().expect("failed to get current dir");
    let videos_dir = if current_dir.ends_with("src-tauri") {
        current_dir.parent().unwrap_or(&current_dir).join("videos")
    } else {
        current_dir.join("videos")
    };
    let uploads_dir = videos_dir.join("uploads");
    if !uploads_dir.exists() {
        std::fs::create_dir_all(&uploads_dir).expect("failed to create uploads dir");
    }

    for i in 0..count {
        let filename = format!("test_clip_{}_{}.mp4", i, Uuid::new_v4());
        let file_path = uploads_dir.join(&filename);
        let file_path_str = file_path.to_string_lossy().to_string();

        // Generate video using FFmpeg
        // testsrc: 5 seconds, 720p, 30fps
        // yuv420p pixel format for maximum compatibility
        let status = std::process::Command::new("ffmpeg")
            .args(&[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=5:size=1280x720:rate=30",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                &file_path_str,
            ])
            .output()
            .expect("Failed to execute ffmpeg");

        if status.status.success() {
            println!("✅ Generated test clip: {}", file_path_str);
            let new_clip = Clip {
                id: Uuid::new_v4().to_string(),
                track_id: "video_track_1".to_string(),
                start: state.duration,
                duration: 5.0,
                source_file: file_path_str,
            };
            state.clips.push(new_clip);
            state.duration += 5.0;
        } else {
            println!(
                "❌ Failed to generate test clip: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }
    }
}

// Helper to get video directories
fn get_video_dirs(_app: &AppHandle) -> (std::path::PathBuf, std::path::PathBuf) {
    // Use current working directory to keep videos inside the project folder during dev
    let current_dir = std::env::current_dir().expect("failed to get current dir");

    // Move videos OUTSIDE src-tauri to prevent auto-reloading during dev
    // If we are in src-tauri, go up one level.
    let videos_dir = if current_dir.ends_with("src-tauri") {
        current_dir.parent().unwrap_or(&current_dir).join("videos")
    } else {
        current_dir.join("videos")
    };

    println!("📂 Video Storage Root: {:?}", videos_dir);

    let uploads_dir = videos_dir.join("uploads");
    let exports_dir = videos_dir.join("exports");

    if !uploads_dir.exists() {
        std::fs::create_dir_all(&uploads_dir).expect("failed to create uploads dir");
    }
    if !exports_dir.exists() {
        std::fs::create_dir_all(&exports_dir).expect("failed to create exports dir");
    }

    (uploads_dir, exports_dir)
}

// --- COMMAND 4: Import Real Video ---
#[tauri::command]
pub fn import_video(
    app: AppHandle,
    engine: State<'_, TimelineEngine>,
    file_path: String,
) -> Result<TimelineState, String> {
    println!("➡️ Importing video: {}", file_path);

    // 1. Probe the file for metadata
    let duration = ffmpeg_probe(&file_path)?;

    // 2. Transcode to H.264 MP4 (Ensure compatibility)
    let (uploads_dir, _) = get_video_dirs(&app);
    let original_path = std::path::Path::new(&file_path);
    let file_stem = original_path.file_stem().unwrap().to_string_lossy();

    // Always use .mp4 extension for the destination
    let unique_name = format!("{}_{}.mp4", file_stem, Uuid::new_v4());
    let dest_path = uploads_dir.join(&unique_name);
    let dest_path_str = dest_path.to_string_lossy().to_string();

    println!("🔄 Transcoding video to H.264: {}", dest_path_str);

    // Run FFmpeg to transcode
    // -c:v libx264: Use H.264 codec
    // -preset fast: Balance speed/quality
    // -pix_fmt yuv420p: Ensure broad compatibility
    // -c:a aac: Ensure audio compatibility
    let status = std::process::Command::new("ffmpeg")
        .args(&[
            "-y",
            "-i",
            &file_path,
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            &dest_path_str,
        ])
        .output()
        .map_err(|e| format!("Failed to execute ffmpeg: {}", e))?;

    if !status.status.success() {
        return Err(format!(
            "Transcoding failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }

    println!("✅ Transcoding Complete: {:?}", dest_path);

    // 3. Lock state
    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;

    // 4. Create Clip with NEW path
    let new_clip = Clip {
        id: Uuid::new_v4().to_string(),
        track_id: "video_track_1".to_string(),
        start: state.duration,
        duration,
        source_file: dest_path_str,
    };

    // 5. Update State
    state.clips.push(new_clip);
    state.duration += duration;

    println!("✅ Video Imported. Duration: {:.2}s", duration);

    // 6. Emit Update
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| e.to_string())?;

    Ok(state.clone())
}

// Helper to run ffprobe
fn ffmpeg_probe(path: &str) -> Result<f64, String> {
    use std::env;
    use std::process::Command;

    // Log the PATH for debugging
    if let Ok(path_env) = env::var("PATH") {
        println!("🔍 PATH: {}", path_env);
    } else {
        println!("⚠️ Could not read PATH env var");
    }

    let run_probe = |cmd: &str| -> Result<f64, String> {
        println!("Trying ffprobe at: {}", cmd);
        let output = Command::new(cmd)
            .args(&[
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "json",
                path,
            ])
            .output()
            .map_err(|e| format!("Failed to run {}: {}", cmd, e))?;

        if !output.status.success() {
            return Err(format!(
                "ffprobe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&output_str)
            .map_err(|e| format!("Failed to parse ffprobe output: {}", e))?;

        let duration_str = json["format"]["duration"]
            .as_str()
            .ok_or("Could not find duration in ffprobe output")?;

        duration_str
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse duration as float: {}", e))
    };

    // Try default first
    match run_probe("ffprobe") {
        Ok(d) => Ok(d),
        Err(e) => {
            println!("⚠️ Default ffprobe failed: {}. Trying fallback...", e);
            // Try Homebrew path
            run_probe("/opt/homebrew/bin/ffprobe")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::TimelineState;

    #[test]
    fn test_add_test_clips_logic() {
        let mut state = TimelineState {
            clips: vec![],
            duration: 0.0,
        };
        add_test_clips_logic(&mut state, 5);
        assert_eq!(state.clips.len(), 5);
        assert_eq!(state.duration, 25.0);
    }
}
