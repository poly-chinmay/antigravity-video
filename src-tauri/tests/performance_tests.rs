// tests/performance_tests.rs
//! Performance Tests for High-Scale Timeline Operations
//!
//! Tests verify that indexed lookups maintain performance at scale:
//! - 50 clips: baseline
//! - 500 clips: target
//! - 5000 clips: stretch goal

use ghost_lib::timeline::{Clip, ProjectSettings, TimelineState};
use std::time::Instant;

/// Helper to create a clip at a specific position
fn make_clip(id: &str, track: &str, start: f64, duration: f64) -> Clip {
    Clip {
        id: id.to_string(),
        track_id: track.to_string(),
        start,
        duration,
        source_file: "/test.mp4".to_string(),
    }
}

/// Helper to create a timeline with N clips spread across tracks
fn create_timeline_with_clips(clip_count: usize) -> TimelineState {
    let mut state = TimelineState::new();
    let tracks = vec!["track_1", "track_2", "track_3"];

    for i in 0..clip_count {
        let track = tracks[i % tracks.len()];
        let start = (i as f64) * 5.0; // 5 second gaps between clips on same track
        let clip = make_clip(
            &format!("clip-{}", i),
            track,
            start,
            4.0, // 4 second duration
        );
        state.add_clip(clip);
    }

    state
}

// =============================================================================
// BASELINE: 50 CLIPS
// =============================================================================

#[test]
fn perf_50_clips_lookup() {
    let state = create_timeline_with_clips(50);

    // Measure O(1) lookup time
    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let clip_id = format!("clip-{}", i % 50);
        let _ = state.get_clip_by_id(&clip_id);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("50 clips - O(1) lookup: {} ns avg", avg_ns);

    // Assert average lookup < 1µs (1000ns)
    assert!(
        avg_ns < 1000,
        "O(1) lookup should be < 1µs, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_50_clips_time_lookup() {
    let state = create_timeline_with_clips(50);

    // Measure O(log n) time-based lookup
    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let time = (i % 250) as f64; // Various time points
        let _ = state.find_clip_at_time(time);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("50 clips - O(log n) time lookup: {} ns avg", avg_ns);

    // Assert average lookup < 5µs for O(log n) across tracks
    assert!(
        avg_ns < 5000,
        "O(log n) time lookup should be < 5µs, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_50_clips_invariants() {
    let state = create_timeline_with_clips(50);

    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = state.validate_invariants();
    }

    let elapsed = start.elapsed();
    let avg_us = elapsed.as_micros() / iterations as u128;

    println!("50 clips - invariant validation: {} µs avg", avg_us);

    // Assert invariant validation < 1ms per call
    assert!(
        avg_us < 1000,
        "50 clip invariant validation should be < 1ms, got {} µs",
        avg_us
    );
}

// =============================================================================
// TARGET: 500 CLIPS
// =============================================================================

#[test]
fn perf_500_clips_lookup() {
    let state = create_timeline_with_clips(500);

    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let clip_id = format!("clip-{}", i % 500);
        let _ = state.get_clip_by_id(&clip_id);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("500 clips - O(1) lookup: {} ns avg", avg_ns);

    // Assert average lookup < 1µs (1000ns)
    assert!(
        avg_ns < 1000,
        "O(1) lookup should be < 1µs at 500 clips, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_500_clips_time_lookup() {
    let state = create_timeline_with_clips(500);

    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let time = (i % 2500) as f64;
        let _ = state.find_clip_at_time(time);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("500 clips - O(log n) time lookup: {} ns avg", avg_ns);

    // O(log n) with 500 clips should still be fast
    assert!(
        avg_ns < 10000,
        "O(log n) time lookup should be < 10µs at 500 clips, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_500_clips_invariants() {
    let state = create_timeline_with_clips(500);

    let start = Instant::now();
    let iterations = 100;

    for _ in 0..iterations {
        let _ = state.validate_invariants();
    }

    let elapsed = start.elapsed();
    let avg_us = elapsed.as_micros() / iterations as u128;

    println!("500 clips - invariant validation: {} µs avg", avg_us);

    // Assert invariant validation < 10ms per call
    assert!(
        avg_us < 10000,
        "500 clip invariant validation should be < 10ms, got {} µs",
        avg_us
    );
}

// =============================================================================
// STRETCH: 5000 CLIPS
// =============================================================================

#[test]
fn perf_5000_clips_lookup() {
    let state = create_timeline_with_clips(5000);

    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let clip_id = format!("clip-{}", i % 5000);
        let _ = state.get_clip_by_id(&clip_id);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("5000 clips - O(1) lookup: {} ns avg", avg_ns);

    // O(1) lookup should remain fast regardless of scale
    assert!(
        avg_ns < 1000,
        "O(1) lookup should be < 1µs at 5000 clips, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_5000_clips_time_lookup() {
    let state = create_timeline_with_clips(5000);

    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let time = (i % 25000) as f64;
        let _ = state.find_clip_at_time(time);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / iterations as u128;

    println!("5000 clips - O(log n) time lookup: {} ns avg", avg_ns);

    // O(log 5000) ≈ 12 operations, should still be < 20µs
    assert!(
        avg_ns < 20000,
        "O(log n) time lookup should be < 20µs at 5000 clips, got {} ns",
        avg_ns
    );
}

#[test]
fn perf_5000_clips_invariants() {
    let state = create_timeline_with_clips(5000);

    let start = Instant::now();
    let iterations = 10;

    for _ in 0..iterations {
        let _ = state.validate_invariants();
    }

    let elapsed = start.elapsed();
    let single_ms = elapsed.as_millis() / iterations as u128;

    println!("5000 clips - invariant validation: {} ms", single_ms);

    // Assert invariant validation < 50ms at 5000 clips
    assert!(
        single_ms < 50,
        "5000 clip invariant validation should be < 50ms, got {} ms",
        single_ms
    );
}

// =============================================================================
// FUNCTIONALITY TESTS
// =============================================================================

#[test]
fn test_index_consistency_after_operations() {
    let mut state = TimelineState::new();

    // Add clips
    state.add_clip(make_clip("a", "track_1", 0.0, 5.0));
    state.add_clip(make_clip("b", "track_1", 10.0, 5.0));
    state.add_clip(make_clip("c", "track_1", 20.0, 5.0));

    // Verify initial lookups
    assert!(state.get_clip_by_id("a").is_some());
    assert!(state.get_clip_by_id("b").is_some());
    assert!(state.get_clip_by_id("c").is_some());

    // Remove middle clip
    let removed = state.remove_clip("b");
    assert!(removed.is_some());

    // Verify b is gone but a and c still work
    assert!(state.get_clip_by_id("a").is_some());
    assert!(state.get_clip_by_id("b").is_none());
    assert!(state.get_clip_by_id("c").is_some());

    // Verify time-based lookup still works
    assert!(state.find_clip_at_time(2.0).is_some()); // Should find clip "a"
    assert!(state.find_clip_at_time(12.0).is_none()); // Clip "b" removed
    assert!(state.find_clip_at_time(22.0).is_some()); // Should find clip "c"
}

#[test]
fn test_adjacent_clips_lookup() {
    let mut state = TimelineState::new();

    state.add_clip(make_clip("first", "track_1", 0.0, 5.0));
    state.add_clip(make_clip("middle", "track_1", 5.0, 5.0));
    state.add_clip(make_clip("last", "track_1", 10.0, 5.0));

    let (prev, next) = state.find_adjacent_clips("middle");
    assert_eq!(prev.map(|c| &c.id), Some(&"first".to_string()));
    assert_eq!(next.map(|c| &c.id), Some(&"last".to_string()));

    let (prev, next) = state.find_adjacent_clips("first");
    assert!(prev.is_none());
    assert_eq!(next.map(|c| &c.id), Some(&"middle".to_string()));

    let (prev, next) = state.find_adjacent_clips("last");
    assert_eq!(prev.map(|c| &c.id), Some(&"middle".to_string()));
    assert!(next.is_none());
}

#[test]
fn test_overlap_detection() {
    let mut state = TimelineState::new();

    state.add_clip(make_clip("existing", "track_1", 5.0, 5.0)); // 5-10

    // Should overlap
    assert!(state.would_overlap("track_1", 4.0, 2.0)); // 4-6 overlaps
    assert!(state.would_overlap("track_1", 7.0, 2.0)); // 7-9 overlaps
    assert!(state.would_overlap("track_1", 4.0, 10.0)); // 4-14 overlaps

    // Should not overlap
    assert!(!state.would_overlap("track_1", 0.0, 4.0)); // 0-4 doesn't overlap
    assert!(!state.would_overlap("track_1", 11.0, 5.0)); // 11-16 doesn't overlap
    assert!(!state.would_overlap("track_2", 5.0, 5.0)); // Different track
}

#[test]
fn test_rebuild_indices() {
    let mut state = TimelineState::new();

    // Manually add clips without using add_clip (simulating deserialization)
    state.clips.push(make_clip("a", "track_1", 0.0, 5.0));
    state.clips.push(make_clip("b", "track_1", 10.0, 5.0));

    // Indices should be empty
    assert!(state.get_clip_by_id("a").is_none());

    // Rebuild indices
    state.rebuild_indices();

    // Now lookups should work
    assert!(state.get_clip_by_id("a").is_some());
    assert!(state.get_clip_by_id("b").is_some());
    assert!(state.find_clip_at_time(2.0).is_some());
}

// =============================================================================
// ACTION EXECUTION PERFORMANCE TESTS
// =============================================================================

#[test]
fn perf_single_action_delete_5000_clips() {
    use ghost_lib::action_preflight::ActionPreflight;
    use ghost_lib::edit_plan::{ActionType, EditAction, EditPlan};

    let state = create_timeline_with_clips(5000);

    // Test DELETE action performance
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Delete,
            target_clip_id: "clip-2500".to_string(), // Middle clip
            parameters: None,
        }],
        thought_process: None,
        confidence: None,
    };

    let start = Instant::now();
    let iterations = 100;

    for _ in 0..iterations {
        let _ = ActionPreflight::preflight_plan(&plan, &state);
    }

    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() / iterations as u128;

    println!("5000 clips - DELETE action: {} ms avg", avg_ms);

    // Assert < 20ms per action
    assert!(
        avg_ms < 20,
        "DELETE action should be < 20ms at 5000 clips, got {} ms",
        avg_ms
    );
}

#[test]
fn perf_single_action_move_5000_clips() {
    use ghost_lib::action_preflight::ActionPreflight;
    use ghost_lib::edit_plan::{ActionParameters, ActionType, EditAction, EditPlan};

    let state = create_timeline_with_clips(5000);

    // Test MOVE action performance
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Move,
            target_clip_id: "clip-2500".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: None,
                new_start_time: Some(50000.0), // Move far away
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let start = Instant::now();
    let iterations = 100;

    for _ in 0..iterations {
        let _ = ActionPreflight::preflight_plan(&plan, &state);
    }

    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() / iterations as u128;

    println!("5000 clips - MOVE action: {} ms avg", avg_ms);

    // Assert < 20ms per action
    assert!(
        avg_ms < 20,
        "MOVE action should be < 20ms at 5000 clips, got {} ms",
        avg_ms
    );
}

#[test]
fn perf_single_action_trim_5000_clips() {
    use ghost_lib::action_preflight::ActionPreflight;
    use ghost_lib::edit_plan::{ActionParameters, ActionType, EditAction, EditPlan};

    let state = create_timeline_with_clips(5000);

    // Test TRIM action performance
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-2500".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: Some(1.0),
                trim_end_delta: Some(-0.5),
                split_time: None,
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let start = Instant::now();
    let iterations = 100;

    for _ in 0..iterations {
        let _ = ActionPreflight::preflight_plan(&plan, &state);
    }

    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() / iterations as u128;

    println!("5000 clips - TRIM action: {} ms avg", avg_ms);

    // Assert < 20ms per action
    assert!(
        avg_ms < 20,
        "TRIM action should be < 20ms at 5000 clips, got {} ms",
        avg_ms
    );
}

#[test]
fn perf_multi_action_plan_5000_clips() {
    use ghost_lib::action_preflight::ActionPreflight;
    use ghost_lib::edit_plan::{ActionParameters, ActionType, EditAction, EditPlan};

    let state = create_timeline_with_clips(5000);

    // Test multi-action plan performance
    let plan = EditPlan {
        actions: vec![
            EditAction {
                action_type: ActionType::Trim,
                target_clip_id: "clip-1000".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: Some(0.5),
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: None,
                }),
            },
            EditAction {
                action_type: ActionType::Move,
                target_clip_id: "clip-2000".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: Some(40000.0),
                }),
            },
            EditAction {
                action_type: ActionType::Delete,
                target_clip_id: "clip-3000".to_string(),
                parameters: None,
            },
            EditAction {
                action_type: ActionType::Trim,
                target_clip_id: "clip-4000".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: Some(-1.0),
                    split_time: None,
                    new_start_time: None,
                }),
            },
        ],
        thought_process: None,
        confidence: None,
    };

    let start = Instant::now();
    let iterations = 50;

    for _ in 0..iterations {
        let _ = ActionPreflight::preflight_plan(&plan, &state);
    }

    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() / iterations as u128;

    println!("5000 clips - 4-action plan: {} ms avg", avg_ms);

    // Assert < 100ms for 4-action plan
    assert!(
        avg_ms < 100,
        "4-action plan should be < 100ms at 5000 clips, got {} ms",
        avg_ms
    );
}
