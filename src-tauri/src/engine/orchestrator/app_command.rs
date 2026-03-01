//! App Command - Unified command routing for orchestrator.
//!
//! # Design
//!
//! AppCommand wraps all engine-specific commands into a single type.
//! The orchestrator routes these to the appropriate engine and
//! handles cross-engine coordination.
//!
//! # Invariants
//!
//! - All commands are serializable for replay/persistence
//! - Commands are routed through orchestrator, never directly to engines

use serde::{Deserialize, Serialize};

use crate::engine::edit_action::EditAction;
use crate::engine::playback::TransportCommand;
use crate::engine::workspace::WorkspaceCommand;

// =============================================================================
// APP COMMAND
// =============================================================================

/// Unified command for application orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppCommand {
    /// Workspace command
    Workspace(WorkspaceCommand),

    /// Timeline edit action
    Timeline(TimelineCommand),

    /// Transport/Playback command
    Transport(TransportCommand),

    /// Compound command (multiple commands atomically)
    Compound(Vec<AppCommand>),

    /// System command (initialization, shutdown, etc.)
    System(SystemCommand),
}

/// Timeline-specific commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineCommand {
    /// Apply an edit action
    Apply(EditAction),

    /// Select clips
    Select { clip_ids: Vec<String> },

    /// Deselect all
    DeselectAll,

    /// Set zoom level
    SetZoom { zoom: f64 },

    /// Set scroll position
    SetScroll { position: i64 },
}

/// System-level commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemCommand {
    /// Initialize application
    Initialize,

    /// Shutdown application
    Shutdown,

    /// Force UI refresh
    RefreshUI,

    /// Clear all caches
    ClearCaches,

    /// Trigger autosave
    Autosave,
}

impl AppCommand {
    /// Create workspace command.
    pub fn workspace(cmd: WorkspaceCommand) -> Self {
        Self::Workspace(cmd)
    }

    /// Create timeline command from edit action.
    pub fn timeline(action: EditAction) -> Self {
        Self::Timeline(TimelineCommand::Apply(action))
    }

    /// Create transport command.
    pub fn transport(cmd: TransportCommand) -> Self {
        Self::Transport(cmd)
    }

    /// Create compound command.
    pub fn compound(commands: Vec<AppCommand>) -> Self {
        Self::Compound(commands)
    }

    /// Get human-readable description.
    pub fn description(&self) -> String {
        match self {
            Self::Workspace(cmd) => format!("Workspace: {}", cmd.description()),
            Self::Timeline(cmd) => format!("Timeline: {}", cmd.description()),
            Self::Transport(cmd) => format!("Transport: {:?}", cmd),
            Self::Compound(cmds) => format!("Compound: {} commands", cmds.len()),
            Self::System(cmd) => format!("System: {:?}", cmd),
        }
    }

    /// Check if command affects timeline.
    pub fn affects_timeline(&self) -> bool {
        matches!(self, Self::Timeline(_) | Self::Compound(_))
    }

    /// Check if command affects playback.
    pub fn affects_playback(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::Compound(_))
    }

    /// Check if command is undoable.
    pub fn is_undoable(&self) -> bool {
        match self {
            Self::Workspace(cmd) => cmd.is_undoable(),
            Self::Timeline(TimelineCommand::Apply(_)) => true,
            Self::Timeline(_) => false,
            Self::Transport(_) => false,
            Self::Compound(cmds) => cmds.iter().all(|c| c.is_undoable()),
            Self::System(_) => false,
        }
    }
}

impl TimelineCommand {
    /// Get description.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Apply(_) => "Apply edit",
            Self::Select { .. } => "Select clips",
            Self::DeselectAll => "Deselect all",
            Self::SetZoom { .. } => "Set zoom",
            Self::SetScroll { .. } => "Set scroll",
        }
    }
}

// =============================================================================
// COMMAND RESULT
// =============================================================================

/// Result of command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResult {
    /// Command succeeded
    Success,

    /// Command succeeded with info
    SuccessWithInfo(String),

    /// Command failed
    Failed(String),

    /// Command was no-op (state unchanged)
    NoOp,

    /// Command requires user confirmation
    RequiresConfirmation(String),
}

impl CommandResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::SuccessWithInfo(_) | Self::NoOp)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::workspace::workspace_types::PanelId;

    #[test]
    fn test_app_command_workspace() {
        let cmd = AppCommand::workspace(WorkspaceCommand::ShowPanel {
            id: PanelId("test".to_string()),
        });
        assert!(cmd.description().contains("Workspace"));
    }

    #[test]
    fn test_app_command_transport() {
        let cmd = AppCommand::transport(TransportCommand::Play);
        assert!(cmd.affects_playback());
        assert!(!cmd.affects_timeline());
    }

    #[test]
    fn test_command_serializable() {
        let cmd = AppCommand::System(SystemCommand::Initialize);
        let json = serde_json::to_string(&cmd).unwrap();
        let _: AppCommand = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_compound_command() {
        let compound = AppCommand::compound(vec![
            AppCommand::transport(TransportCommand::Stop),
            AppCommand::System(SystemCommand::RefreshUI),
        ]);

        match compound {
            AppCommand::Compound(cmds) => assert_eq!(cmds.len(), 2),
            _ => panic!("Expected compound"),
        }
    }

    #[test]
    fn test_command_result() {
        let success = CommandResult::Success;
        assert!(success.is_success());

        let failed = CommandResult::Failed("error".to_string());
        assert!(failed.is_error());
    }
}
