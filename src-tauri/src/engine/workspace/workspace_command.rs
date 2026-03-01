//! Workspace Commands - All mutations go through commands.
//!
//! # Invariants
//!
//! - All workspace mutations MUST go through WorkspaceCommand
//! - Commands are pure data representing intent
//! - Commands are serializable for replay/undo
//! - No side effects in command definitions

use serde::{Deserialize, Serialize};

use super::workspace_types::{
    PanelId, PanelPosition, ProjectId, WindowMode, WindowPosition, WindowSize,
};

// =============================================================================
// WORKSPACE COMMAND
// =============================================================================

/// Command for mutating workspace state.
///
/// # Invariants
///
/// - All mutations to WorkspaceState must be expressed as a command
/// - Commands are pure data: no logic, no side effects
/// - Commands are serializable for persistence/undo
/// - WorkspaceEngine.apply_command() is the only way to execute
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkspaceCommand {
    // =========================================================================
    // PROJECT COMMANDS
    // =========================================================================
    /// Create a new project
    CreateProject { name: String },

    /// Open an existing project
    OpenProject {
        id: ProjectId,
        name: String,
        path: Option<String>,
    },

    /// Close a project
    CloseProject { id: ProjectId },

    /// Set the active project
    SetActiveProject { id: ProjectId },

    /// Mark project as dirty (unsaved changes)
    MarkProjectDirty { id: ProjectId, dirty: bool },

    /// Rename a project
    RenameProject { id: ProjectId, name: String },

    /// Update project path (after save)
    SetProjectPath { id: ProjectId, path: String },

    // =========================================================================
    // PANEL COMMANDS
    // =========================================================================
    /// Show a panel
    ShowPanel { id: PanelId },

    /// Hide a panel
    HidePanel { id: PanelId },

    /// Toggle panel visibility
    TogglePanel { id: PanelId },

    /// Move panel to new position
    MovePanel {
        id: PanelId,
        position: PanelPosition,
    },

    /// Resize panel
    ResizePanel { id: PanelId, size: u32 },

    /// Collapse/expand panel
    SetPanelCollapsed { id: PanelId, collapsed: bool },

    /// Reorder panel within dock region
    ReorderPanel { id: PanelId, order: u32 },

    /// Add a new panel
    AddPanel {
        id: PanelId,
        title: String,
        position: PanelPosition,
    },

    /// Remove a panel
    RemovePanel { id: PanelId },

    // =========================================================================
    // FOCUS COMMANDS
    // =========================================================================
    /// Focus a panel
    FocusPanel { id: PanelId },

    /// Clear focus (no panel focused)
    ClearFocus,

    /// Cycle focus to next panel
    FocusNext,

    /// Cycle focus to previous panel
    FocusPrevious,

    // =========================================================================
    // WINDOW COMMANDS
    // =========================================================================
    /// Set window size
    SetWindowSize { size: WindowSize },

    /// Set window position
    SetWindowPosition { position: WindowPosition },

    /// Set window mode (normal, maximized, fullscreen, minimized)
    SetWindowMode { mode: WindowMode },

    /// Toggle maximized state
    ToggleMaximized,

    /// Toggle fullscreen state
    ToggleFullscreen,

    /// Set always on top
    SetAlwaysOnTop { enabled: bool },

    // =========================================================================
    // WORKSPACE COMMANDS
    // =========================================================================
    /// Rename workspace
    RenameWorkspace { name: String },

    /// Reset workspace to defaults
    ResetToDefaults,

    /// Update last modified timestamp
    Touch,
}

impl WorkspaceCommand {
    /// Get a human-readable description of the command.
    pub fn description(&self) -> &'static str {
        match self {
            Self::CreateProject { .. } => "Create project",
            Self::OpenProject { .. } => "Open project",
            Self::CloseProject { .. } => "Close project",
            Self::SetActiveProject { .. } => "Set active project",
            Self::MarkProjectDirty { .. } => "Mark project dirty",
            Self::RenameProject { .. } => "Rename project",
            Self::SetProjectPath { .. } => "Set project path",
            Self::ShowPanel { .. } => "Show panel",
            Self::HidePanel { .. } => "Hide panel",
            Self::TogglePanel { .. } => "Toggle panel",
            Self::MovePanel { .. } => "Move panel",
            Self::ResizePanel { .. } => "Resize panel",
            Self::SetPanelCollapsed { .. } => "Set panel collapsed",
            Self::ReorderPanel { .. } => "Reorder panel",
            Self::AddPanel { .. } => "Add panel",
            Self::RemovePanel { .. } => "Remove panel",
            Self::FocusPanel { .. } => "Focus panel",
            Self::ClearFocus => "Clear focus",
            Self::FocusNext => "Focus next",
            Self::FocusPrevious => "Focus previous",
            Self::SetWindowSize { .. } => "Set window size",
            Self::SetWindowPosition { .. } => "Set window position",
            Self::SetWindowMode { .. } => "Set window mode",
            Self::ToggleMaximized => "Toggle maximized",
            Self::ToggleFullscreen => "Toggle fullscreen",
            Self::SetAlwaysOnTop { .. } => "Set always on top",
            Self::RenameWorkspace { .. } => "Rename workspace",
            Self::ResetToDefaults => "Reset to defaults",
            Self::Touch => "Touch",
        }
    }

    /// Check if command is undoable.
    pub fn is_undoable(&self) -> bool {
        match self {
            Self::Touch => false,
            _ => true,
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serializable() {
        let cmd = WorkspaceCommand::ShowPanel {
            id: PanelId("test".to_string()),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: WorkspaceCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn test_command_description() {
        let cmd = WorkspaceCommand::FocusPanel {
            id: PanelId("test".to_string()),
        };
        assert_eq!(cmd.description(), "Focus panel");
    }

    #[test]
    fn test_command_undoable() {
        let cmd = WorkspaceCommand::MovePanel {
            id: PanelId("test".to_string()),
            position: PanelPosition::Left,
        };
        assert!(cmd.is_undoable());

        let touch = WorkspaceCommand::Touch;
        assert!(!touch.is_undoable());
    }

    #[test]
    fn test_all_commands_serializable() {
        let commands = vec![
            WorkspaceCommand::CreateProject {
                name: "Test".to_string(),
            },
            WorkspaceCommand::CloseProject {
                id: ProjectId("p1".to_string()),
            },
            WorkspaceCommand::ShowPanel {
                id: PanelId("panel".to_string()),
            },
            WorkspaceCommand::SetWindowSize {
                size: WindowSize {
                    width: 800,
                    height: 600,
                },
            },
            WorkspaceCommand::ToggleMaximized,
            WorkspaceCommand::ResetToDefaults,
        ];

        for cmd in commands {
            let json = serde_json::to_string(&cmd).unwrap();
            let _: WorkspaceCommand = serde_json::from_str(&json).unwrap();
        }
    }
}
