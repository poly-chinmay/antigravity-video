// tests/invariants_tests.rs
//! Tests for Antigravity Invariants
//!
//! These tests verify that validate_invariants() correctly
//! enforces the constitutional rules of the timeline engine.

use ghost_lib::timeline::{Clip, ProjectSettings, TimelineState};

/// Helper to create a valid test clip
fn make_clip(id: &str, start: f64, duration: f64) -> Clip {
    Clip {
        id: id.to_string(),
        track_id: "video_track_1".to_string(),
        start,
        duration,
        source_file: "/test.mp4".to_string(),
    }
}

/// Helper to create a valid baseline state
fn make_valid_state() -> TimelineState {
    let mut state = TimelineState {
        clips: vec![make_clip("clip-1", 0.0, 5.0), make_clip("clip-2", 5.0, 5.0)],
        duration: 10.0,
        project_duration: 10.0,
        playhead_time: 0.0,
        version: 1,
        settings: ProjectSettings::default(),
        ..TimelineState::default()
    };
    state.rebuild_indices();
    state
}

// =============================================================================
// PASSING TESTS - Valid states should pass all invariants
// =============================================================================

#[test]
fn invariant_valid_timeline_passes() {
    let state = make_valid_state();
    let result = state.validate_invariants();
    assert!(result.is_ok(), "Valid timeline should pass: {:?}", result);
}

#[test]
fn invariant_empty_timeline_passes() {
    let state = TimelineState::new();
    let result = state.validate_invariants();
    assert!(result.is_ok(), "Empty timeline should pass: {:?}", result);
}

#[test]
fn invariant_project_duration_exceeds_content_passes() {
    let mut state = make_valid_state();
    state.project_duration = 20.0; // Exceeds content duration of 10s
    let result = state.validate_invariants();
    assert!(
        result.is_ok(),
        "project_duration > content is valid: {:?}",
        result
    );
}

// =============================================================================
// FAILING TESTS - Invalid states should be rejected
// =============================================================================

#[test]
fn duplicate_clip_id_fails() {
    let mut state = make_valid_state();
    // Add a clip with duplicate ID
    state.clips.push(make_clip("clip-1", 10.0, 5.0)); // Same ID as first clip

    let result = state.validate_invariants();
    assert!(result.is_err(), "Duplicate ClipId should fail");
    assert!(
        result.unwrap_err().contains("Duplicate ClipId"),
        "Error should mention duplicate"
    );
}

#[test]
fn negative_duration_fails() {
    let mut state = make_valid_state();
    state.clips[0].duration = -5.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Negative duration should fail");
    assert!(
        result.unwrap_err().contains("invalid duration"),
        "Error should mention invalid duration"
    );
}

#[test]
fn zero_duration_fails() {
    let mut state = make_valid_state();
    state.clips[0].duration = 0.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Zero duration should fail");
}

#[test]
fn negative_start_fails() {
    let mut state = make_valid_state();
    state.clips[0].start = -1.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Negative start should fail");
    assert!(
        result.unwrap_err().contains("negative start"),
        "Error should mention negative start"
    );
}

#[test]
fn playhead_out_of_bounds_fails() {
    let mut state = make_valid_state();
    state.playhead_time = 15.0; // Beyond project_duration of 10s

    let result = state.validate_invariants();
    assert!(result.is_err(), "Playhead beyond duration should fail");
    assert!(
        result.unwrap_err().contains("playhead_time"),
        "Error should mention playhead"
    );
}

#[test]
fn negative_playhead_fails() {
    let mut state = make_valid_state();
    state.playhead_time = -1.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Negative playhead should fail");
}

#[test]
fn project_duration_less_than_content_fails() {
    let mut state = make_valid_state();
    state.project_duration = 5.0; // Less than content duration of 10s

    let result = state.validate_invariants();
    assert!(result.is_err(), "project_duration < content should fail");
    assert!(
        result.unwrap_err().contains("project_duration"),
        "Error should mention project_duration"
    );
}

#[test]
fn invalid_frame_rate_fails() {
    let mut state = make_valid_state();
    state.settings.timeline_frame_rate = 0.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Zero frame rate should fail");
    assert!(
        result.unwrap_err().contains("timeline_frame_rate"),
        "Error should mention frame rate"
    );
}

#[test]
fn negative_frame_rate_fails() {
    let mut state = make_valid_state();
    state.settings.timeline_frame_rate = -30.0;

    let result = state.validate_invariants();
    assert!(result.is_err(), "Negative frame rate should fail");
}
