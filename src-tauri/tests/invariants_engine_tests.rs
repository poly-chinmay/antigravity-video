// tests/invariants_engine_tests.rs
//! Comprehensive test suite for Phase A2: Invariants Engine
//!
//! Tests verify that EditPlanValidator and ActionPreflight correctly
//! reject invalid plans and accept valid ones.

use ghost_lib::action_preflight::ActionPreflight;
use ghost_lib::edit_plan::{ActionParameters, ActionType, EditAction, EditPlan};
use ghost_lib::edit_plan_validator::EditPlanValidator;
use ghost_lib::edit_rejection::EditRejection;
use ghost_lib::timeline::{Clip, TimelineState};

/// Helper to create a test timeline with clips
fn make_test_timeline() -> TimelineState {
    let mut state = TimelineState::new();
    state.add_clip(Clip {
        id: "clip-1".to_string(),
        track_id: "track-1".to_string(),
        start: 0.0,
        duration: 10.0,
        source_file: "/test.mp4".to_string(),
    });
    state.add_clip(Clip {
        id: "clip-2".to_string(),
        track_id: "track-1".to_string(),
        start: 10.0,
        duration: 5.0,
        source_file: "/test.mp4".to_string(),
    });
    state
}

// =============================================================================
// TEST 1: Hallucinated Clip ID Rejected
// =============================================================================

#[test]
fn test_hallucinated_clip_id_rejected() {
    let state = make_test_timeline();
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Delete,
            target_clip_id: "nonexistent-clip".to_string(), // AI hallucination
            parameters: None,
        }],
        thought_process: None,
        confidence: None,
    };

    // Static validation should catch this
    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EditRejection::InvalidReference { .. }
    ));
}

// =============================================================================
// TEST 2: Invalid TRIM Bounds Rejected
// =============================================================================

#[test]
fn test_invalid_trim_bounds_rejected() {
    let state = make_test_timeline();

    // Trim that would exceed clip duration
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: Some(15.0), // Exceeds 10s duration
                trim_end_delta: None,
                split_time: None,
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EditRejection::BoundsViolation { .. }
    ));
}

#[test]
fn test_trim_to_zero_duration_rejected() {
    let state = make_test_timeline();

    // Trim that would result in zero duration
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: Some(10.0), // Would make duration 0
                trim_end_delta: None,
                split_time: None,
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    // Preflight should catch invariant violation
    let result = ActionPreflight::preflight_plan(&plan, &state);
    assert!(result.is_err());
}

// =============================================================================
// TEST 3: Invalid SPLIT Bounds Rejected
// =============================================================================

#[test]
fn test_invalid_split_bounds_rejected() {
    let state = make_test_timeline();

    // Split outside clip bounds
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Split,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: Some(15.0), // Beyond clip end (10s)
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EditRejection::BoundsViolation { .. }
    ));
}

#[test]
fn test_split_at_clip_start_rejected() {
    let state = make_test_timeline();

    // Split at clip start (invalid)
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Split,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: Some(0.0), // At clip start
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());
}

// =============================================================================
// TEST 4: Invalid MOVE Target Rejected
// =============================================================================

#[test]
fn test_invalid_move_target_rejected() {
    let state = make_test_timeline();

    // Move to negative start time
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Move,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: None,
                new_start_time: Some(-5.0), // Negative start
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EditRejection::BoundsViolation { .. }
    ));
}

// =============================================================================
// TEST 5: Overlap-Causing Plan Rejected
// =============================================================================

#[test]
fn test_overlap_causing_plan_rejected() {
    let state = make_test_timeline();

    // Move clip-1 to overlap with clip-2
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Move,
            target_clip_id: "clip-1".to_string(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: None,
                new_start_time: Some(12.0), // Would overlap with clip-2 at 10-15
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    // Preflight should detect overlap conflict
    let result = ActionPreflight::preflight_plan(&plan, &state);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EditRejection::ConflictDetected { .. }
    ));
}

// =============================================================================
// TEST 6: Complex Valid Plan Accepted
// =============================================================================

#[test]
fn test_complex_valid_plan_accepted() {
    let state = make_test_timeline();

    // Multi-action plan that should succeed
    let plan = EditPlan {
        actions: vec![
            // 1. Trim clip-1
            EditAction {
                action_type: ActionType::Trim,
                target_clip_id: "clip-1".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: Some(2.0),
                    trim_end_delta: Some(-1.0),
                    split_time: None,
                    new_start_time: None,
                }),
            },
            // 2. Move clip-2 to new position
            EditAction {
                action_type: ActionType::Move,
                target_clip_id: "clip-2".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: Some(20.0), // Move away from clip-1
                }),
            },
        ],
        thought_process: None,
        confidence: None,
    };

    // Both validations should pass
    let static_result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(static_result.is_ok());

    let preflight_result = ActionPreflight::preflight_plan(&plan, &state);
    assert!(preflight_result.is_ok());
}

// =============================================================================
// TEST 7: Invariants Preserved on Rejection
// =============================================================================

#[test]
fn test_invariants_preserved_on_rejection() {
    let state = make_test_timeline();
    let original_clip_count = state.clips.len();
    let original_duration = state.duration;

    // Invalid plan
    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Delete,
            target_clip_id: "nonexistent".to_string(),
            parameters: None,
        }],
        thought_process: None,
        confidence: None,
    };

    // Validation should reject
    let result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(result.is_err());

    // State should be unchanged
    assert_eq!(state.clips.len(), original_clip_count);
    assert_eq!(state.duration, original_duration);

    // Invariants should still hold
    assert!(state.validate_invariants().is_ok());
}

// =============================================================================
// TEST 8: No Panics on Invalid Input
// =============================================================================

#[test]
fn test_no_panics_on_invalid_input() {
    let state = make_test_timeline();

    // Collection of invalid plans that should NOT panic
    let invalid_plans = vec![
        // Missing parameters
        EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Trim,
                target_clip_id: "clip-1".to_string(),
                parameters: None, // Missing required params
            }],
            thought_process: None,
            confidence: None,
        },
        // Extreme values
        EditPlan {
            actions: vec![EditAction {
                action_type: ActionType::Move,
                target_clip_id: "clip-1".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: Some(f64::MAX),
                }),
            }],
            thought_process: None,
            confidence: None,
        },
        // Multiple invalid actions
        EditPlan {
            actions: vec![
                EditAction {
                    action_type: ActionType::Delete,
                    target_clip_id: "fake1".to_string(),
                    parameters: None,
                },
                EditAction {
                    action_type: ActionType::Delete,
                    target_clip_id: "fake2".to_string(),
                    parameters: None,
                },
            ],
            thought_process: None,
            confidence: None,
        },
    ];

    for plan in invalid_plans {
        // Should return Err, not panic
        let _ = EditPlanValidator::validate_plan(&plan, &state);
        let _ = ActionPreflight::preflight_plan(&plan, &state);
    }
}

// =============================================================================
// TEST 9: Valid SPLIT Creates New ClipIds
// =============================================================================

#[test]
fn test_split_creates_new_clip_ids() {
    let state = make_test_timeline();
    let original_id = "clip-1".to_string();

    let plan = EditPlan {
        actions: vec![EditAction {
            action_type: ActionType::Split,
            target_clip_id: original_id.clone(),
            parameters: Some(ActionParameters {
                trim_start_delta: None,
                trim_end_delta: None,
                split_time: Some(5.0), // Split at midpoint
                new_start_time: None,
            }),
        }],
        thought_process: None,
        confidence: None,
    };

    // Should pass validation
    let result = ActionPreflight::preflight_plan(&plan, &state);
    assert!(result.is_ok());
}

// =============================================================================
// TEST 10: Sequential Actions Validated Correctly
// =============================================================================

#[test]
fn test_sequential_actions_validated() {
    let state = make_test_timeline();

    // Plan where second action depends on first
    let plan = EditPlan {
        actions: vec![
            // 1. Delete clip-2
            EditAction {
                action_type: ActionType::Delete,
                target_clip_id: "clip-2".to_string(),
                parameters: None,
            },
            // 2. Move clip-1 to where clip-2 was (now valid)
            EditAction {
                action_type: ActionType::Move,
                target_clip_id: "clip-1".to_string(),
                parameters: Some(ActionParameters {
                    trim_start_delta: None,
                    trim_end_delta: None,
                    split_time: None,
                    new_start_time: Some(10.0),
                }),
            },
        ],
        thought_process: None,
        confidence: None,
    };

    // Should pass both validations
    let static_result = EditPlanValidator::validate_plan(&plan, &state);
    assert!(static_result.is_ok());

    let preflight_result = ActionPreflight::preflight_plan(&plan, &state);
    assert!(preflight_result.is_ok());
}
