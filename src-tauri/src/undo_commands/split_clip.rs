// src-tauri/src/undo_commands/split_clip.rs
//! SplitClipCommand - Reversible clip splitting

use crate::reversible_command::{memory_size_of_clip, memory_size_of_string, ReversibleCommand};
use crate::timeline::{Clip, TimelineState};

/// Command to split a clip into two clips at a specific time
#[derive(Debug, Clone)]
pub struct SplitClipCommand {
    /// ID of the clip to split
    clip_id: String,
    /// Time at which to split the clip
    split_time: f64,

    // Captured state for undo
    /// The original clip before split (captured during execute)
    original_clip: Option<Clip>,
    /// IDs of the two new clips created by split
    new_clip_ids: Option<(String, String)>,
}

impl SplitClipCommand {
    /// Create a new split command
    pub fn new(clip_id: String, split_time: f64) -> Self {
        Self {
            clip_id,
            split_time,
            original_clip: None,
            new_clip_ids: None,
        }
    }
}

impl ReversibleCommand for SplitClipCommand {
    fn execute(&mut self, state: &mut TimelineState) -> Result<(), String> {
        // Find and capture the original clip
        let original = state
            .get_clip_by_id(&self.clip_id)
            .ok_or_else(|| format!("Clip '{}' not found", self.clip_id))?
            .clone();

        self.original_clip = Some(original.clone());

        // Validate split time is within clip bounds
        if self.split_time <= original.start || self.split_time >= original.end() {
            return Err(format!(
                "Split time {:.2} must be within clip bounds [{:.2}, {:.2})",
                self.split_time,
                original.start,
                original.end()
            ));
        }

        // Calculate durations for new clips
        let first_duration = self.split_time - original.start;
        let second_duration = original.duration - first_duration;

        // Create two new clips with NEW ClipIds (Phase A1 requirement)
        let first_clip = Clip {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: original.track_id.clone(),
            start: original.start,
            duration: first_duration,
            source_file: original.source_file.clone(),
        };

        let second_clip = Clip {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: original.track_id.clone(),
            start: self.split_time,
            duration: second_duration,
            source_file: original.source_file.clone(),
        };

        // Capture new IDs for undo
        self.new_clip_ids = Some((first_clip.id.clone(), second_clip.id.clone()));

        // Remove original and add new clips
        state
            .remove_clip(&self.clip_id)
            .ok_or_else(|| format!("Failed to remove original clip '{}'", self.clip_id))?;

        state.add_clip(first_clip);
        state.add_clip(second_clip);

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn undo(&mut self, state: &mut TimelineState) -> Result<(), String> {
        let original = self
            .original_clip
            .as_ref()
            .ok_or("Cannot undo: no original clip captured")?;

        let (first_id, second_id) = self
            .new_clip_ids
            .as_ref()
            .ok_or("Cannot undo: no new clip IDs captured")?;

        // Remove the two split clips
        state
            .remove_clip(first_id)
            .ok_or_else(|| format!("Failed to remove first split clip '{}'", first_id))?;

        state
            .remove_clip(second_id)
            .ok_or_else(|| format!("Failed to remove second split clip '{}'", second_id))?;

        // Re-add the original clip
        state.add_clip(original.clone());

        // Validate invariants
        state.validate_invariants()?;

        Ok(())
    }

    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + memory_size_of_string(&self.clip_id)
            + self
                .original_clip
                .as_ref()
                .map(memory_size_of_clip)
                .unwrap_or(0)
            + self
                .new_clip_ids
                .as_ref()
                .map(|(id1, id2)| id1.len() + id2.len())
                .unwrap_or(0)
    }

    fn description(&self) -> String {
        format!("Split clip '{}' at {:.2}s", self.clip_id, self.split_time)
    }

    fn type_name(&self) -> &'static str {
        "SplitClipCommand"
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
    fn test_split_execute_and_undo() {
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));

        assert_eq!(state.clips.len(), 1);

        // Execute split at midpoint
        let mut cmd = SplitClipCommand::new("clip-1".to_string(), 5.0);
        assert!(cmd.execute(&mut state).is_ok());

        // Should now have 2 clips
        assert_eq!(state.clips.len(), 2);

        // Verify durations
        let clips: Vec<_> = state.clips.iter().collect();
        assert!(clips.iter().any(|c| c.duration == 5.0));
        assert!(clips.iter().any(|c| c.duration == 5.0));

        // Undo split
        assert!(cmd.undo(&mut state).is_ok());

        // Should be back to 1 clip
        assert_eq!(state.clips.len(), 1);
        assert_eq!(state.clips[0].duration, 10.0);
    }

    #[test]
    fn test_split_invalid_time_fails() {
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));

        // Split at clip start (invalid)
        let mut cmd = SplitClipCommand::new("clip-1".to_string(), 0.0);
        assert!(cmd.execute(&mut state).is_err());

        // Split at clip end (invalid)
        let mut cmd = SplitClipCommand::new("clip-1".to_string(), 10.0);
        assert!(cmd.execute(&mut state).is_err());
    }
}
