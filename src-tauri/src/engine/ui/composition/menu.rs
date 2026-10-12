//! Menu - Menu tree for UI composition.
//!
//! # Design
//!
//! Menus are built entirely from CommandRegistry.
//! Each menu item is linked to a CommandId and displays
//! the keyboard shortcut from Keymap.
//!
//! No engine references - purely declarative.

use serde::{Deserialize, Serialize};

use crate::engine::commands::{CommandId, CommandRegistry, Keymap};

// =============================================================================
// MENU ITEM
// =============================================================================

/// A single menu item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MenuItem {
    /// Command item
    Command {
        /// Display label
        label: String,
        /// Command to execute
        command: String,
        /// Keyboard shortcut (display string)
        shortcut: Option<String>,
        /// Icon name
        icon: Option<String>,
        /// Whether item is enabled
        enabled: bool,
    },

    /// Separator
    Separator,

    /// Submenu
    Submenu {
        /// Submenu label
        label: String,
        /// Submenu items
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    /// Create a command item.
    pub fn command(label: impl Into<String>, command: impl Into<String>) -> Self {
        Self::Command {
            label: label.into(),
            command: command.into(),
            shortcut: None,
            icon: None,
            enabled: true,
        }
    }

    /// Create a separator.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a submenu.
    pub fn submenu(label: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self::Submenu {
            label: label.into(),
            items,
        }
    }

    /// Add shortcut display.
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        if let Self::Command {
            shortcut: ref mut s,
            ..
        } = self
        {
            *s = Some(shortcut.into());
        }
        self
    }

    /// Add icon.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        if let Self::Command {
            icon: ref mut i, ..
        } = self
        {
            *i = Some(icon.into());
        }
        self
    }

    /// Set disabled.
    pub fn disabled(mut self) -> Self {
        if let Self::Command {
            enabled: ref mut e, ..
        } = self
        {
            *e = false;
        }
        self
    }
}

// =============================================================================
// MENU
// =============================================================================

/// A top-level menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Menu {
    /// Menu label
    pub label: String,
    /// Menu items
    pub items: Vec<MenuItem>,
}

impl Menu {
    /// Create a new menu.
    pub fn new(label: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            label: label.into(),
            items,
        }
    }
}

// =============================================================================
// MENU BAR
// =============================================================================

/// Complete menu bar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuBar {
    /// All menus
    pub menus: Vec<Menu>,
}

impl MenuBar {
    /// Create an empty menu bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a menu.
    pub fn add_menu(&mut self, menu: Menu) {
        self.menus.push(menu);
    }

    /// Build menu bar from command registry and keymap.
    pub fn from_registry(registry: &CommandRegistry, keymap: &Keymap) -> Self {
        let mut bar = Self::new();

        // File menu
        bar.add_menu(Menu::new(
            "File",
            vec![
                make_menu_item("file.save", "Save", registry, keymap),
                make_menu_item("file.export", "Export...", registry, keymap),
                MenuItem::separator(),
                MenuItem::command("Close Project", "file.close"),
            ],
        ));

        // Edit menu
        bar.add_menu(Menu::new(
            "Edit",
            vec![
                make_menu_item("edit.undo", "Undo", registry, keymap),
                make_menu_item("edit.redo", "Redo", registry, keymap),
                MenuItem::separator(),
                make_menu_item("edit.cut", "Cut", registry, keymap),
                make_menu_item("edit.copy", "Copy", registry, keymap),
                make_menu_item("edit.paste", "Paste", registry, keymap),
                make_menu_item("edit.delete", "Delete", registry, keymap),
                MenuItem::separator(),
                make_menu_item("edit.select_all", "Select All", registry, keymap),
                make_menu_item("edit.deselect", "Deselect", registry, keymap),
            ],
        ));

        // View menu
        bar.add_menu(Menu::new(
            "View",
            vec![
                make_menu_item("view.zoom_in", "Zoom In", registry, keymap),
                make_menu_item("view.zoom_out", "Zoom Out", registry, keymap),
                make_menu_item("view.zoom_fit", "Zoom to Fit", registry, keymap),
                MenuItem::separator(),
                MenuItem::submenu(
                    "Panels",
                    vec![
                        MenuItem::command("Timeline", "view.panel.timeline"),
                        MenuItem::command("Preview", "view.panel.preview"),
                        MenuItem::command("Media Browser", "view.panel.media_browser"),
                        MenuItem::command("Properties", "view.panel.properties"),
                        MenuItem::command("Effects", "view.panel.effects"),
                    ],
                ),
            ],
        ));

        // Transport menu
        bar.add_menu(Menu::new(
            "Transport",
            vec![
                make_menu_item("transport.play_pause", "Play/Pause", registry, keymap),
                make_menu_item("transport.stop", "Stop", registry, keymap),
                MenuItem::separator(),
                make_menu_item("transport.seek_start", "Go to Start", registry, keymap),
                make_menu_item("transport.seek_end", "Go to End", registry, keymap),
                MenuItem::separator(),
                make_menu_item("transport.step_forward", "Step Forward", registry, keymap),
                make_menu_item("transport.step_backward", "Step Backward", registry, keymap),
            ],
        ));

        bar
    }
}

/// Helper to create menu item from command.
fn make_menu_item(
    cmd_id: &str,
    label: &str,
    registry: &CommandRegistry,
    keymap: &Keymap,
) -> MenuItem {
    let command_id = CommandId::new(cmd_id);
    let enabled = registry.exists(&command_id);

    // Find shortcut
    let shortcut = keymap
        .get_bindings(&command_id)
        .and_then(|bindings| bindings.first())
        .map(|b| b.format());

    let mut item = MenuItem::command(label, cmd_id);
    if let Some(s) = shortcut {
        item = item.with_shortcut(s);
    }
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
    fn test_menu_item() {
        let item = MenuItem::command("Cut", "edit.cut")
            .with_shortcut("Cmd+X")
            .with_icon("scissors");

        match item {
            MenuItem::Command {
                label,
                command,
                shortcut,
                icon,
                ..
            } => {
                assert_eq!(label, "Cut");
                assert_eq!(command, "edit.cut");
                assert_eq!(shortcut, Some("Cmd+X".to_string()));
                assert_eq!(icon, Some("scissors".to_string()));
            }
            _ => panic!("Wrong item type"),
        }
    }

    #[test]
    fn test_menu_serializable() {
        let menu = Menu::new(
            "Edit",
            vec![
                MenuItem::command("Undo", "edit.undo"),
                MenuItem::separator(),
                MenuItem::command("Cut", "edit.cut"),
            ],
        );

        let json = serde_json::to_string(&menu).unwrap();
        let deserialized: Menu = serde_json::from_str(&json).unwrap();

        assert_eq!(menu.label, deserialized.label);
        assert_eq!(menu.items.len(), deserialized.items.len());
    }

    #[test]
    fn test_menu_from_registry() {
        let registry = CommandRegistry::with_defaults();
        let keymap = Keymap::with_defaults();

        let menu_bar = MenuBar::from_registry(&registry, &keymap);

        assert!(!menu_bar.menus.is_empty());

        // Check Edit menu has undo with shortcut
        let edit_menu = menu_bar.menus.iter().find(|m| m.label == "Edit").unwrap();
        let undo_item = edit_menu.items.first().unwrap();

        match undo_item {
            MenuItem::Command {
                label, shortcut, ..
            } => {
                assert_eq!(label, "Undo");
                assert!(shortcut.is_some());
            }
            _ => panic!("Expected command item"),
        }
    }

    #[test]
    fn test_submenu() {
        let submenu = MenuItem::submenu(
            "Tools",
            vec![
                MenuItem::command("Select", "tool.select"),
                MenuItem::command("Move", "tool.move"),
            ],
        );

        match submenu {
            MenuItem::Submenu { label, items } => {
                assert_eq!(label, "Tools");
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected submenu"),
        }
    }
}
