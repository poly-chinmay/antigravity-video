//! CommandRegistry - Command registration and lookup.
//!
//! # Design
//!
//! The registry holds all available commands and their metadata.
//! Commands are registered at startup.

use std::collections::HashMap;

use super::command::{CommandCategory, CommandDescriptor, CommandId};

// =============================================================================
// COMMAND REGISTRY
// =============================================================================

/// Registry of all available commands.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    /// All registered commands
    commands: HashMap<CommandId, CommandDescriptor>,

    /// Commands grouped by category
    by_category: HashMap<CommandCategory, Vec<CommandId>>,
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create registry with default commands.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }

    /// Register default commands.
    pub fn register_defaults(&mut self) {
        use super::command::commands;
        use CommandCategory::*;

        // Edit commands
        self.register(
            CommandDescriptor::new(commands::undo(), "Undo", History)
                .with_description("Undo last action")
                .with_shortcut("Cmd+Z"),
        );
        self.register(
            CommandDescriptor::new(commands::redo(), "Redo", History)
                .with_description("Redo last undone action")
                .with_shortcut("Cmd+Shift+Z"),
        );
        self.register(
            CommandDescriptor::new(commands::cut(), "Cut", Edit)
                .with_description("Cut selected clips")
                .mutating()
                .undoable()
                .with_shortcut("Cmd+X"),
        );
        self.register(
            CommandDescriptor::new(commands::copy(), "Copy", Edit)
                .with_description("Copy selected clips")
                .with_shortcut("Cmd+C"),
        );
        self.register(
            CommandDescriptor::new(commands::paste(), "Paste", Edit)
                .with_description("Paste clips")
                .mutating()
                .undoable()
                .with_shortcut("Cmd+V"),
        );
        self.register(
            CommandDescriptor::new(commands::delete(), "Delete", Edit)
                .with_description("Delete selected clips")
                .mutating()
                .undoable()
                .with_shortcut("Delete"),
        );
        self.register(
            CommandDescriptor::new(commands::select_all(), "Select All", Selection)
                .with_description("Select all clips")
                .with_shortcut("Cmd+A"),
        );
        self.register(
            CommandDescriptor::new(commands::deselect(), "Deselect", Selection)
                .with_description("Deselect all clips")
                .with_shortcut("Cmd+D"),
        );

        // Tool commands
        self.register(
            CommandDescriptor::new(commands::tool_select(), "Select Tool", Tool)
                .with_description("Switch to selection tool")
                .with_shortcut("V"),
        );
        self.register(
            CommandDescriptor::new(commands::tool_move(), "Move Tool", Tool)
                .with_description("Switch to move tool")
                .with_shortcut("M"),
        );
        self.register(
            CommandDescriptor::new(commands::tool_trim(), "Trim Tool", Tool)
                .with_description("Switch to trim tool")
                .with_shortcut("T"),
        );
        self.register(
            CommandDescriptor::new(commands::tool_razor(), "Razor Tool", Tool)
                .with_description("Switch to razor tool")
                .with_shortcut("B"),
        );

        // Transport commands
        self.register(
            CommandDescriptor::new(commands::play_pause(), "Play/Pause", Transport)
                .with_description("Toggle playback")
                .with_shortcut("Space"),
        );
        self.register(
            CommandDescriptor::new(commands::play(), "Play", Transport)
                .with_description("Start playback")
                .with_shortcut("L"),
        );
        self.register(
            CommandDescriptor::new(commands::stop(), "Stop", Transport)
                .with_description("Stop playback")
                .with_shortcut("J"),
        );
        self.register(
            CommandDescriptor::new(commands::seek_start(), "Go to Start", Transport)
                .with_description("Seek to timeline start")
                .with_shortcut("Home"),
        );
        self.register(
            CommandDescriptor::new(commands::seek_end(), "Go to End", Transport)
                .with_description("Seek to timeline end")
                .with_shortcut("End"),
        );
        self.register(
            CommandDescriptor::new(commands::step_forward(), "Step Forward", Transport)
                .with_description("Step one frame forward")
                .with_shortcut("ArrowRight"),
        );
        self.register(
            CommandDescriptor::new(commands::step_backward(), "Step Backward", Transport)
                .with_description("Step one frame backward")
                .with_shortcut("ArrowLeft"),
        );

        // View commands
        self.register(
            CommandDescriptor::new(commands::zoom_in(), "Zoom In", View)
                .with_description("Zoom in timeline")
                .with_shortcut("Cmd+="),
        );
        self.register(
            CommandDescriptor::new(commands::zoom_out(), "Zoom Out", View)
                .with_description("Zoom out timeline")
                .with_shortcut("Cmd+-"),
        );
        self.register(
            CommandDescriptor::new(commands::zoom_fit(), "Zoom to Fit", View)
                .with_description("Zoom to fit timeline")
                .with_shortcut("Cmd+0"),
        );

        // File commands
        self.register(
            CommandDescriptor::new(commands::save(), "Save", File)
                .with_description("Save project")
                .with_shortcut("Cmd+S"),
        );
        self.register(
            CommandDescriptor::new(commands::export(), "Export", File)
                .with_description("Export video")
                .with_shortcut("Cmd+Shift+E"),
        );
    }

    /// Register a command.
    pub fn register(&mut self, descriptor: CommandDescriptor) {
        let id = descriptor.id.clone();
        let category = descriptor.category;

        self.commands.insert(id.clone(), descriptor);
        self.by_category.entry(category).or_default().push(id);
    }

    /// Get command descriptor by ID.
    pub fn get(&self, id: &CommandId) -> Option<&CommandDescriptor> {
        self.commands.get(id)
    }

    /// Check if command exists.
    pub fn exists(&self, id: &CommandId) -> bool {
        self.commands.contains_key(id)
    }

    /// Get all commands in a category.
    pub fn by_category(&self, category: CommandCategory) -> Vec<&CommandDescriptor> {
        self.by_category
            .get(&category)
            .map(|ids| ids.iter().filter_map(|id| self.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all commands.
    pub fn all(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands.values()
    }

    /// Get command count.
    pub fn count(&self) -> usize {
        self.commands.len()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::commands::command::commands;

    #[test]
    fn test_registry_register() {
        let mut registry = CommandRegistry::new();

        let desc = CommandDescriptor::new("test.cmd", "Test Command", CommandCategory::Edit);
        registry.register(desc);

        assert!(registry.exists(&CommandId::new("test.cmd")));
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_registry_defaults() {
        let registry = CommandRegistry::with_defaults();

        // Check some commands exist
        assert!(registry.exists(&commands::undo()));
        assert!(registry.exists(&commands::play_pause()));
        assert!(registry.exists(&commands::tool_select()));

        assert!(registry.count() > 10);
    }

    #[test]
    fn test_registry_by_category() {
        let registry = CommandRegistry::with_defaults();

        let tool_cmds = registry.by_category(CommandCategory::Tool);
        assert!(!tool_cmds.is_empty());

        let transport_cmds = registry.by_category(CommandCategory::Transport);
        assert!(!transport_cmds.is_empty());
    }

    #[test]
    fn test_registry_get() {
        let registry = CommandRegistry::with_defaults();

        let desc = registry.get(&commands::undo()).unwrap();
        assert_eq!(desc.name, "Undo");
        assert_eq!(desc.category, CommandCategory::History);
    }
}
