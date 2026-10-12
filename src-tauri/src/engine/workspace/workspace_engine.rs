//! Workspace Engine - Central coordinator for workspace state.
//!
//! # Design
//!
//! WorkspaceEngine is the single source of truth for UI layout state.
//! It coordinates:
//! - Workspace state management
//! - Persistence with crash recovery
//! - UIModel diffing for minimal updates
//! - Multi-workspace support
//!
//! # Invariants
//!
//! - Engine owns truth
//! - UI is derived from engine state
//! - All mutations go through engine
//! - No business logic in React

use crate::engine::commands::{CommandRegistry, Keymap};
use crate::engine::ui::composition::{PanelId, PanelPosition, Theme, UIModel, UIPreferences};

use super::ui_diff::{UIDiffSet, UIDiffer, UISnapshot};
use super::workspace_persistence::{PersistenceResult, WorkspacePersistence};
use super::workspace_state::{PanelState, WorkspaceCollection, WorkspaceState};

// =============================================================================
// WORKSPACE ENGINE
// =============================================================================

/// Central workspace management engine.
#[derive(Debug)]
pub struct WorkspaceEngine {
    /// Workspace collection
    collection: WorkspaceCollection,

    /// Persistence handler
    persistence: Option<WorkspacePersistence>,

    /// UI differ for minimal updates
    differ: UIDiffer,

    /// Command registry (for building UIModel)
    registry: CommandRegistry,

    /// Keymap (for building UIModel)
    keymap: Keymap,

    /// Auto-save enabled
    auto_save: bool,

    /// Dirty flag
    dirty: bool,
}

impl WorkspaceEngine {
    /// Create a new workspace engine.
    pub fn new() -> Self {
        Self {
            collection: WorkspaceCollection::new(),
            persistence: None,
            differ: UIDiffer::new(),
            registry: CommandRegistry::with_defaults(),
            keymap: Keymap::with_defaults(),
            auto_save: true,
            dirty: false,
        }
    }

    /// Create with persistence.
    pub fn with_persistence(base_path: impl Into<std::path::PathBuf>) -> Self {
        let persistence = WorkspacePersistence::new(base_path);
        let collection = persistence.load_or_default();

        Self {
            collection,
            persistence: Some(persistence),
            differ: UIDiffer::new(),
            registry: CommandRegistry::with_defaults(),
            keymap: Keymap::with_defaults(),
            auto_save: true,
            dirty: false,
        }
    }

    /// Set command registry.
    pub fn set_registry(&mut self, registry: CommandRegistry) {
        self.registry = registry;
    }

    /// Set keymap.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    /// Enable/disable auto-save.
    pub fn set_auto_save(&mut self, enabled: bool) {
        self.auto_save = enabled;
    }

    // =========================================================================
    // WORKSPACE QUERIES
    // =========================================================================

    /// Get active workspace name.
    pub fn active_workspace_name(&self) -> &str {
        &self.collection.active
    }

    /// Get active workspace state.
    pub fn active_workspace(&self) -> Option<&WorkspaceState> {
        self.collection.active()
    }

    /// Get workspace names.
    pub fn workspace_names(&self) -> Vec<&str> {
        self.collection.names()
    }

    /// Get workspace count.
    pub fn workspace_count(&self) -> usize {
        self.collection.workspaces.len()
    }

    // =========================================================================
    // WORKSPACE MUTATIONS
    // =========================================================================

    /// Switch to a different workspace.
    pub fn switch_workspace(&mut self, name: &str) -> bool {
        if self.collection.switch_to(name) {
            self.mark_dirty();
            self.maybe_auto_save();
            true
        } else {
            false
        }
    }

    /// Create a new workspace.
    pub fn create_workspace(&mut self, name: &str) -> bool {
        if self.collection.workspaces.contains_key(name) {
            return false;
        }

        let workspace = WorkspaceState::new(name);
        self.collection.add(workspace);
        self.mark_dirty();
        self.maybe_auto_save();
        true
    }

    /// Duplicate current workspace.
    pub fn duplicate_workspace(&mut self, new_name: &str) -> bool {
        let current = self.collection.active.clone();
        if self.collection.duplicate(&current, new_name) {
            self.mark_dirty();
            self.maybe_auto_save();
            true
        } else {
            false
        }
    }

    /// Delete a workspace.
    pub fn delete_workspace(&mut self, name: &str) -> bool {
        if self.collection.delete(name) {
            self.mark_dirty();
            self.maybe_auto_save();
            true
        } else {
            false
        }
    }

    // =========================================================================
    // PANEL MUTATIONS
    // =========================================================================

    /// Set panel visibility.
    pub fn set_panel_visible(&mut self, panel_id: &str, visible: bool) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.set_panel_visible(panel_id, visible);
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Toggle panel visibility.
    pub fn toggle_panel(&mut self, panel_id: &str) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.toggle_panel(panel_id);
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Set panel collapsed state.
    pub fn set_panel_collapsed(&mut self, panel_id: &str, collapsed: bool) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.set_panel_collapsed(panel_id, collapsed);
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Resize panel.
    pub fn resize_panel(&mut self, panel_id: &str, size: u32) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.resize_panel(panel_id, size);
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Move panel to new position.
    pub fn move_panel(&mut self, panel_id: &str, position: PanelPosition) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.move_panel(panel_id, position);
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    // =========================================================================
    // THEME & PREFERENCES
    // =========================================================================

    /// Set theme.
    pub fn set_theme(&mut self, theme: Theme) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.theme = theme;
            workspace.touch();
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Set preferences.
    pub fn set_preferences(&mut self, preferences: UIPreferences) {
        if let Some(workspace) = self.collection.active_mut() {
            workspace.preferences = preferences;
            workspace.touch();
            self.mark_dirty();
            self.maybe_auto_save();
        }
    }

    /// Get current theme.
    pub fn theme(&self) -> Option<&Theme> {
        self.collection.active().map(|ws| &ws.theme)
    }

    /// Get current preferences.
    pub fn preferences(&self) -> Option<&UIPreferences> {
        self.collection.active().map(|ws| &ws.preferences)
    }

    // =========================================================================
    // UI MODEL
    // =========================================================================

    /// Build current UIModel.
    pub fn build_ui_model(&self) -> UIModel {
        let mut model = UIModel::build(&self.registry, &self.keymap);

        if let Some(workspace) = self.collection.active() {
            model.theme = workspace.theme.clone();
            model.preferences = workspace.preferences.clone();
            model.layout = workspace.layout.clone();
        }

        model
    }

    /// Compute UI diff from last state.
    pub fn compute_diff(&mut self) -> UIDiffSet {
        let Some(workspace) = self.collection.active() else {
            return UIDiffSet::full_refresh(self.differ.sequence() + 1);
        };

        let panels = workspace
            .panels
            .values()
            .map(|p| (p.id.0.clone(), p.visible, p.position, p.size, p.collapsed));

        let snapshot = UISnapshot::capture(
            panels,
            &workspace.theme,
            &workspace.preferences,
            &self.collection.active,
        );

        self.differ
            .diff(snapshot, &workspace.theme, &workspace.preferences)
    }

    /// Force full refresh on next diff.
    pub fn invalidate(&mut self) {
        self.differ.reset();
    }

    // =========================================================================
    // PERSISTENCE
    // =========================================================================

    /// Save workspaces to disk.
    pub fn save(&mut self) -> PersistenceResult<()> {
        if let Some(ref persistence) = self.persistence {
            persistence.save(&mut self.collection)?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Load workspaces from disk.
    pub fn load(&mut self) -> PersistenceResult<()> {
        if let Some(ref persistence) = self.persistence {
            self.collection = persistence.load()?;
            self.differ.reset();
            self.dirty = false;
        }
        Ok(())
    }

    /// Check if there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn maybe_auto_save(&mut self) {
        if self.auto_save {
            let _ = self.save();
        }
    }
}

impl Default for WorkspaceEngine {
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
    use tempfile::TempDir;

    #[test]
    fn test_workspace_engine_new() {
        let engine = WorkspaceEngine::new();

        assert_eq!(engine.active_workspace_name(), "Editing");
        assert_eq!(engine.workspace_count(), 1);
    }

    #[test]
    fn test_workspace_roundtrip() {
        let temp_dir = TempDir::new().unwrap();

        // Create and modify
        {
            let mut engine = WorkspaceEngine::with_persistence(temp_dir.path());
            engine.set_auto_save(false);

            engine.set_panel_visible("panel.history", true);
            engine.create_workspace("Color Grading");

            engine.save().unwrap();
        }

        // Load in new engine
        {
            let engine = WorkspaceEngine::with_persistence(temp_dir.path());

            assert_eq!(engine.workspace_count(), 2);
            assert!(engine.workspace_names().contains(&"Color Grading"));
        }
    }

    #[test]
    fn test_docking_consistency() {
        let mut engine = WorkspaceEngine::new();

        // Move panel
        engine.move_panel("panel.properties", PanelPosition::Left);

        // Check it stuck
        let workspace = engine.active_workspace().unwrap();
        let panel = workspace.get_panel("panel.properties").unwrap();
        assert_eq!(panel.position, PanelPosition::Left);
    }

    #[test]
    fn test_multi_workspace_switching() {
        let mut engine = WorkspaceEngine::new();

        // Create second workspace
        engine.create_workspace("Audio");
        assert_eq!(engine.workspace_count(), 2);

        // Switch to it
        assert!(engine.switch_workspace("Audio"));
        assert_eq!(engine.active_workspace_name(), "Audio");

        // Modify it
        engine.set_panel_visible("panel.audio_mixer", true);

        // Switch back
        assert!(engine.switch_workspace("Editing"));
        assert_eq!(engine.active_workspace_name(), "Editing");

        // Original should be unchanged
        let workspace = engine.active_workspace().unwrap();
        let mixer = workspace.get_panel("panel.audio_mixer");
        // In default editing, audio_mixer starts hidden
        if let Some(panel) = mixer {
            assert!(!panel.visible);
        }
    }

    #[test]
    fn test_build_ui_model() {
        let engine = WorkspaceEngine::new();
        let model = engine.build_ui_model();

        assert!(!model.menu_bar.menus.is_empty());
        assert!(!model.toolbars.is_empty());
        assert!(!model.panels.is_empty());
    }

    #[test]
    fn test_compute_diff() {
        let mut engine = WorkspaceEngine::new();

        // First diff is full refresh
        let diff1 = engine.compute_diff();
        assert!(diff1.is_full_refresh());

        // No change - empty diff
        let diff2 = engine.compute_diff();
        assert!(diff2.is_empty());

        // Make a change
        engine.set_panel_visible("panel.history", true);
        let diff3 = engine.compute_diff();

        assert!(!diff3.is_full_refresh());
        assert!(!diff3.is_empty());
    }

    #[test]
    fn test_theme_and_preferences() {
        let mut engine = WorkspaceEngine::new();

        let new_theme = Theme {
            name: "Light".to_string(),
            dark: false,
            accent_color: "#10b981".to_string(),
        };

        engine.set_theme(new_theme.clone());

        assert_eq!(engine.theme().unwrap().name, "Light");
        assert!(!engine.theme().unwrap().dark);
    }

    #[test]
    fn test_dirty_flag() {
        let mut engine = WorkspaceEngine::new();
        engine.set_auto_save(false);

        assert!(!engine.is_dirty());

        engine.toggle_panel("panel.history");
        assert!(engine.is_dirty());
    }
}
