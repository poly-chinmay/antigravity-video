#[cfg(test)]
mod tests {
    use ghost_lib::action_router::validate_state_invariants;
    use ghost_lib::edit_plan::{ActionType, EditPlan};
    use ghost_lib::llm::parse_edit_plan;
    use ghost_lib::timeline::{Clip, TimelineState};
    use ghost_lib::validator::{validate_actions_against_state, Action};

    // Mocking State is hard in integration tests without full app setup.
    // We will test the components that *would* be called by the command.

    #[test]
    fn test_parse_valid_plan() {
        let json = r#"
        {
            "thought_process": "Deleting the bad clip",
            "actions": [
                {
                    "type": "DELETE",
                    "target_clip_id": "123"
                }
            ]
        }
        "#;
        let plan = parse_edit_plan(json).expect("Failed to parse valid plan");
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type, ActionType::Delete);
        assert_eq!(plan.actions[0].target_clip_id, "123");
    }

    #[test]
    fn test_validation_logic() {
        // Test Empty Plan
        let _empty_plan = EditPlan {
            thought_process: Some("Nothing".to_string()),
            actions: vec![],
            confidence: None,
        };

        let state = TimelineState {
            clips: vec![],
            duration: 0.0,
            playhead_time: 0.0,
            version: 0,
        };
        let actions = vec![Action::DeleteClip {
            id: "missing".to_string(),
        }];

        let result = validate_actions_against_state(&actions, &state);
        assert!(result.is_err());
    }

    // =========================================================================
    // IMPOSSIBLE STATE TESTS
    // These verify that validate_state_invariants rejects corrupted states.
    // =========================================================================

    #[test]
    fn test_impossible_state_negative_duration() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: -5.0, // INVALID: negative duration
                source_file: "/test.mp4".to_string(),
            }],
            duration: 0.0,
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject negative duration clip");
    }

    #[test]
    fn test_impossible_state_zero_duration() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: 0.0, // INVALID: zero duration
                source_file: "/test.mp4".to_string(),
            }],
            duration: 0.0,
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject zero duration clip");
    }

    #[test]
    fn test_impossible_state_negative_start() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: -1.0, // INVALID: negative start
                duration: 5.0,
                source_file: "/test.mp4".to_string(),
            }],
            duration: 4.0,
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject negative start time");
    }

    #[test]
    fn test_impossible_state_overlapping_clips() {
        let state = TimelineState {
            clips: vec![
                Clip {
                    id: "clip1".to_string(),
                    track_id: "v1".to_string(),
                    start: 0.0,
                    duration: 10.0, // Ends at 10s
                    source_file: "/test.mp4".to_string(),
                },
                Clip {
                    id: "clip2".to_string(),
                    track_id: "v1".to_string(), // SAME track
                    start: 5.0,                 // INVALID: Starts at 5s, overlaps clip1
                    duration: 10.0,
                    source_file: "/test2.mp4".to_string(),
                },
            ],
            duration: 15.0,
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(
            result.is_err(),
            "Should reject overlapping clips on same track"
        );
    }

    #[test]
    fn test_impossible_state_playhead_beyond_duration() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: 10.0,
                source_file: "/test.mp4".to_string(),
            }],
            duration: 10.0,
            playhead_time: 15.0, // INVALID: beyond duration
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject playhead beyond duration");
    }

    #[test]
    fn test_impossible_state_negative_playhead() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: 10.0,
                source_file: "/test.mp4".to_string(),
            }],
            duration: 10.0,
            playhead_time: -5.0, // INVALID: negative playhead
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject negative playhead");
    }

    #[test]
    fn test_impossible_state_duration_mismatch() {
        let state = TimelineState {
            clips: vec![Clip {
                id: "clip1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: 10.0, // Clip ends at 10s
                source_file: "/test.mp4".to_string(),
            }],
            duration: 5.0, // INVALID: should be 10.0
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_err(), "Should reject duration mismatch");
    }

    #[test]
    fn test_valid_state_passes() {
        let state = TimelineState {
            clips: vec![
                Clip {
                    id: "clip1".to_string(),
                    track_id: "v1".to_string(),
                    start: 0.0,
                    duration: 5.0,
                    source_file: "/test.mp4".to_string(),
                },
                Clip {
                    id: "clip2".to_string(),
                    track_id: "v1".to_string(),
                    start: 5.0, // Starts exactly where clip1 ends
                    duration: 5.0,
                    source_file: "/test2.mp4".to_string(),
                },
            ],
            duration: 10.0,
            playhead_time: 3.0, // Valid: within [0, 10]
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_ok(), "Valid state should pass all invariants");
    }

    #[test]
    fn test_empty_timeline_valid() {
        let state = TimelineState {
            clips: vec![],
            duration: 0.0,
            playhead_time: 0.0,
            version: 0,
        };
        let result = validate_state_invariants(&state);
        assert!(result.is_ok(), "Empty timeline should be valid");
    }
}
