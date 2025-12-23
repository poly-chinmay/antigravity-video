// src-tauri/src/lib.rs

pub mod action_router;
pub mod commands;
pub mod edit_plan;
pub mod ffmpeg;
pub mod llm;
pub mod preferences;
pub mod prompt;
pub mod timeline;
pub mod validator;

#[cfg(test)]
mod llm_tests;

use commands::{add_clip, add_test_clips, get_timeline_state, import_video};
use ffmpeg::FFmpegEngine;
use llm::{log_artifact, send_prompt_to_ollama, ArtifactType, LlmResponseMetadata};
use preferences::PreferenceManager;
use prompt::{build_context_block, build_prompt, SYSTEM_PROMPT};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager, State}; // Import Manager trait for .path() and Emitter for .emit()
use timeline::TimelineEngine;
use tokio::sync::Mutex;

#[tauri::command]
fn get_user_preferences(prefs: State<'_, PreferenceManager>) -> preferences::UserPreferences {
    prefs.get_preferences()
}

// Item 7: Active Requests State
struct ActiveRequests(Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>);

impl ActiveRequests {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

// Item 6: Read Artifact Command
#[tauri::command]
fn read_artifact(app_handle: tauri::AppHandle, filename: String) -> Result<String, String> {
    // Sanitize filename
    if filename.contains("..") || !filename.ends_with(".txt") {
        return Err("Invalid filename".to_string());
    }

    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    let artifacts_dir = config_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("artifacts");
    let file_path = artifacts_dir.join(filename);

    std::fs::read_to_string(file_path).map_err(|e| e.to_string())
}

// Item 7: Cancel Request Command
#[tauri::command]
async fn cancel_request(
    active_requests: State<'_, ActiveRequests>,
    request_id: String,
) -> Result<(), String> {
    let mut map = active_requests.0.lock().await;
    if let Some(handle) = map.remove(&request_id) {
        handle.abort();
        Ok(())
    } else {
        Ok(()) // Already finished or didn't exist
    }
}

#[tauri::command]
async fn build_prompt_preview(
    state: tauri::State<'_, TimelineEngine>,
    user_input: String,
) -> Result<String, String> {
    // Only return the Context + User Input part for editing
    let context = build_context_block(&state);
    Ok(format!("{}\nUser Instruction: {}", context, user_input))
}

#[tauri::command]
async fn process_user_prompt(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, TimelineEngine>,
    active_requests: tauri::State<'_, ActiveRequests>,
    prefs: tauri::State<'_, PreferenceManager>, // Inject Preferences
    user_input: String,
    prompt_override: Option<String>,
    request_id: String,
) -> Result<LlmResponseMetadata, String> {
    // Fix #5: Guardrail for empty timeline
    {
        let timeline = state.state.lock().unwrap();
        if timeline.clips.is_empty() {
            return Ok(LlmResponseMetadata {
                text: "No clips in timeline. Cannot perform edit operations.".to_string(),
                latency_ms: 0,
                char_count: 52,
                truncated: false,
                artifact_filename: "".to_string(),
            });
        }
    }

    println!(
        "🚀 [Backend] process_user_prompt called with input: '{}'",
        user_input
    );

    // 1. Build the prompt (or use override)
    let full_prompt = if let Some(override_text) = prompt_override {
        println!("⚠️ Using Prompt Override");
        // If overridden, we assume the user edited the CONTEXT + INSTRUCTION part.
        // We still prepend the SYSTEM_PROMPT to ensure rules are followed.
        // NOTE: We might want to inject preferences here too, but for override we assume user knows what they are doing.
        // For now, let's just use the override as is, or prepend the raw system prompt.
        // Let's stick to the previous behavior for override but maybe we should inject prefs?
        // Let's keep it simple: Override means override.
        format!("{}\n{}", SYSTEM_PROMPT, override_text)
    } else {
        build_prompt(&state, &prefs, &user_input)
    };

    // 2. Log the prompt artifact
    log_artifact(&app_handle, ArtifactType::Prompt, &full_prompt);

    // 3. Send to Ollama (blocking call wrapped in spawn_blocking)
    let (tx, rx) = tokio::sync::oneshot::channel();
    let prompt_clone = full_prompt.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let result = send_prompt_to_ollama(&prompt_clone);
        let _ = tx.send(result);
    });

    // Track the request
    active_requests
        .0
        .lock()
        .await
        .insert(request_id.clone(), handle);

    // 4. Wait for result with timeout
    let final_result = match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("Request cancelled or sender dropped".to_string()),
        Err(_) => Err("Global request timeout reached (60s)".to_string()),
    };

    // Cleanup
    active_requests.0.lock().await.remove(&request_id);

    match final_result {
        Ok((text, latency_ms, char_count, truncated)) => {
            println!(
                "✅ [Backend] Received response from Ollama ({} chars, {}ms)",
                char_count, latency_ms
            );
            println!("📄 [Backend] Response Preview: {:.100}...", text);

            // Log the response (full text)
            let artifact_filename = log_artifact(&app_handle, ArtifactType::LlmResponse, &text);

            // Return rich metadata
            Ok(LlmResponseMetadata {
                text, // This might be truncated if Item 8 logic in llm.rs triggered
                latency_ms,
                char_count,
                truncated,
                artifact_filename,
            })
        }
        Err(e) => {
            let error_msg = format!("LLM Error: {}", e);
            log_artifact(&app_handle, ArtifactType::Error, &error_msg);
            Err(e)
        }
    }
}

// --- WEEK 7: Apply Edit Plan ---
#[tauri::command]
async fn apply_edit_plan(
    engine: State<'_, TimelineEngine>,
    prefs: State<'_, PreferenceManager>,
    app_handle: tauri::AppHandle,
    raw_llm_output: String,
) -> Result<String, String> {
    use action_router::run_edit_plan;
    use llm::parse_edit_plan;
    use validator::validate_plan;

    println!(
        "🚀 [Backend] apply_edit_plan called with raw output length: {}",
        raw_llm_output.len()
    );

    // 1. Parse
    let plan = match parse_edit_plan(&raw_llm_output) {
        Ok(p) => p,
        Err(e) => {
            let err_msg = format!("LLM Parse Error: {}", e);
            log_artifact(&app_handle, ArtifactType::Error, &err_msg);
            app_handle.emit("LLM_ERROR", &err_msg).unwrap_or(());
            return Err(err_msg);
        }
    };

    println!("✅ [Backend] Plan Parsed Successfully: {:?}", plan);
    println!("🔍 [Backend] Plan Actions: {:?}", plan.actions);

    // 2. Validate
    if let Err(e) = validate_plan(&plan, &engine) {
        let err_msg = format!("Plan Validation Rejected: {}", e);
        log_artifact(&app_handle, ArtifactType::Error, &err_msg);
        app_handle.emit("LLM_ERROR", &err_msg).unwrap_or(());
        return Err(err_msg);
    }
    println!("✅ [Backend] Plan Validated Successfully");

    // 3. Execute
    match run_edit_plan(&engine, &app_handle, &prefs, plan.clone()) {
        Ok(_new_state) => {
            // Log success
            let plan_json = serde_json::to_string_pretty(&plan).unwrap_or_default();
            log_artifact(
                &app_handle,
                ArtifactType::ApplyEditPlan {
                    plan: plan_json,
                    result: "Success".to_string(),
                },
                &raw_llm_output,
            );
            Ok("Plan applied successfully".to_string())
        }
        Err(e) => {
            let err_msg = format!("Router Execution Error: {}", e);
            log_artifact(&app_handle, ArtifactType::Error, &err_msg);
            return Err(err_msg);
        }
    }
}

// STEP 4 FIX: Atomic AI Edit Command
// This replaces the two-step process (process_user_prompt + apply_edit_plan)
// Frontend sends user intent, backend handles everything atomically
#[tauri::command]
async fn execute_ai_edit(
    app_handle: tauri::AppHandle,
    engine: tauri::State<'_, TimelineEngine>,
    active_requests: tauri::State<'_, ActiveRequests>,
    prefs: tauri::State<'_, PreferenceManager>,
    user_input: String,
    request_id: String,
) -> Result<String, String> {
    use action_router::run_edit_plan;
    use llm::parse_edit_plan;
    use validator::validate_plan;

    println!(
        "🚀 [Backend] execute_ai_edit called with input: '{}'",
        user_input
    );

    // Guard: Empty timeline
    {
        let timeline = engine.state.lock().unwrap();
        if timeline.clips.is_empty() {
            return Err("No clips in timeline. Cannot perform edit operations.".to_string());
        }
    }

    // 1. Build prompt
    let full_prompt = build_prompt(&engine, &prefs, &user_input);
    log_artifact(&app_handle, ArtifactType::Prompt, &full_prompt);

    // 2. Send to LLM (blocking call wrapped in spawn_blocking)
    let (tx, rx) = tokio::sync::oneshot::channel();
    let prompt_clone = full_prompt.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let result = send_prompt_to_ollama(&prompt_clone);
        let _ = tx.send(result);
    });

    // Track request for cancellation
    active_requests
        .0
        .lock()
        .await
        .insert(request_id.clone(), handle);

    // 3. Wait for LLM response
    let llm_result = match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            active_requests.0.lock().await.remove(&request_id);
            return Err("Request cancelled or sender dropped".to_string());
        }
        Err(_) => {
            active_requests.0.lock().await.remove(&request_id);
            return Err("Global request timeout reached (60s)".to_string());
        }
    };

    active_requests.0.lock().await.remove(&request_id);

    let (llm_text, latency_ms, char_count, _truncated) = match llm_result {
        Ok(r) => r,
        Err(e) => {
            // Human-friendly: Network/LLM issues
            let user_msg = "AI service is temporarily unavailable. Please try again.".to_string();
            log_artifact(
                &app_handle,
                ArtifactType::Error,
                &format!("LLM Error: {}", e),
            );
            return Err(user_msg);
        }
    };

    println!(
        "✅ [Backend] LLM Response ({} chars, {}ms)",
        char_count, latency_ms
    );
    log_artifact(&app_handle, ArtifactType::LlmResponse, &llm_text);

    // 4. Parse EditPlan
    let plan = match parse_edit_plan(&llm_text) {
        Ok(p) => p,
        Err(e) => {
            // Human-friendly: Parse errors mean AI response was unclear
            let user_msg = "AI response was unclear. Try rephrasing your request.".to_string();
            log_artifact(
                &app_handle,
                ArtifactType::Error,
                &format!("Parse Error: {}", e),
            );
            app_handle.emit("LLM_ERROR", &user_msg).unwrap_or(());
            return Err(user_msg);
        }
    };

    println!("✅ [Backend] Plan Parsed: {:?}", plan);

    // 4.5 CONFIDENCE GATE: Reject low-confidence plans
    const CONFIDENCE_THRESHOLD: f32 = 0.6;
    let confidence = plan.confidence.unwrap_or(0.5); // Default to uncertain if not provided
    if confidence < CONFIDENCE_THRESHOLD {
        let thought = plan
            .thought_process
            .as_deref()
            .unwrap_or("No explanation provided");
        let user_msg = format!(
            "AI is uncertain about this edit (confidence: {:.0}%). Please rephrase or be more specific.\nAI's interpretation: {}",
            confidence * 100.0,
            thought
        );
        log_artifact(
            &app_handle,
            ArtifactType::Error,
            &format!("Low confidence ({:.2}): {}", confidence, thought),
        );
        app_handle.emit("LLM_ERROR", &user_msg).unwrap_or(());
        return Err(user_msg);
    }
    println!(
        "✅ [Backend] Confidence Gate Passed: {:.0}%",
        confidence * 100.0
    );

    // 5. Validate Plan
    if let Err(e) = validate_plan(&plan, &engine) {
        // Human-friendly: Validation errors mean the edit isn't possible
        let user_msg =
            "That edit isn't possible with the current clips. Check your timeline.".to_string();
        log_artifact(
            &app_handle,
            ArtifactType::Error,
            &format!("Validation Error: {}", e),
        );
        app_handle.emit("LLM_ERROR", &user_msg).unwrap_or(());
        return Err(user_msg);
    }

    println!("✅ [Backend] Plan Validated");

    // 6. Execute Plan (with rollback on failure - from Step 3)
    match run_edit_plan(&engine, &app_handle, &prefs, plan.clone()) {
        Ok(_new_state) => {
            let plan_json = serde_json::to_string_pretty(&plan).unwrap_or_default();
            log_artifact(
                &app_handle,
                ArtifactType::ApplyEditPlan {
                    plan: plan_json,
                    result: "Success".to_string(),
                },
                &llm_text,
            );
            println!("✅ [Backend] AI Edit Applied Successfully");
            Ok("AI edit applied successfully".to_string())
        }
        Err(e) => {
            // Human-friendly: Execution errors mean something went wrong applying the edit
            let user_msg = "Failed to apply edit. The timeline may have changed.".to_string();
            log_artifact(
                &app_handle,
                ArtifactType::Error,
                &format!("Execution Error: {}", e),
            );
            Err(user_msg)
        }
    }
}

// --- COMMANDS ---

/// Seek the timeline playhead to a specific time.
/// Returns the clamped time value.
#[tauri::command]
async fn seek_timeline(
    engine: State<'_, TimelineEngine>,
    app_handle: tauri::AppHandle,
    time: f64,
) -> Result<f64, String> {
    let clamped_time = engine.seek(time);

    // Emit state update so frontend stays in sync
    let state = engine.state.lock().map_err(|_| "Failed to lock state")?;
    app_handle
        .emit("STATE_UPDATE", &*state)
        .map_err(|e| e.to_string())?;

    Ok(clamped_time)
}

/// Get the currently active clip at the playhead position.
#[tauri::command]
fn get_active_clip(engine: State<'_, TimelineEngine>) -> Result<Option<timeline::Clip>, String> {
    Ok(engine.get_current_clip())
}

/// Export the timeline to a video file using FFmpeg.
/// This is NOT preview - it generates an actual rendered output file.
#[tauri::command]
async fn export_timeline(
    ffmpeg: State<'_, FFmpegEngine>,
    engine: State<'_, TimelineEngine>,
    _app_handle: tauri::AppHandle,
) -> Result<String, String> {
    // 1. Get Timeline State
    let state = {
        let guard = engine.state.lock().unwrap();
        guard.clone()
    };

    // 2. Determine Output Path
    let current_dir = std::env::current_dir().map_err(|e| e.to_string())?;

    let videos_dir = if current_dir.ends_with("src-tauri") {
        current_dir.parent().unwrap_or(&current_dir).join("videos")
    } else {
        current_dir.join("videos")
    };

    let exports_dir = videos_dir.join("exports");
    if !exports_dir.exists() {
        std::fs::create_dir_all(&exports_dir).map_err(|e| e.to_string())?;
    }

    let filename = format!("export_{}.mp4", uuid::Uuid::new_v4());
    let output_path = exports_dir.join(filename);

    // 3. Render using FFmpeg
    let output_path_clone = output_path.clone();
    let ffmpeg_engine = (*ffmpeg).clone();

    let _ffmpeg_result = tokio::task::spawn_blocking(move || {
        ffmpeg_engine.render_timeline(&state, &output_path_clone)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // 4. Return Path
    Ok(output_path.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // Initialize Logger
            env_logger::init();

            let app_handle = app.handle();
            // Initialize PreferenceManager with app_handle
            let prefs_manager = PreferenceManager::new(app_handle);
            app.manage(prefs_manager);

            // Initialize the God State
            let timeline_engine = TimelineEngine::new();

            // STEP 2 FIX: Emit initial STATE_UPDATE so frontend starts with correct state
            // This replaces the need for frontend to call fetchState()
            {
                let state = timeline_engine.state.lock().unwrap();
                let app_handle_clone = app.handle().clone();
                let initial_state = state.clone();
                // Use spawn to emit after setup completes
                std::thread::spawn(move || {
                    // Small delay to ensure frontend listener is registered
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let _ = app_handle_clone.emit("STATE_UPDATE", &initial_state);
                    println!("📡 [Backend] Emitted initial STATE_UPDATE");
                });
            }

            app.manage(timeline_engine);
            app.manage(ActiveRequests::new()); // Register ActiveRequests

            // Initialize FFmpegEngine
            app.manage(FFmpegEngine::new());

            Ok(())
        })
        // Register the commands
        .invoke_handler(tauri::generate_handler![
            get_timeline_state,
            add_clip,
            add_test_clips,
            import_video,
            process_user_prompt,
            build_prompt_preview,
            read_artifact,
            cancel_request,
            execute_ai_edit, // STEP 4 FIX: Atomic AI edit (replaces apply_edit_plan)
            get_user_preferences,
            export_timeline, // Renamed from render_preview
            seek_timeline,   // New: playhead control
            get_active_clip  // New: get clip at playhead
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
