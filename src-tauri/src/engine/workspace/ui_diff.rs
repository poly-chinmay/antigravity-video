//! UI Diff - Compute minimal differences between UIModel states.
//!
//! # Design
//!
//! Instead of sending the entire UIModel on every change,
//! we compute a diff and send only what changed.
//!
//! This significantly reduces data transfer and React re-renders.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::engine::ui::composition::{PanelId, PanelPosition, Theme, UIModel, UIPreferences};

// =============================================================================
// DIFF TYPES
// =============================================================================

/// A single UI change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum UIDiff {
    /// Full model refresh (for initial load or major changes)
    FullRefresh,

    /// Panel visibility changed
    PanelVisibility { panel_id: String, visible: bool },

    /// Panel position changed
    PanelPosition {
        panel_id: String,
        position: PanelPosition,
    },

    /// Panel size changed
    PanelSize { panel_id: String, size: u32 },

    /// Panel collapsed state changed
    PanelCollapsed { panel_id: String, collapsed: bool },

    /// Theme changed
    ThemeChanged { theme: Theme },

    /// Preferences changed
    PreferencesChanged { preferences: UIPreferences },

    /// Active workspace changed
    WorkspaceChanged { workspace_name: String },

    /// Menu item enabled/disabled
    MenuItemEnabled { command_id: String, enabled: bool },

    /// Toolbar button toggled
    ToolbarButtonToggled { command_id: String, toggled: bool },
}

// =============================================================================
// DIFF SET
// =============================================================================

/// A set of diffs to apply.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UIDiffSet {
    /// All diffs in this set
    pub diffs: Vec<UIDiff>,
    /// Sequence number for ordering
    pub sequence: u64,
}

impl UIDiffSet {
    /// Create an empty diff set.
    pub fn new(sequence: u64) -> Self {
        Self {
            diffs: Vec::new(),
            sequence,
        }
    }

    /// Create a full refresh diff.
    pub fn full_refresh(sequence: u64) -> Self {
        Self {
            diffs: vec![UIDiff::FullRefresh],
            sequence,
        }
    }

    /// Add a diff.
    pub fn add(&mut self, diff: UIDiff) {
        self.diffs.push(diff);
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }

    /// Check if this is a full refresh.
    pub fn is_full_refresh(&self) -> bool {
        self.diffs.iter().any(|d| matches!(d, UIDiff::FullRefresh))
    }

    /// Get diff count.
    pub fn len(&self) -> usize {
        self.diffs.len()
    }
}

// =============================================================================
// UI SNAPSHOT
// =============================================================================

/// Lightweight snapshot for diffing.
#[derive(Debug, Clone)]
pub struct UISnapshot {
    /// Panel visibility by ID
    pub panel_visibility: std::collections::HashMap<String, bool>,
    /// Panel positions by ID
    pub panel_positions: std::collections::HashMap<String, PanelPosition>,
    /// Panel sizes by ID
    pub panel_sizes: std::collections::HashMap<String, u32>,
    /// Panel collapsed states
    pub panel_collapsed: std::collections::HashMap<String, bool>,
    /// Theme (serialized for comparison)
    pub theme_hash: u64,
    /// Preferences (serialized for comparison)
    pub preferences_hash: u64,
    /// Active workspace
    pub active_workspace: String,
}

impl UISnapshot {
    /// Capture snapshot from current state.
    pub fn capture(
        panels: impl Iterator<Item = (String, bool, PanelPosition, u32, bool)>,
        theme: &Theme,
        preferences: &UIPreferences,
        active_workspace: &str,
    ) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut panel_visibility = std::collections::HashMap::new();
        let mut panel_positions = std::collections::HashMap::new();
        let mut panel_sizes = std::collections::HashMap::new();
        let mut panel_collapsed = std::collections::HashMap::new();

        for (id, visible, position, size, collapsed) in panels {
            panel_visibility.insert(id.clone(), visible);
            panel_positions.insert(id.clone(), position);
            panel_sizes.insert(id.clone(), size);
            panel_collapsed.insert(id, collapsed);
        }

        // Hash theme
        let mut hasher = DefaultHasher::new();
        theme.name.hash(&mut hasher);
        theme.dark.hash(&mut hasher);
        theme.accent_color.hash(&mut hasher);
        let theme_hash = hasher.finish();

        // Hash preferences
        let mut hasher = DefaultHasher::new();
        preferences.show_tooltips.hash(&mut hasher);
        preferences.tooltip_delay_ms.hash(&mut hasher);
        preferences.animations_enabled.hash(&mut hasher);
        // Float comparison via bits
        let sens_bits = preferences.zoom_sensitivity.to_bits();
        sens_bits.hash(&mut hasher);
        let preferences_hash = hasher.finish();

        Self {
            panel_visibility,
            panel_positions,
            panel_sizes,
            panel_collapsed,
            theme_hash,
            preferences_hash,
            active_workspace: active_workspace.to_string(),
        }
    }
}

// =============================================================================
// UI DIFFER
// =============================================================================

/// Computes minimal diffs between UI states.
#[derive(Debug)]
pub struct UIDiffer {
    /// Previous snapshot
    previous: Option<UISnapshot>,
    /// Sequence counter
    sequence: u64,
}

impl UIDiffer {
    /// Create a new differ.
    pub fn new() -> Self {
        Self {
            previous: None,
            sequence: 0,
        }
    }

    /// Compute diff between previous and current state.
    pub fn diff(
        &mut self,
        current: UISnapshot,
        theme: &Theme,
        preferences: &UIPreferences,
    ) -> UIDiffSet {
        self.sequence += 1;

        let Some(prev) = &self.previous else {
            // First diff - full refresh
            self.previous = Some(current);
            return UIDiffSet::full_refresh(self.sequence);
        };

        let mut diff_set = UIDiffSet::new(self.sequence);

        // Check workspace change
        if current.active_workspace != prev.active_workspace {
            diff_set.add(UIDiff::WorkspaceChanged {
                workspace_name: current.active_workspace.clone(),
            });
        }

        // Check theme change
        if current.theme_hash != prev.theme_hash {
            diff_set.add(UIDiff::ThemeChanged {
                theme: theme.clone(),
            });
        }

        // Check preferences change
        if current.preferences_hash != prev.preferences_hash {
            diff_set.add(UIDiff::PreferencesChanged {
                preferences: preferences.clone(),
            });
        }

        // Check panel changes
        let all_panels: HashSet<_> = current
            .panel_visibility
            .keys()
            .chain(prev.panel_visibility.keys())
            .cloned()
            .collect();

        for panel_id in all_panels {
            // Visibility
            let curr_vis = current.panel_visibility.get(&panel_id).copied();
            let prev_vis = prev.panel_visibility.get(&panel_id).copied();
            if curr_vis != prev_vis {
                if let Some(visible) = curr_vis {
                    diff_set.add(UIDiff::PanelVisibility {
                        panel_id: panel_id.clone(),
                        visible,
                    });
                }
            }

            // Position
            let curr_pos = current.panel_positions.get(&panel_id);
            let prev_pos = prev.panel_positions.get(&panel_id);
            if curr_pos != prev_pos {
                if let Some(position) = curr_pos {
                    diff_set.add(UIDiff::PanelPosition {
                        panel_id: panel_id.clone(),
                        position: *position,
                    });
                }
            }

            // Size
            let curr_size = current.panel_sizes.get(&panel_id).copied();
            let prev_size = prev.panel_sizes.get(&panel_id).copied();
            if curr_size != prev_size {
                if let Some(size) = curr_size {
                    diff_set.add(UIDiff::PanelSize {
                        panel_id: panel_id.clone(),
                        size,
                    });
                }
            }

            // Collapsed
            let curr_col = current.panel_collapsed.get(&panel_id).copied();
            let prev_col = prev.panel_collapsed.get(&panel_id).copied();
            if curr_col != prev_col {
                if let Some(collapsed) = curr_col {
                    diff_set.add(UIDiff::PanelCollapsed {
                        panel_id: panel_id.clone(),
                        collapsed,
                    });
                }
            }
        }

        // Update previous
        self.previous = Some(current);

        diff_set
    }

    /// Reset differ (forces full refresh on next diff).
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// Get current sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl Default for UIDiffer {
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

    fn make_panels() -> Vec<(String, bool, PanelPosition, u32, bool)> {
        vec![
            (
                "panel.timeline".to_string(),
                true,
                PanelPosition::Bottom,
                250,
                false,
            ),
            (
                "panel.preview".to_string(),
                true,
                PanelPosition::Center,
                0,
                false,
            ),
            (
                "panel.history".to_string(),
                false,
                PanelPosition::Right,
                200,
                false,
            ),
        ]
    }

    #[test]
    fn test_first_diff_is_full_refresh() {
        let mut differ = UIDiffer::new();
        let theme = Theme::default();
        let prefs = UIPreferences::default();

        let snapshot = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");

        let diff = differ.diff(snapshot, &theme, &prefs);

        assert!(diff.is_full_refresh());
    }

    #[test]
    fn test_minimal_ui_diff() {
        let mut differ = UIDiffer::new();
        let theme = Theme::default();
        let prefs = UIPreferences::default();

        // First snapshot
        let snapshot1 = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let _ = differ.diff(snapshot1, &theme, &prefs);

        // Second snapshot - only history visibility changed
        let mut panels2 = make_panels();
        panels2[2].1 = true; // history now visible

        let snapshot2 = UISnapshot::capture(panels2.into_iter(), &theme, &prefs, "Editing");
        let diff = differ.diff(snapshot2, &theme, &prefs);

        // Should have exactly one diff
        assert!(!diff.is_full_refresh());
        assert_eq!(diff.len(), 1);

        match &diff.diffs[0] {
            UIDiff::PanelVisibility { panel_id, visible } => {
                assert_eq!(panel_id, "panel.history");
                assert!(*visible);
            }
            _ => panic!("Expected PanelVisibility diff"),
        }
    }

    #[test]
    fn test_no_diff_when_unchanged() {
        let mut differ = UIDiffer::new();
        let theme = Theme::default();
        let prefs = UIPreferences::default();

        // First snapshot
        let snapshot1 = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let _ = differ.diff(snapshot1, &theme, &prefs);

        // Same snapshot again
        let snapshot2 = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let diff = differ.diff(snapshot2, &theme, &prefs);

        assert!(diff.is_empty());
    }

    #[test]
    fn test_theme_change_diff() {
        let mut differ = UIDiffer::new();
        let theme1 = Theme::default();
        let prefs = UIPreferences::default();

        let snapshot1 = UISnapshot::capture(make_panels().into_iter(), &theme1, &prefs, "Editing");
        let _ = differ.diff(snapshot1, &theme1, &prefs);

        // Change theme
        let theme2 = Theme {
            name: "Light".to_string(),
            dark: false,
            accent_color: "#ef4444".to_string(),
        };

        let snapshot2 = UISnapshot::capture(make_panels().into_iter(), &theme2, &prefs, "Editing");
        let diff = differ.diff(snapshot2, &theme2, &prefs);

        assert!(!diff.is_full_refresh());
        assert!(diff
            .diffs
            .iter()
            .any(|d| matches!(d, UIDiff::ThemeChanged { .. })));
    }

    #[test]
    fn test_workspace_change_diff() {
        let mut differ = UIDiffer::new();
        let theme = Theme::default();
        let prefs = UIPreferences::default();

        let snapshot1 = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let _ = differ.diff(snapshot1, &theme, &prefs);

        // Change workspace
        let snapshot2 =
            UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Color Grading");
        let diff = differ.diff(snapshot2, &theme, &prefs);

        assert!(diff.diffs.iter().any(|d| matches!(
            d,
            UIDiff::WorkspaceChanged { workspace_name } if workspace_name == "Color Grading"
        )));
    }

    #[test]
    fn test_diff_serializable() {
        let diff_set = UIDiffSet {
            diffs: vec![
                UIDiff::PanelVisibility {
                    panel_id: "test".to_string(),
                    visible: true,
                },
                UIDiff::ThemeChanged {
                    theme: Theme::default(),
                },
            ],
            sequence: 42,
        };

        let json = serde_json::to_string(&diff_set).unwrap();
        let deserialized: UIDiffSet = serde_json::from_str(&json).unwrap();

        assert_eq!(diff_set.sequence, deserialized.sequence);
        assert_eq!(diff_set.diffs.len(), deserialized.diffs.len());
    }

    #[test]
    fn test_differ_reset() {
        let mut differ = UIDiffer::new();
        let theme = Theme::default();
        let prefs = UIPreferences::default();

        // First diff
        let snapshot = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let _ = differ.diff(snapshot, &theme, &prefs);

        // Reset
        differ.reset();

        // Next diff should be full refresh
        let snapshot = UISnapshot::capture(make_panels().into_iter(), &theme, &prefs, "Editing");
        let diff = differ.diff(snapshot, &theme, &prefs);

        assert!(diff.is_full_refresh());
    }
}
