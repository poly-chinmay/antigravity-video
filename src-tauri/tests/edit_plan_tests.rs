#[cfg(test)]
mod tests {
    use ghost_lib::edit_plan::{ActionType, EditPlan};
    use ghost_lib::llm::parse_edit_plan;
    use ghost_lib::timeline::{Clip, TimelineState};
    use ghost_lib::validator::{validate_actions_against_state, validate_plan, Action};
    use tauri::State;

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
        // 1. Test Empty Plan
        let empty_plan = EditPlan {
            thought_process: Some("Nothing".to_string()),
            actions: vec![],
            confidence: None,
        };
        // We can't call validate_plan easily because it needs State, but we can check the logic if we extracted it.
        // For now, let's trust the unit tests in validator.rs for the logic.
        // But we can test the validator::validate_actions_against_state which is the core logic.

        let state = TimelineState {
            clips: vec![],
            duration: 0.0,
        };
        let actions = vec![Action::DeleteClip {
            id: "missing".to_string(),
        }];

        let result = validate_actions_against_state(&actions, &state);
        assert!(result.is_err());
    }
}
