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
    #[error("Post-mutation invariant violated: {0}")]
    InvariantViolation(String),
}

/// AUTHORITATIVE LIST OF TIMELINE INVARIANTS
/// These MUST hold true after EVERY mutation, or the mutation is rejected.
///
/// 1. All clips must have duration > 0
/// 2. All clips must have start >= 0
/// 3. No overlapping clips on the same track
/// 4. Timeline duration = max(start + duration) across all clips (or 0 if empty)
/// 5. playhead_time ∈ [0, duration]
///
/// If ANY invariant fails, the mutation MUST be rolled back.
pub fn validate_state_invariants(state: &TimelineState) -> Result<(), RouterError> {
    // Invariant 1: All clips must have positive duration
    for clip in &state.clips {
        if clip.duration <= 0.0 {
            return Err(RouterError::InvariantViolation(format!(
                "Clip '{}' has invalid duration: {:.2}s (must be > 0)",
                clip.id, clip.duration
            )));
        }
    }

    // Invariant 2: All clips must have non-negative start time
    for clip in &state.clips {
        if clip.start < 0.0 {
            return Err(RouterError::InvariantViolation(format!(
                "Clip '{}' has negative start time: {:.2}s",
                clip.id, clip.start
            )));
        }
    }

    // Invariant 3: No overlapping clips on the same track
    let mut clips_by_track: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for clip in &state.clips {
        clips_by_track
            .entry(clip.track_id.clone())
            .or_default()
            .push((clip.id.clone(), clip.start, clip.start + clip.duration));
    }

    for (track_id, mut clips) in clips_by_track {
        clips.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        for i in 1..clips.len() {
            let prev_end = clips[i - 1].2;
            let curr_start = clips[i].1;
            // Allow tiny gaps due to floating point precision
            if prev_end > curr_start + 0.001 {
                return Err(RouterError::InvariantViolation(format!(
                    "Clips '{}' and '{}' overlap on track '{}' (prev ends at {:.2}s, next starts at {:.2}s)",
                    clips[i - 1].0, clips[i].0, track_id, prev_end, curr_start
                )));
            }
        }
    }

    // Invariant 4: Duration must equal max(start + duration) or 0 if empty
    let calculated_duration = state
        .clips
        .iter()
        .map(|c| c.start + c.duration)
        .fold(0.0, f64::max);
    if (state.duration - calculated_duration).abs() > 0.001 {
        return Err(RouterError::InvariantViolation(format!(
            "Duration mismatch: stored={:.2}s, calculated={:.2}s",
            state.duration, calculated_duration
        )));
    }

    // Invariant 5: Playhead must be within valid range [0, duration]
    if state.playhead_time < 0.0 || state.playhead_time > state.duration + 0.001 {
        return Err(RouterError::InvariantViolation(format!(
            "Playhead {:.2}s is outside valid range [0, {:.2}]",
            state.playhead_time, state.duration
        )));
    }

    Ok(())
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

    // STEP 3 FIX: Snapshot state BEFORE mutations for rollback capability
    let snapshot = state.clone();

    // 2. Pre-Validation Pass: Check target clips exist
    for action in &plan.actions {
        if !state.clips.iter().any(|c| c.id == action.target_clip_id) {
            return Err(RouterError::ClipNotFound(action.target_clip_id.clone()).to_string());
        }
    }

    // 3. Execution Pass
    for action in &plan.actions {
        println!(
            "▶️ [Router] Executing {:?} on clip {}",
            action.action_type, action.target_clip_id
        );

        match &action.action_type {
            ActionType::Delete => {
                if let Some(index) = state
                    .clips
                    .iter()
                    .position(|c| c.id == action.target_clip_id)
                {
                    let removed = state.clips.remove(index);
                    println!("  ✓ Deleted clip: {}", removed.id);
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
                            let old_start = clip.start;
                            // Enforce non-negative start time
                            clip.start = new_start.max(0.0);
                            println!(
                                "  ✓ Moved clip from {:.2}s to {:.2}s",
                                old_start, clip.start
                            );
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
                        let original_duration = clip.duration;

                        // Trim Start
                        if let Some(delta) = params.trim_start_delta {
                            clip.start += delta;
                            clip.duration -= delta;
                        }

                        // Trim End
                        if let Some(delta) = params.trim_end_delta {
                            clip.duration += delta; // Delta is usually negative for shortening
                        }

                        // Enforce minimum duration (0.1s)
                        const MIN_DURATION: f64 = 0.1;
                        if clip.duration < MIN_DURATION {
                            clip.duration = MIN_DURATION;
                        }

                        // Enforce non-negative start
                        if clip.start < 0.0 {
                            clip.start = 0.0;
                        }

                        println!(
                            "  ✓ Trimmed clip: {:.2}s -> {:.2}s",
                            original_duration, clip.duration
                        );
                    }
                }
            }
            ActionType::Split => {
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

                                println!(
                                    "  ✓ Split clip at {:.2}s, new clip: {}",
                                    split_time, new_clip.id
                                );

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
    state.duration = state
        .clips
        .iter()
        .map(|c| c.start + c.duration)
        .fold(0.0, f64::max);

    // STEP 5 FIX: Clamp playhead to valid range after mutations
    // Invariant: playhead_time ∈ [0, duration] always
    let old_playhead = state.playhead_time;
    state.playhead_time = state.playhead_time.clamp(0.0, state.duration);
    if (old_playhead - state.playhead_time).abs() > 0.001 {
        println!(
            "⚠️ [Router] Playhead clamped: {:.2}s → {:.2}s",
            old_playhead, state.playhead_time
        );
    }

    // STEP 3 FIX: Post-Mutation Validation with ROLLBACK
    // Invalid state CANNOT persist - this is a hard reject
    if let Err(e) = validate_state_invariants(&state) {
        println!(
            "❌ [Router] Invariant violation detected: {}. ROLLING BACK.",
            e
        );
        // Restore snapshot - atomicity enforced
        *state = snapshot;
        return Err(format!("Mutation rejected - invariant violated: {}", e));
    }

    // 6. Increment version counter
    state.version += 1;

    println!(
        "📊 [Backend] State AFTER execution: {} clips, {:.2}s, version {}",
        state.clips.len(),
        state.duration,
        state.version
    );

    // 7. Emit Update
    let _ = app_handle.emit("STATE_UPDATE", &*state);

    // 8. Log Interaction
    let details = serde_json::json!({
        "plan": plan,
        "resulting_duration": state.duration
    });
    prefs.log_interaction("AI_EDIT_APPLIED", details);

    Ok(state.clone())
}
