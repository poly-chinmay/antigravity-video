//! EngineError - All error types for TimelineEngine operations.
//!
//! # Design
//!
//! Errors are categorized by source:
//! - Validation errors (invariant violations)
//! - Data errors (missing/invalid parameters)
//! - Storage errors (persistence failures)
//! - Undo errors (undo/redo failures)
//! - Safety errors (panic recovery, confirmation required)

use crate::engine::invariants::InvariantViolation;

/// All errors that can occur in TimelineEngine operations.
#[derive(Debug)]
pub enum EngineError {
    // =========================================================================
    // VALIDATION ERRORS
    // =========================================================================
    /// Pre-mutation invariant check failed
    PreValidationFailed(InvariantViolation),

    /// Post-mutation invariant check failed
    PostValidationFailed(InvariantViolation),

    // =========================================================================
    // DATA ERRORS
    // =========================================================================
    /// Action requires clip_data but none provided
    MissingClipData,

    /// Action requires clip_id but none provided
    MissingClipId,

    /// Required parameter is missing
    MissingParameter(String),

    /// Referenced clip does not exist
    ClipNotFound(String),

    /// Clip ID already exists
    DuplicateClipId(String),

    // =========================================================================
    // VALUE ERRORS
    // =========================================================================
    /// Position is invalid (e.g., negative)
    InvalidPosition(i64),

    /// Duration is invalid (e.g., zero or negative)
    InvalidDuration(i64),

    /// Split position is invalid (outside clip bounds)
    InvalidSplitPosition(i64),

    /// Trim would extend source_in past source start (negative)
    TrimBeyondSourceStart { current_source_in: i64, delta: i64 },

    /// Trim would extend source_out past source duration
    TrimBeyondSourceEnd {
        source_duration: i64,
        requested_source_out: i64,
    },

    /// Source file is unavailable (deleted, moved, or inaccessible)
    SourceUnavailable {
        clip_id: String,
        source_file: String,
        reason: String,
    },

    // =========================================================================
    // STORAGE ERRORS
    // =========================================================================
    /// Storage initialization failed
    StorageInit(String),

    /// Event store operation failed
    EventStoreFailed(String),

    // =========================================================================
    // UNDO ERRORS
    // =========================================================================
    /// Undo capture failed
    UndoCaptureFailed(String),

    /// Undo operation failed
    UndoFailed(String),

    /// Redo operation failed
    RedoFailed(String),

    /// Nothing to undo
    NothingToUndo,

    /// Nothing to redo
    NothingToRedo,

    // =========================================================================
    // SAFETY ERRORS
    // =========================================================================
    /// A panic occurred during mutation (state was rolled back)
    PanicRecovered { message: String },

    /// Operation affects too many clips, confirmation required
    ConfirmationRequired {
        clip_count: usize,
        action_type: String,
    },

    /// Engine state is locked (concurrent access issue)
    EngineLocked,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Validation
            Self::PreValidationFailed(v) => write!(f, "Pre-validation failed: {}", v),
            Self::PostValidationFailed(v) => write!(f, "Post-validation failed: {}", v),

            // Data
            Self::MissingClipData => write!(f, "Action requires clip_data but none provided"),
            Self::MissingClipId => write!(f, "Action requires clip_id but none provided"),
            Self::MissingParameter(p) => write!(f, "Missing required parameter: {}", p),
            Self::ClipNotFound(id) => write!(f, "Clip not found: {}", id),
            Self::DuplicateClipId(id) => write!(f, "Duplicate clip ID: {}", id),

            // Value
            Self::InvalidPosition(p) => write!(f, "Invalid position: {} nanos", p),
            Self::InvalidDuration(d) => write!(f, "Invalid duration: {} nanos", d),
            Self::InvalidSplitPosition(p) => write!(f, "Invalid split position: {} nanos", p),
            Self::TrimBeyondSourceStart {
                current_source_in,
                delta,
            } => {
                write!(
                    f,
                    "Cannot trim start: source_in {} + delta {} would be negative",
                    current_source_in, delta
                )
            }
            Self::TrimBeyondSourceEnd {
                source_duration,
                requested_source_out,
            } => {
                write!(
                    f,
                    "Cannot trim end: source_out {} exceeds source duration {}",
                    requested_source_out, source_duration
                )
            }
            Self::SourceUnavailable {
                clip_id,
                source_file,
                reason,
            } => {
                write!(
                    f,
                    "Source unavailable for clip {}: {} - {}",
                    clip_id, source_file, reason
                )
            }

            // Storage
            Self::StorageInit(e) => write!(f, "Storage initialization failed: {}", e),
            Self::EventStoreFailed(e) => write!(f, "Event store operation failed: {}", e),

            // Undo
            Self::UndoCaptureFailed(e) => write!(f, "Undo capture failed: {}", e),
            Self::UndoFailed(e) => write!(f, "Undo failed: {}", e),
            Self::RedoFailed(e) => write!(f, "Redo failed: {}", e),
            Self::NothingToUndo => write!(f, "Nothing to undo"),
            Self::NothingToRedo => write!(f, "Nothing to redo"),

            // Safety
            Self::PanicRecovered { message } => {
                write!(f, "Panic recovered (state rolled back): {}", message)
            }
            Self::ConfirmationRequired {
                clip_count,
                action_type,
            } => {
                write!(
                    f,
                    "Confirmation required: {} affects {} clips",
                    action_type, clip_count
                )
            }
            Self::EngineLocked => write!(f, "Engine is locked by another operation"),
        }
    }
}

impl std::error::Error for EngineError {}

// Serialize for Tauri
impl serde::Serialize for EngineError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
