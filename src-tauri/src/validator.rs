// src-tauri/src/validator.rs
use crate::edit_plan::EditPlan;
use crate::timeline::TimelineEngine;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize, PartialEq)]
#[allow(dead_code)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub offending_action: Option<String>,
}

// A simple enum to represent actions we might validate
// In Week 7, this will be the actual Action struct.
// For now, we simulate it with a tuple or struct.
#[allow(dead_code)]
pub enum Action {
    DeleteClip { id: String },
    // Add more actions as needed
}

pub fn validate_plan(plan: &EditPlan, engine: &State<'_, TimelineEngine>) -> Result<(), String> {
    if plan.actions.is_empty() {
        return Err("Plan Validation Rejected: Plan contains no actions.".to_string());
    }

    // Lock the state to check against current clips
    let state = engine
        .state
        .lock()
        .map_err(|_| "Failed to acquire state lock".to_string())?;

    for action in &plan.actions {
        // Rule: Target clip must exist
        if !state.clips.iter().any(|c| c.id == action.target_clip_id) {
            return Err(format!(
                "Validation Failed: Target clip ID '{}' not found in timeline.",
                action.target_clip_id
            ));
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub fn validate_actions_against_state(
    actions: &[Action],
    state: &crate::timeline::TimelineState,
) -> Result<(), ValidationError> {
    for action in actions {
        match action {
            Action::DeleteClip { id } => {
                // Rule: Target clip must exist
                if !state.clips.iter().any(|c| c.id == *id) {
                    return Err(ValidationError {
                        code: "VALIDATION_REJECTED".to_string(),
                        message: format!("Clip with ID {} not found.", id),
                        offending_action: Some(format!("DeleteClip({})", id)),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;
    use crate::timeline::TimelineState;

    #[test]
    fn test_validate_delete_non_existent() {
        let state = TimelineState {
            clips: vec![],
            duration: 0.0,
        };
        let actions = vec![Action::DeleteClip {
            id: "missing".to_string(),
        }];

        let result = validate_actions_against_state(&actions, &state);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "VALIDATION_REJECTED");
    }

    #[test]
    fn test_validate_delete_existing() {
        let clip = Clip {
            id: "existing".to_string(),
            track_id: "v1".to_string(),
            start: 0.0,
            duration: 5.0,
            source_file: "test.mp4".to_string(),
        };
        let state = TimelineState {
            clips: vec![clip],
            duration: 5.0,
        };
        let actions = vec![Action::DeleteClip {
            id: "existing".to_string(),
        }];

        let result = validate_actions_against_state(&actions, &state);
        assert!(result.is_ok());
    }
}
