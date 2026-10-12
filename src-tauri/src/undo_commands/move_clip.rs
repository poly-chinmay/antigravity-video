// src-tauri/src/undo_commands/move_clip.rs
//! MoveClipCommand - Reversible clip movement

use crate::reversible_command::{memory_size_of_string, ReversibleCommand};
use crate::timeline::TimelineState;

/// Command to move a clip to a new start time
#[derive(Debug, Clone)]
pub struct MoveClipCommand {
    /// ID of the clip to move
    clip_id: String,
    /// New start time
    new_start: f64,

    // Captured state for undo
    /// Original start time (captured during execute)
    old_start: Option<f64>,
}

impl MoveClipCommand {
    /// Create a new move command
    pub fn new(clip_id: String, new_start: f64) -> Self {
        Self {
            clip_id,
            new_start,
            old_start: None,
        }
    }

    /// Update the target position (for coalescing)
    pub fn update_target(&mut self, new_start: f64) {
        self.new_start = new_start;
    }
}

impl ReversibleCommand for MoveClipCommand {
    fn execute(&mut self, state: &mut TimelineState) -> Result<(), String> {
        // Find the clip
        let clip = state
            .get_clip_by_id_mut(&self.clip_id)
            .ok_or_else(|| format!("Clip '{}' not found", self.clip_id))?;

        // Capture old start for undo
        self.old_start = Some(clip.start);

        // Apply the move
        clip.start = self.new_start;

        // Recalculate duration
        state.recalculate_duration();

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn undo(&mut self, state: &mut TimelineState) -> Result<(), String> {
        let old_start = self.old_start.ok_or("Cannot undo: no old start captured")?;

        // Find the clip
        let clip = state
            .get_clip_by_id_mut(&self.clip_id)
            .ok_or_else(|| format!("Clip '{}' not found", self.clip_id))?;

        // Restore old start
        clip.start = old_start;

        // Recalculate duration
        state.recalculate_duration();

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + memory_size_of_string(&self.clip_id)
    }

    fn description(&self) -> String {
        format!("Move clip '{}' to {:.2}s", self.clip_id, self.new_start)
    }

    fn can_coalesce_with(&self, other: &dyn ReversibleCommand) -> bool {
        // Can coalesce if same type and same clip
        other.type_name() == self.type_name() && other.target_clip_id() == Some(&self.clip_id)
    }

    fn type_name(&self) -> &'static str {
        "MoveClipCommand"
    }

    fn target_clip_id(&self) -> Option<&str> {
        Some(&self.clip_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;

    fn make_test_clip(id: &str, start: f64) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_move_execute_and_undo() {
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1", 0.0));

        // Execute move
        let mut cmd = MoveClipCommand::new("clip-1".to_string(), 5.0);
        assert!(cmd.execute(&mut state).is_ok());

        let clip = state.get_clip_by_id("clip-1").unwrap();
        assert_eq!(clip.start, 5.0);

        // Undo move
        assert!(cmd.undo(&mut state).is_ok());

        let clip = state.get_clip_by_id("clip-1").unwrap();
        assert_eq!(clip.start, 0.0);
    }

    #[test]
    fn test_move_coalescing_check() {
        let cmd1 = MoveClipCommand::new("clip-1".to_string(), 5.0);
        let cmd2 = MoveClipCommand::new("clip-1".to_string(), 10.0);

        assert!(cmd1.can_coalesce_with(&cmd2));
    }
}
