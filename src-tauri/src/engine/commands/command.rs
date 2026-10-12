//! Command - Command trait and types for the command system.
//!
//! # Design
//!
//! Commands are the primary way user actions are executed:
//! - Keyboard shortcuts trigger commands
//! - Tool bar buttons trigger commands
//! - Menu items trigger commands
//!
//! Commands NEVER mutate state directly. They route through:
//! - InteractionController (for tool operations)
//! - TimelineEngine (for EditActions)
//! - PlaybackScheduler (for transport)
//!
//! # Command Flow
//!
//! ```text
//! User Input → KeyMap → CommandId → Router → Handler → Result
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

// =============================================================================
// COMMAND ID
// =============================================================================

/// Unique identifier for a command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandId(pub String);

impl CommandId {
    /// Create a new command ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for CommandId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// =============================================================================
// COMMAND CATEGORY
// =============================================================================

/// Category of command for grouping and permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandCategory {
    /// Editing commands (cut, copy, paste, delete)
    Edit,
    /// Tool selection (select, move, trim, razor)
    Tool,
    /// Transport controls (play, pause, stop, seek)
    Transport,
    /// View controls (zoom, scroll, focus)
    View,
    /// Selection commands
    Selection,
    /// File operations (save, export)
    File,
    /// Undo/Redo
    History,
    /// Application-level (preferences, quit)
    Application,
}

// =============================================================================
// COMMAND RESULT
// =============================================================================

/// Result of command execution.
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command executed successfully
    Success,

    /// Command executed with a message
    SuccessWithMessage(String),

    /// Command was not applicable in current context
    NotApplicable(String),

    /// Command failed with error
    Failed(String),

    /// Command produced an edit action (to be applied to engine)
    EditAction(crate::engine::edit_action::EditAction),

    /// Command changed the tool
    ToolChanged(crate::engine::interaction::ToolType),

    /// Command changed playback state
    PlaybackChanged,

    /// Command requires confirmation
    RequiresConfirmation(String),
}

impl CommandResult {
    /// Check if result is success.
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            CommandResult::Success
                | CommandResult::SuccessWithMessage(_)
                | CommandResult::EditAction(_)
                | CommandResult::ToolChanged(_)
                | CommandResult::PlaybackChanged
        )
    }

    /// Check if result is failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, CommandResult::Failed(_))
    }
}

// =============================================================================
// COMMAND DESCRIPTOR
// =============================================================================

/// Static metadata about a command.
#[derive(Debug, Clone)]
pub struct CommandDescriptor {
    /// Unique command identifier
    pub id: CommandId,

    /// Human-readable name
    pub name: String,

    /// Description for tooltips
    pub description: String,

    /// Category for grouping
    pub category: CommandCategory,

    /// Whether command modifies state
    pub is_mutating: bool,

    /// Whether command can be undone
    pub is_undoable: bool,

    /// Default keyboard shortcut (if any)
    pub default_shortcut: Option<String>,
}

impl CommandDescriptor {
    /// Create a new command descriptor.
    pub fn new(
        id: impl Into<CommandId>,
        name: impl Into<String>,
        category: CommandCategory,
    ) -> Self {
        let id = id.into();
        Self {
            name: name.into(),
            description: String::new(),
            category,
            is_mutating: false,
            is_undoable: false,
            default_shortcut: None,
            id,
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: mark as mutating.
    pub fn mutating(mut self) -> Self {
        self.is_mutating = true;
        self
    }

    /// Builder: mark as undoable.
    pub fn undoable(mut self) -> Self {
        self.is_undoable = true;
        self
    }

    /// Builder: set default shortcut.
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.default_shortcut = Some(shortcut.into());
        self
    }
}

// =============================================================================
// STANDARD COMMANDS
// =============================================================================

/// Standard command IDs.
pub mod commands {
    use super::CommandId;

    // Edit commands
    pub fn undo() -> CommandId {
        CommandId::new("edit.undo")
    }
    pub fn redo() -> CommandId {
        CommandId::new("edit.redo")
    }
    pub fn cut() -> CommandId {
        CommandId::new("edit.cut")
    }
    pub fn copy() -> CommandId {
        CommandId::new("edit.copy")
    }
    pub fn paste() -> CommandId {
        CommandId::new("edit.paste")
    }
    pub fn delete() -> CommandId {
        CommandId::new("edit.delete")
    }
    pub fn select_all() -> CommandId {
        CommandId::new("edit.select_all")
    }
    pub fn deselect() -> CommandId {
        CommandId::new("edit.deselect")
    }

    // Tool commands
    pub fn tool_select() -> CommandId {
        CommandId::new("tool.select")
    }
    pub fn tool_move() -> CommandId {
        CommandId::new("tool.move")
    }
    pub fn tool_trim() -> CommandId {
        CommandId::new("tool.trim")
    }
    pub fn tool_razor() -> CommandId {
        CommandId::new("tool.razor")
    }

    // Transport commands
    pub fn play() -> CommandId {
        CommandId::new("transport.play")
    }
    pub fn pause() -> CommandId {
        CommandId::new("transport.pause")
    }
    pub fn stop() -> CommandId {
        CommandId::new("transport.stop")
    }
    pub fn play_pause() -> CommandId {
        CommandId::new("transport.play_pause")
    }
    pub fn seek_start() -> CommandId {
        CommandId::new("transport.seek_start")
    }
    pub fn seek_end() -> CommandId {
        CommandId::new("transport.seek_end")
    }
    pub fn step_forward() -> CommandId {
        CommandId::new("transport.step_forward")
    }
    pub fn step_backward() -> CommandId {
        CommandId::new("transport.step_backward")
    }

    // View commands
    pub fn zoom_in() -> CommandId {
        CommandId::new("view.zoom_in")
    }
    pub fn zoom_out() -> CommandId {
        CommandId::new("view.zoom_out")
    }
    pub fn zoom_fit() -> CommandId {
        CommandId::new("view.zoom_fit")
    }
    pub fn zoom_selection() -> CommandId {
        CommandId::new("view.zoom_selection")
    }

    // File commands
    pub fn save() -> CommandId {
        CommandId::new("file.save")
    }
    pub fn export() -> CommandId {
        CommandId::new("file.export")
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_id() {
        let id = CommandId::new("test.command");
        assert_eq!(id.0, "test.command");
        assert_eq!(id.to_string(), "test.command");
    }

    #[test]
    fn test_command_id_from_str() {
        let id: CommandId = "test.cmd".into();
        assert_eq!(id.0, "test.cmd");
    }

    #[test]
    fn test_command_descriptor() {
        let desc = CommandDescriptor::new("edit.cut", "Cut", CommandCategory::Edit)
            .with_description("Cut selected clips")
            .mutating()
            .undoable()
            .with_shortcut("Cmd+X");

        assert_eq!(desc.id.0, "edit.cut");
        assert_eq!(desc.name, "Cut");
        assert!(desc.is_mutating);
        assert!(desc.is_undoable);
        assert_eq!(desc.default_shortcut, Some("Cmd+X".to_string()));
    }

    #[test]
    fn test_command_result_is_success() {
        assert!(CommandResult::Success.is_success());
        assert!(CommandResult::SuccessWithMessage("ok".to_string()).is_success());
        assert!(!CommandResult::Failed("err".to_string()).is_success());
        assert!(!CommandResult::NotApplicable("na".to_string()).is_success());
    }

    #[test]
    fn test_standard_commands() {
        assert_eq!(commands::undo().0, "edit.undo");
        assert_eq!(commands::play().0, "transport.play");
        assert_eq!(commands::tool_select().0, "tool.select");
    }
}
