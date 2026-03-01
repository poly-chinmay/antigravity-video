// src-tauri/src/lib.rs

pub mod action_preflight;
pub mod action_router;
pub mod commands;
pub mod edit_plan;
pub mod edit_plan_validator;
pub mod edit_rejection;
pub mod engine; // NEW: Hardened God State & TimelineEngine
pub mod export; // NEW: Export pipeline
pub mod ffmpeg;
pub mod llm;
pub mod media;
pub mod persistence;
pub mod preferences;
pub mod prompt;
pub mod reversible_command;
pub mod telemetry;
pub mod timeline;
pub mod undo_commands;
pub mod undo_redo_manager;
pub mod validator;

#[cfg(test)]
mod llm_tests;

use commands::{add_clip, add_test_clips, get_timeline_state, import_video};
use ffmpeg::FFmpegEngine;
use llm::{log_artifact, send_prompt_to_ollama, ArtifactType, LlmResponseMetadata};
use persistence::{EventStore, WriteAheadLog};
use preferences::PreferenceManager;
use prompt::{build_context_block, build_prompt, SYSTEM_PROMPT};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State}; // Import Manager trait for .path() and Emitter for .emit()
use timeline::TimelineEngine;
use tokio::sync::Mutex;
use undo_redo_manager::UndoRedoManager;

mod undo_commands_tauri;

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
    event_store: tauri::State<'_, Arc<std::sync::Mutex<EventStore>>>,
    wal: tauri::State<'_, Arc<std::sync::Mutex<WriteAheadLog>>>,
    user_input: String,
    request_id: String,
) -> Result<String, String> {
    use action_router::run_edit_plan;
    use llm::parse_edit_plan;
    use persistence::{Event, WALEntry};
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

    // 5.5 Write WAL entry BEFORE mutation (crash safety)
    let current_version = {
        let state = engine
            .state
            .lock()
            .map_err(|_| "Failed to lock state".to_string())?;
        state.version
    };
    let pre_mutation_event = Event::new(
        current_version + 1,
        plan.clone(),
        Some(user_input.clone()),
        plan.confidence,
        0,     // Execution time not yet known
        false, // Not yet successful
    );
    let wal_entry = WALEntry::new(current_version + 1, pre_mutation_event);
    if let Ok(mut wal_lock) = wal.lock() {
        if let Err(e) = wal_lock.append(&wal_entry) {
            eprintln!("❌ [WAL] Failed to write pre-mutation entry: {}", e);
            return Err("Failed to persist operation. Please try again.".to_string());
        }
    } else {
        return Err("Failed to acquire WAL lock.".to_string());
    }

    // 6. Execute Plan (with rollback on failure - from Step 3)
    let start_time = std::time::Instant::now();
    match run_edit_plan(&engine, &app_handle, &prefs, plan.clone()) {
        Ok(new_state) => {
            let execution_time_ms = start_time.elapsed().as_millis() as u64;

            // Log event to persistent store
            let event = Event::new(
                new_state.version,
                plan.clone(),
                Some(user_input.clone()),
                plan.confidence,
                execution_time_ms,
                true,
            );
            if let Ok(store) = event_store.lock() {
                if let Err(e) = store.append(&event) {
                    eprintln!("⚠️ [EventStore] Failed to persist event: {}", e);
                }
            }

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

// =============================================================================
// NEW EXPORT PIPELINE (v2)
// =============================================================================

/// Start a new export with progress tracking.
///
/// This is the v2 export that supports:
/// - Progress polling via `get_export_progress`
/// - Cancellation via `cancel_export`
/// - Proper source_in/source_out handling
#[tauri::command]
async fn export_timeline_v2(
    export_service: State<'_, std::sync::Mutex<export::ExportService>>,
    engine: State<'_, TimelineEngine>,
    output_path: String,
) -> Result<export::ExportJobId, String> {
    use std::path::PathBuf;

    // Get timeline state
    let timeline = {
        let guard = engine.state.lock().unwrap();
        guard.clone()
    };

    // Build config
    let config = export::ExportConfig {
        timeline,
        output_path: PathBuf::from(&output_path),
        preset: export::ExportPreset::h264_1080p(),
    };

    // Start export
    let mut service = export_service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    service.start_export(config).map_err(|e| e.to_string())
}

/// Get progress of an active export.
#[tauri::command]
fn get_export_progress(
    export_service: State<'_, std::sync::Mutex<export::ExportService>>,
    job_id: String,
) -> Result<Option<export::ExportProgress>, String> {
    let mut service = export_service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Check for completion first
    if let Some(result) = service.check_complete(&job_id) {
        // Return final progress based on result
        let final_status = match result {
            export::ExportResult::Success { .. } => export::ExportStatus::Complete,
            export::ExportResult::Cancelled => export::ExportStatus::Cancelled,
            export::ExportResult::Failed { error } => export::ExportStatus::Failed {
                message: error.to_string(),
            },
        };

        return Ok(Some(export::ExportProgress {
            job_id: job_id.clone(),
            frames_completed: 0,
            frames_total: 0,
            elapsed_secs: 0.0,
            eta_secs: None,
            status: final_status,
        }));
    }

    // Poll for progress
    Ok(service.poll_progress(&job_id))
}

/// Cancel an active export.
#[tauri::command]
fn cancel_export(
    export_service: State<'_, std::sync::Mutex<export::ExportService>>,
    job_id: String,
) -> Result<(), String> {
    let mut service = export_service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    service.cancel(&job_id).map_err(|e| e.to_string())
}

/// Import a media file and return verified metadata.
///
/// This runs FFprobe on a worker thread to extract and validate
/// metadata from the given file path. The source is automatically
/// registered in the MediaRegistry for reuse.
///
/// # Arguments
///
/// * `path` - Absolute path to the media file
///
/// # Returns
///
/// * `Ok(MediaSource)` - Verified media source with metadata
/// * `Err(String)` - Error message if import failed
#[tauri::command]
async fn import_media_file(
    path: String,
    registry: State<'_, media::MediaRegistry>,
) -> Result<media::MediaSource, String> {
    use std::path::PathBuf;

    let path_buf = PathBuf::from(&path);

    // Check if already imported (avoid re-probing)
    if let Some(existing) = registry.get_by_path(&path_buf) {
        return Ok(existing);
    }

    // Run FFprobe on a worker thread to avoid blocking Tauri's async runtime
    let result = tokio::task::spawn_blocking(move || media::import_media(&path_buf))
        .await
        .map_err(|e| format!("Task join error: {}", e))?;

    // Register the source
    if let Ok(ref source) = result {
        registry.register(source.clone());
        println!("📁 [MediaRegistry] Registered: {}", source.display_name);
    }

    result.map_err(|e| e.to_string())
}

/// Get the media pool view model for the UI.
///
/// Returns all imported media sources with availability status.
#[tauri::command]
fn get_media_pool(registry: State<'_, media::MediaRegistry>) -> engine::ui::MediaPoolViewModel {
    engine::ui::MediaPoolViewModel::from_registry(&registry)
}

/// Add a media source to the timeline.
///
/// Creates a new clip from the given source ID at the current playhead.
///
/// # Arguments
///
/// * `source_id` - ID of the media source from the pool
///
/// # Returns
///
/// * `Ok(clip_id)` - ID of the created clip
/// * `Err(String)` - Error if source not found or add failed
#[tauri::command]
fn add_media_to_timeline(
    app: AppHandle,
    source_id: String,
    registry: State<'_, media::MediaRegistry>,
    engine: State<'_, TimelineEngine>,
) -> Result<String, String> {
    // Get source from registry and validate it's available
    let source = registry
        .get(&source_id)
        .ok_or_else(|| format!("Source not found: {}", source_id))?;

    // Check if source file exists (offline detection)
    if !source.path.exists() {
        return Err(format!(
            "Source is offline: {} (file not found at {:?})",
            source.display_name, source.path
        ));
    }

    // Create clip from source
    let mut state = engine
        .state
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Use playhead position as start time
    let start = state.playhead_time;

    // Create new clip
    let clip = timeline::Clip::new(
        "track_0".to_string(),
        start,
        source.duration_secs,
        source.path.to_string_lossy().to_string(),
    );

    let clip_id = clip.id.clone();
    state.add_clip(clip);

    println!(
        "🎬 [MediaPool] Added clip from {} at {:.2}s",
        source.display_name, start
    );

    // Emit STATE_UPDATE so frontend updates timeline
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| format!("Failed to emit update: {}", e))?;

    Ok(clip_id)
}

/// Move a clip to a new start time.
///
/// # Arguments
///
/// * `clip_id` - ID of the clip to move
/// * `new_start_time` - New start time in seconds
///
/// # Returns
///
/// * `Ok(())` - Move succeeded
/// * `Err(String)` - Move failed (overlap, invalid position, etc.)
#[tauri::command]
fn move_clip(
    app: AppHandle,
    clip_id: String,
    new_start_time: f64,
    new_track_id: Option<String>, // Optional: change track
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<undo_redo_manager::UndoRedoManager>>,
) -> Result<(), String> {
    // Clamp to >= 0
    let new_start = new_start_time.max(0.0);

    let mut state = engine
        .state
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Find the clip
    let clip_idx = state
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or_else(|| format!("Clip not found: {}", clip_id))?;

    let clip_duration = state.clips[clip_idx].duration;
    let current_track_id = state.clips[clip_idx].track_id.clone();
    let target_track_id = new_track_id
        .clone()
        .unwrap_or_else(|| current_track_id.clone());
    let new_end = new_start + clip_duration;

    // Validate target track exists
    if !state.tracks.iter().any(|t| t.id == target_track_id) {
        return Err(format!("Track not found: {}", target_track_id));
    }

    // Check for overlap with other clips ON THE TARGET TRACK ONLY
    for (i, other) in state.clips.iter().enumerate() {
        if i == clip_idx {
            continue;
        }
        // Only check clips on the target track
        if other.track_id != target_track_id {
            continue;
        }
        let other_end = other.start + other.duration;
        // Overlap if: new_start < other_end AND new_end > other_start
        if new_start < other_end && new_end > other.start {
            return Err(format!(
                "Move rejected: overlap with clip '{}' at {:.2}s-{:.2}s",
                other.id, other.start, other_end
            ));
        }
    }

    // Apply the move (position and optionally track)
    let old_start = state.clips[clip_idx].start;
    state.clips[clip_idx].start = new_start;
    if let Some(ref track_id) = new_track_id {
        state.clips[clip_idx].track_id = track_id.clone();
    }

    // Update timeline duration
    state.recalculate_duration();
    state.version += 1;

    println!(
        "🎬 [Move] Clip {} moved to {:.2}s on track {} (from {:.2}s)",
        clip_id, new_start, target_track_id, old_start
    );

    // Emit STATE_UPDATE
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| format!("Failed to emit update: {}", e))?;

    Ok(())
}

/// Trim a clip's duration (right edge).
///
/// # Arguments
///
/// * `clip_id` - ID of the clip to trim
/// * `new_duration` - New duration in seconds (must be > 0)
///
/// # Returns
///
/// * `Ok(())` - Trim succeeded
/// * `Err(String)` - Trim failed (invalid duration, overlap, etc.)
#[tauri::command]
fn trim_clip(
    app: AppHandle,
    clip_id: String,
    new_duration: f64,
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<undo_redo_manager::UndoRedoManager>>,
) -> Result<(), String> {
    // Duration must be positive
    if new_duration <= 0.0 {
        return Err("Duration must be greater than 0".to_string());
    }

    // Minimum duration of 0.1 seconds
    let clamped_duration = new_duration.max(0.1);

    let mut state = engine
        .state
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Find the clip
    let clip_idx = state
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or_else(|| format!("Clip not found: {}", clip_id))?;

    let clip_start = state.clips[clip_idx].start;
    let old_duration = state.clips[clip_idx].duration;
    let new_end = clip_start + clamped_duration;

    // Check for overlap with other clips (if extending)
    if clamped_duration > old_duration {
        for (i, other) in state.clips.iter().enumerate() {
            if i == clip_idx {
                continue;
            }
            // Only check clips that start after our clip
            if other.start >= clip_start {
                if new_end > other.start {
                    return Err(format!(
                        "Trim rejected: would overlap with clip '{}' at {:.2}s",
                        other.id, other.start
                    ));
                }
            }
        }
    }

    // Calculate trim delta (end_delta)
    let trim_end_delta = clamped_duration - old_duration;

    // Create reversible command for undo
    let cmd = undo_commands::TrimClipCommand::new(clip_id.clone(), None, Some(trim_end_delta));

    // Execute via undo manager
    let mut manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;
    manager.execute_command(Box::new(cmd), &mut state)?;

    println!(
        "✂️ [Trim] Clip {} duration: {:.2}s → {:.2}s (undoable)",
        clip_id, old_duration, clamped_duration
    );

    // Emit STATE_UPDATE
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| format!("Failed to emit update: {}", e))?;

    Ok(())
}

/// Trim a clip's left edge (start).
///
/// Adjusts the clip's start position and reduces duration accordingly.
///
/// # Arguments
///
/// * `clip_id` - ID of the clip to trim
/// * `new_start_time` - New start time in seconds
///
/// # Returns
///
/// * `Ok(())` - Trim succeeded
/// * `Err(String)` - Trim failed (invalid position, overlap, etc.)
#[tauri::command]
fn trim_clip_start(
    app: AppHandle,
    clip_id: String,
    new_start_time: f64,
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<undo_redo_manager::UndoRedoManager>>,
) -> Result<(), String> {
    let mut state = engine
        .state
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Find the clip
    let clip_idx = state
        .clips
        .iter()
        .position(|c| c.id == clip_id)
        .ok_or_else(|| format!("Clip not found: {}", clip_id))?;

    let old_start = state.clips[clip_idx].start;
    let old_duration = state.clips[clip_idx].duration;
    let old_end = old_start + old_duration;

    // Clamp new_start to >= 0
    let new_start = new_start_time.max(0.0);

    // New start must be before old end (to maintain positive duration)
    if new_start >= old_end {
        return Err(format!(
            "Trim rejected: new start {:.2}s must be before clip end {:.2}s",
            new_start, old_end
        ));
    }

    // Calculate new duration
    let new_duration = old_end - new_start;

    // Minimum duration check
    if new_duration < 0.1 {
        return Err("Trim rejected: duration would be less than 0.1s".to_string());
    }

    // Check for overlap with previous clips (clips that end at or after new_start)
    for (i, other) in state.clips.iter().enumerate() {
        if i == clip_idx {
            continue;
        }
        let other_end = other.start + other.duration;
        // Check if moving start left would overlap with another clip
        if new_start < old_start && other_end > new_start && other.start < new_start {
            return Err(format!(
                "Trim rejected: would overlap with clip '{}' at {:.2}s-{:.2}s",
                other.id, other.start, other_end
            ));
        }
    }

    // Calculate trim_start_delta for TrimClipCommand
    // Positive delta = move start forward (shrink from left)
    // Negative delta = move start backward (extend from left)
    let trim_start_delta = new_start - old_start;

    // Create reversible command for undo
    let cmd = undo_commands::TrimClipCommand::new(clip_id.clone(), Some(trim_start_delta), None);

    // Execute via undo manager
    let mut manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;
    manager.execute_command(Box::new(cmd), &mut state)?;

    println!(
        "✂️ [TrimStart] Clip {} start: {:.2}s → {:.2}s (undoable)",
        clip_id, old_start, new_start
    );

    // Emit STATE_UPDATE
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| format!("Failed to emit update: {}", e))?;

    Ok(())
}

/// Add a new track to the timeline.
#[tauri::command]
fn add_track(
    app: AppHandle,
    name: String,
    engine: State<'_, TimelineEngine>,
) -> Result<String, String> {
    let mut state = engine
        .state
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    let index = state.tracks.len();
    let track = timeline::Track::new(name.clone(), index);
    let track_id = track.id.clone();
    state.tracks.push(track);
    state.version += 1;

    println!("➕ [Track] Added track '{}' ({})", name, track_id);

    // Emit STATE_UPDATE
    app.emit("STATE_UPDATE", &*state)
        .map_err(|e| format!("Failed to emit update: {}", e))?;

    Ok(track_id)
}

#[tauri::command]
fn render_preview_frame(
    engine: State<'_, TimelineEngine>,
    time_secs: f64,
) -> Result<String, String> {
    use engine::preview;
    use std::path::Path;

    let timeline = engine.state.lock().map_err(|e| e.to_string())?;

    // Use track-aware selection to find top clip
    let clip = timeline
        .get_visible_clip_at_time(time_secs)
        .ok_or("No clip at time")?;

    // Calculate source time mapping
    // V1 Limitation: No source_offset field yet, so we just do relative time.
    // source_time = timeline_time - clip_start
    let source_time = (time_secs - clip.start).max(0.0);

    let frame =
        preview::frame_renderer::render_frame_at_time(Path::new(&clip.source_file), source_time)
            .map_err(|e| e.to_string())?;

    Ok(frame.path.to_string_lossy().to_string())
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
            app.manage(ActiveRequests::new());

            // Initialize FFmpegEngine
            app.manage(FFmpegEngine::new());

            // Initialize MediaRegistry for media pool
            app.manage(media::MediaRegistry::new());

            // Initialize ExportService
            app.manage(std::sync::Mutex::new(export::ExportService::new()));

            // Initialize EventStore for persistent event sourcing
            let current_dir = std::env::current_dir().expect("Failed to get current dir");
            let events_base = if current_dir.ends_with("src-tauri") {
                current_dir.parent().unwrap_or(&current_dir).join("data")
            } else {
                current_dir.join("data")
            };
            match EventStore::new(events_base.clone()) {
                Ok(store) => {
                    println!("📁 [EventStore] Initialized at {:?}", store.events_path());
                    app.manage(Arc::new(std::sync::Mutex::new(store)));
                }
                Err(e) => {
                    eprintln!("⚠️ [EventStore] Failed to initialize: {}", e);
                    let fallback = EventStore::new(std::env::temp_dir().join("ghost_events"))
                        .expect("Failed to create fallback EventStore");
                    app.manage(Arc::new(std::sync::Mutex::new(fallback)));
                }
            }

            // Initialize WriteAheadLog for crash-safe mutations
            match WriteAheadLog::new(events_base) {
                Ok(wal) => {
                    println!("📝 [WAL] Initialized at {:?}", wal.wal_path());
                    app.manage(Arc::new(std::sync::Mutex::new(wal)));
                }
                Err(e) => {
                    eprintln!("⚠️ [WAL] Failed to initialize: {}", e);
                    let fallback = WriteAheadLog::new(std::env::temp_dir().join("ghost_wal"))
                        .expect("Failed to create fallback WAL");
                    app.manage(Arc::new(std::sync::Mutex::new(fallback)));
                }
            }

            // Initialize UndoRedoManager
            let undo_manager = UndoRedoManager::new();
            app.manage(std::sync::Mutex::new(undo_manager));

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
            export_timeline,     // Renamed from render_preview
            export_timeline_v2,  // NEW: Export with progress
            get_export_progress, // NEW: Poll export progress
            cancel_export,       // NEW: Cancel running export
            seek_timeline,       // New: playhead control
            get_active_clip,     // New: get clip at playhead
            undo_commands_tauri::undo_command,
            undo_commands_tauri::redo_command,
            undo_commands_tauri::undo_multiple_command,
            undo_commands_tauri::can_undo,
            undo_commands_tauri::can_redo,
            import_media_file,     // Media import capability
            get_media_pool,        // NEW: Media pool view model
            add_media_to_timeline, // NEW: Add media to timeline
            move_clip,             // NEW: Move clip to new position
            trim_clip,             // NEW: Trim clip duration (right edge)
            trim_clip_start,       // NEW: Trim clip start (left edge)
            add_track,             // NEW: Add track to timeline
            render_preview_frame,  // NEW: Render single preview frame
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
