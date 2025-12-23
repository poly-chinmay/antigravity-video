use crate::edit_plan::{ActionType, EditPlan};
use crate::preferences::PreferenceManager;
use crate::timeline::{TimelineEngine, TimelineState};
use tauri::{AppHandle, Emitter, State};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Target clip {0} not found")]
    ClipNotFound(String),
    #[error("Failed to acquire state lock")]
    LockError,
    #[error("Invalid action parameters: {0}")]
    InvalidParameters(String),
}

pub fn run_edit_plan(
    engine: &State<'_, TimelineEngine>,
    app_handle: &AppHandle,
    prefs: &State<'_, PreferenceManager>,
    plan: EditPlan,
) -> Result<TimelineState, String> {
    println!(
        "🚀 [Backend] Action Router: Executing Edit Plan with {} actions",
        plan.actions.len()
    );
    println!("📋 [Backend] Plan Details: {:?}", plan);

    // 1. Acquire Lock
    let mut state = engine
        .state
        .lock()
        .map_err(|_| "Failed to acquire state lock".to_string())?;

    println!(
        "📊 [Backend] State BEFORE execution: {} clips, {:.2}s",
        state.clips.len(),
        state.duration
    );

    // 2. Validation Pass (Simulated)
    // In a real app, we would run deep validation here.
    // For now, we check if target clips exist.
    for action in &plan.actions {
        if !state.clips.iter().any(|c| c.id == action.target_clip_id) {
            return Err(RouterError::ClipNotFound(action.target_clip_id.clone()).to_string());
        }
    }

    // 3. Execution Pass
    for action in &plan.actions {
        match &action.action_type {
            ActionType::Delete => {
                if let Some(index) = state
                    .clips
                    .iter()
                    .position(|c| c.id == action.target_clip_id)
                {
                    state.clips.remove(index);
                }
            }
            ActionType::Move => {
                if let Some(clip) = state
                    .clips
                    .iter_mut()
                    .find(|c| c.id == action.target_clip_id)
                {
                    if let Some(params) = &action.parameters {
                        if let Some(new_start) = params.new_start_time {
                            clip.start = new_start.max(0.0);
                        }
                    }
                }
            }
            ActionType::Trim => {
                if let Some(clip) = state
                    .clips
                    .iter_mut()
                    .find(|c| c.id == action.target_clip_id)
                {
                    if let Some(params) = &action.parameters {
                        if let Some(delta) = params.trim_start_delta {
                            clip.start += delta;
                            clip.duration -= delta;
                            // Ensure positive duration
                            if clip.duration < 0.1 {
                                clip.duration = 0.1; // Min duration
                            }
                        }
                        // Trim End
                        if let Some(delta) = params.trim_end_delta {
                            clip.duration += delta; // Delta is usually negative for shortening
                            if clip.duration < 0.1 {
                                clip.duration = 0.1;
                            }
                        }
                    }
                }
            }
            ActionType::Split => {
                // Split is complex: we need to find the clip, modify it, and insert a new one.
                // We can't mutate `state.clips` while iterating easily, so we use indices.
                if let Some(index) = state
                    .clips
                    .iter()
                    .position(|c| c.id == action.target_clip_id)
                {
                    if let Some(params) = &action.parameters {
                        if let Some(split_time) = params.split_time {
                            let original_clip = &mut state.clips[index];

                            // Calculate relative split point
                            let relative_split = split_time - original_clip.start;

                            if relative_split > 0.0 && relative_split < original_clip.duration {
                                // Create new clip (second half)
                                let new_duration = original_clip.duration - relative_split;
                                let mut new_clip = original_clip.clone();
                                new_clip.id = Uuid::new_v4().to_string();
                                new_clip.start = split_time;
                                new_clip.duration = new_duration;

                                // Modify original (first half)
                                original_clip.duration = relative_split;

                                // Insert new clip after original
                                state.clips.insert(index + 1, new_clip);
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Recalculate Duration
    // Simple logic: max(start + duration) of all clips
    state.duration = state
        .clips
        .iter()
        .map(|c| c.start + c.duration)
        .fold(0.0, f64::max);

    println!(
        "📊 [Backend] State AFTER execution: {} clips, {:.2}s",
        state.clips.len(),
        state.duration
    );

    // 5. Emit Update
    let _ = app_handle.emit("STATE_UPDATE", &*state);

    // 6. Log Interaction (Week 9)
    let details = serde_json::json!({
        "plan": plan,
        "resulting_duration": state.duration
    });
    prefs.log_interaction("AI_EDIT_APPLIED", details);

    Ok(state.clone())
}
