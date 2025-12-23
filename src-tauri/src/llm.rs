// src-tauri/src/llm.rs
use crate::edit_plan::EditPlan; // Import EditPlan
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri::Manager;
use thiserror::Error; // Import Error derive

// --- CONSTANTS ---
const MAX_RESPONSE_CHARS: usize = 16000; // Truncate responses longer than this

// --- STRUCTS & ENUMS ---

// The raw JSON structure Ollama sends back
#[derive(Deserialize, Debug)]
struct OllamaResponse {
    // model: String, // Unused for now
    // created_at: String, // Unused for now
    response: String,
    // done: bool, // Unused for now
}

// Types of artifacts we can log
pub enum ArtifactType {
    Prompt,
    LlmResponse,
    Error,
    ApplyEditPlan { plan: String, result: String },
}

// The rich metadata we will send back to the frontend
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LlmResponseMetadata {
    pub text: String,
    pub latency_ms: u64,
    pub char_count: usize,
    pub truncated: bool,
    pub artifact_filename: String,
}

// --- FUNCTIONS ---

// Helper to get the path to the "artifacts" folder next to the app executable
fn get_artifacts_dir(app_handle: &AppHandle) -> PathBuf {
    let app_dir = app_handle
        .path()
        .app_config_dir()
        .expect("failed to get app config dir");
    // We want to store artifacts next to the executable, not in config dir for this phase
    let exe_dir = app_dir.parent().unwrap().parent().unwrap();
    let artifacts_dir = exe_dir.join("artifacts");

    if !artifacts_dir.exists() {
        fs::create_dir_all(&artifacts_dir).expect("failed to create artifacts dir");
    }
    artifacts_dir
}

// Helper to save text to a timestamped file
pub fn log_artifact(app_handle: &AppHandle, artifact_type: ArtifactType, content: &str) -> String {
    let dir = get_artifacts_dir(app_handle);
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let timestamp = since_the_epoch.as_millis();

    let (prefix, final_content) = match artifact_type {
        ArtifactType::Prompt => ("prompt", content.to_string()),
        ArtifactType::LlmResponse => ("llm_response", content.to_string()),
        ArtifactType::Error => ("error", content.to_string()),
        ArtifactType::ApplyEditPlan { plan, result } => (
            "apply_plan",
            format!(
                "{{\n  \"plan\": {},\n  \"result\": \"{}\",\n  \"raw_input\": \"{}\"\n}}",
                plan, result, content
            ),
        ),
    };

    let filename = format!("artifact_{}_{}.txt", prefix, timestamp);
    let file_path = dir.join(&filename);

    // Use 0o600 permission for privacy (read/write for owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).mode(0o600);
        let mut file = options
            .open(&file_path)
            .expect("failed to create artifact file with secure permissions");
        file.write_all(final_content.as_bytes())
            .expect("failed to write to artifact file");
    }

    #[cfg(not(unix))]
    {
        // Fallback for Windows where mode(0o600) isn't the same
        let mut file = File::create(&file_path).expect("failed to create artifact file");
        file.write_all(final_content.as_bytes())
            .expect("failed to write to artifact file");
    }

    println!("📝 Artifact logged: {:?}", filename);
    filename
}

// The main function to send data to Ollama
// NOTE: This is now a BLOCKING function because we wrap it in a blocking Tokio task in lib.rs
pub fn send_prompt_to_ollama(prompt: &str) -> Result<(String, u64, usize, bool), String> {
    let client = Client::new();
    // Using 127.0.0.1 directly to avoid IPv6 resolution issues
    let ollama_url = "http://127.0.0.1:11434/api/generate";

    let request_body = json!({
        "model": "llama3.2",
        "prompt": prompt,
        "stream": false
    });

    println!(
        "⏳ [Backend] Sending request to Ollama at {}...",
        ollama_url
    );
    let start_time = Instant::now();

    // Use blocking send
    let response = client
        .post(ollama_url)
        .json(&request_body)
        .send()
        .map_err(|e| format!("Failed to send request to Ollama: {}", e))?;

    let latency_ms = start_time.elapsed().as_millis() as u64;

    if !response.status().is_success() {
        return Err(format!(
            "Ollama returned an error status: {}",
            response.status()
        ));
    }

    let response_text = response
        .text()
        .map_err(|e| format!("Failed to read response text: {}", e))?;

    let ollama_response: OllamaResponse = serde_json::from_str(&response_text).map_err(|e| {
        format!(
            "Failed to parse JSON response from Ollama: {}. Raw text: {}",
            e, response_text
        )
    })?;

    let mut final_text = ollama_response.response;
    let char_count = final_text.chars().count();
    let mut truncated = false;

    // Truncation logic
    if char_count > MAX_RESPONSE_CHARS {
        // Keep first N characters
        let truncated_str: String = final_text.chars().take(MAX_RESPONSE_CHARS).collect();
        final_text = format!(
            "{}\n\n[RESPONSE TRUNCATED DUE TO LENGTH - SEE ARTIFACT FOR FULL TEXT]",
            truncated_str
        );
        truncated = true;
        println!(
            "⚠️ Response truncated ({} chars > {})",
            char_count, MAX_RESPONSE_CHARS
        );
    }

    // Return tuple: (text, latency, char_count, truncated status)
    println!("✅ [Backend] Ollama Response Text: {:.200}...", final_text);
    Ok((final_text, latency_ms, char_count, truncated))
}

// --- WEEK 7: JSON PARSING ---

#[derive(Error, Debug)]
pub enum LlmParseError {
    #[error("Empty input")]
    EmptyInput,
    #[error("No JSON found in response")]
    NoJsonFound,
    #[error("Failed to parse JSON: {0}")]
    JsonParseError(#[from] serde_json::Error),
}

pub fn parse_edit_plan(raw: &str) -> Result<EditPlan, LlmParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LlmParseError::EmptyInput);
    }

    // 1. Sanitize: Find the first '{' and last '}'
    let start = trimmed.find('{').ok_or(LlmParseError::NoJsonFound)?;
    let end = trimmed.rfind('}').ok_or(LlmParseError::NoJsonFound)?;

    if start > end {
        return Err(LlmParseError::NoJsonFound);
    }

    let json_str = &trimmed[start..=end];

    // 2. Parse
    let plan: EditPlan = serde_json::from_str(json_str)?;
    Ok(plan)
}

pub fn is_valid_uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}
