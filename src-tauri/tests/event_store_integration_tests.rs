//! tests/event_store_integration_tests.rs
//! Integration tests for EventStore with UndoRedoManager

use ghost_lib::persistence::event_store::{Event, EventStore};
use ghost_lib::timeline::{Clip, TimelineState};
use ghost_lib::undo_commands::DeleteClipCommand;
use ghost_lib::undo_redo_manager::UndoRedoManager;
use std::fs;
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
fn test_event_store_atomic_write() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create and append event
    let event = Event::new(
        1,
        ghost_lib::edit_plan::EditPlan {
            actions: vec![],
            thought_process: None,
            confidence: None,
        },
        None,
        None,
        10,
        true,
    );

    store.append(&event).unwrap();

    // Verify event was written
    let events = store.load_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].version, 1);
}

#[test]
fn test_event_version_monotonicity() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Append events in order
    for i in 1..=5 {
        let event = Event::new(
            i,
            ghost_lib::edit_plan::EditPlan {
                actions: vec![],
                thought_process: None,
                confidence: None,
            },
            None,
            None,
            10,
            true,
        );
        store.append(&event).unwrap();
    }

    // Verify monotonic ordering
    let events = store.load_all().unwrap();
    assert_eq!(events.len(), 5);
    for i in 0..5 {
        assert_eq!(events[i].version, (i + 1) as u64);
    }
}

#[test]
fn test_crash_recovery_simulation() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Write event 1
    let event1 = Event::new(
        1,
        ghost_lib::edit_plan::EditPlan {
            actions: vec![],
            thought_process: None,
            confidence: None,
        },
        None,
        None,
        10,
        true,
    );
    store.append(&event1).unwrap();

    // Simulate crash by creating incomplete temp file
    let temp_path = temp_dir.path().join("events").join("00000002.json.tmp");
    fs::write(&temp_path, "incomplete data").unwrap();

    // Load should still work and ignore temp file
    let events = store.load_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].version, 1);

    // Cleanup temp file
    fs::remove_file(&temp_path).ok();
}

#[test]
fn test_event_data_completeness() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Create event with full metadata
    let event = Event::new(
        1,
        ghost_lib::edit_plan::EditPlan {
            actions: vec![ghost_lib::edit_plan::EditAction {
                action_type: ghost_lib::edit_plan::ActionType::Delete,
                target_clip_id: "clip-1".to_string(),
                parameters: None,
            }],
            thought_process: Some("Delete clip".to_string()),
            confidence: Some(0.95),
        },
        Some("User wants to delete clip-1".to_string()),
        Some(0.95),
        50,
        true,
    );

    store.append(&event).unwrap();

    // Load and verify all data preserved
    let events = store.load_all().unwrap();
    assert_eq!(events.len(), 1);

    let loaded = &events[0];
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.edit_plan.actions.len(), 1);
    assert_eq!(
        loaded.user_intent,
        Some("User wants to delete clip-1".to_string())
    );
    assert_eq!(loaded.ai_confidence, Some(0.95));
    assert_eq!(loaded.execution_time_ms, 50);
    assert_eq!(loaded.success, true);
}

#[test]
fn test_get_events_since() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Append 10 events
    for i in 1..=10 {
        let event = Event::new(
            i,
            ghost_lib::edit_plan::EditPlan {
                actions: vec![],
                thought_process: None,
                confidence: None,
            },
            None,
            None,
            10,
            true,
        );
        store.append(&event).unwrap();
    }

    // Get events since version 5
    let events = store.get_range(5, 10).unwrap();
    assert_eq!(events.len(), 6);
    assert_eq!(events[0].version, 5);
    assert_eq!(events[5].version, 10);
}

#[test]
fn test_empty_store() {
    let temp_dir = TempDir::new().unwrap();
    let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

    // Empty store should return empty vec
    let events = store.load_all().unwrap();
    assert_eq!(events.len(), 0);

    // Latest version should be None
    assert_eq!(store.get_latest_version().unwrap(), None);
}
