//! Workspace State - State model with versioning for workspace persistence.
//!
//! # Design
//!
//! WorkspaceState is the authoritative source for UI layout state.
//! It includes:
//! - Panel visibility and positions
//! - Layout configuration
//! - Theme and preferences
//! - Version for migration
//!
//! The engine owns this state - UI is derived from it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::engine::ui::composition::{
    PanelId, PanelPosition, Theme, UIPreferences, WorkspaceLayout,
};

// =============================================================================
// VERSION
// =============================================================================

/// Current workspace format version.
pub const WORKSPACE_VERSION: u32 = 1;

// =============================================================================
// PANEL STATE
// =============================================================================

/// Runtime state of a panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelState {
    /// Panel ID
    pub id: PanelId,
    /// Whether visible
    pub visible: bool,
    /// Current position
    pub position: PanelPosition,
    /// Current size (width for left/right, height for top/bottom)
    pub size: u32,
    /// Whether collapsed
    pub collapsed: bool,
}

impl PanelState {
    /// Create new panel state.
    pub fn new(id: impl Into<PanelId>) -> Self {
        Self {
            id: id.into(),
            visible: true,
            position: PanelPosition::Center,
            size: 250,
            collapsed: false,
        }
    }

    /// Builder: set position.
    pub fn at(mut self, position: PanelPosition) -> Self {
        self.position = position;
        self
    }

    /// Builder: set hidden.
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    /// Builder: set size.
    pub fn with_size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
}

// =============================================================================
// WORKSPACE STATE
// =============================================================================

/// Complete workspace state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// Format version for migration
    pub version: u32,

    /// Workspace name
    pub name: String,

    /// Whether this is the active workspace
    pub active: bool,

    /// Panel states by ID
    pub panels: HashMap<String, PanelState>,

    /// Current layout configuration
    pub layout: WorkspaceLayout,

    /// Theme
    pub theme: Theme,

    /// Preferences
    pub preferences: UIPreferences,

    /// Last modified timestamp (Unix millis)
    pub last_modified: u64,

    /// Checksum for integrity validation
    pub checksum: Option<String>,
}

impl WorkspaceState {
    /// Create a new workspace state with defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: WORKSPACE_VERSION,
            name: name.into(),
            active: false,
            panels: HashMap::new(),
            layout: WorkspaceLayout::default_editing(),
            theme: Theme::default(),
            preferences: UIPreferences::default(),
            last_modified: Self::now_millis(),
            checksum: None,
        }
    }

    /// Create default editing workspace.
    pub fn default_editing() -> Self {
        let mut state = Self::new("Editing");
        state.active = true;

        // Add standard panels
        state.add_panel(
            PanelState::new("panel.timeline")
                .at(PanelPosition::Bottom)
                .with_size(250),
        );
        state.add_panel(PanelState::new("panel.preview").at(PanelPosition::Center));
        state.add_panel(
            PanelState::new("panel.media_browser")
                .at(PanelPosition::Left)
                .with_size(280),
        );
        state.add_panel(
            PanelState::new("panel.properties")
                .at(PanelPosition::Right)
                .with_size(280),
        );
        state.add_panel(
            PanelState::new("panel.effects")
                .at(PanelPosition::Right)
                .with_size(280),
        );
        state.add_panel(
            PanelState::new("panel.audio_mixer")
                .at(PanelPosition::Bottom)
                .hidden(),
        );
        state.add_panel(
            PanelState::new("panel.history")
                .at(PanelPosition::Right)
                .hidden(),
        );

        state
    }

    /// Add a panel state.
    pub fn add_panel(&mut self, panel: PanelState) {
        self.panels.insert(panel.id.0.clone(), panel);
    }

    /// Get panel state.
    pub fn get_panel(&self, id: &str) -> Option<&PanelState> {
        self.panels.get(id)
    }

    /// Get mutable panel state.
    pub fn get_panel_mut(&mut self, id: &str) -> Option<&mut PanelState> {
        self.panels.get_mut(id)
    }

    /// Set panel visibility.
    pub fn set_panel_visible(&mut self, id: &str, visible: bool) {
        if let Some(panel) = self.panels.get_mut(id) {
            panel.visible = visible;
            self.touch();
        }
    }

    /// Toggle panel visibility.
    pub fn toggle_panel(&mut self, id: &str) {
        if let Some(panel) = self.panels.get_mut(id) {
            panel.visible = !panel.visible;
            self.touch();
        }
    }

    /// Set panel collapsed state.
    pub fn set_panel_collapsed(&mut self, id: &str, collapsed: bool) {
        if let Some(panel) = self.panels.get_mut(id) {
            panel.collapsed = collapsed;
            self.touch();
        }
    }

    /// Resize panel.
    pub fn resize_panel(&mut self, id: &str, size: u32) {
        if let Some(panel) = self.panels.get_mut(id) {
            panel.size = size;
            self.touch();
        }
    }

    /// Move panel to new position.
    pub fn move_panel(&mut self, id: &str, position: PanelPosition) {
        if let Some(panel) = self.panels.get_mut(id) {
            panel.position = position;
            self.touch();
        }
    }

    /// Update last modified timestamp.
    pub fn touch(&mut self) {
        self.last_modified = Self::now_millis();
        self.checksum = None; // Invalidate checksum
    }

    /// Calculate checksum for integrity.
    pub fn calculate_checksum(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.name.hash(&mut hasher);
        self.version.hash(&mut hasher);
        self.panels.len().hash(&mut hasher);

        // Sort keys for deterministic ordering
        let mut keys: Vec<_> = self.panels.keys().collect();
        keys.sort();

        for id in keys {
            if let Some(panel) = self.panels.get(id) {
                id.hash(&mut hasher);
                panel.visible.hash(&mut hasher);
                panel.size.hash(&mut hasher);
                panel.collapsed.hash(&mut hasher);
            }
        }

        self.checksum = Some(format!("{:016x}", hasher.finish()));
    }

    /// Validate checksum.
    pub fn validate_checksum(&self) -> bool {
        // If no checksum, consider valid (legacy data)
        let Some(original_checksum) = &self.checksum else {
            return true;
        };

        let mut copy = self.clone();
        copy.calculate_checksum();
        copy.checksum.as_ref() == Some(original_checksum)
    }

    /// Get current timestamp in milliseconds.
    fn now_millis() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self::default_editing()
    }
}

// =============================================================================
// WORKSPACE COLLECTION
// =============================================================================

/// Collection of named workspaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCollection {
    /// Format version
    pub version: u32,

    /// Active workspace name
    pub active: String,

    /// All workspaces by name
    pub workspaces: HashMap<String, WorkspaceState>,
}

impl WorkspaceCollection {
    /// Create a new collection.
    pub fn new() -> Self {
        let mut collection = Self {
            version: WORKSPACE_VERSION,
            active: "Editing".to_string(),
            workspaces: HashMap::new(),
        };

        // Add default workspace
        collection.add(WorkspaceState::default_editing());

        collection
    }

    /// Add a workspace.
    pub fn add(&mut self, workspace: WorkspaceState) {
        let name = workspace.name.clone();
        self.workspaces.insert(name, workspace);
    }

    /// Get active workspace.
    pub fn active(&self) -> Option<&WorkspaceState> {
        self.workspaces.get(&self.active)
    }

    /// Get mutable active workspace.
    pub fn active_mut(&mut self) -> Option<&mut WorkspaceState> {
        self.workspaces.get_mut(&self.active)
    }

    /// Switch active workspace.
    pub fn switch_to(&mut self, name: &str) -> bool {
        if self.workspaces.contains_key(name) {
            // Deactivate current
            if let Some(current) = self.workspaces.get_mut(&self.active) {
                current.active = false;
            }

            // Activate new
            self.active = name.to_string();
            if let Some(new) = self.workspaces.get_mut(name) {
                new.active = true;
            }
            true
        } else {
            false
        }
    }

    /// Get workspace by name.
    pub fn get(&self, name: &str) -> Option<&WorkspaceState> {
        self.workspaces.get(name)
    }

    /// Get workspace names.
    pub fn names(&self) -> Vec<&str> {
        self.workspaces.keys().map(|s| s.as_str()).collect()
    }

    /// Duplicate a workspace.
    pub fn duplicate(&mut self, source: &str, new_name: &str) -> bool {
        if let Some(source_ws) = self.workspaces.get(source) {
            let mut new_ws = source_ws.clone();
            new_ws.name = new_name.to_string();
            new_ws.active = false;
            new_ws.touch();
            self.workspaces.insert(new_name.to_string(), new_ws);
            true
        } else {
            false
        }
    }

    /// Delete a workspace (cannot delete if only one left).
    pub fn delete(&mut self, name: &str) -> bool {
        if self.workspaces.len() <= 1 {
            return false;
        }

        if self.workspaces.remove(name).is_some() {
            // If deleted active, switch to another
            if self.active == name {
                if let Some(new_active) = self.workspaces.keys().next().cloned() {
                    self.switch_to(&new_active);
                }
            }
            true
        } else {
            false
        }
    }
}

impl Default for WorkspaceCollection {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_state_new() {
        let state = WorkspaceState::new("Test");

        assert_eq!(state.name, "Test");
        assert_eq!(state.version, WORKSPACE_VERSION);
        assert!(state.last_modified > 0);
    }

    #[test]
    fn test_workspace_default_editing() {
        let state = WorkspaceState::default_editing();

        assert_eq!(state.name, "Editing");
        assert!(state.active);
        assert!(!state.panels.is_empty());

        // Check standard panels exist
        assert!(state.get_panel("panel.timeline").is_some());
        assert!(state.get_panel("panel.preview").is_some());
    }

    #[test]
    fn test_panel_visibility() {
        let mut state = WorkspaceState::default_editing();

        state.set_panel_visible("panel.history", true);
        assert!(state.get_panel("panel.history").unwrap().visible);

        state.toggle_panel("panel.history");
        assert!(!state.get_panel("panel.history").unwrap().visible);
    }

    #[test]
    fn test_workspace_serializable() {
        let state = WorkspaceState::default_editing();

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WorkspaceState = serde_json::from_str(&json).unwrap();

        assert_eq!(state.name, deserialized.name);
        assert_eq!(state.panels.len(), deserialized.panels.len());
    }

    #[test]
    fn test_checksum() {
        let mut state = WorkspaceState::default_editing();

        state.calculate_checksum();
        assert!(state.checksum.is_some());
        assert!(state.validate_checksum());
    }

    #[test]
    fn test_workspace_collection() {
        let mut collection = WorkspaceCollection::new();

        assert_eq!(collection.active, "Editing");
        assert!(collection.active().is_some());
    }

    #[test]
    fn test_multi_workspace_switching() {
        let mut collection = WorkspaceCollection::new();

        // Add second workspace
        let color = WorkspaceState::new("Color Grading");
        collection.add(color);

        assert_eq!(collection.names().len(), 2);

        // Switch
        assert!(collection.switch_to("Color Grading"));
        assert_eq!(collection.active, "Color Grading");
        assert!(collection.active().unwrap().active);

        // Switch back
        assert!(collection.switch_to("Editing"));
        assert_eq!(collection.active, "Editing");
    }

    #[test]
    fn test_workspace_duplicate() {
        let mut collection = WorkspaceCollection::new();

        assert!(collection.duplicate("Editing", "Editing (Copy)"));
        assert_eq!(collection.workspaces.len(), 2);
        assert!(collection.get("Editing (Copy)").is_some());
    }

    #[test]
    fn test_workspace_delete() {
        let mut collection = WorkspaceCollection::new();
        collection.add(WorkspaceState::new("Second"));

        // Can delete when more than one
        assert!(collection.delete("Second"));
        assert_eq!(collection.workspaces.len(), 1);

        // Cannot delete last workspace
        assert!(!collection.delete("Editing"));
        assert_eq!(collection.workspaces.len(), 1);
    }
}
