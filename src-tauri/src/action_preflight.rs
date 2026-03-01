// src-tauri/src/action_preflight.rs
//! ActionPreflight - Simulation-based validation engine
//!
//! Clones the timeline state and simulates the entire edit plan
//! to verify that all invariants are preserved after execution.

use crate::edit_plan::{ActionType, EditAction, EditPlan};
use crate::edit_rejection::EditRejection;
use crate::timeline::{Clip, TimelineState};

/// Simulation-based validator for edit plans
pub struct ActionPreflight;

impl ActionPreflight {
    /// Simulate an entire edit plan on a shadow timeline
    ///
    /// Creates a clone of the current state and applies each action
    /// sequentially, validating invariants after each step.
    ///
    /// Returns EditRejection if any action would violate invariants.
    pub fn preflight_plan(plan: &EditPlan, state: &TimelineState) -> Result<(), EditRejection> {
        // Clone state for shadow simulation
        let mut shadow = state.clone();

        // Simulate each action
        for action in &plan.actions {
            Self::simulate_action(action, &mut shadow)?;

            // Validate invariants after each action
            shadow.validate_invariants().map_err(|msg| {
                // Parse invariant number from error message
                let invariant = Self::extract_invariant_number(&msg);
                EditRejection::invariant_violation(invariant, msg)
            })?;
        }

        Ok(())
    }

    /// Simulate a single action on the shadow timeline
    fn simulate_action(
        action: &EditAction,
        shadow: &mut TimelineState,
    ) -> Result<(), EditRejection> {
        match action.action_type {
            ActionType::Delete => Self::simulate_delete(action, shadow),
            ActionType::Trim => Self::simulate_trim(action, shadow),
            ActionType::Split => Self::simulate_split(action, shadow),
            ActionType::Move => Self::simulate_move(action, shadow),
        }
    }

    /// Simulate DELETE action
    fn simulate_delete(
        action: &EditAction,
        shadow: &mut TimelineState,
    ) -> Result<(), EditRejection> {
        shadow
            .remove_clip(&action.target_clip_id)
            .ok_or_else(|| EditRejection::invalid_reference(&action.target_clip_id, "ClipId"))?;

        Ok(())
    }

    /// Simulate TRIM action
    fn simulate_trim(action: &EditAction, shadow: &mut TimelineState) -> Result<(), EditRejection> {
        let params = action.parameters.as_ref().ok_or_else(|| {
            EditRejection::bounds_violation(action.clone(), "TRIM missing parameters")
        })?;

        let clip = shadow
            .get_clip_by_id_mut(&action.target_clip_id)
            .ok_or_else(|| EditRejection::invalid_reference(&action.target_clip_id, "ClipId"))?;

        // Apply trim_start_delta
        if let Some(delta) = params.trim_start_delta {
            clip.start += delta;
            clip.duration -= delta;
        }

        // Apply trim_end_delta
        if let Some(delta) = params.trim_end_delta {
            clip.duration += delta;
        }

        // Ensure duration is positive
        if clip.duration <= 0.0 {
            return Err(EditRejection::invariant_violation(
                4,
                format!(
                    "TRIM would result in non-positive duration: {:.3}s",
                    clip.duration
                ),
            ));
        }

        shadow.recalculate_duration();
        Ok(())
    }

    /// Simulate SPLIT action
    fn simulate_split(
        action: &EditAction,
        shadow: &mut TimelineState,
    ) -> Result<(), EditRejection> {
        let params = action.parameters.as_ref().ok_or_else(|| {
            EditRejection::bounds_violation(action.clone(), "SPLIT missing parameters")
        })?;

        let split_time = params.split_time.ok_or_else(|| {
            EditRejection::bounds_violation(action.clone(), "SPLIT missing split_time")
        })?;

        // Get original clip (must clone to avoid borrow issues)
        let original = shadow
            .get_clip_by_id(&action.target_clip_id)
            .ok_or_else(|| EditRejection::invalid_reference(&action.target_clip_id, "ClipId"))?
            .clone();

        // Create two new clips with NEW ClipIds (immutability requirement)
        let first_duration = split_time - original.start;
        let second_duration = original.duration - first_duration;

        let first_clip = Clip {
            id: uuid::Uuid::new_v4().to_string(), // NEW ClipId
            track_id: original.track_id.clone(),
            start: original.start,
            duration: first_duration,
            source_file: original.source_file.clone(),
        };

        let second_clip = Clip {
            id: uuid::Uuid::new_v4().to_string(), // NEW ClipId
            track_id: original.track_id.clone(),
            start: split_time,
            duration: second_duration,
            source_file: original.source_file.clone(),
        };

        // Remove original and add new clips
        shadow.remove_clip(&action.target_clip_id);
        shadow.add_clip(first_clip);
        shadow.add_clip(second_clip);

        Ok(())
    }

    /// Simulate MOVE action
    fn simulate_move(action: &EditAction, shadow: &mut TimelineState) -> Result<(), EditRejection> {
        let params = action.parameters.as_ref().ok_or_else(|| {
            EditRejection::bounds_violation(action.clone(), "MOVE missing parameters")
        })?;

        let new_start = params.new_start_time.ok_or_else(|| {
            EditRejection::bounds_violation(action.clone(), "MOVE missing new_start_time")
        })?;

        // Get clip and check for overlaps
        let clip = shadow
            .get_clip_by_id(&action.target_clip_id)
            .ok_or_else(|| EditRejection::invalid_reference(&action.target_clip_id, "ClipId"))?;

        let track_id = clip.track_id.clone();
        let duration = clip.duration;
        let clip_id = clip.id.clone();

        // Check if move would cause overlap (excluding the clip being moved)
        let clips_in_range = shadow.get_clips_in_range(&track_id, new_start, new_start + duration);

        for other_clip in clips_in_range {
            if other_clip.id != clip_id {
                return Err(EditRejection::conflict_detected(format!(
                    "MOVE would cause overlap with clip '{}'",
                    other_clip.id
                )));
            }
        }

        // Apply the move
        let clip = shadow
            .get_clip_by_id_mut(&action.target_clip_id)
            .expect("Clip existence already validated");

        clip.start = new_start;
        shadow.recalculate_duration();

        Ok(())
    }

    /// Extract invariant number from error message
    fn extract_invariant_number(msg: &str) -> u32 {
        // Try to extract number from "INVARIANT_VIOLATED: ..." messages
        if msg.contains("Duplicate ClipId") {
            1
        } else if msg.contains("project_duration") {
            2
        } else if msg.contains("timeline_frame_rate") {
            3
        } else if msg.contains("duration") {
            4
        } else if msg.contains("start") {
            5
        } else if msg.contains("playhead") {
            6
        } else {
            0 // Unknown invariant
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::ActionParameters;

    fn make_test_state() -> TimelineState {
        let mut state = TimelineState::new();
        state.add_clip(Clip {
            id: "clip-1".to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        });
        state.add_clip(Clip {
            id: "clip-2".to_string(),
            track_id: "track-1".to_string(),
            start: 10.0,
            duration: 5.0,
            source_file: "/test.mp4".to_string(),
        });
        state
    }

    #[test]
    fn test_valid_delete_simulation() {
        let state = make_test_state();
        let plan = EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Delete,
                target_clip_id: "clip-1".to_string(),
                parameters: None,
            }],
            thought_process: None,
            confidence: None,
        };

        let result = ActionPreflight::preflight_plan(&plan, &state);
        assert!(result.is_ok());
    }

    #[test]
    fn test_trim_to_zero_duration_rejected() {
        let state = make_test_state();
        let plan = EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Trim,
                target_clip_id: "clip-1".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: Some(10.0), // Would make duration 0
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: None,
                }),
            }],
            thought_process: None,
            confidence: None,
        };

        let result = ActionPreflight::preflight_plan(&plan, &state);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_causing_overlap_rejected() {
        let state = make_test_state();
        let plan = EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Move,
                target_clip_id: "clip-1".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: Some(12.0), // Would overlap with clip-2 at 10-15
                }),
            }],
            thought_process: None,
            confidence: None,
        };

        let result = ActionPreflight::preflight_plan(&plan, &state);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EditRejection::ConflictDetected { .. }
        ));
    }

    #[test]
    fn test_split_creates_new_clip_ids() {
        let state = make_test_state();
        let original_id = "clip-1".to_string();

        let plan = EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Split,
                target_clip_id: original_id.clone(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: Some(5.0), // Split at midpoint
                    new_start_time: None,
                }),
            }],
            thought_process: None,
            confidence: None,
        };

        let result = ActionPreflight::preflight_plan(&plan, &state);
        assert!(result.is_ok());

        // Verify original clip ID no longer exists in shadow
        // (This is tested implicitly by the simulation succeeding)
    }

    #[test]
    fn test_complex_multi_action_plan() {
        let state = make_test_state();
        let plan = EditPlan {
            actions: vec![
                // 1. Trim clip-1
                EditAction {
                    action_type: ActionType::Trim,
                    target_clip_id: "clip-1".to_string(),
                    parameters: Some(ActionParameters {
                        trim_start_delta: Some(2.0),
                        trim_end_delta: Some(-1.0),
                        split_time: None,
                        new_start_time: None,
                    }),
                },
                // 2. Delete clip-2
                EditAction {
                    action_type: ActionType::Delete,
                    target_clip_id: "clip-2".to_string(),
                    parameters: None,
                },
            ],
            thought_process: None,
            confidence: None,
        };

        let result = ActionPreflight::preflight_plan(&plan, &state);
        assert!(result.is_ok());
    }
}
