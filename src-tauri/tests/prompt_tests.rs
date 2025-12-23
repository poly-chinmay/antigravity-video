#[cfg(test)]
mod tests {
    use ghost_lib::llm::is_valid_uuid;
    use ghost_lib::prompt::{build_prompt, simplify_timeline_for_prompt};
    use ghost_lib::timeline::{Clip, TimelineEngine};
    use uuid::Uuid;

    #[test]
    fn test_simplify_timeline_structure() {
        let engine = TimelineEngine::new();
        let id1 = Uuid::new_v4().to_string();
        let id2 = Uuid::new_v4().to_string();
        let id3 = Uuid::new_v4().to_string();

        {
            let mut state = engine.state.lock().unwrap();
            state.clips.push(Clip {
                id: id1.clone(),
                track_id: "video_track_1".to_string(),
                start: 0.0,
                duration: 5.5,
                source_file: "/path/1.mp4".to_string(),
            });
            state.clips.push(Clip {
                id: id2.clone(),
                track_id: "video_track_1".to_string(),
                start: 5.5,
                duration: 3.2,
                source_file: "/path/2.mp4".to_string(),
            });
            state.clips.push(Clip {
                id: id3.clone(),
                track_id: "audio_track_1".to_string(),
                start: 0.0,
                duration: 10.0,
                source_file: "/path/3.mp3".to_string(),
            });
        }

        let state = engine.state.lock().unwrap();
        let simplified = simplify_timeline_for_prompt(&state, 50);

        assert_eq!(simplified.len(), 3);

        // Check types and values
        assert_eq!(simplified[0].id, id1);
        assert_eq!(simplified[0].timeline_start, 0.0);
        assert_eq!(simplified[0].duration, 5.5);
        assert_eq!(simplified[0].track_id.as_deref(), Some("video_track_1"));

        assert_eq!(simplified[1].id, id2);
        assert_eq!(simplified[1].timeline_start, 5.5);

        assert_eq!(simplified[2].id, id3);
        assert_eq!(simplified[2].duration, 10.0);
    }

    #[test]
    fn test_build_prompt_contains_json_context() {
        let engine = TimelineEngine::new();
        let id = Uuid::new_v4().to_string();
        {
            let mut state = engine.state.lock().unwrap();
            state.clips.push(Clip {
                id: id.clone(),
                track_id: "v1".to_string(),
                start: 10.0,
                duration: 4.0,
                source_file: "foo.mp4".to_string(),
            });
        }

        let prefs = ghost_lib::preferences::PreferenceManager::new_in_memory();
        let prompt = build_prompt(&engine, &prefs, "Trim the clip");

        // Check for JSON structure
        assert!(prompt.contains("\"timeline_context\""));
        assert!(prompt.contains(&id));
        assert!(prompt.contains("\"timeline_start\":10.0"));
        assert!(prompt.contains("\"duration\":4.0"));

        // Check for System Prompt rules
        assert!(prompt.contains("IMPORTANT: All timing values must be in seconds"));
    }

    #[test]
    fn test_is_valid_uuid() {
        let valid = Uuid::new_v4().to_string();
        assert!(is_valid_uuid(&valid));

        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("12345"));
    }

    #[test]
    fn test_empty_timeline_prompt() {
        let engine = TimelineEngine::new();
        let prefs = ghost_lib::preferences::PreferenceManager::new_in_memory();
        let prompt = build_prompt(&engine, &prefs, "Hello");
        assert!(prompt.contains("NOTE: timeline contains 0 clips."));
    }
}
