//! Panel - Panel descriptors for UI composition.
//!
//! # Design
//!
//! Panels are the primary containers for UI content.
//! Each panel has:
//! - Unique identifier
//! - Title and icon
//! - Content type
//! - Docking position
//!
//! Panels are purely declarative - no engine references.

use serde::{Deserialize, Serialize};

// =============================================================================
// PANEL ID
// =============================================================================

/// Unique identifier for a panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelId(pub String);

impl PanelId {
    /// Create a new panel ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for PanelId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// =============================================================================
// PANEL TYPE
// =============================================================================

/// Type of panel content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelType {
    /// Timeline panel
    Timeline,
    /// Video preview
    Preview,
    /// Media browser (file system)
    MediaBrowser,
    /// Media pool (imported media)
    MediaPool,
    /// Properties/inspector
    Properties,
    /// Effects library
    Effects,
    /// Transitions library
    Transitions,
    /// Audio mixer
    AudioMixer,
    /// Project files
    ProjectFiles,
    /// History/undo
    History,
}

// =============================================================================
// PANEL POSITION
// =============================================================================

/// Docking position for panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelPosition {
    /// Left dock
    Left,
    /// Right dock
    Right,
    /// Top dock
    Top,
    /// Bottom dock
    Bottom,
    /// Center (main content)
    Center,
    /// Floating window
    Floating,
}

impl Default for PanelPosition {
    fn default() -> Self {
        Self::Center
    }
}

// =============================================================================
// PANEL DESCRIPTOR
// =============================================================================

/// Descriptor for a panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelDescriptor {
    /// Unique identifier
    pub id: PanelId,

    /// Display title
    pub title: String,

    /// Icon name (for icon lookup)
    pub icon: Option<String>,

    /// Panel type
    pub panel_type: PanelType,

    /// Default docking position
    pub default_position: PanelPosition,

    /// Whether panel is visible
    pub visible: bool,

    /// Whether panel can be closed
    pub closable: bool,

    /// Minimum width (pixels)
    pub min_width: u32,

    /// Minimum height (pixels)
    pub min_height: u32,
}

impl PanelDescriptor {
    /// Create a new panel descriptor.
    pub fn new(id: impl Into<PanelId>, title: impl Into<String>, panel_type: PanelType) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            panel_type,
            default_position: PanelPosition::Center,
            visible: true,
            closable: true,
            min_width: 200,
            min_height: 100,
        }
    }

    /// Builder: set icon.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Builder: set position.
    pub fn at_position(mut self, position: PanelPosition) -> Self {
        self.default_position = position;
        self
    }

    /// Builder: set hidden initially.
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    /// Builder: set unclosable.
    pub fn unclosable(mut self) -> Self {
        self.closable = false;
        self
    }

    /// Builder: set minimum size.
    pub fn with_min_size(mut self, width: u32, height: u32) -> Self {
        self.min_width = width;
        self.min_height = height;
        self
    }
}

// =============================================================================
// STANDARD PANELS
// =============================================================================

/// Standard panel definitions.
pub mod panels {
    use super::*;

    pub fn timeline() -> PanelDescriptor {
        PanelDescriptor::new("panel.timeline", "Timeline", PanelType::Timeline)
            .with_icon("timeline")
            .at_position(PanelPosition::Bottom)
            .unclosable()
            .with_min_size(400, 200)
    }

    pub fn preview() -> PanelDescriptor {
        PanelDescriptor::new("panel.preview", "Preview", PanelType::Preview)
            .with_icon("play")
            .at_position(PanelPosition::Center)
            .unclosable()
            .with_min_size(320, 240)
    }

    pub fn media_browser() -> PanelDescriptor {
        PanelDescriptor::new(
            "panel.media_browser",
            "Media Browser",
            PanelType::MediaBrowser,
        )
        .with_icon("folder")
        .at_position(PanelPosition::Left)
        .with_min_size(200, 150)
    }

    pub fn properties() -> PanelDescriptor {
        PanelDescriptor::new("panel.properties", "Properties", PanelType::Properties)
            .with_icon("settings")
            .at_position(PanelPosition::Right)
            .with_min_size(200, 150)
    }

    pub fn effects() -> PanelDescriptor {
        PanelDescriptor::new("panel.effects", "Effects", PanelType::Effects)
            .with_icon("sparkle")
            .at_position(PanelPosition::Right)
            .with_min_size(200, 150)
    }

    pub fn audio_mixer() -> PanelDescriptor {
        PanelDescriptor::new("panel.audio_mixer", "Audio Mixer", PanelType::AudioMixer)
            .with_icon("volume")
            .at_position(PanelPosition::Bottom)
            .hidden()
            .with_min_size(300, 150)
    }

    pub fn history() -> PanelDescriptor {
        PanelDescriptor::new("panel.history", "History", PanelType::History)
            .with_icon("clock")
            .at_position(PanelPosition::Right)
            .hidden()
            .with_min_size(180, 100)
    }

    /// Media Pool panel - displays all imported media.
    pub fn media_pool() -> PanelDescriptor {
        PanelDescriptor::new("panel.media_pool", "Media Pool", PanelType::MediaPool)
            .with_icon("film")
            .at_position(PanelPosition::Left)
            .with_min_size(200, 200)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_descriptor() {
        let panel = PanelDescriptor::new("test", "Test Panel", PanelType::Timeline)
            .with_icon("test-icon")
            .at_position(PanelPosition::Left);

        assert_eq!(panel.id.0, "test");
        assert_eq!(panel.title, "Test Panel");
        assert_eq!(panel.icon, Some("test-icon".to_string()));
        assert_eq!(panel.default_position, PanelPosition::Left);
    }

    #[test]
    fn test_panel_serializable() {
        let panel = panels::timeline();

        let json = serde_json::to_string(&panel).unwrap();
        let deserialized: PanelDescriptor = serde_json::from_str(&json).unwrap();

        assert_eq!(panel.id, deserialized.id);
        assert_eq!(panel.title, deserialized.title);
    }

    #[test]
    fn test_standard_panels() {
        let timeline = panels::timeline();
        assert_eq!(timeline.panel_type, PanelType::Timeline);
        assert!(!timeline.closable);

        let preview = panels::preview();
        assert_eq!(preview.panel_type, PanelType::Preview);

        let media = panels::media_browser();
        assert!(media.closable);
    }
}
