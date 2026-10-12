//! Workspace Error - Error types for workspace operations.
//!
//! # Invariants
//!
//! - All workspace operations return Result<_, WorkspaceError>
//! - Errors are descriptive and actionable
//! - Errors are serializable for logging/reporting

use serde::{Deserialize, Serialize};

use super::workspace_types::{PanelId, ProjectId};

// =============================================================================
// WORKSPACE ERROR
// =============================================================================

/// Error type for workspace operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkspaceError {
    /// Project not found
    ProjectNotFound { id: ProjectId },

    /// Panel not found
    PanelNotFound { id: PanelId },

    /// Project already exists
    ProjectAlreadyExists { id: ProjectId },

    /// Panel already exists
    PanelAlreadyExists { id: PanelId },

    /// No active project
    NoActiveProject,

    /// Cannot close last project
    CannotCloseLastProject,

    /// Invalid panel position
    InvalidPanelPosition { id: PanelId, reason: String },

    /// Invalid window state
    InvalidWindowState { reason: String },

    /// Lock acquisition failed (internal)
    LockFailed,

    /// Persistence error
    PersistenceError { message: String },

    /// Checksum validation failed
    ChecksumInvalid,

    /// Version mismatch
    VersionMismatch { expected: u32, found: u32 },

    /// Command not applicable in current state
    CommandNotApplicable { command: String, reason: String },
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectNotFound { id } => write!(f, "Project not found: {}", id.0),
            Self::PanelNotFound { id } => write!(f, "Panel not found: {}", id.0),
            Self::ProjectAlreadyExists { id } => write!(f, "Project already exists: {}", id.0),
            Self::PanelAlreadyExists { id } => write!(f, "Panel already exists: {}", id.0),
            Self::NoActiveProject => write!(f, "No active project"),
            Self::CannotCloseLastProject => write!(f, "Cannot close last project"),
            Self::InvalidPanelPosition { id, reason } => {
                write!(f, "Invalid panel position for {}: {}", id.0, reason)
            }
            Self::InvalidWindowState { reason } => write!(f, "Invalid window state: {}", reason),
            Self::LockFailed => write!(f, "Lock acquisition failed"),
            Self::PersistenceError { message } => write!(f, "Persistence error: {}", message),
            Self::ChecksumInvalid => write!(f, "Checksum validation failed"),
            Self::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "Version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            Self::CommandNotApplicable { command, reason } => {
                write!(f, "Command '{}' not applicable: {}", command, reason)
            }
        }
    }
}

impl std::error::Error for WorkspaceError {}

/// Result type for workspace operations.
pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = WorkspaceError::ProjectNotFound {
            id: ProjectId("test".to_string()),
        };
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_error_serializable() {
        let err = WorkspaceError::PanelNotFound {
            id: PanelId("panel.test".to_string()),
        };

        let json = serde_json::to_string(&err).unwrap();
        let deserialized: WorkspaceError = serde_json::from_str(&json).unwrap();

        assert_eq!(err, deserialized);
    }
}
