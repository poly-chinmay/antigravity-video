// src-tauri/src/validator.rs
use crate::edit_plan::EditPlan;
use crate::timeline::TimelineEngine;
use tauri::State;

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
