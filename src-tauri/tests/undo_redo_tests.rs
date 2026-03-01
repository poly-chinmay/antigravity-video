//! tests/undo_redo_tests.rs
//! Comprehensive test suite for Phase B: Undo/Redo Engine

use ghost_lib::timeline::{Clip, TimelineState};
use ghost_lib::undo_commands::{
    DeleteClipCommand, MoveClipCommand, SplitClipCommand, TrimClipCommand,
};
use ghost_lib::undo_redo_manager::{UndoRedoConfig, UndoRedoManager};
use std::time::Instant;

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
// PERFECT INVERSION TESTS
// =============================================================================

#[test]
fn test_delete_perfect_inversion() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    let original_state = state.clone();

    // Delete
    let cmd = Box::new(DeleteClipCommand::new("clip-1".to_string()));
    manager.execute_command(cmd, &mut state).unwrap();
    assert_eq!(state.clips.len(), 0);

    // Undo should restore exact state
    manager.undo(&mut state).unwrap();
    assert_eq!(state.clips.len(), original_state.clips.len());
    assert_eq!(state.clips[0].id, original_state.clips[0].id);
    assert_eq!(state.clips[0].start, original_state.clips[0].start);
    assert_eq!(state.clips[0].duration, original_state.clips[0].duration);
}

#[test]
fn test_move_perfect_inversion() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    let original_start = state.clips[0].start;

    // Move
    let cmd = Box::new(MoveClipCommand::new("clip-1".to_string(), 5.0));
    manager.execute_command(cmd, &mut state).unwrap();
    assert_eq!(state.get_clip_by_id("clip-1").unwrap().start, 5.0);

    // Undo should restore exact position
    manager.undo(&mut state).unwrap();
    assert_eq!(
        state.get_clip_by_id("clip-1").unwrap().start,
        original_start
    );
}

#[test]
fn test_trim_perfect_inversion() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    let original_start = state.clips[0].start;
    let original_duration = state.clips[0].duration;

    // Trim
    let cmd = Box::new(TrimClipCommand::new(
        "clip-1".to_string(),
        Some(2.0),
        Some(-1.0),
    ));
    manager.execute_command(cmd, &mut state).unwrap();

    // Undo should restore exact bounds
    manager.undo(&mut state).unwrap();
    let clip = state.get_clip_by_id("clip-1").unwrap();
    assert_eq!(clip.start, original_start);
    assert_eq!(clip.duration, original_duration);
}

#[test]
fn test_split_perfect_inversion() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    assert_eq!(state.clips.len(), 1);

    // Split
    let cmd = Box::new(SplitClipCommand::new("clip-1".to_string(), 5.0));
    manager.execute_command(cmd, &mut state).unwrap();
    assert_eq!(state.clips.len(), 2);

    // Undo should restore single clip
    manager.undo(&mut state).unwrap();
    assert_eq!(state.clips.len(), 1);
    assert_eq!(state.clips[0].duration, 10.0);
}

// =============================================================================
// REDO BRANCH SAFETY TESTS
// =============================================================================

#[test]
fn test_new_command_clears_redo_branch() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));
    state.add_clip(make_clip("clip-2", 10.0, 10.0));

    // Execute and undo
    let cmd1 = Box::new(DeleteClipCommand::new("clip-1".to_string()));
    manager.execute_command(cmd1, &mut state).unwrap();
    manager.undo(&mut state).unwrap();

    assert_eq!(manager.redo_count(), 1);

    // New command should clear redo
    let cmd2 = Box::new(DeleteClipCommand::new("clip-2".to_string()));
    manager.execute_command(cmd2, &mut state).unwrap();

    assert_eq!(manager.redo_count(), 0);
    assert!(!manager.can_redo());
}

#[test]
fn test_undo_redo_undo_sequence() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // Execute
    let cmd = Box::new(DeleteClipCommand::new("clip-1".to_string()));
    manager.execute_command(cmd, &mut state).unwrap();
    assert_eq!(state.clips.len(), 0);

    // Undo
    manager.undo(&mut state).unwrap();
    assert_eq!(state.clips.len(), 1);

    // Redo
    manager.redo(&mut state).unwrap();
    assert_eq!(state.clips.len(), 0);

    // Undo again
    manager.undo(&mut state).unwrap();
    assert_eq!(state.clips.len(), 1);
}

#[test]
fn test_multiple_redo_after_undo() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));
    state.add_clip(make_clip("clip-2", 10.0, 10.0));

    // Execute two commands
    manager
        .execute_command(
            Box::new(DeleteClipCommand::new("clip-1".to_string())),
            &mut state,
        )
        .unwrap();
    manager
        .execute_command(
            Box::new(DeleteClipCommand::new("clip-2".to_string())),
            &mut state,
        )
        .unwrap();

    // Undo both
    manager.undo(&mut state).unwrap();
    manager.undo(&mut state).unwrap();

    assert_eq!(manager.redo_count(), 2);

    // Redo both
    manager.redo(&mut state).unwrap();
    manager.redo(&mut state).unwrap();

    assert_eq!(state.clips.len(), 0);
}

// =============================================================================
// COALESCING TESTS
// =============================================================================

#[test]
fn test_move_commands_coalesce() {
    let config = UndoRedoConfig {
        max_undo_count: 100,
        max_memory_bytes: 10 * 1024 * 1024,
        coalesce_window_ms: 500,
    };
    let mut manager = UndoRedoManager::with_config(config);
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // Execute multiple moves quickly
    manager
        .execute_command(
            Box::new(MoveClipCommand::new("clip-1".to_string(), 5.0)),
            &mut state,
        )
        .unwrap();

    manager
        .execute_command(
            Box::new(MoveClipCommand::new("clip-1".to_string(), 10.0)),
            &mut state,
        )
        .unwrap();

    // Should have coalesced into single command
    assert_eq!(manager.undo_count(), 1);

    // Single undo should restore to original position
    manager.undo(&mut state).unwrap();
    assert_eq!(state.get_clip_by_id("clip-1").unwrap().start, 0.0);
}

#[test]
fn test_different_commands_dont_coalesce() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // Execute different command types
    manager
        .execute_command(
            Box::new(MoveClipCommand::new("clip-1".to_string(), 5.0)),
            &mut state,
        )
        .unwrap();

    manager
        .execute_command(
            Box::new(TrimClipCommand::new("clip-1".to_string(), Some(1.0), None)),
            &mut state,
        )
        .unwrap();

    // Should NOT coalesce
    assert_eq!(manager.undo_count(), 2);
}

#[test]
fn test_coalesce_window_timeout() {
    let config = UndoRedoConfig {
        max_undo_count: 100,
        max_memory_bytes: 10 * 1024 * 1024,
        coalesce_window_ms: 100, // 100ms window
    };
    let mut manager = UndoRedoManager::with_config(config);
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    // First move
    manager
        .execute_command(
            Box::new(MoveClipCommand::new("clip-1".to_string(), 5.0)),
            &mut state,
        )
        .unwrap();

    // Wait for window to expire
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Second move (should NOT coalesce)
    manager
        .execute_command(
            Box::new(MoveClipCommand::new("clip-1".to_string(), 10.0)),
            &mut state,
        )
        .unwrap();

    assert_eq!(manager.undo_count(), 2);
}

// =============================================================================
// MEMORY BOUND TESTS
// =============================================================================

#[test]
fn test_memory_bound_enforced() {
    let config = UndoRedoConfig {
        max_undo_count: 1000,
        max_memory_bytes: 1024, // 1 KB limit
        coalesce_window_ms: 0,  // Disable coalescing
    };
    let mut manager = UndoRedoManager::with_config(config);
    let mut state = TimelineState::new();

    // Add many clips
    for i in 0..100 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }

    // Execute many delete commands
    for i in 0..100 {
        let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
        manager.execute_command(cmd, &mut state).unwrap();
    }

    // Memory should be bounded
    assert!(manager.total_memory_bytes() <= 1024);
    assert!(manager.undo_count() < 100); // Some commands dropped
}

#[test]
fn test_count_bound_enforced() {
    let config = UndoRedoConfig {
        max_undo_count: 10,
        max_memory_bytes: 10 * 1024 * 1024,
        coalesce_window_ms: 0,
    };
    let mut manager = UndoRedoManager::with_config(config);
    let mut state = TimelineState::new();

    // Add many clips
    for i in 0..50 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }

    // Execute 50 commands
    for i in 0..50 {
        let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
        manager.execute_command(cmd, &mut state).unwrap();
    }

    // Should only keep 10
    assert_eq!(manager.undo_count(), 10);
}

#[test]
fn test_oldest_commands_dropped() {
    let config = UndoRedoConfig {
        max_undo_count: 3,
        max_memory_bytes: 10 * 1024 * 1024,
        coalesce_window_ms: 0,
    };
    let mut manager = UndoRedoManager::with_config(config);
    let mut state = TimelineState::new();

    for i in 0..5 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }

    // Execute 5 commands
    for i in 0..5 {
        let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
        manager.execute_command(cmd, &mut state).unwrap();
    }

    assert_eq!(manager.undo_count(), 3);

    // Undo 3 times should restore clips 2, 3, 4 (oldest 0, 1 were dropped)
    manager.undo(&mut state).unwrap(); // Restore clip-4
    manager.undo(&mut state).unwrap(); // Restore clip-3
    manager.undo(&mut state).unwrap(); // Restore clip-2

    assert!(state.get_clip_by_id("clip-2").is_some());
    assert!(state.get_clip_by_id("clip-3").is_some());
    assert!(state.get_clip_by_id("clip-4").is_some());
}

// =============================================================================
// BATCHED UNDO TESTS
// =============================================================================

#[test]
fn test_batched_undo_preserves_invariants() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();

    // Add 10 clips
    for i in 0..10 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }

    // Delete all 10
    for i in 0..10 {
        let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
        manager.execute_command(cmd, &mut state).unwrap();
    }

    // Batch undo all 10
    assert!(manager.undo_multiple(10, &mut state).is_ok());

    // Invariants should hold
    assert!(state.validate_invariants().is_ok());
    assert_eq!(state.clips.len(), 10);
}

#[test]
fn test_batched_undo_count_validation() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();
    state.add_clip(make_clip("clip-1", 0.0, 10.0));

    manager
        .execute_command(
            Box::new(DeleteClipCommand::new("clip-1".to_string())),
            &mut state,
        )
        .unwrap();

    // Try to undo more than available
    let result = manager.undo_multiple(5, &mut state);
    assert!(result.is_err());
}

// =============================================================================
// PERFORMANCE TESTS
// =============================================================================

#[test]
fn test_undo_performance() {
    let mut manager = UndoRedoManager::new();
    let mut state = TimelineState::new();

    // Add 100 clips
    for i in 0..100 {
        state.add_clip(make_clip(&format!("clip-{}", i), i as f64 * 10.0, 10.0));
    }

    // Execute 100 delete commands
    for i in 0..100 {
        let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
        manager.execute_command(cmd, &mut state).unwrap();
    }

    // Measure time to undo all 100
    let start = Instant::now();
    for _ in 0..100 {
        manager.undo(&mut state).unwrap();
    }
    let elapsed = start.elapsed();

    println!("100 undo operations: {:?}", elapsed);

    // Should be < 15ms total
    assert!(
        elapsed.as_millis() < 15,
        "100 undo operations took {:?}, should be < 15ms",
        elapsed
    );
}
