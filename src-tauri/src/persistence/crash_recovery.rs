// src-tauri/src/persistence/crash_recovery.rs
//! Crash Recovery - Detect and recover from partial writes

use crate::persistence::event_replay::replay_event;
use crate::persistence::event_store::EventStore;
use crate::persistence::snapshot_store::SnapshotStore;
use crate::timeline::TimelineState;
use std::time::Instant;

/// Maximum acceptable data loss in seconds
pub const MAX_LOSS_SECONDS: u64 = 30;

/// Recovery result with state and user message
pub struct RecoveryResult {
    pub state: TimelineState,
    pub message: Option<String>,
    pub events_recovered: u64,
    pub recovery_time_ms: u64,
}

/// Recover project state after crash
///
/// Process:
/// 1. Load latest snapshot
/// 2. Identify committed events since snapshot
/// 3. Filter out partial/incomplete writes
/// 4. Replay valid events
/// 5. Generate user message if data loss detected
pub fn recover_from_crash(
    snapshot_store: &SnapshotStore,
    event_store: &EventStore,
) -> Result<RecoveryResult, String> {
    let start = Instant::now();

    // 1. Try to load latest snapshot
    let (snapshot_version, mut state) = match snapshot_store.latest() {
        Ok(Some((version, state))) => {
            println!("🔄 [Recovery] Loaded snapshot at v{}", version);
            (version, state)
        }
        Ok(None) => {
            println!("🔄 [Recovery] No snapshot found, starting fresh");
            (0, TimelineState::new())
        }
        Err(e) => {
            println!("⚠️ [Recovery] Snapshot load failed: {}", e);
            (0, TimelineState::new())
        }
    };

    // 2. Get latest event version
    let latest_version = event_store
        .get_latest_version()
        .map_err(|e| format!("Failed to get latest version: {}", e))?
        .unwrap_or(snapshot_version);

    // 3. Get events since snapshot
    let events = if latest_version > snapshot_version {
        event_store
            .get_range(snapshot_version + 1, latest_version)
            .map_err(|e| format!("Failed to load events: {}", e))?
    } else {
        Vec::new()
    };

    // 4. Filter valid events (skip failed ones)
    let valid_events: Vec<_> = events.iter().filter(|e| e.success).collect();

    println!(
        "🔄 [Recovery] Replaying {} valid events (v{} to v{})",
        valid_events.len(),
        snapshot_version + 1,
        latest_version
    );

    // 5. Replay valid events
    let mut replay_errors = 0;
    for event in &valid_events {
        if let Err(e) = replay_event(event, &mut state) {
            println!(
                "⚠️ [Recovery] Event v{} replay failed: {}",
                event.version, e
            );
            replay_errors += 1;
        }
    }

    // 6. Validate final state
    if let Err(e) = state.validate_invariants() {
        return Err(format!("Recovery resulted in invalid state: {}", e));
    }

    let recovery_time_ms = start.elapsed().as_millis() as u64;

    // 7. Generate user message
    let message = if replay_errors > 0 || events.len() != valid_events.len() {
        let lost_events = events.len() - valid_events.len() + replay_errors;
        let estimated_loss_seconds = lost_events as u64 * 2; // Assume ~2 seconds per event
        Some(format!(
            "Recovered project to last saved state. You may have lost up to {} seconds of edits.",
            estimated_loss_seconds.min(MAX_LOSS_SECONDS)
        ))
    } else {
        None
    };

    Ok(RecoveryResult {
        state,
        message,
        events_recovered: valid_events.len() as u64,
        recovery_time_ms,
    })
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
    fn test_recovery_with_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Create and save snapshot
        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1"));
        snapshot_store.save(50, &state).unwrap();

        // Recover
        let result = recover_from_crash(&snapshot_store, &event_store).unwrap();

        assert_eq!(result.state.clips.len(), 1);
        assert!(result.message.is_none());
    }

    #[test]
    fn test_recovery_skips_failed_events() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Create failed event (will be skipped)
        let failed_event = Event::new(
            1,
            EditPlan {
                actions: vec![],
                thought_process: None,
                confidence: None,
            },
            None,
            None,
            10,
            false, // success = false
        );
        event_store.append(&failed_event).unwrap();

        // Recover
        let result = recover_from_crash(&snapshot_store, &event_store).unwrap();

        assert_eq!(result.events_recovered, 0);
        assert!(result.message.is_some()); // Should indicate potential loss
    }

    #[test]
    fn test_recovery_no_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Recover with no data
        let result = recover_from_crash(&snapshot_store, &event_store).unwrap();

        assert_eq!(result.state.clips.len(), 0);
        assert!(result.message.is_none());
    }
}
