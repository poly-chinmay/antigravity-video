//! Workspace Types - Pure data types with no methods.
//!
//! # Invariants
//!
//! - All types are pure data (no methods, no logic)
//! - All types derive Serialize/Deserialize
//! - All types are Clone for snapshot creation
//! - No UI dependencies
//! - No mutation logic (handled by WorkspaceEngine)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// IDENTIFIERS
// =============================================================================

/// Unique identifier for a panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelId(pub String);

/// Unique identifier for a project.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub String);

/// Unique identifier for a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(pub String);

// =============================================================================
// PANEL POSITION
// =============================================================================

/// Docking position for panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelPosition {
    Left,
    Right,
    Top,
    Bottom,
    Center,
    Floating,
}

impl Default for PanelPosition {
    fn default() -> Self {
        Self::Center
    }
}

// =============================================================================
// PANEL STATE (Pure Data)
// =============================================================================

/// State of a single panel. Pure data, no methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelState {
    /// Panel identifier
    pub id: PanelId,

    /// Display title
    pub title: String,

    /// Whether panel is visible
    pub visible: bool,

    /// Docking position
    pub position: PanelPosition,

    /// Size in pixels (width for L/R, height for T/B)
    pub size: u32,

    /// Whether panel is collapsed
    pub collapsed: bool,

    /// Whether panel has focus
    pub focused: bool,

    /// Z-index for floating panels
    pub z_index: u32,

    /// Order within dock region
    pub order: u32,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            id: PanelId(String::new()),
            title: String::new(),
            visible: true,
            position: PanelPosition::Center,
            size: 250,
            collapsed: false,
            focused: false,
            z_index: 0,
            order: 0,
        }
    }
}

// =============================================================================
// PROJECT STATE (Pure Data)
// =============================================================================

/// State of a project. Pure data, no methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectState {
    /// Project identifier
    pub id: ProjectId,

    /// Project name
    pub name: String,

    /// File path (if saved)
    pub path: Option<String>,

    /// Whether project has unsaved changes
    pub dirty: bool,

    /// Last modified timestamp (Unix millis)
    pub last_modified: u64,

    /// Last accessed timestamp (Unix millis)
    pub last_accessed: u64,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            id: ProjectId(String::new()),
            name: String::from("Untitled"),
            path: None,
            dirty: false,
            last_modified: 0,
            last_accessed: 0,
        }
    }
}

// =============================================================================
// WINDOW STATE (Pure Data)
// =============================================================================

/// Window dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowSize {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

/// Window position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

impl Default for WindowPosition {
    fn default() -> Self {
        Self { x: 100, y: 100 }
    }
}

/// Window display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMode {
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
}

impl Default for WindowMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// State of the application window. Pure data, no methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    /// Window size
    pub size: WindowSize,

    /// Window position
    pub position: WindowPosition,

    /// Display mode
    pub mode: WindowMode,

    /// Whether window is always on top
    pub always_on_top: bool,

    /// Whether window decorations are visible
    pub decorations: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            size: WindowSize::default(),
            position: WindowPosition::default(),
            mode: WindowMode::Normal,
            always_on_top: false,
            decorations: true,
        }
    }
}

// =============================================================================
// WORKSPACE STATE (Pure Data)
// =============================================================================

/// Format version for persistence migration.
pub const WORKSPACE_FORMAT_VERSION: u32 = 2;

/// Complete workspace state. Pure data, no methods.
///
/// # Invariants
///
/// - This is pure data: no methods, no logic
/// - WorkspaceEngine is the sole owner and mutator
/// - All reads return cloned snapshots
/// - Deterministic: same inputs → same state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// Format version for migration
    pub version: u32,

    /// Workspace identifier
    pub id: WorkspaceId,

    /// Workspace name
    pub name: String,

    /// All projects
    pub projects: HashMap<String, ProjectState>,

    /// Active project ID (if any)
    pub active_project: Option<ProjectId>,

    /// All panels
    pub panels: HashMap<String, PanelState>,

    /// Currently focused panel (if any)
    pub focused_panel: Option<PanelId>,

    /// Window state
    pub window: WindowState,

    /// Last modified timestamp
    pub last_modified: u64,

    /// Checksum for integrity validation
    pub checksum: Option<String>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            version: WORKSPACE_FORMAT_VERSION,
            id: WorkspaceId(uuid()),
            name: String::from("Default"),
            projects: HashMap::new(),
            active_project: None,
            panels: HashMap::new(),
            focused_panel: None,
            window: WindowState::default(),
            last_modified: now_millis(),
            checksum: None,
        }
    }
}

// =============================================================================
// HELPERS (Pure functions, not methods)
// =============================================================================

/// Generate a UUID string.
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Get current timestamp in milliseconds.
fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// =============================================================================
// FACTORY FUNCTIONS (Not methods - keeps types pure)
// =============================================================================

/// Create a new PanelState with the given ID and title.
pub fn create_panel(id: &str, title: &str) -> PanelState {
    PanelState {
        id: PanelId(id.to_string()),
        title: title.to_string(),
        ..Default::default()
    }
}

/// Create a new ProjectState with the given name.
pub fn create_project(name: &str) -> ProjectState {
    ProjectState {
        id: ProjectId(uuid()),
        name: name.to_string(),
        last_modified: now_millis(),
        last_accessed: now_millis(),
        ..Default::default()
    }
}

/// Create a default workspace with standard panels.
pub fn create_default_workspace() -> WorkspaceState {
    let mut state = WorkspaceState::default();

    // Add standard panels
    let panels = vec![
        ("panel.timeline", "Timeline", PanelPosition::Bottom, 250),
        ("panel.preview", "Preview", PanelPosition::Center, 0),
        (
            "panel.media_browser",
            "Media Browser",
            PanelPosition::Left,
            280,
        ),
        ("panel.properties", "Properties", PanelPosition::Right, 280),
        ("panel.effects", "Effects", PanelPosition::Right, 280),
        (
            "panel.audio_mixer",
            "Audio Mixer",
            PanelPosition::Bottom,
            200,
        ),
        ("panel.history", "History", PanelPosition::Right, 200),
    ];

    for (idx, (id, title, pos, size)) in panels.into_iter().enumerate() {
        let panel = PanelState {
            id: PanelId(id.to_string()),
            title: title.to_string(),
            visible: !matches!(id, "panel.audio_mixer" | "panel.history"),
            position: pos,
            size,
            order: idx as u32,
            ..Default::default()
        };
        state.panels.insert(id.to_string(), panel);
    }

    state
}

/// Calculate checksum for a workspace state.
pub fn calculate_checksum(state: &WorkspaceState) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    state.version.hash(&mut hasher);
    state.name.hash(&mut hasher);
    state.panels.len().hash(&mut hasher);
    state.projects.len().hash(&mut hasher);

    // Sort for deterministic ordering
    let mut panel_ids: Vec<_> = state.panels.keys().collect();
    panel_ids.sort();
    for id in panel_ids {
        if let Some(panel) = state.panels.get(id) {
            id.hash(&mut hasher);
            panel.visible.hash(&mut hasher);
            panel.size.hash(&mut hasher);
        }
    }

    format!("{:016x}", hasher.finish())
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_state_default() {
        let panel = PanelState::default();
        assert!(panel.visible);
        assert!(!panel.collapsed);
        assert!(!panel.focused);
    }

    #[test]
    fn test_workspace_state_default() {
        let state = WorkspaceState::default();
        assert_eq!(state.version, WORKSPACE_FORMAT_VERSION);
        assert!(state.projects.is_empty());
        assert!(state.panels.is_empty());
        assert!(state.active_project.is_none());
    }

    #[test]
    fn test_create_default_workspace() {
        let state = create_default_workspace();
        assert!(!state.panels.is_empty());
        assert!(state.panels.contains_key("panel.timeline"));
        assert!(state.panels.contains_key("panel.preview"));
    }

    #[test]
    fn test_create_panel() {
        let panel = create_panel("test.panel", "Test Panel");
        assert_eq!(panel.id.0, "test.panel");
        assert_eq!(panel.title, "Test Panel");
    }

    #[test]
    fn test_create_project() {
        let project = create_project("My Project");
        assert_eq!(project.name, "My Project");
        assert!(!project.id.0.is_empty());
    }

    #[test]
    fn test_checksum_deterministic() {
        let state = create_default_workspace();
        let cs1 = calculate_checksum(&state);
        let cs2 = calculate_checksum(&state);
        assert_eq!(cs1, cs2);
    }

    #[test]
    fn test_types_serializable() {
        let state = create_default_workspace();
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WorkspaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }
}
