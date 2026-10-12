//! Keymap - Keyboard shortcut bindings.
//!
//! # Design
//!
//! Keymaps bind keyboard shortcuts to command IDs:
//! - Platform-specific modifiers (Cmd on Mac, Ctrl on Windows)
//! - Configurable bindings
//! - Conflict detection
//!
//! # Key Format
//!
//! Keys are represented as strings: "Cmd+Shift+Z", "Ctrl+C", etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::command::CommandId;

// =============================================================================
// KEY MODIFIER
// =============================================================================

/// Keyboard modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct KeyModifiers {
    /// Cmd (Mac) / Ctrl (Windows/Linux)
    pub cmd_or_ctrl: bool,
    /// Shift
    pub shift: bool,
    /// Alt / Option
    pub alt: bool,
    /// Ctrl (Mac only, distinct from Cmd)
    pub ctrl: bool,
}

impl KeyModifiers {
    /// No modifiers.
    pub const NONE: KeyModifiers = KeyModifiers {
        cmd_or_ctrl: false,
        shift: false,
        alt: false,
        ctrl: false,
    };

    /// Just Cmd/Ctrl.
    pub fn cmd() -> Self {
        Self {
            cmd_or_ctrl: true,
            ..Default::default()
        }
    }

    /// Cmd/Ctrl + Shift.
    pub fn cmd_shift() -> Self {
        Self {
            cmd_or_ctrl: true,
            shift: true,
            ..Default::default()
        }
    }

    /// Just Shift.
    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Default::default()
        }
    }

    /// Just Alt.
    pub fn alt() -> Self {
        Self {
            alt: true,
            ..Default::default()
        }
    }
}

// =============================================================================
// KEY BINDING
// =============================================================================

/// A keyboard shortcut binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Key code (e.g., "A", "Z", "Space", "Delete")
    pub key: String,
    /// Modifiers
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Create a new key binding.
    pub fn new(key: impl Into<String>, modifiers: KeyModifiers) -> Self {
        Self {
            key: key.into(),
            modifiers,
        }
    }

    /// Create binding with no modifiers.
    pub fn key(key: impl Into<String>) -> Self {
        Self::new(key, KeyModifiers::NONE)
    }

    /// Create binding with Cmd/Ctrl.
    pub fn cmd(key: impl Into<String>) -> Self {
        Self::new(key, KeyModifiers::cmd())
    }

    /// Create binding with Cmd/Ctrl+Shift.
    pub fn cmd_shift(key: impl Into<String>) -> Self {
        Self::new(key, KeyModifiers::cmd_shift())
    }

    /// Parse from string format "Cmd+Shift+Z".
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }

        let key = parts.last()?.to_string();
        let mut modifiers = KeyModifiers::default();

        for &part in &parts[..parts.len() - 1] {
            match part.to_lowercase().as_str() {
                "cmd" | "ctrl" | "command" | "control" => modifiers.cmd_or_ctrl = true,
                "shift" => modifiers.shift = true,
                "alt" | "option" => modifiers.alt = true,
                _ => {}
            }
        }

        Some(Self { key, modifiers })
    }

    /// Format as string.
    pub fn format(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.cmd_or_ctrl {
            parts.push("Cmd");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

// =============================================================================
// KEYMAP
// =============================================================================

/// Keymap binding keyboard shortcuts to commands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Keymap {
    /// Binding -> CommandId
    bindings: HashMap<KeyBinding, CommandId>,
    /// CommandId -> Bindings (reverse lookup)
    #[serde(skip)]
    command_bindings: HashMap<CommandId, Vec<KeyBinding>>,
}

impl Keymap {
    /// Create an empty keymap.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create keymap with default bindings.
    pub fn with_defaults() -> Self {
        let mut keymap = Self::new();
        keymap.load_defaults();
        keymap
    }

    /// Load default key bindings.
    pub fn load_defaults(&mut self) {
        use super::command::commands;

        // Edit commands
        self.bind(KeyBinding::cmd("Z"), commands::undo());
        self.bind(KeyBinding::cmd_shift("Z"), commands::redo());
        self.bind(KeyBinding::cmd("X"), commands::cut());
        self.bind(KeyBinding::cmd("C"), commands::copy());
        self.bind(KeyBinding::cmd("V"), commands::paste());
        self.bind(KeyBinding::key("Delete"), commands::delete());
        self.bind(KeyBinding::key("Backspace"), commands::delete());
        self.bind(KeyBinding::cmd("A"), commands::select_all());
        self.bind(KeyBinding::cmd("D"), commands::deselect());

        // Tool commands
        self.bind(KeyBinding::key("V"), commands::tool_select());
        self.bind(KeyBinding::key("M"), commands::tool_move());
        self.bind(KeyBinding::key("T"), commands::tool_trim());
        self.bind(KeyBinding::key("B"), commands::tool_razor());

        // Transport commands
        self.bind(KeyBinding::key("Space"), commands::play_pause());
        self.bind(KeyBinding::key("K"), commands::play_pause());
        self.bind(KeyBinding::key("L"), commands::play());
        self.bind(KeyBinding::key("J"), commands::stop());
        self.bind(KeyBinding::key("Home"), commands::seek_start());
        self.bind(KeyBinding::key("End"), commands::seek_end());
        self.bind(KeyBinding::key("ArrowRight"), commands::step_forward());
        self.bind(KeyBinding::key("ArrowLeft"), commands::step_backward());

        // View commands
        self.bind(KeyBinding::cmd("="), commands::zoom_in());
        self.bind(KeyBinding::cmd("-"), commands::zoom_out());
        self.bind(KeyBinding::cmd("0"), commands::zoom_fit());

        // File commands
        self.bind(KeyBinding::cmd("S"), commands::save());
        self.bind(KeyBinding::cmd_shift("E"), commands::export());
    }

    /// Bind a key to a command.
    pub fn bind(&mut self, key: KeyBinding, command: CommandId) {
        // Remove old binding if exists
        if let Some(old_cmd) = self.bindings.get(&key) {
            if let Some(bindings) = self.command_bindings.get_mut(old_cmd) {
                bindings.retain(|k| k != &key);
            }
        }

        // Add new binding
        self.bindings.insert(key.clone(), command.clone());
        self.command_bindings.entry(command).or_default().push(key);
    }

    /// Unbind a key.
    pub fn unbind(&mut self, key: &KeyBinding) {
        if let Some(cmd) = self.bindings.remove(key) {
            if let Some(bindings) = self.command_bindings.get_mut(&cmd) {
                bindings.retain(|k| k != key);
            }
        }
    }

    /// Get command for key binding.
    pub fn get_command(&self, key: &KeyBinding) -> Option<&CommandId> {
        self.bindings.get(key)
    }

    /// Get bindings for a command.
    pub fn get_bindings(&self, command: &CommandId) -> Option<&[KeyBinding]> {
        self.command_bindings.get(command).map(|v| v.as_slice())
    }

    /// Check if key is bound.
    pub fn is_bound(&self, key: &KeyBinding) -> bool {
        self.bindings.contains_key(key)
    }

    /// Get all bindings.
    pub fn all_bindings(&self) -> impl Iterator<Item = (&KeyBinding, &CommandId)> {
        self.bindings.iter()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_binding_parse() {
        let binding = KeyBinding::parse("Cmd+Shift+Z").unwrap();
        assert_eq!(binding.key, "Z");
        assert!(binding.modifiers.cmd_or_ctrl);
        assert!(binding.modifiers.shift);
        assert!(!binding.modifiers.alt);
    }

    #[test]
    fn test_key_binding_format() {
        let binding = KeyBinding::cmd_shift("Z");
        assert_eq!(binding.format(), "Cmd+Shift+Z");
    }

    #[test]
    fn test_keymap_bind() {
        let mut keymap = Keymap::new();
        let cmd = CommandId::new("test.cmd");
        let key = KeyBinding::cmd("T");

        keymap.bind(key.clone(), cmd.clone());

        assert_eq!(keymap.get_command(&key), Some(&cmd));
        assert!(keymap.is_bound(&key));
    }

    #[test]
    fn test_keymap_defaults() {
        let keymap = Keymap::with_defaults();

        // Check some default bindings
        let undo_key = KeyBinding::cmd("Z");
        let space_key = KeyBinding::key("Space");

        assert!(keymap.is_bound(&undo_key));
        assert!(keymap.is_bound(&space_key));
    }

    #[test]
    fn test_keymap_unbind() {
        let mut keymap = Keymap::new();
        let cmd = CommandId::new("test.cmd");
        let key = KeyBinding::cmd("T");

        keymap.bind(key.clone(), cmd.clone());
        assert!(keymap.is_bound(&key));

        keymap.unbind(&key);
        assert!(!keymap.is_bound(&key));
    }

    #[test]
    fn test_keymap_rebind() {
        let mut keymap = Keymap::new();
        let cmd1 = CommandId::new("cmd1");
        let cmd2 = CommandId::new("cmd2");
        let key = KeyBinding::cmd("T");

        keymap.bind(key.clone(), cmd1.clone());
        assert_eq!(keymap.get_command(&key), Some(&cmd1));

        keymap.bind(key.clone(), cmd2.clone());
        assert_eq!(keymap.get_command(&key), Some(&cmd2));
    }
}
