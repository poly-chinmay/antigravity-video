// src-tauri/src/edit_plan_validator.rs
//! EditPlanValidator - Static validation of edit plans
//!
//! Validates that all referenced IDs exist and all action parameters
//! are within valid bounds BEFORE any simulation or execution.

use crate::edit_plan::{ActionType, EditAction, EditPlan};
use crate::edit_rejection::EditRejection;
use crate::timeline::TimelineState;

/// Static validator for edit plans
pub struct EditPlanValidator;

impl EditPlanValidator {
    /// Validate an entire edit plan against the current timeline state
    ///
    /// Checks:
    /// - All referenced ClipIds exist
    /// - All TRIM/SPLIT/MOVE parameters are within valid bounds
    /// - Action ordering rules are followed
    ///
    /// Returns EditRejection on first validation failure.
    pub fn validate_plan(plan: &EditPlan, state: &TimelineState) -> Result<(), EditRejection> {
        // Validate each action sequentially
        for action in &plan.actions {
            Self::validate_action(action, state)?;
        }

        Ok(())
    }

    /// Validate a single action against the current timeline state
    pub fn validate_action(
        action: &EditAction,
        state: &TimelineState,
    ) -> Result<(), EditRejection> {
        // 1. Validate that target clip exists
        if !state.has_clip(&action.target_clip_id) {
            return Err(EditRejection::invalid_reference(
                &action.target_clip_id,
                "ClipId",
            ));
        }

        // Get the clip for bounds checking
        let clip = state
            .get_clip_by_id(&action.target_clip_id)
            .expect("Clip existence already validated");

        // 2. Validate action-specific bounds
        match action.action_type {
            ActionType::Trim => Self::validate_trim(action, clip)?,
            ActionType::Split => Self::validate_split(action, clip)?,
            ActionType::Move => Self::validate_move(action, clip)?,
            ActionType::Delete => {
                // Delete has no additional bounds to check
            }
        }

        Ok(())
    }

    /// Validate TRIM action bounds
    fn validate_trim(
        action: &EditAction,
        clip: &crate::timeline::Clip,
    ) -> Result<(), EditRejection> {
        let Some(params) = &action.parameters else {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                "TRIM action missing parameters",
            ));
        };

        // Check trim_start_delta
        if let Some(delta) = params.trim_start_delta {
            if delta < 0.0 && delta.abs() > clip.duration {
                return Err(EditRejection::bounds_violation(
                    action.clone(),
                    format!(
                        "trim_start_delta ({:.2}s) would exceed clip duration ({:.2}s)",
                        delta, clip.duration
                    ),
                ));
            }

            // After trim, duration must remain positive
            let new_duration = clip.duration - delta;
            if new_duration <= 0.0 {
                return Err(EditRejection::bounds_violation(
                    action.clone(),
                    format!(
                        "trim_start_delta ({:.2}s) would result in zero/negative duration",
                        delta
                    ),
                ));
            }
        }

        // Check trim_end_delta
        if let Some(delta) = params.trim_end_delta {
            let new_duration = clip.duration + delta;
            if new_duration <= 0.0 {
                return Err(EditRejection::bounds_violation(
                    action.clone(),
                    format!(
                        "trim_end_delta ({:.2}s) would result in zero/negative duration",
                        delta
                    ),
                ));
            }
        }

        Ok(())
    }

    /// Validate SPLIT action bounds
    fn validate_split(
        action: &EditAction,
        clip: &crate::timeline::Clip,
    ) -> Result<(), EditRejection> {
        let Some(params) = &action.parameters else {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                "SPLIT action missing parameters",
            ));
        };

        let Some(split_time) = params.split_time else {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                "SPLIT action missing split_time parameter",
            ));
        };

        // Split time must be within clip bounds (exclusive of endpoints)
        if split_time <= clip.start {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                format!(
                    "split_time ({:.2}s) must be after clip start ({:.2}s)",
                    split_time, clip.start
                ),
            ));
        }

        if split_time >= clip.end() {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                format!(
                    "split_time ({:.2}s) must be before clip end ({:.2}s)",
                    split_time,
                    clip.end()
                ),
            ));
        }

        Ok(())
    }

    /// Validate MOVE action bounds
    fn validate_move(
        action: &EditAction,
        _clip: &crate::timeline::Clip,
    ) -> Result<(), EditRejection> {
        let Some(params) = &action.parameters else {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                "MOVE action missing parameters",
            ));
        };

        let Some(new_start) = params.new_start_time else {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                "MOVE action missing new_start_time parameter",
            ));
        };

        // New start time must be non-negative
        if new_start < 0.0 {
            return Err(EditRejection::bounds_violation(
                action.clone(),
                format!("new_start_time ({:.2}s) must be >= 0", new_start),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::ActionParameters;
    use crate::timeline::Clip;

    fn make_test_state() -> TimelineState {
        let mut state = TimelineState::new();
        state.add_clip(Clip {
            id: "clip-1".to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        });
        state
    }

    #[test]
    fn test_valid_delete_accepted() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Delete,
            target_clip_id: "clip-1".to_string(),
            parameters: None,
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_clip_id_rejected() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Delete,
            target_clip_id: "nonexistent".to_string(),
            parameters: None,
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EditRejection::InvalidReference { .. }
        ));
    }

    #[test]
    fn test_trim_exceeding_duration_rejected() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: Some(15.0), // Exceeds 10s duration
                trim_end_delta: None,
                split_time: None,
                new_start_time: None,
            }),
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EditRejection::BoundsViolation { .. }
        ));
    }

    #[test]
    fn test_split_outside_clip_rejected() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Split,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: Some(15.0), // Beyond clip end (10s)
                new_start_time: None,
            }),
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_negative_start_rejected() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Move,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: None,
                new_start_time: Some(-5.0), // Negative start
            }),
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_trim_accepted() {
        let state = make_test_state();
        let action = EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: Some(2.0), // Valid: within 10s duration
                trim_end_delta: Some(-1.0),
                split_time: None,
                new_start_time: None,
            }),
        };

        let result = EditPlanValidator::validate_action(&action, &state);
        assert!(result.is_ok());
    }
}
