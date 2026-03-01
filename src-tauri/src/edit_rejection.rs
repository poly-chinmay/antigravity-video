// src-tauri/src/edit_rejection.rs
//! EditRejection - Deterministic error model for edit plan validation
//!
//! Provides structured error types for all validation failures in the
//! Invariants Engine. No panics - all failures return EditRejection.

use crate::edit_plan::EditAction;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Deterministic error model for edit plan validation failures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EditRejection {
    /// Referenced ClipId, TrackId, or MediaId does not exist
    InvalidReference {
        /// The ID that was not found
        id: String,
        /// Type of reference (e.g., "ClipId", "TrackId")
        ref_type: String,
    },

    /// Action violates bounds constraints (TRIM/SPLIT/MOVE)
    BoundsViolation {
        /// The action that violated bounds
        action: EditAction,
        /// Description of the violation
        reason: String,
    },

    /// Action would violate a timeline invariant
    InvariantViolation {
        /// Invariant number (1-6 from docs/invariants.md)
        invariant: u32,
        /// Description of the violation
        description: String,
    },

    /// Action would cause a conflict (e.g., overlap)
    ConflictDetected {
        /// Description of the conflict
        description: String,
    },
}

impl EditRejection {
    /// Create an InvalidReference error
    pub fn invalid_reference(id: impl Into<String>, ref_type: impl Into<String>) -> Self {
        Self::InvalidReference {
            id: id.into(),
            ref_type: ref_type.into(),
        }
    }

    /// Create a BoundsViolation error
    pub fn bounds_violation(action: EditAction, reason: impl Into<String>) -> Self {
        Self::BoundsViolation {
            action,
            reason: reason.into(),
        }
    }

    /// Create an InvariantViolation error
    pub fn invariant_violation(invariant: u32, description: impl Into<String>) -> Self {
        Self::InvariantViolation {
            invariant,
            description: description.into(),
        }
    }

    /// Create a ConflictDetected error
    pub fn conflict_detected(description: impl Into<String>) -> Self {
        Self::ConflictDetected {
            description: description.into(),
        }
    }

    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Self::InvalidReference { id, ref_type } => {
                format!("Referenced {} '{}' does not exist", ref_type, id)
            }
            Self::BoundsViolation { reason, .. } => {
                format!("Invalid operation: {}", reason)
            }
            Self::InvariantViolation {
                invariant,
                description,
            } => {
                format!("Would violate invariant #{}: {}", invariant, description)
            }
            Self::ConflictDetected { description } => {
                format!("Conflict detected: {}", description)
            }
        }
    }

    /// Get a technical error message for logging
    pub fn technical_message(&self) -> String {
        match self {
            Self::InvalidReference { id, ref_type } => {
                format!("INVALID_REFERENCE: {} '{}' not found", ref_type, id)
            }
            Self::BoundsViolation { action, reason } => {
                format!("BOUNDS_VIOLATION: {:?} - {}", action.action_type, reason)
            }
            Self::InvariantViolation {
                invariant,
                description,
            } => {
                format!("INVARIANT_VIOLATION: #{} - {}", invariant, description)
            }
            Self::ConflictDetected { description } => {
                format!("CONFLICT_DETECTED: {}", description)
            }
        }
    }
}

impl fmt::Display for EditRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for EditRejection {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::{ActionType, EditAction};

    #[test]
    fn test_invalid_reference_creation() {
        let err = EditRejection::invalid_reference("clip-123", "ClipId");
        assert!(matches!(err, EditRejection::InvalidReference { .. }));
        assert!(err.user_message().contains("clip-123"));
        assert!(err.user_message().contains("ClipId"));
    }

    #[test]
    fn test_bounds_violation_creation() {
        let action = EditAction {
            action_type: ActionType::Trim,
            target_clip_id: "clip-1".to_string(),
            parameters: None,
        };
        let err = EditRejection::bounds_violation(action, "Trim exceeds clip duration");
        assert!(matches!(err, EditRejection::BoundsViolation { .. }));
        assert!(err.user_message().contains("Trim exceeds"));
    }

    #[test]
    fn test_invariant_violation_creation() {
        let err = EditRejection::invariant_violation(1, "Duplicate ClipId detected");
        assert!(matches!(err, EditRejection::InvariantViolation { .. }));
        assert!(err.user_message().contains("invariant #1"));
    }

    #[test]
    fn test_conflict_detected_creation() {
        let err = EditRejection::conflict_detected("Clips would overlap");
        assert!(matches!(err, EditRejection::ConflictDetected { .. }));
        assert!(err.user_message().contains("overlap"));
    }

    #[test]
    fn test_technical_vs_user_messages() {
        let err = EditRejection::invalid_reference("clip-999", "ClipId");
        let user_msg = err.user_message();
        let tech_msg = err.technical_message();

        // User message should be friendly
        assert!(!user_msg.contains("INVALID_REFERENCE"));

        // Technical message should have error code
        assert!(tech_msg.contains("INVALID_REFERENCE"));
    }
}
