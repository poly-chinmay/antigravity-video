//! UI Model - Final UI description for React.
//!
//! # Design
//!
//! UIModel is the complete, serializable UI state sent to React.
//! Contains:
//! - Menu bar
//! - Toolbars
//! - Panel descriptors
//! - Layout configuration
//! - Theme/preferences
//!
//! Built entirely from CommandRegistry + Keymap.
//! NO engine references - purely declarative.

use serde::{Deserialize, Serialize};

use crate::engine::commands::{CommandRegistry, Keymap};

use super::layout::WorkspaceLayout;
use super::menu::MenuBar;
use super::panel::PanelDescriptor;
use super::toolbar::Toolbar;

// =============================================================================
// THEME
// =============================================================================

/// UI theme configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Theme name
    pub name: String,
    /// Whether dark mode
    pub dark: bool,
    /// Accent color (hex)
    pub accent_color: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            dark: true,
            accent_color: "#6366f1".to_string(), // Indigo
        }
    }
}

// =============================================================================
// UI PREFERENCES
// =============================================================================

/// UI preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UIPreferences {
    /// Show tooltips
    pub show_tooltips: bool,
    /// Tooltip delay (ms)
    pub tooltip_delay_ms: u32,
    /// Animation enabled
    pub animations_enabled: bool,
    /// Zoom sensitivity
    pub zoom_sensitivity: f32,
}

impl Default for UIPreferences {
    fn default() -> Self {
        Self {
            show_tooltips: true,
            tooltip_delay_ms: 500,
            animations_enabled: true,
            zoom_sensitivity: 1.0,
        }
    }
}

// =============================================================================
// UI MODEL
// =============================================================================

/// Complete UI model for React.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIModel {
    /// Version for cache invalidation
    pub version: u32,

    /// Menu bar
    pub menu_bar: MenuBar,

    /// Toolbars
    pub toolbars: Vec<Toolbar>,

    /// All panel descriptors
    pub panels: Vec<PanelDescriptor>,

    /// Current workspace layout
    pub layout: WorkspaceLayout,

    /// Theme
    pub theme: Theme,

    /// Preferences
    pub preferences: UIPreferences,
}

impl UIModel {
    /// Build complete UI model from registry and keymap.
    pub fn build(registry: &CommandRegistry, keymap: &Keymap) -> Self {
        use super::panel::panels;

        Self {
            version: 1,
            menu_bar: MenuBar::from_registry(registry, keymap),
            toolbars: vec![
                Toolbar::tools_toolbar(registry, keymap),
                Toolbar::transport_toolbar(registry, keymap),
                Toolbar::edit_toolbar(registry, keymap),
            ],
            panels: vec![
                panels::timeline(),
                panels::preview(),
                panels::media_browser(),
                panels::media_pool(),
                panels::properties(),
                panels::effects(),
                panels::audio_mixer(),
                panels::history(),
            ],
            layout: WorkspaceLayout::default_editing(),
            theme: Theme::default(),
            preferences: UIPreferences::default(),
        }
    }

    /// Get panel by ID.
    pub fn get_panel(&self, id: &str) -> Option<&PanelDescriptor> {
        self.panels.iter().find(|p| p.id.0 == id)
    }

    /// Get toolbar by ID.
    pub fn get_toolbar(&self, id: &str) -> Option<&Toolbar> {
        self.toolbars.iter().find(|t| t.id == id)
    }
}

impl Default for UIModel {
    fn default() -> Self {
        Self::build(&CommandRegistry::with_defaults(), &Keymap::with_defaults())
    }
}

// =============================================================================
// UI MODEL BUILDER
// =============================================================================

/// Builder for customizing UIModel.
#[derive(Debug)]
pub struct UIModelBuilder {
    registry: CommandRegistry,
    keymap: Keymap,
    theme: Theme,
    preferences: UIPreferences,
}

impl UIModelBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry: CommandRegistry::with_defaults(),
            keymap: Keymap::with_defaults(),
            theme: Theme::default(),
            preferences: UIPreferences::default(),
        }
    }

    /// Use custom registry.
    pub fn with_registry(mut self, registry: CommandRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Use custom keymap.
    pub fn with_keymap(mut self, keymap: Keymap) -> Self {
        self.keymap = keymap;
        self
    }

    /// Use custom theme.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Use custom preferences.
    pub fn with_preferences(mut self, preferences: UIPreferences) -> Self {
        self.preferences = preferences;
        self
    }

    /// Build the UI model.
    pub fn build(self) -> UIModel {
        let mut model = UIModel::build(&self.registry, &self.keymap);
        model.theme = self.theme;
        model.preferences = self.preferences;
        model
    }
}

impl Default for UIModelBuilder {
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
    fn test_ui_model_build() {
        let registry = CommandRegistry::with_defaults();
        let keymap = Keymap::with_defaults();

        let model = UIModel::build(&registry, &keymap);

        assert!(!model.menu_bar.menus.is_empty());
        assert!(!model.toolbars.is_empty());
        assert!(!model.panels.is_empty());
    }

    #[test]
    fn test_ui_model_serializable() {
        let model = UIModel::default();

        let json = serde_json::to_string(&model).unwrap();
        let deserialized: UIModel = serde_json::from_str(&json).unwrap();

        assert_eq!(model.version, deserialized.version);
        assert_eq!(model.panels.len(), deserialized.panels.len());
    }

    #[test]
    fn test_ui_model_complete() {
        let model = UIModel::default();

        // Has all required components
        assert!(!model.menu_bar.menus.is_empty());
        assert!(model.toolbars.len() >= 3);
        assert!(model.panels.len() >= 5);
        assert!(!model.layout.name.is_empty());
        assert!(model.theme.dark);
    }

    #[test]
    fn test_get_panel() {
        let model = UIModel::default();

        let timeline = model.get_panel("panel.timeline");
        assert!(timeline.is_some());
        assert_eq!(timeline.unwrap().title, "Timeline");
    }

    #[test]
    fn test_builder() {
        let theme = Theme {
            name: "Custom".to_string(),
            dark: false,
            accent_color: "#ef4444".to_string(),
        };

        let model = UIModelBuilder::new().with_theme(theme).build();

        assert_eq!(model.theme.name, "Custom");
        assert!(!model.theme.dark);
    }
}
