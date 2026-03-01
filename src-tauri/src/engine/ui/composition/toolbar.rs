//! Toolbar - Toolbar layout for UI composition.
//!
//! # Design
//!
//! Toolbars contain groups of buttons linked to commands.
//! Built entirely from CommandRegistry - no engine references.

use serde::{Deserialize, Serialize};

use crate::engine::commands::{CommandId, CommandRegistry, Keymap};

// =============================================================================
// TOOLBAR ITEM
// =============================================================================

/// A toolbar item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolbarItem {
    /// Button linked to command
    Button {
        /// Command to execute
        command: String,
        /// Icon name
        icon: String,
        /// Tooltip (includes shortcut)
        tooltip: String,
        /// Whether button is enabled
        enabled: bool,
        /// Whether button is toggled (for toggle buttons)
        toggled: bool,
    },

    /// Separator
    Separator,

    /// Spacer (flexible space)
    Spacer,

    /// Group of buttons
    Group {
        /// Group items
        items: Vec<ToolbarItem>,
        /// Whether group is exclusive (radio-button style)
        exclusive: bool,
    },
}

impl ToolbarItem {
    /// Create a button.
    pub fn button(
        command: impl Into<String>,
        icon: impl Into<String>,
        tooltip: impl Into<String>,
    ) -> Self {
        Self::Button {
            command: command.into(),
            icon: icon.into(),
            tooltip: tooltip.into(),
            enabled: true,
            toggled: false,
        }
    }

    /// Create a separator.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a spacer.
    pub fn spacer() -> Self {
        Self::Spacer
    }

    /// Create a button group.
    pub fn group(items: Vec<ToolbarItem>, exclusive: bool) -> Self {
        Self::Group { items, exclusive }
    }

    /// Set toggled state.
    pub fn toggled(mut self, toggled: bool) -> Self {
        if let Self::Button {
            toggled: ref mut t, ..
        } = self
        {
            *t = toggled;
        }
        self
    }

    /// Set disabled.
    pub fn disabled(mut self) -> Self {
        if let Self::Button {
            enabled: ref mut e, ..
        } = self
        {
            *e = false;
        }
        self
    }
}

// =============================================================================
// TOOLBAR
// =============================================================================

/// A toolbar configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toolbar {
    /// Toolbar identifier
    pub id: String,
    /// Toolbar items
    pub items: Vec<ToolbarItem>,
}

impl Toolbar {
    /// Create a new toolbar.
    pub fn new(id: impl Into<String>, items: Vec<ToolbarItem>) -> Self {
        Self {
            id: id.into(),
            items,
        }
    }

    /// Build tools toolbar from registry.
    pub fn tools_toolbar(registry: &CommandRegistry, keymap: &Keymap) -> Self {
        Self::new(
            "toolbar.tools",
            vec![
                ToolbarItem::group(
                    vec![
                        make_button("tool.select", "cursor", "Select Tool", registry, keymap),
                        make_button("tool.move", "move", "Move Tool", registry, keymap),
                        make_button("tool.trim", "trim", "Trim Tool", registry, keymap),
                        make_button("tool.razor", "scissors", "Razor Tool", registry, keymap),
                    ],
                    true,
                ), // exclusive group
            ],
        )
    }

    /// Build transport toolbar from registry.
    pub fn transport_toolbar(registry: &CommandRegistry, keymap: &Keymap) -> Self {
        Self::new(
            "toolbar.transport",
            vec![
                make_button(
                    "transport.seek_start",
                    "skip-back",
                    "Go to Start",
                    registry,
                    keymap,
                ),
                make_button(
                    "transport.step_backward",
                    "step-back",
                    "Step Back",
                    registry,
                    keymap,
                ),
                make_button(
                    "transport.play_pause",
                    "play",
                    "Play/Pause",
                    registry,
                    keymap,
                ),
                make_button(
                    "transport.step_forward",
                    "step-forward",
                    "Step Forward",
                    registry,
                    keymap,
                ),
                make_button(
                    "transport.seek_end",
                    "skip-forward",
                    "Go to End",
                    registry,
                    keymap,
                ),
            ],
        )
    }

    /// Build edit toolbar from registry.
    pub fn edit_toolbar(registry: &CommandRegistry, keymap: &Keymap) -> Self {
        Self::new(
            "toolbar.edit",
            vec![
                make_button("edit.undo", "undo", "Undo", registry, keymap),
                make_button("edit.redo", "redo", "Redo", registry, keymap),
                ToolbarItem::separator(),
                make_button("edit.cut", "scissors", "Cut", registry, keymap),
                make_button("edit.copy", "copy", "Copy", registry, keymap),
                make_button("edit.paste", "clipboard", "Paste", registry, keymap),
            ],
        )
    }
}

/// Helper to create toolbar button from command.
fn make_button(
    cmd_id: &str,
    icon: &str,
    label: &str,
    registry: &CommandRegistry,
    keymap: &Keymap,
) -> ToolbarItem {
    let command_id = CommandId::new(cmd_id);
    let enabled = registry.exists(&command_id);

    // Build tooltip with shortcut
    let shortcut = keymap
        .get_bindings(&command_id)
        .and_then(|bindings| bindings.first())
        .map(|b| b.format());

    let tooltip = match shortcut {
        Some(s) => format!("{} ({})", label, s),
        None => label.to_string(),
    };

    let mut item = ToolbarItem::button(cmd_id, icon, tooltip);
    if !enabled {
        item = item.disabled();
    }
    item
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolbar_item() {
        let item = ToolbarItem::button("edit.cut", "scissors", "Cut (Cmd+X)");

        match item {
            ToolbarItem::Button {
                command,
                icon,
                tooltip,
                ..
            } => {
                assert_eq!(command, "edit.cut");
                assert_eq!(icon, "scissors");
                assert_eq!(tooltip, "Cut (Cmd+X)");
            }
            _ => panic!("Wrong item type"),
        }
    }

    #[test]
    fn test_toolbar_serializable() {
        let toolbar = Toolbar::new(
            "test",
            vec![
                ToolbarItem::button("cmd1", "icon1", "Tool 1"),
                ToolbarItem::separator(),
                ToolbarItem::button("cmd2", "icon2", "Tool 2"),
            ],
        );

        let json = serde_json::to_string(&toolbar).unwrap();
        let deserialized: Toolbar = serde_json::from_str(&json).unwrap();

        assert_eq!(toolbar.id, deserialized.id);
        assert_eq!(toolbar.items.len(), deserialized.items.len());
    }

    #[test]
    fn test_toolbar_from_commands() {
        let registry = CommandRegistry::with_defaults();
        let keymap = Keymap::with_defaults();

        let tools = Toolbar::tools_toolbar(&registry, &keymap);
        assert!(!tools.items.is_empty());

        let transport = Toolbar::transport_toolbar(&registry, &keymap);
        assert!(!transport.items.is_empty());
    }

    #[test]
    fn test_button_group() {
        let group = ToolbarItem::group(
            vec![
                ToolbarItem::button("a", "a", "A"),
                ToolbarItem::button("b", "b", "B"),
            ],
            true,
        );

        match group {
            ToolbarItem::Group { items, exclusive } => {
                assert_eq!(items.len(), 2);
                assert!(exclusive);
            }
            _ => panic!("Expected group"),
        }
    }
}
