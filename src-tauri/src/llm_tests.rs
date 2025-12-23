#[cfg(test)]
mod tests {
    use crate::llm::parse_edit_plan;

    #[test]
    fn test_parse_clean_json() {
        let input = r#"
        {
            "thought_process": "Delete the first clip",
            "actions": [
                {
                    "type": "DELETE",
                    "target_clip_id": "abc-123",
                    "parameters": null
                }
            ]
        }
        "#;
        let plan = parse_edit_plan(input).expect("Failed to parse clean JSON");
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].is_delete());
    }

    #[test]
    fn test_parse_markdown_json() {
        let input = r#"
        Here is the plan:
        ```json
        {
            "thought_process": "Delete the first clip",
            "actions": [
                {
                    "type": "DELETE",
                    "target_clip_id": "abc-123"
                }
            ]
        }
        ```
        Hope this helps!
        "#;
        let plan = parse_edit_plan(input).expect("Failed to parse markdown JSON");
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn test_parse_nested_braces() {
        let input = r#"
        {
            "thought_process": "I will delete the clip with id {abc-123}",
            "actions": []
        }
        "#;
        let plan = parse_edit_plan(input).expect("Failed to parse nested braces");
        assert_eq!(plan.actions.len(), 0);
        assert_eq!(
            plan.thought_process.unwrap(),
            "I will delete the clip with id {abc-123}"
        );
    }
}
