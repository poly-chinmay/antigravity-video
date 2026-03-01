// src-tauri/src/persistence/hybrid_loader.rs
//! Hybrid loader - load snapshot + replay bounded events

use crate::persistence::event_replay::replay_event;
use crate::persistence::event_store::EventStore;
use crate::persistence::snapshot_store::SnapshotStore;
use crate::timeline::TimelineState;

/// Load project state using hybrid approach
///
/// 1. Load latest snapshot
/// 2. Get events since snapshot (bounded ≤50)
/// 3. Replay events on state
///
/// This ensures fast loading with bounded replay overhead.
pub fn load_project(
    snapshot_store: &SnapshotStore,
    event_store: &EventStore,
) -> Result<TimelineState, String> {
    // Try to load latest snapshot
    match snapshot_store.latest() {
        Ok(Some((snapshot_version, state))) => {
            println!(
                "📸 [HybridLoader] Loaded snapshot at version {}",
                snapshot_version
            );

            // Get latest event version
            let latest_version = event_store
                .get_latest_version()
                .map_err(|e| format!("Failed to get latest version: {}", e))?
                .unwrap_or(snapshot_version);

            if latest_version > snapshot_version {
                // Replay events since snapshot
                let events = event_store
                    .get_range(snapshot_version + 1, latest_version)
                    .map_err(|e| format!("Failed to load events: {}", e))?;

                println!(
                    "📼 [HybridLoader] Replaying {} events (v{} to v{})",
                    events.len(),
                    snapshot_version + 1,
                    latest_version
                );

                // Verify bounded replay (≤50 events)
                if events.len() > 50 {
                    return Err(format!(
                        "Replay exceeded bound: {} events (max 50)",
                        events.len()
                    ));
                }

                let mut state = state;
                for event in events {
                    replay_event(&event, &mut state)?;
                }

                Ok(state)
            } else {
                // No new events, return snapshot state
                Ok(state)
            }
        }
        Ok(None) => {
            // No snapshot, replay all events
            println!("📼 [HybridLoader] No snapshot found, replaying all events");

            let mut state = TimelineState::new();
            let events = event_store
                .load_all()
                .map_err(|e| format!("Failed to load events: {}", e))?;

            for event in events {
                replay_event(&event, &mut state)?;
            }

            Ok(state)
        }
        Err(e) => Err(format!("Failed to load snapshot: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::{ActionType, EditAction, EditPlan};
    use crate::persistence::event_store::Event;
    use crate::timeline::Clip;
    use tempfile::TempDir;

    fn make_clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_hybrid_load_with_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Create initial state and save snapshot
        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1"));
        snapshot_store.save(50, &state).unwrap();

        // Add event after snapshot
        let event = Event::new(
            51,
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
        event_store.append(&event).unwrap();

        // Load project
        let loaded_state = load_project(&snapshot_store, &event_store).unwrap();

        // Should have replayed delete
        assert_eq!(loaded_state.clips.len(), 0);
    }

    #[test]
    fn test_hybrid_load_no_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // No snapshot, just events
        // Should create new state and replay all

        let loaded_state = load_project(&snapshot_store, &event_store).unwrap();
        assert_eq!(loaded_state.clips.len(), 0);
    }
}
