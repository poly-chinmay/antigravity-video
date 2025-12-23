use crate::timeline::TimelineState;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct FFmpegEngine;

impl FFmpegEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn render_timeline(&self, state: &TimelineState, output_path: &Path) -> Result<(), String> {
        if state.clips.is_empty() {
            return Err("Timeline is empty".to_string());
        }

        // 1. Sort clips by start time to ensure correct sequence
        let mut clips = state.clips.clone();
        clips.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

        // 2. Build FFmpeg Command
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y"); // Overwrite output

        // Add Inputs
        for clip in &clips {
            cmd.arg("-i").arg(&clip.source_file);
        }

        // 3. Build Filter Complex
        // Goal: Scale all inputs to 1920x1080 (with padding) -> Trim -> Concat
        let mut filter_complex = String::new();
        let mut concat_inputs = String::new();

        for (i, clip) in clips.iter().enumerate() {
            // Video Filter Chain:
            // 1. Scale to fit within 1920x1080 while maintaining aspect ratio
            // 2. Pad to exactly 1920x1080 (centering the video)
            // 3. Trim to duration
            // 4. Reset timestamps

            // scale=1920:1080:force_original_aspect_ratio=decrease
            // pad=1920:1080:(ow-iw)/2:(oh-ih)/2

            filter_complex.push_str(&format!(
                "[{}:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,trim=duration={:.4},setpts=PTS-STARTPTS[v{}];",
                i, clip.duration, i
            ));

            concat_inputs.push_str(&format!("[v{}]", i));
        }

        // Concat Filter
        filter_complex.push_str(&format!(
            "{}concat=n={}:v=1:a=0[outv]",
            concat_inputs,
            clips.len()
        ));

        cmd.arg("-filter_complex").arg(filter_complex);
        cmd.arg("-map").arg("[outv]");

        // Output Format (MP4 / H.264)
        cmd.arg("-c:v").arg("libx264");
        cmd.arg("-preset").arg("fast");
        cmd.arg("-pix_fmt").arg("yuv420p"); // Ensure compatibility
        cmd.arg(output_path);

        println!("🎥 Running FFmpeg: {:?}", cmd);

        // 3. Execute
        let output = cmd
            .output()
            .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("FFmpeg failed: {}", stderr));
        }

        println!("✅ Render Complete: {:?}", output_path);
        Ok(())
    }
}
