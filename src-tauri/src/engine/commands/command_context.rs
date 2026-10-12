//! CommandContext - Execution context for commands.
//!
//! # Design
//!
//! CommandContext provides read-only access to application state
//! and write access through controlled channels (InteractionController,
//! TimelineEngine, etc.).
//!
//! Commands receive a context but CANNOT mutate state directly.
//! They must return CommandResult with the intended effect.

use crate::engine::interaction::{InteractionController, ToolType};
use crate::engine::media_time::MediaTime;
use crate::engine::playback::PlaybackScheduler;
use crate::engine::timeline_state::{ClipId, TimelineState};

// =============================================================================
// COMMAND CONTEXT
// =============================================================================

/// Read-only snapshot of application state for command execution.
#[derive(Debug)]
pub struct CommandContext<'a> {
    /// Timeline state (read-only)
    pub timeline: &'a TimelineState,

    /// Playback scheduler (read-only for position/state queries)
    pub playback: &'a PlaybackScheduler,

    /// Interaction controller (read-only for selection queries)
    pub interaction: &'a InteractionController,

    /// Current playhead position
    pub playhead_position: MediaTime,

    /// Currently selected clips
    pub selected_clips: Vec<ClipId>,

    /// Current tool
    pub current_tool: ToolType,

    /// Whether playback is active
    pub is_playing: bool,
}

impl<'a> CommandContext<'a> {
    /// Create a new command context.
    pub fn new(
        timeline: &'a TimelineState,
        playback: &'a PlaybackScheduler,
        interaction: &'a InteractionController,
    ) -> Self {
        Self {
            playhead_position: playback.position(),
            selected_clips: interaction.selected_clips().to_vec(),
            current_tool: interaction.current_tool(),
            is_playing: playback.is_playing(),
            timeline,
            playback,
            interaction,
        }
    }

    /// Check if any clips are selected.
    pub fn has_selection(&self) -> bool {
        !self.selected_clips.is_empty()
    }

    /// Get selected clip count.
    pub fn selection_count(&self) -> usize {
        self.selected_clips.len()
    }

    /// Get first selected clip.
    pub fn first_selected(&self) -> Option<&ClipId> {
        self.selected_clips.first()
    }

    /// Get timeline duration.
    pub fn timeline_duration(&self) -> MediaTime {
        self.timeline.duration
    }

    /// Get clip count.
    pub fn clip_count(&self) -> usize {
        self.timeline.clip_count()
    }
}

// =============================================================================
// MUTABLE CONTEXT (for internal use)
// =============================================================================

/// Mutable context for applying command effects.
/// This is used internally by the command router, not by commands themselves.
pub struct MutableContext<'a> {
    /// Interaction controller (mutable)
    pub interaction: &'a mut InteractionController,

    /// Playback scheduler (mutable)
    pub playback: &'a mut PlaybackScheduler,
}

impl<'a> MutableContext<'a> {
    /// Create mutable context.
    pub fn new(
        interaction: &'a mut InteractionController,
        playback: &'a mut PlaybackScheduler,
    ) -> Self {
        Self {
            interaction,
            playback,
        }
    }

    /// Set current tool.
    pub fn set_tool(&mut self, tool: ToolType) {
        self.interaction.set_tool(tool);
    }

    /// Toggle play/pause.
    pub fn toggle_playback(&mut self) {
        if self.playback.is_playing() {
            self.playback.pause();
        } else {
            self.playback.play();
        }
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.playback.play();
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.playback.pause();
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        self.playback.stop();
    }

    /// Seek to position.
    pub fn seek(&mut self, position: MediaTime) {
        self.playback.seek(position);
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_clip(id: &str, start_ms: i64, duration_ms: i64) -> Clip {
        Clip::new(id, "t1", ms(start_ms), ms(duration_ms), "test.mp4")
    }

    fn make_state(clips: Vec<Clip>) -> TimelineState {
        let mut state = TimelineState::new();
        state.clips = clips;
        state.rebuild_indices();
        state.recalculate_duration();
        state
    }

    #[test]
    fn test_context_creation() {
        let timeline = make_state(vec![make_clip("c1", 0, 5000)]);
        let playback = PlaybackScheduler::with_duration(ms(5000));
        let interaction = InteractionController::default_controller();

        let ctx = CommandContext::new(&timeline, &playback, &interaction);

        assert_eq!(ctx.clip_count(), 1);
        assert!(!ctx.has_selection());
        assert_eq!(ctx.current_tool, ToolType::Select);
    }

    #[test]
    fn test_context_queries() {
        let timeline = make_state(vec![make_clip("c1", 0, 5000), make_clip("c2", 5000, 5000)]);
        let playback = PlaybackScheduler::with_duration(ms(10000));
        let interaction = InteractionController::default_controller();

        let ctx = CommandContext::new(&timeline, &playback, &interaction);

        assert_eq!(ctx.timeline_duration(), ms(10000));
        assert_eq!(ctx.clip_count(), 2);
    }
}
