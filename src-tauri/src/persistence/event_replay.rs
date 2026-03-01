// src-tauri/src/persistence/event_replay.rs
//! Event replay - reconstruct state from events

use crate::edit_plan::{ActionType, EditPlan};
use crate::persistence::event_store::Event;
use crate::timeline::TimelineState;

/// Replay a single event on the timeline state
///
/// Converts the event's EditPlan into state mutations and applies them.
/// Validates invariants after replay.
pub fn replay_event(event: &Event, state: &mut TimelineState) -> Result<(), String> {
    if !event.success {
        // Skip failed events
        return Ok(());
    }

    // Apply each action in the edit plan
    for action in &event.edit_plan.actions {
        match action.action_type {
            ActionType::Delete => {
                // Delete clip
                state.remove_clip(&action.target_clip_id).ok_or_else(|| {
                    format!("Clip '{}' not found during replay", action.target_clip_id)
                })?;
            }
            ActionType::Move => {
                // Move clip
                let params = action
                    .parameters
                    .as_ref()
                    .ok_or("MOVE action missing parameters")?;
                let new_start = params
                    .new_start_time
                    .ok_or("MOVE action missing new_start_time")?;

                let clip = state
                    .get_clip_by_id_mut(&action.target_clip_id)
                    .ok_or_else(|| {
                        format!("Clip '{}' not found during replay", action.target_clip_id)
                    })?;

                clip.start = new_start;
                state.recalculate_duration();
            }
            ActionType::Trim => {
                // Trim clip
                let params = action
                    .parameters
                    .as_ref()
                    .ok_or("TRIM action missing parameters")?;

                let clip = state
                    .get_clip_by_id_mut(&action.target_clip_id)
                    .ok_or_else(|| {
                        format!("Clip '{}' not found during replay", action.target_clip_id)
                    })?;

                if let Some(delta) = params.trim_start_delta {
                    clip.start += delta;
                    clip.duration -= delta;
                }

                if let Some(delta) = params.trim_end_delta {
                    clip.duration += delta;
                }

                state.recalculate_duration();
            }
            ActionType::Split => {
                // Split clip - more complex, creates new clips
                let params = action
                    .parameters
                    .as_ref()
                    .ok_or("SPLIT action missing parameters")?;
                let split_time = params.split_time.ok_or("SPLIT action missing split_time")?;

                let original = state
                    .get_clip_by_id(&action.target_clip_id)
                    .ok_or_else(|| {
                        format!("Clip '{}' not found during replay", action.target_clip_id)
                    })?
                    .clone();

                // Remove original
                state
                    .remove_clip(&action.target_clip_id)
                    .ok_or_else(|| format!("Failed to remove clip '{}'", action.target_clip_id))?;

                // Create two new clips
                let first_duration = split_time - original.start;
                let second_duration = original.duration - first_duration;

                let first_clip = crate::timeline::Clip {
                    id: uuid::Uuid::new_v4().to_string(),
                    track_id: original.track_id.clone(),
                    start: original.start,
                    duration: first_duration,
                    source_file: original.source_file.clone(),
                };

                let second_clip = crate::timeline::Clip {
                    id: uuid::Uuid::new_v4().to_string(),
                    track_id: original.track_id.clone(),
                    start: split_time,
                    duration: second_duration,
                    source_file: original.source_file.clone(),
                };

                state.add_clip(first_clip);
                state.add_clip(second_clip);
            }
        }
    }

    // Validate invariants after replay
    state.validate_invariants()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::{ActionParameters, EditAction};
    use crate::timeline::Clip;

    fn make_clip(id: &str, start: f64, duration: f64) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start,
            duration,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_replay_delete_action() {
        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1", 0.0, 10.0));

        let event = Event::new(
            1,
            EditPlan {
                actions: vec![EditAction {
                    action_type: ActionType::Delete,
                    target_clip_id: "clip-1".to_string(),
                    parameters: None,
                }],
                thought_process: None,
                confidence: None,
            },
            None,
            None,
            10,
            true,
        );

        replay_event(&event, &mut state).unwrap();
        assert_eq!(state.clips.len(), 0);
    }

    #[test]
    fn test_replay_move_action() {
        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1", 0.0, 10.0));

        let event = Event::new(
            1,
            EditPlan {
                actions: vec![EditAction {
                    action_type: ActionType::Move,
                    target_clip_id: "clip-1".to_string(),
                    parameters: Some(ActionParameters {
                        new_start_time: Some(5.0),
                        trim_start_delta: None,
                        trim_end_delta: None,
                        split_time: None,
                    }),
                }],
                thought_process: None,
                confidence: None,
            },
            None,
            None,
            10,
            true,
        );

        replay_event(&event, &mut state).unwrap();
        assert_eq!(state.get_clip_by_id("clip-1").unwrap().start, 5.0);
    }
}
