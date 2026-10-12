//! tests/persistence_integration_tests.rs
//! Comprehensive persistence integration tests for Phase C3

use ghost_lib::persistence::{recover_from_crash, EventStore, SnapshotManager, SnapshotStore};
use ghost_lib::timeline::{Clip, TimelineState};
use std::time::Instant;
use tempfile::TempDir;

fn make_clip(id: &str, start: f64, duration: f64) -> Clip {
    Clip {
        id: id.to_string(),
        track_id: "track-1".to_string(),
        start,
        duration,
        source_file: "/test.mp4".to_string(),
    }
}

// =============================================================================
// C3.6.1: Undo/Redo After Restart
// =============================================================================

#[test]
fn test_state_persists_after_simulated_restart() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create initial state
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));
    state.add_clip(make_clip("clip-2", 10.0, 10.0));

    // Save snapshot
    snapshot_store.save(50, &state).unwrap();

    // Simulate restart by loading
    let recovered = recover_from_crash(&snapshot_store, &event_store).unwrap();

    assert_eq!(recovered.state.clips.len(), 2);
    assert!(recovered.state.get_clip_by_id("clip-1").is_some());
    assert!(recovered.state.get_clip_by_id("clip-2").is_some());
}

// =============================================================================
// C3.6.2: Crash Recovery Bounded Loss
// =============================================================================

#[test]
fn test_crash_recovery_bounded_loss() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create state and save snapshot
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));
    snapshot_store.save(50, &state).unwrap();

    // Recover (nothing to replay)
    let result = recover_from_crash(&snapshot_store, &event_store).unwrap();

    // Should have no message if no data loss
    assert!(result.message.is_none());
}

// =============================================================================
// C3.6.3: Load Time Performance Test
// =============================================================================

#[test]
fn test_load_time_performance() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create state with 500 clips (simulating moderate-sized project)
    let mut state = TimelineState::new();
    for i in 0..500 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }
    snapshot_store.save(50, &state).unwrap();

    // Measure load time
    let start = Instant::now();
    let _recovered = recover_from_crash(&snapshot_store, &event_store).unwrap();
    let elapsed = start.elapsed();

    println!("Load time for 500 clips: {:?}", elapsed);

    // Should load in < 1 second
    assert!(
        elapsed.as_millis() < 1000,
        "Load took {:?}, should be < 1s",
        elapsed
    );
}

// =============================================================================
// C3.6.4: Full Lifecycle Test
// =============================================================================

#[test]
fn test_full_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Phase 1: Create project
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // Phase 2: Make edits
    state.add_clip(make_clip("clip-2", 10.0, 15.0));
    state.add_clip(make_clip("clip-3", 25.0, 20.0));

    // Phase 3: Save snapshot
    snapshot_store.save(50, &state).unwrap();

    // Verify state before "crash"
    assert_eq!(state.clips.len(), 3);
    let total_duration = state.duration;

    // Phase 4: Simulate crash and recovery
    let recovered = recover_from_crash(&snapshot_store, &event_store).unwrap();

    // Phase 5: Verify recovered state matches
    assert_eq!(recovered.state.clips.len(), 3);
    assert_eq!(recovered.state.duration, total_duration);
    assert!(recovered.state.validate_invariants().is_ok());
}

// =============================================================================
// C3.6.5: Snapshot Manager Automatic Trigger
// =============================================================================

#[test]
fn test_snapshot_manager_automatic_trigger() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let mut manager = SnapshotManager::new(snapshot_store);

    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // Should NOT trigger at v1
    assert!(!manager.maybe_create_snapshot(1, &state).unwrap());

    // Should trigger at v50
    assert!(manager.maybe_create_snapshot(50, &state).unwrap());
    assert_eq!(manager.last_snapshot_version(), 50);
}

// =============================================================================
// C3.6.6: Invariants After Recovery
// =============================================================================

#[test]
fn test_invariants_after_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
    let event_store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create valid state
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));
    state.add_clip(make_clip("clip-2", 15.0, 10.0)); // Non-overlapping

    // Verify invariants before save
    assert!(state.validate_invariants().is_ok());

    // Save and recover
    snapshot_store.save(50, &state).unwrap();
    let recovered = recover_from_crash(&snapshot_store, &event_store).unwrap();

    // Invariants must still hold
    assert!(recovered.state.validate_invariants().is_ok());
}
