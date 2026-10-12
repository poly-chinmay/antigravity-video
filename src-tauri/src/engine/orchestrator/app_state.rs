//! App State - Unified application state snapshot.
//!
//! # Design
//!
//! AppSnapshot captures the complete application state at a point in time.
//! It aggregates state from all engines into a single, consistent view.
//!
//! # Invariants
//!
//! - AppSnapshot is immutable once created
//! - All engine states captured atomically
//! - Serializable for persistence/transmission

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;
use crate::engine::playback::TransportState;
use crate::engine::timeline_state::{Clip, TimelineState};
use crate::engine::workspace::workspace_types::{WindowState, WorkspaceState};

// =============================================================================
// PLAYBACK SNAPSHOT
// =============================================================================

/// Snapshot of playback state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSnapshot {
    /// Current playhead position
    pub position: MediaTime,

    /// Transport state (playing, paused, stopped)
    pub transport: TransportState,

    /// Playback rate
    pub rate: f64,

    /// Loop enabled
    pub loop_enabled: bool,

    /// Loop start position
    pub loop_start: Option<MediaTime>,

    /// Loop end position
    pub loop_end: Option<MediaTime>,
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            position: MediaTime::ZERO,
            transport: TransportState::Stopped,
            rate: 1.0,
            loop_enabled: false,
            loop_start: None,
            loop_end: None,
        }
    }
}

// =============================================================================
// TIMELINE SNAPSHOT
// =============================================================================

/// Snapshot of timeline state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineSnapshot {
    /// All clips
    pub clips: Vec<Clip>,

    /// Total duration
    pub duration: MediaTime,

    /// Selected clip IDs
    pub selected: Vec<String>,

    /// Zoom level (pixels per second)
    pub zoom: f64,

    /// Scroll position (nanoseconds)
    pub scroll_position: i64,
}

impl Default for TimelineSnapshot {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            duration: MediaTime::ZERO,
            selected: Vec::new(),
            zoom: 100.0,
            scroll_position: 0,
        }
    }
}

impl From<&TimelineState> for TimelineSnapshot {
    fn from(state: &TimelineState) -> Self {
        Self {
            clips: state.clips.clone(),
            duration: state.duration,
            selected: Vec::new(), // Selection tracked separately
            zoom: 100.0,          // Default zoom
            scroll_position: 0,
        }
    }
}

// =============================================================================
// WORKSPACE SNAPSHOT
// =============================================================================

/// Snapshot of workspace state (simplified for UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// Workspace name
    pub name: String,

    /// All projects
    pub projects: Vec<ProjectInfo>,

    /// Active project ID
    pub active_project: Option<String>,

    /// Panel states
    pub panels: Vec<PanelInfo>,

    /// Focused panel ID
    pub focused_panel: Option<String>,

    /// Window state
    pub window: WindowState,
}

/// Simplified project info for UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub dirty: bool,
}

/// Simplified panel info for UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelInfo {
    pub id: String,
    pub title: String,
    pub visible: bool,
    pub position: String,
    pub size: u32,
    pub collapsed: bool,
    pub focused: bool,
}

impl From<&WorkspaceState> for WorkspaceSnapshot {
    fn from(state: &WorkspaceState) -> Self {
        Self {
            name: state.name.clone(),
            projects: state
                .projects
                .values()
                .map(|p| ProjectInfo {
                    id: p.id.0.clone(),
                    name: p.name.clone(),
                    path: p.path.clone(),
                    dirty: p.dirty,
                })
                .collect(),
            active_project: state.active_project.as_ref().map(|id| id.0.clone()),
            panels: state
                .panels
                .values()
                .map(|p| PanelInfo {
                    id: p.id.0.clone(),
                    title: p.title.clone(),
                    visible: p.visible,
                    position: format!("{:?}", p.position),
                    size: p.size,
                    collapsed: p.collapsed,
                    focused: p.focused,
                })
                .collect(),
            focused_panel: state.focused_panel.as_ref().map(|id| id.0.clone()),
            window: state.window.clone(),
        }
    }
}

// =============================================================================
// APP SNAPSHOT
// =============================================================================

/// Complete application state snapshot.
///
/// # Invariants
///
/// - All engine states captured atomically
/// - Immutable once created
/// - Fully serializable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    /// Snapshot version (for cache invalidation)
    pub version: u64,

    /// Capture timestamp (Unix millis)
    pub timestamp: u64,

    /// Workspace state
    pub workspace: WorkspaceSnapshot,

    /// Timeline state
    pub timeline: TimelineSnapshot,

    /// Playback state
    pub playback: PlaybackSnapshot,

    /// Whether any state has unsaved changes
    pub dirty: bool,
}

impl AppSnapshot {
    /// Create a new snapshot from engine states.
    pub fn capture(
        version: u64,
        workspace: &WorkspaceState,
        timeline: &TimelineState,
        playback: PlaybackSnapshot,
    ) -> Self {
        Self {
            version,
            timestamp: now_millis(),
            workspace: WorkspaceSnapshot::from(workspace),
            timeline: TimelineSnapshot::from(timeline),
            playback,
            dirty: workspace.projects.values().any(|p| p.dirty),
        }
    }
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            version: 0,
            timestamp: now_millis(),
            workspace: WorkspaceSnapshot {
                name: String::from("Default"),
                projects: Vec::new(),
                active_project: None,
                panels: Vec::new(),
                focused_panel: None,
                window: WindowState::default(),
            },
            timeline: TimelineSnapshot::default(),
            playback: PlaybackSnapshot::default(),
            dirty: false,
        }
    }
}

// =============================================================================
// HELPER
// =============================================================================

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_snapshot_default() {
        let snapshot = AppSnapshot::default();
        assert_eq!(snapshot.version, 0);
        assert!(!snapshot.dirty);
    }

    #[test]
    fn test_playback_snapshot_default() {
        let snapshot = PlaybackSnapshot::default();
        assert_eq!(snapshot.position, MediaTime::ZERO);
        assert_eq!(snapshot.transport, TransportState::Stopped);
    }

    #[test]
    fn test_timeline_snapshot_default() {
        let snapshot = TimelineSnapshot::default();
        assert!(snapshot.clips.is_empty());
        assert_eq!(snapshot.duration, MediaTime::ZERO);
    }

    #[test]
    fn test_snapshot_serializable() {
        let snapshot = AppSnapshot::default();
        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: AppSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot.version, deserialized.version);
    }
}
