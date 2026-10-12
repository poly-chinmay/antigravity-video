// src-tauri/src/undo_commands/delete_clip.rs
//! DeleteClipCommand - Reversible clip deletion

use crate::reversible_command::{memory_size_of_clip, memory_size_of_string, ReversibleCommand};
use crate::timeline::{Clip, TimelineState};

/// Command to delete a clip from the timeline
#[derive(Debug, Clone)]
pub struct DeleteClipCommand {
    /// ID of the clip to delete
    clip_id: String,

    // Captured state for undo
    /// The deleted clip (captured during execute)
    deleted_clip: Option<Clip>,
    /// Original index in clips vec (for exact restoration)
    original_index: Option<usize>,
}

impl DeleteClipCommand {
    /// Create a new delete command
    pub fn new(clip_id: String) -> Self {
        Self {
            clip_id,
            deleted_clip: None,
            original_index: None,
        }
    }
}

impl ReversibleCommand for DeleteClipCommand {
    fn execute(&mut self, state: &mut TimelineState) -> Result<(), String> {
        // Find the clip
        let idx = state
            .clip_id_index
            .get(&self.clip_id)
            .copied()
            .ok_or_else(|| format!("Clip '{}' not found", self.clip_id))?;

        // Capture for undo
        self.original_index = Some(idx);
        self.deleted_clip = Some(state.clips[idx].clone());

        // Remove the clip
        state
            .remove_clip(&self.clip_id)
            .ok_or_else(|| format!("Failed to remove clip '{}'", self.clip_id))?;

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn undo(&mut self, state: &mut TimelineState) -> Result<(), String> {
        let clip = self
            .deleted_clip
            .as_ref()
            .ok_or("Cannot undo: no clip data captured")?;

        // Re-add the clip
        state.add_clip(clip.clone());

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + memory_size_of_string(&self.clip_id)
            + self
                .deleted_clip
                .as_ref()
                .map(memory_size_of_clip)
                .unwrap_or(0)
    }

    fn description(&self) -> String {
        format!("Delete clip '{}'", self.clip_id)
    }

    fn type_name(&self) -> &'static str {
        "DeleteClipCommand"
    }

    fn target_clip_id(&self) -> Option<&str> {
        Some(&self.clip_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_delete_execute_and_undo() {
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));

        assert_eq!(state.clips.len(), 1);

        // Execute delete
        let mut cmd = DeleteClipCommand::new("clip-1".to_string());
        assert!(cmd.execute(&mut state).is_ok());
        assert_eq!(state.clips.len(), 0);

        // Undo delete
        assert!(cmd.undo(&mut state).is_ok());
        assert_eq!(state.clips.len(), 1);
        assert_eq!(state.clips[0].id, "clip-1");
    }

    #[test]
    fn test_delete_nonexistent_fails() {
        let mut state = TimelineState::new();
        let mut cmd = DeleteClipCommand::new("nonexistent".to_string());

        assert!(cmd.execute(&mut state).is_err());
    }

    #[test]
    fn test_memory_size() {
        let cmd = DeleteClipCommand::new("test-clip".to_string());
        let size = cmd.memory_size();
        assert!(size > 0);
    }
}
