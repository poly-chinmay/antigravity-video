//! RenderCommand - Render instructions for the frame pipeline.
//!
//! # Design
//!
//! RenderCommands are immutable instructions that describe what to render.
//! They are produced by the FrameScheduler and consumed by the Renderer.
//!
//! # Thread Safety
//!
//! RenderCommands are Clone and can be safely passed across thread boundaries.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::VisibleClip;

use super::frame_clock::FrameId;

// =============================================================================
// RENDER PRIORITY
// =============================================================================

/// Priority level for render commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderPriority {
    /// Background/prefetch renders
    Low = 0,
    /// Normal playback renders
    Normal = 1,
    /// User-initiated seeks
    High = 2,
    /// Critical timing (scrubbing)
    Critical = 3,
}

impl Default for RenderPriority {
    fn default() -> Self {
        Self::Normal
    }
}

// =============================================================================
// CLIP RENDER INFO
// =============================================================================

/// Information needed to render a single clip.
#[derive(Debug, Clone)]
pub struct ClipRenderInfo {
    /// Clip ID
    pub clip_id: String,

    /// Track ID
    pub track_id: String,

    /// Source file path
    pub source_file: String,

    /// Offset into the source video
    pub source_offset: MediaTime,

    /// Track layer (for compositing order)
    pub layer: u32,
}

impl ClipRenderInfo {
    /// Create from VisibleClip.
    pub fn from_visible_clip(clip: &VisibleClip, layer: u32) -> Self {
        Self {
            clip_id: clip.id.clone(),
            track_id: clip.track_id.clone(),
            source_file: clip.source_file.clone(),
            source_offset: clip.playback_offset,
            layer,
        }
    }
}

// =============================================================================
// RENDER COMMAND
// =============================================================================

/// A command to render a single frame.
#[derive(Debug, Clone)]
pub struct RenderCommand {
    /// Frame identifier
    pub frame_id: FrameId,

    /// Timeline position for this frame
    pub position: MediaTime,

    /// Clips to render (in layer order)
    pub clips: Vec<ClipRenderInfo>,

    /// Render priority
    pub priority: RenderPriority,

    /// Target width in pixels
    pub width: u32,

    /// Target height in pixels
    pub height: u32,

    /// Whether this is a keyframe (for seeking)
    pub is_keyframe: bool,

    /// Deadline for this frame (absolute wall time)
    pub deadline_ns: Option<u64>,
}

impl RenderCommand {
    /// Create a new render command.
    pub fn new(frame_id: FrameId, position: MediaTime, clips: Vec<ClipRenderInfo>) -> Self {
        Self {
            frame_id,
            position,
            clips,
            priority: RenderPriority::Normal,
            width: 1920,
            height: 1080,
            is_keyframe: false,
            deadline_ns: None,
        }
    }

    /// Create a high-priority seek command.
    pub fn seek(frame_id: FrameId, position: MediaTime, clips: Vec<ClipRenderInfo>) -> Self {
        Self {
            frame_id,
            position,
            clips,
            priority: RenderPriority::High,
            width: 1920,
            height: 1080,
            is_keyframe: true,
            deadline_ns: None,
        }
    }

    /// Set render dimensions.
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: RenderPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set deadline.
    pub fn with_deadline(mut self, deadline_ns: u64) -> Self {
        self.deadline_ns = Some(deadline_ns);
        self
    }

    /// Check if deadline has passed.
    pub fn is_expired(&self, current_time_ns: u64) -> bool {
        self.deadline_ns.is_some_and(|d| current_time_ns > d)
    }

    /// Get number of clips to render.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Check if this is an empty frame (no clips).
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

// =============================================================================
// RENDER RESULT
// =============================================================================

/// Result of rendering a frame.
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// Frame that was rendered
    pub frame_id: FrameId,

    /// Timeline position
    pub position: MediaTime,

    /// Render duration in nanoseconds
    pub render_time_ns: u64,

    /// Whether render was successful
    pub success: bool,

    /// Error message if failed
    pub error: Option<String>,
}

impl RenderResult {
    /// Create a successful result.
    pub fn success(frame_id: FrameId, position: MediaTime, render_time_ns: u64) -> Self {
        Self {
            frame_id,
            position,
            render_time_ns,
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(frame_id: FrameId, position: MediaTime, error: String) -> Self {
        Self {
            frame_id,
            position,
            render_time_ns: 0,
            success: false,
            error: Some(error),
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    #[test]
    fn test_render_command_new() {
        let cmd = RenderCommand::new(FrameId(42), ms(1000), vec![]);

        assert_eq!(cmd.frame_id, FrameId(42));
        assert_eq!(cmd.position, ms(1000));
        assert!(cmd.is_empty());
        assert_eq!(cmd.priority, RenderPriority::Normal);
    }

    #[test]
    fn test_render_command_seek() {
        let cmd = RenderCommand::seek(FrameId(100), ms(5000), vec![]);

        assert_eq!(cmd.priority, RenderPriority::High);
        assert!(cmd.is_keyframe);
    }

    #[test]
    fn test_render_command_deadline() {
        let cmd = RenderCommand::new(FrameId(1), ms(0), vec![]).with_deadline(1_000_000_000);

        assert!(!cmd.is_expired(500_000_000));
        assert!(cmd.is_expired(1_500_000_000));
    }

    #[test]
    fn test_render_result() {
        let success = RenderResult::success(FrameId(1), ms(100), 5_000_000);
        assert!(success.success);
        assert!(success.error.is_none());

        let failure = RenderResult::failure(FrameId(2), ms(200), "decode error".to_string());
        assert!(!failure.success);
        assert_eq!(failure.error, Some("decode error".to_string()));
    }

    #[test]
    fn test_clip_render_info() {
        let visible = VisibleClip {
            id: "c1".to_string(),
            track_id: "t1".to_string(),
            start: ms(0),
            duration: ms(5000),
            end: ms(5000),
            source_file: "video.mp4".to_string(),
            playback_offset: ms(1000),
        };

        let info = ClipRenderInfo::from_visible_clip(&visible, 0);

        assert_eq!(info.clip_id, "c1");
        assert_eq!(info.source_offset, ms(1000));
        assert_eq!(info.layer, 0);
    }
}
