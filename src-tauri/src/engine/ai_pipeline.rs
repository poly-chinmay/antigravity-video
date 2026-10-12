//! AI Pipeline - Hardened LLM control surface.
//!
//! # Design Invariants
//!
//! 1. LLM NEVER touches state directly
//! 2. All LLM output goes through UntrustedAIResponse
//! 3. Pipeline stages: Parse → Schema → Semantic → Safety → Engine
//! 4. Any failure → entire request rejected, no state change
//! 5. No partial application of actions
//!
//! # Pipeline Diagram
//!
//! ```text
//! User Intent (Frontend)
//!         ↓
//! IntentRequest (Rust)
//!         ↓
//! LLM Prompt Builder
//!         ↓
//! LLM Response (Untrusted)
//!         ↓
//! JSON Parser
//!         ↓
//! Schema Validator
//!         ↓
//! Semantic Validator
//!         ↓
//! Safety Validator
//!         ↓
//! EditAction List
//!         ↓
//! TimelineEngine.apply_action()
//!         ↓
//! Event Store → State Mutation → Snapshot → UI Event
//! ```

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::edit_action::{ActionParameters, ActionType, EditAction};
use crate::engine::errors::EngineError;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_engine::TimelineEngine;
use crate::engine::timeline_state::{Clip, TimelineState};

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Maximum number of delete operations per request
const MAX_DELETES_PER_REQUEST: usize = 10;

/// Maximum number of clips affected per request
const MAX_AFFECTED_CLIPS: usize = 50;

/// Maximum actions per single AI request
const MAX_ACTIONS_PER_REQUEST: usize = 100;

/// Schema version for AI edit plans
const SCHEMA_VERSION: u32 = 1;

// =============================================================================
// UNTRUSTED AI RESPONSE (Entry Point)
// =============================================================================

/// Wrapper for untrusted LLM output.
///
/// # Safety
///
/// ALL LLM responses MUST enter the system through this type.
/// No parsing or interpretation outside this boundary.
#[derive(Debug, Clone)]
pub struct UntrustedAIResponse {
    /// Raw string from LLM - completely untrusted
    raw: String,
}

impl UntrustedAIResponse {
    /// Create from raw LLM output.
    pub fn from_raw(raw: String) -> Self {
        Self { raw }
    }

    /// Get the raw content (for pipeline processing only).
    pub(crate) fn raw(&self) -> &str {
        &self.raw
    }
}

// =============================================================================
// AI FAILURE CLASSES
// =============================================================================

/// Categories of AI pipeline failures.
///
/// Each variant maps to a specific pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIFailure {
    /// JSON parsing failed (invalid JSON syntax)
    ParseError {
        message: String,
        position: Option<usize>,
    },

    /// Schema validation failed (missing/invalid fields)
    SchemaViolation {
        message: String,
        field: Option<String>,
    },

    /// Semantic validation failed (references don't exist)
    SemanticViolation {
        message: String,
        clip_id: Option<String>,
    },

    /// Safety rules violated
    SafetyViolation { rule: SafetyRule, message: String },

    /// Engine rejected the action(s)
    EngineRejected {
        message: String,
        action_index: Option<usize>,
    },
}

/// Specific safety rules that can be violated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SafetyRule {
    TooManyDeletes { count: usize, max: usize },
    TooManyAffectedClips { count: usize, max: usize },
    TooManyActions { count: usize, max: usize },
    NegativePosition { clip_id: String, value: i64 },
    InvalidDuration { clip_id: String, value: i64 },
    UnknownField { field: String },
    FileSystemAccess { path: String },
    PathTraversal { path: String },
    AbsolutePath { path: String },
    NonExistentClipId { clip_id: String },
}

impl std::fmt::Display for AIFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError { message, .. } => write!(f, "Parse error: {}", message),
            Self::SchemaViolation { message, .. } => write!(f, "Schema violation: {}", message),
            Self::SemanticViolation { message, .. } => write!(f, "Semantic violation: {}", message),
            Self::SafetyViolation { message, .. } => write!(f, "Safety violation: {}", message),
            Self::EngineRejected { message, .. } => write!(f, "Engine rejected: {}", message),
        }
    }
}

// =============================================================================
// FRONTEND CONTRACT
// =============================================================================

/// Result sent to frontend after AI pipeline processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum AIResult {
    /// All actions accepted and applied
    Accepted {
        actions_applied: usize,
        thought_process: Option<String>,
    },

    /// Request rejected - no state change occurred
    Rejected { failure: AIFailure, message: String },
}

impl AIResult {
    pub fn accepted(actions_applied: usize, thought_process: Option<String>) -> Self {
        Self::Accepted {
            actions_applied,
            thought_process,
        }
    }

    pub fn rejected(failure: AIFailure) -> Self {
        let message = failure.to_string();
        Self::Rejected { failure, message }
    }
}

// =============================================================================
// AI EDIT PLAN SCHEMA
// =============================================================================

/// Schema for AI-generated edit plans.
///
/// Validated manually from serde_json::Value, not just via derive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEditPlan {
    /// Schema version for compatibility
    pub version: u32,

    /// List of actions to perform
    pub actions: Vec<AIAction>,

    /// LLM's reasoning (for audit/debugging)
    pub thought_process: Option<String>,

    /// LLM's confidence (0.0-1.0)
    pub confidence: Option<f64>,
}

/// A single action in an AI edit plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAction {
    /// Action type name
    pub action_type: String,

    /// Target clip ID (for delete, move, trim, split)
    pub clip_id: Option<String>,

    /// Clip data (for add)
    pub clip_data: Option<AIClipData>,

    /// New start time in seconds (for move)
    pub new_start_time: Option<f64>,

    /// New track ID (for move)
    pub new_track_id: Option<String>,

    /// Trim start delta in seconds
    pub trim_start_delta: Option<f64>,

    /// Trim end delta in seconds
    pub trim_end_delta: Option<f64>,

    /// Split position in seconds (relative to clip start)
    pub split_time: Option<f64>,
}

/// Clip data from AI (for add_clip action).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIClipData {
    pub id: String,
    pub track_id: String,
    pub start: f64,
    pub duration: f64,
    pub source_file: String,
}

// =============================================================================
// AI PIPELINE
// =============================================================================

/// The hardened AI processing pipeline.
pub struct AIPipeline<'a> {
    engine: &'a TimelineEngine,
}

impl<'a> AIPipeline<'a> {
    /// Create a new AI pipeline bound to an engine.
    pub fn new(engine: &'a TimelineEngine) -> Self {
        Self { engine }
    }

    /// Process an untrusted AI response through all validation stages.
    ///
    /// # Pipeline Stages
    ///
    /// 1. Parse JSON
    /// 2. Validate schema
    /// 3. Validate safety limits (counts, bounds)
    /// 4. Validate semantics (references exist)
    /// 5. Validate safety rules (content validation)
    /// 6. Convert to EditActions
    /// 7. Apply atomically to engine
    ///
    /// # Atomicity
    ///
    /// If ANY stage fails, no state change occurs.
    pub fn process(&self, response: UntrustedAIResponse) -> AIResult {
        // Stage 1: Parse JSON
        let json_value = match self.parse_json(response.raw()) {
            Ok(v) => v,
            Err(failure) => return AIResult::rejected(failure),
        };

        // Stage 2: Validate schema
        let plan = match self.validate_schema(&json_value) {
            Ok(p) => p,
            Err(failure) => return AIResult::rejected(failure),
        };

        // Stage 3: Validate safety limits (fail-fast before expensive checks)
        let state = self.engine.snapshot();
        if let Err(failure) = self.validate_safety_limits(&plan) {
            return AIResult::rejected(failure);
        }

        // Stage 4: Validate semantics
        if let Err(failure) = self.validate_semantics(&plan, &state) {
            return AIResult::rejected(failure);
        }

        // Stage 5: Validate safety content (paths, positions, etc.)
        if let Err(failure) = self.validate_safety_content(&plan) {
            return AIResult::rejected(failure);
        }

        // Stage 6: Convert to EditActions
        let actions = match self.convert_to_actions(&plan) {
            Ok(a) => a,
            Err(failure) => return AIResult::rejected(failure),
        };

        // Stage 7: Apply atomically
        match self.apply_atomically(actions) {
            Ok(count) => AIResult::accepted(count, plan.thought_process),
            Err(failure) => AIResult::rejected(failure),
        }
    }

    // =========================================================================
    // STAGE 1: JSON PARSING
    // =========================================================================

    fn parse_json(&self, raw: &str) -> Result<Value, AIFailure> {
        // Try to find JSON in the response (LLMs sometimes add prose)
        let json_str = Self::extract_json(raw).unwrap_or(raw);

        serde_json::from_str(json_str).map_err(|e| AIFailure::ParseError {
            message: e.to_string(),
            position: Some(e.column()),
        })
    }

    /// Extract JSON from LLM response that may include prose.
    fn extract_json(raw: &str) -> Option<&str> {
        // Try to find JSON block markers
        if let Some(start) = raw.find("```json") {
            let content_start = start + 7;
            if let Some(end) = raw[content_start..].find("```") {
                return Some(&raw[content_start..content_start + end]);
            }
        }

        // Try to find raw JSON object
        if let Some(start) = raw.find('{') {
            if let Some(end) = raw.rfind('}') {
                if end > start {
                    return Some(&raw[start..=end]);
                }
            }
        }

        None
    }

    // =========================================================================
    // STAGE 2: SCHEMA VALIDATION
    // =========================================================================

    fn validate_schema(&self, value: &Value) -> Result<AIEditPlan, AIFailure> {
        // Must be an object
        let obj = value
            .as_object()
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "Root must be an object".into(),
                field: None,
            })?;

        // Check for unknown fields
        let known_fields: HashSet<&str> = ["version", "actions", "thought_process", "confidence"]
            .into_iter()
            .collect();

        for key in obj.keys() {
            if !known_fields.contains(key.as_str()) {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::UnknownField { field: key.clone() },
                    message: format!("Unknown field: {}", key),
                });
            }
        }

        // Validate version
        let version = obj.get("version").and_then(|v| v.as_u64()).ok_or_else(|| {
            AIFailure::SchemaViolation {
                message: "Missing or invalid 'version' field".into(),
                field: Some("version".into()),
            }
        })? as u32;

        if version != SCHEMA_VERSION {
            return Err(AIFailure::SchemaViolation {
                message: format!("Unsupported schema version: {}", version),
                field: Some("version".into()),
            });
        }

        // Validate actions array
        let actions_value = obj
            .get("actions")
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "Missing 'actions' field".into(),
                field: Some("actions".into()),
            })?;

        let actions_arr = actions_value
            .as_array()
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "'actions' must be an array".into(),
                field: Some("actions".into()),
            })?;

        // Parse each action
        let mut actions = Vec::with_capacity(actions_arr.len());
        for (i, action_value) in actions_arr.iter().enumerate() {
            let action = self.validate_action_schema(action_value, i)?;
            actions.push(action);
        }

        // Optional fields
        let thought_process = obj
            .get("thought_process")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let confidence = obj.get("confidence").and_then(|v| v.as_f64());

        Ok(AIEditPlan {
            version,
            actions,
            thought_process,
            confidence,
        })
    }

    fn validate_action_schema(&self, value: &Value, index: usize) -> Result<AIAction, AIFailure> {
        let obj = value
            .as_object()
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: format!("Action {} must be an object", index),
                field: Some(format!("actions[{}]", index)),
            })?;

        // Check for unknown fields in action
        let known_action_fields: HashSet<&str> = [
            "action_type",
            "clip_id",
            "clip_data",
            "new_start_time",
            "new_track_id",
            "trim_start_delta",
            "trim_end_delta",
            "split_time",
        ]
        .into_iter()
        .collect();

        for key in obj.keys() {
            if !known_action_fields.contains(key.as_str()) {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::UnknownField { field: key.clone() },
                    message: format!("Unknown field in action {}: {}", index, key),
                });
            }
        }

        // Required: action_type
        let action_type = obj
            .get("action_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: format!("Action {} missing 'action_type'", index),
                field: Some(format!("actions[{}].action_type", index)),
            })?
            .to_string();

        // Validate action_type is known
        match action_type.as_str() {
            "add_clip" | "delete_clip" | "move_clip" | "trim_clip" | "split_clip" => {}
            _ => {
                return Err(AIFailure::SchemaViolation {
                    message: format!("Unknown action type: {}", action_type),
                    field: Some(format!("actions[{}].action_type", index)),
                })
            }
        }

        // Parse optional fields
        let clip_id = obj
            .get("clip_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let clip_data = if let Some(cd_value) = obj.get("clip_data") {
            Some(self.validate_clip_data_schema(cd_value, index)?)
        } else {
            None
        };

        let new_start_time = obj.get("new_start_time").and_then(|v| v.as_f64());
        let new_track_id = obj
            .get("new_track_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let trim_start_delta = obj.get("trim_start_delta").and_then(|v| v.as_f64());
        let trim_end_delta = obj.get("trim_end_delta").and_then(|v| v.as_f64());
        let split_time = obj.get("split_time").and_then(|v| v.as_f64());

        Ok(AIAction {
            action_type,
            clip_id,
            clip_data,
            new_start_time,
            new_track_id,
            trim_start_delta,
            trim_end_delta,
            split_time,
        })
    }

    fn validate_clip_data_schema(
        &self,
        value: &Value,
        action_index: usize,
    ) -> Result<AIClipData, AIFailure> {
        let obj = value
            .as_object()
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: format!("clip_data in action {} must be an object", action_index),
                field: Some(format!("actions[{}].clip_data", action_index)),
            })?;

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "clip_data.id required".into(),
                field: Some("clip_data.id".into()),
            })?
            .to_string();

        let track_id = obj
            .get("track_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "clip_data.track_id required".into(),
                field: Some("clip_data.track_id".into()),
            })?
            .to_string();

        let start = obj.get("start").and_then(|v| v.as_f64()).ok_or_else(|| {
            AIFailure::SchemaViolation {
                message: "clip_data.start required".into(),
                field: Some("clip_data.start".into()),
            }
        })?;

        let duration = obj
            .get("duration")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "clip_data.duration required".into(),
                field: Some("clip_data.duration".into()),
            })?;

        let source_file = obj
            .get("source_file")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AIFailure::SchemaViolation {
                message: "clip_data.source_file required".into(),
                field: Some("clip_data.source_file".into()),
            })?
            .to_string();

        Ok(AIClipData {
            id,
            track_id,
            start,
            duration,
            source_file,
        })
    }

    // =========================================================================
    // STAGE 3: SEMANTIC VALIDATION
    // =========================================================================

    fn validate_semantics(
        &self,
        plan: &AIEditPlan,
        state: &TimelineState,
    ) -> Result<(), AIFailure> {
        let existing_ids: HashSet<_> = state.clips.iter().map(|c| c.id.as_str()).collect();

        for (i, action) in plan.actions.iter().enumerate() {
            // For actions that reference existing clips, verify they exist
            if let Some(ref clip_id) = action.clip_id {
                match action.action_type.as_str() {
                    "delete_clip" | "move_clip" | "trim_clip" | "split_clip" => {
                        if !existing_ids.contains(clip_id.as_str()) {
                            return Err(AIFailure::SafetyViolation {
                                rule: SafetyRule::NonExistentClipId {
                                    clip_id: clip_id.clone(),
                                },
                                message: format!(
                                    "Clip '{}' does not exist (action {})",
                                    clip_id, i
                                ),
                            });
                        }
                    }
                    _ => {}
                }
            }

            // For add_clip, verify clip data is present
            if action.action_type == "add_clip" && action.clip_data.is_none() {
                return Err(AIFailure::SemanticViolation {
                    message: format!("add_clip action {} requires clip_data", i),
                    clip_id: None,
                });
            }

            // For delete/move/trim/split, verify clip_id is present
            match action.action_type.as_str() {
                "delete_clip" | "move_clip" | "trim_clip" | "split_clip" => {
                    if action.clip_id.is_none() {
                        return Err(AIFailure::SemanticViolation {
                            message: format!(
                                "{} action {} requires clip_id",
                                action.action_type, i
                            ),
                            clip_id: None,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    // =========================================================================
    // STAGE 3: SAFETY LIMITS VALIDATION
    // =========================================================================

    /// Validate safety limits (counts, bounds) - fail fast before expensive checks.
    fn validate_safety_limits(&self, plan: &AIEditPlan) -> Result<(), AIFailure> {
        // Rule: Max actions per request
        if plan.actions.len() > MAX_ACTIONS_PER_REQUEST {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::TooManyActions {
                    count: plan.actions.len(),
                    max: MAX_ACTIONS_PER_REQUEST,
                },
                message: format!(
                    "Too many actions: {} (max {})",
                    plan.actions.len(),
                    MAX_ACTIONS_PER_REQUEST
                ),
            });
        }

        // Rule: Max deletes per request
        let delete_count = plan
            .actions
            .iter()
            .filter(|a| a.action_type == "delete_clip")
            .count();

        if delete_count > MAX_DELETES_PER_REQUEST {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::TooManyDeletes {
                    count: delete_count,
                    max: MAX_DELETES_PER_REQUEST,
                },
                message: format!(
                    "Too many deletes: {} (max {})",
                    delete_count, MAX_DELETES_PER_REQUEST
                ),
            });
        }

        // Rule: Max affected clips
        let affected_clips: HashSet<_> = plan
            .actions
            .iter()
            .filter_map(|a| a.clip_id.as_ref())
            .collect();

        if affected_clips.len() > MAX_AFFECTED_CLIPS {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::TooManyAffectedClips {
                    count: affected_clips.len(),
                    max: MAX_AFFECTED_CLIPS,
                },
                message: format!(
                    "Too many affected clips: {} (max {})",
                    affected_clips.len(),
                    MAX_AFFECTED_CLIPS
                ),
            });
        }

        Ok(())
    }

    // =========================================================================
    // STAGE 5: SAFETY CONTENT VALIDATION
    // =========================================================================

    /// Validate safety content (positions, durations, paths).
    fn validate_safety_content(&self, plan: &AIEditPlan) -> Result<(), AIFailure> {
        for action in &plan.actions {
            self.validate_action_safety(action)?;
        }
        Ok(())
    }

    fn validate_action_safety(&self, action: &AIAction) -> Result<(), AIFailure> {
        // Rule: No negative positions
        if let Some(new_start) = action.new_start_time {
            if new_start < 0.0 {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::NegativePosition {
                        clip_id: action.clip_id.clone().unwrap_or_default(),
                        value: (new_start * 1_000_000_000.0) as i64,
                    },
                    message: format!("Negative position: {}", new_start),
                });
            }
        }

        // Rule: No zero/negative durations
        if let Some(ref clip_data) = action.clip_data {
            if clip_data.duration <= 0.0 {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::InvalidDuration {
                        clip_id: clip_data.id.clone(),
                        value: (clip_data.duration * 1_000_000_000.0) as i64,
                    },
                    message: format!("Invalid duration: {}", clip_data.duration),
                });
            }

            if clip_data.start < 0.0 {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::NegativePosition {
                        clip_id: clip_data.id.clone(),
                        value: (clip_data.start * 1_000_000_000.0) as i64,
                    },
                    message: format!("Negative start position: {}", clip_data.start),
                });
            }

            // Rule: No filesystem access / path traversal / absolute paths
            self.validate_source_file(&clip_data.source_file)?;
        }

        Ok(())
    }

    fn validate_source_file(&self, path: &str) -> Result<(), AIFailure> {
        // Rule: No path traversal
        if path.contains("..") {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::PathTraversal {
                    path: path.to_string(),
                },
                message: format!("Path traversal detected: {}", path),
            });
        }

        // Rule: No absolute paths
        if path.starts_with('/') || path.starts_with('\\') {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::AbsolutePath {
                    path: path.to_string(),
                },
                message: format!("Absolute path not allowed: {}", path),
            });
        }

        // Check for Windows absolute paths
        if path.len() >= 2 && path.chars().nth(1) == Some(':') {
            return Err(AIFailure::SafetyViolation {
                rule: SafetyRule::AbsolutePath {
                    path: path.to_string(),
                },
                message: format!("Absolute path not allowed: {}", path),
            });
        }

        // Rule: No suspicious filesystem access patterns
        let suspicious = ["etc/passwd", "etc/shadow", ".ssh", "id_rsa", ".env"];
        for pattern in suspicious {
            if path.to_lowercase().contains(pattern) {
                return Err(AIFailure::SafetyViolation {
                    rule: SafetyRule::FileSystemAccess {
                        path: path.to_string(),
                    },
                    message: format!("Suspicious path access: {}", path),
                });
            }
        }

        Ok(())
    }

    // =========================================================================
    // STAGE 5: CONVERT TO EDIT ACTIONS
    // =========================================================================

    fn convert_to_actions(&self, plan: &AIEditPlan) -> Result<Vec<EditAction>, AIFailure> {
        let mut actions = Vec::with_capacity(plan.actions.len());

        for (i, ai_action) in plan.actions.iter().enumerate() {
            let action = self.convert_action(ai_action, i)?;
            actions.push(action);
        }

        Ok(actions)
    }

    fn convert_action(&self, ai_action: &AIAction, index: usize) -> Result<EditAction, AIFailure> {
        let action_type = match ai_action.action_type.as_str() {
            "add_clip" => ActionType::AddClip,
            "delete_clip" => ActionType::DeleteClip,
            "move_clip" => ActionType::MoveClip,
            "trim_clip" => ActionType::TrimClip,
            "split_clip" => ActionType::SplitClip,
            other => {
                return Err(AIFailure::SchemaViolation {
                    message: format!("Unknown action type: {}", other),
                    field: Some(format!("actions[{}].action_type", index)),
                })
            }
        };

        let mut action = EditAction::new(action_type);

        // Set clip_id if present
        if let Some(ref clip_id) = ai_action.clip_id {
            action.clip_id = Some(clip_id.clone());
        }

        // Set clip_data if present
        if let Some(ref cd) = ai_action.clip_data {
            action.clip_data = Some(Clip::new(
                cd.id.clone(),
                cd.track_id.clone(),
                MediaTime::from_seconds(cd.start),
                MediaTime::from_seconds(cd.duration),
                cd.source_file.clone(),
            ));
        }

        // Set parameters
        action.parameters = ActionParameters {
            new_start_time: ai_action.new_start_time.map(MediaTime::from_seconds),
            new_track_id: ai_action.new_track_id.clone(),
            trim_start_delta: ai_action.trim_start_delta.map(MediaTime::from_seconds),
            trim_end_delta: ai_action.trim_end_delta.map(MediaTime::from_seconds),
            split_time: ai_action.split_time.map(MediaTime::from_seconds),
        };

        Ok(action)
    }

    // =========================================================================
    // STAGE 6: ATOMIC APPLICATION
    // =========================================================================

    /// Apply all actions atomically.
    ///
    /// # No Partial Application
    ///
    /// If ANY action fails, ALL actions are rolled back.
    fn apply_atomically(&self, actions: Vec<EditAction>) -> Result<usize, AIFailure> {
        let initial_version = self.engine.version();
        let action_count = actions.len();

        // Apply each action
        for (i, action) in actions.into_iter().enumerate() {
            if let Err(e) = self.engine.apply_action(action) {
                // Rollback by undoing all applied actions
                self.rollback_to_version(initial_version);

                return Err(AIFailure::EngineRejected {
                    message: e.to_string(),
                    action_index: Some(i),
                });
            }
        }

        Ok(action_count)
    }

    /// Rollback to a previous version by undoing.
    fn rollback_to_version(&self, target_version: u64) {
        while self.engine.version() > target_version && self.engine.can_undo() {
            let _ = self.engine.undo();
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine_with_clip() -> TimelineEngine {
        let engine = TimelineEngine::new();
        let clip = Clip::new(
            "c1",
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );
        let action = EditAction::add_clip(clip);
        engine.apply_action(action).unwrap();
        engine
    }

    #[test]
    fn test_parse_error() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw("not valid json".into());
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(failure, AIFailure::ParseError { .. }));
            }
            _ => panic!("Expected parse error"),
        }
    }

    #[test]
    fn test_schema_violation_missing_version() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(r#"{"actions": []}"#.into());
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(failure, AIFailure::SchemaViolation { .. }));
            }
            _ => panic!("Expected schema violation"),
        }
    }

    #[test]
    fn test_unknown_field_rejected() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [],
            "evil_field": "hack"
        }"#
            .into(),
        );
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::UnknownField { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for unknown field"),
        }
    }

    #[test]
    fn test_semantic_violation_nonexistent_clip() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {"action_type": "delete_clip", "clip_id": "nonexistent"}
            ]
        }"#
            .into(),
        );
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::NonExistentClipId { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for nonexistent clip"),
        }
    }

    #[test]
    fn test_safety_negative_position() {
        let engine = make_engine_with_clip();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {"action_type": "move_clip", "clip_id": "c1", "new_start_time": -5.0}
            ]
        }"#
            .into(),
        );
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::NegativePosition { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for negative position"),
        }
    }

    #[test]
    fn test_safety_path_traversal() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {
                    "action_type": "add_clip",
                    "clip_data": {
                        "id": "evil",
                        "track_id": "t1",
                        "start": 0,
                        "duration": 5,
                        "source_file": "../../../etc/passwd"
                    }
                }
            ]
        }"#
            .into(),
        );
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::PathTraversal { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for path traversal"),
        }
    }

    #[test]
    fn test_safety_absolute_path() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {
                    "action_type": "add_clip",
                    "clip_data": {
                        "id": "evil",
                        "track_id": "t1",
                        "start": 0,
                        "duration": 5,
                        "source_file": "/etc/passwd"
                    }
                }
            ]
        }"#
            .into(),
        );
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::AbsolutePath { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for absolute path"),
        }
    }

    #[test]
    fn test_too_many_deletes() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        // Create 15 delete actions (exceeds MAX_DELETES_PER_REQUEST = 10)
        let delete_actions: Vec<_> = (0..15)
            .map(|i| format!(r#"{{"action_type": "delete_clip", "clip_id": "c{}"}}"#, i))
            .collect();

        let json = format!(
            r#"{{"version": 1, "actions": [{}]}}"#,
            delete_actions.join(",")
        );
        let response = UntrustedAIResponse::from_raw(json);
        let result = pipeline.process(response);

        match result {
            AIResult::Rejected { failure, .. } => {
                assert!(matches!(
                    failure,
                    AIFailure::SafetyViolation {
                        rule: SafetyRule::TooManyDeletes { .. },
                        ..
                    }
                ));
            }
            _ => panic!("Expected safety violation for too many deletes"),
        }
    }

    #[test]
    fn test_successful_add_clip() {
        let engine = TimelineEngine::new();
        let pipeline = AIPipeline::new(&engine);

        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {
                    "action_type": "add_clip",
                    "clip_data": {
                        "id": "c1",
                        "track_id": "track1",
                        "start": 0,
                        "duration": 5,
                        "source_file": "video.mp4"
                    }
                }
            ],
            "thought_process": "Adding a clip to the timeline"
        }"#
            .into(),
        );

        let result = pipeline.process(response);

        match result {
            AIResult::Accepted {
                actions_applied,
                thought_process,
            } => {
                assert_eq!(actions_applied, 1);
                assert_eq!(
                    thought_process,
                    Some("Adding a clip to the timeline".to_string())
                );
                assert_eq!(engine.clip_count(), 1);
            }
            AIResult::Rejected { failure, .. } => {
                panic!("Expected success, got: {:?}", failure);
            }
        }
    }

    #[test]
    fn test_no_partial_application() {
        let engine = make_engine_with_clip();
        let initial_count = engine.clip_count();
        let pipeline = AIPipeline::new(&engine);

        // First action valid, second invalid (nonexistent clip)
        let response = UntrustedAIResponse::from_raw(
            r#"{
            "version": 1,
            "actions": [
                {
                    "action_type": "add_clip",
                    "clip_data": {
                        "id": "c2",
                        "track_id": "track1",
                        "start": 10,
                        "duration": 5,
                        "source_file": "video2.mp4"
                    }
                },
                {"action_type": "delete_clip", "clip_id": "nonexistent"}
            ]
        }"#
            .into(),
        );

        let result = pipeline.process(response);

        // Should fail due to nonexistent clip
        assert!(matches!(result, AIResult::Rejected { .. }));

        // State should be unchanged (first action rolled back)
        assert_eq!(engine.clip_count(), initial_count);
    }
}
