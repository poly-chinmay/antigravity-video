use crate::preferences::{PreferenceManager, UserPreferences};
use crate::timeline::TimelineEngine;
use serde::Serialize;
use serde_json::json;

#[derive(Serialize, Debug)]
pub struct SimplifiedClip {
    pub id: String,
    pub timeline_start: f64, // seconds
    pub duration: f64,       // seconds
    pub track_id: Option<String>,
}

pub fn simplify_timeline_for_prompt(
    state: &crate::timeline::TimelineState,
    max_clips: usize,
) -> Vec<SimplifiedClip> {
    state
        .clips
        .iter()
        .take(max_clips)
        .map(|c| SimplifiedClip {
            id: c.id.clone(),
            timeline_start: c.start,
            duration: c.duration,
            track_id: Some(c.track_id.clone()),
        })
        .collect()
}

// Helper to summarize preferences for the AI
fn format_preference_context(prefs: &UserPreferences) -> String {
    let mut summary = String::new();

    // 1. General Settings
    summary.push_str(&format!(
        "USER PREFERENCES:\n- Default Transition Duration: {:.1}s\n- Auto-Ripple Edits: {}\n",
        prefs.general.default_transition_duration, prefs.general.auto_ripple_edits
    ));

    // 2. Interaction History Analysis
    let total_interactions = prefs.interactions.len();
    if total_interactions > 0 {
        let recent_count = 10;
        let recent_events = prefs.interactions.iter().rev().take(recent_count);

        let mut manual_moves = 0;
        let mut manual_trims = 0;
        let mut ai_edits = 0;

        for event in recent_events {
            match event.event_type.as_str() {
                "MANUAL_MOVE" => manual_moves += 1,
                "MANUAL_TRIM" => manual_trims += 1,
                "AI_EDIT_APPLIED" => ai_edits += 1,
                _ => {}
            }
        }

        summary.push_str(&format!(
            "- Recent Activity (last {}): {} AI Edits, {} Manual Moves, {} Manual Trims.\n",
            std::cmp::min(total_interactions, recent_count),
            ai_edits,
            manual_moves,
            manual_trims
        ));
    } else {
        summary.push_str("- No prior interaction history.\n");
    }

    summary
}

pub const SYSTEM_PROMPT: &str = r#"
You are "Ghost", an intelligent video editing assistant.
Your goal is to interpret natural language instructions into a JSON EditPlan based on the provided timeline context.

[PREFERENCE_CONTEXT]
{{PREFERENCE_CONTEXT}}

TIMELINE CONTEXT:
The user will provide a JSON representation of the current timeline state.
You must use the exact Clip IDs provided in the context. Do not invent IDs.

OUTPUT FORMAT:
You must output ONLY a valid JSON object matching this structure:
{
  "thought_process": "Brief explanation of your interpretation (optional)",
  "confidence": 0.0-1.0,
  "actions": [
    {
      "type": "DELETE", // ONLY: "DELETE", "MOVE", "TRIM", "SPLIT"
      "target_clip_id": "uuid-string",
      "parameters": {
        // "new_start_time": float (for MOVE)
        // "trim_start_delta": float (for TRIM, negative to shorten from start)
        // "trim_end_delta": float (for TRIM, negative to shorten from end)
        // "split_time": float (for SPLIT)
      }
    }
  ]
}

CRITICAL RULES:
1. No text outside JSON.
2. No trailing comments.
3. If you are unsure, return an empty actions array with confidence < 0.5.
4. SPLIT Rule: You may NOT reference or modify the newly created clip in the same plan.
5. UNSUPPORTED ACTIONS: "Speed", "Merge", "Color", "Effect", "Export". Return empty actions if requested.

EDITORIAL DISCIPLINE (VERY IMPORTANT):
6. PREFER TRIM over DELETE when the user wants to shorten content.
7. NEVER delete more than 2 clips in one plan unless explicitly asked ("delete all", "remove everything").
8. AVOID micro-edits: Do NOT trim less than 0.3 seconds unless explicitly requested.
9. When uncertain, explain your uncertainty in thought_process and set confidence < 0.6.

SELF-CHECK (MANDATORY):
Before outputting an EditPlan, verify:
- All target_clip_id values exist in the provided timeline_context
- All timing values are within clip boundaries
- The plan matches the user's apparent intent
If any check fails, output an empty actions array and explain why in thought_process.

CONFIDENCE SCORING:
- 0.9-1.0: Clear instruction, exact match to context
- 0.7-0.8: Reasonable interpretation, minor assumptions made
- 0.5-0.6: Ambiguous instruction, best-effort guess
- 0.0-0.4: Very unclear, likely wrong - use empty actions instead

EXAMPLES:

Input: "Delete the first clip"
Context: [{"id": "abc-123", "timeline_start": 0.0, ...}]
Output:
{
  "thought_process": "User wants to remove the clip at position 0",
  "confidence": 0.95,
  "actions": [
    { "type": "DELETE", "target_clip_id": "abc-123" }
  ]
}

Input: "Trim 2 seconds from the end"
Context: [{"id": "xyz-789", "timeline_start": 5.0, "duration": 10.0}]
Output:
{
  "thought_process": "Trimming end of the only clip by -2s",
  "confidence": 0.9,
  "actions": [
    { "type": "TRIM", "target_clip_id": "xyz-789", "parameters": { "trim_end_delta": -2.0 } }
  ]
}
"#;

pub fn build_context_block(engine: &TimelineEngine) -> String {
    let state = engine.state.lock().unwrap();
    let max_clips = 50;

    // 1. Simplify Context
    let simplified = simplify_timeline_for_prompt(&state, max_clips);

    // 2. Log to console
    println!(
        "Sending timeline context: {}",
        serde_json::to_string(&simplified).unwrap_or_default()
    );

    let timeline_context_json = json!({
        "timeline_context": simplified
    });

    let mut context_str =
        serde_json::to_string(&timeline_context_json).unwrap_or_else(|_| "{}".to_string());

    // Handle empty timeline case explicitly
    if state.clips.is_empty() {
        context_str = "NOTE: timeline contains 0 clips.".to_string();
    } else if state.clips.len() > max_clips {
        let omitted = state.clips.len() - max_clips;
        context_str = format!("NOTE: {} clips omitted.\n{}", omitted, context_str);
    }

    format!("TIMELINE_CONTEXT:\n{}", context_str)
}

pub fn build_prompt(
    engine: &TimelineEngine,
    prefs: &PreferenceManager,
    user_input: &str,
) -> String {
    // 1. Get Preference Context
    let user_prefs = prefs.get_preferences();
    let pref_context_str = format_preference_context(&user_prefs);

    // 2. Inject into System Prompt
    let system_prompt_with_prefs =
        SYSTEM_PROMPT.replace("{{PREFERENCE_CONTEXT}}", &pref_context_str);

    // 3. Build Timeline Context
    let context_block = build_context_block(engine);

    // 4. Combine
    format!(
        "{}\n\n{}\n\nUSER:\n\"{}\"\n",
        system_prompt_with_prefs, context_block, user_input
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{Clip, TimelineEngine};

    #[test]
    fn test_simplify_timeline() {
        let engine = TimelineEngine::new();
        {
            let mut state = engine.state.lock().unwrap();
            state.clips.push(Clip {
                id: "test-id-1".to_string(),
                track_id: "v1".to_string(),
                start: 0.0,
                duration: 5.0,
                source_file: "/path/1.mp4".to_string(),
            });
        }

        let state = engine.state.lock().unwrap();
        let simplified = simplify_timeline_for_prompt(&state, 10);
        assert_eq!(simplified.len(), 1);
        assert_eq!(simplified[0].id, "test-id-1");
        assert_eq!(simplified[0].timeline_start, 0.0);
        assert_eq!(simplified[0].duration, 5.0);
        assert_eq!(simplified[0].track_id.as_deref(), Some("v1"));
    }
}
