//! Layout - Docking and workspace layout model.
//!
//! # Design
//!
//! Layouts describe how panels are arranged in the workspace.
//! Supports docking regions, tabs, and splits.
//! Fully serializable for workspace persistence.

use serde::{Deserialize, Serialize};

use super::panel::{PanelId, PanelPosition};

// =============================================================================
// LAYOUT NODE
// =============================================================================

/// A node in the layout tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutNode {
    /// Single panel
    Panel(PanelId),

    /// Tabbed panels
    Tabs {
        /// Panel IDs in tabs
        panels: Vec<PanelId>,
        /// Currently active tab index
        active: usize,
    },

    /// Horizontal split
    HSplit {
        /// Left/top children
        children: Vec<LayoutNode>,
        /// Split ratios (0.0-1.0)
        ratios: Vec<f32>,
    },

    /// Vertical split
    VSplit {
        /// Top/bottom children
        children: Vec<LayoutNode>,
        /// Split ratios (0.0-1.0)
        ratios: Vec<f32>,
    },
}

impl LayoutNode {
    /// Create a single panel node.
    pub fn panel(id: impl Into<PanelId>) -> Self {
        Self::Panel(id.into())
    }

    /// Create a tabbed node.
    pub fn tabs(panels: Vec<PanelId>) -> Self {
        Self::Tabs { panels, active: 0 }
    }

    /// Create a horizontal split.
    pub fn hsplit(children: Vec<LayoutNode>, ratios: Vec<f32>) -> Self {
        Self::HSplit { children, ratios }
    }

    /// Create a vertical split.
    pub fn vsplit(children: Vec<LayoutNode>, ratios: Vec<f32>) -> Self {
        Self::VSplit { children, ratios }
    }

    /// Create equal horizontal split.
    pub fn hsplit_equal(children: Vec<LayoutNode>) -> Self {
        let n = children.len();
        let ratio = 1.0 / n as f32;
        let ratios = vec![ratio; n];
        Self::HSplit { children, ratios }
    }

    /// Create equal vertical split.
    pub fn vsplit_equal(children: Vec<LayoutNode>) -> Self {
        let n = children.len();
        let ratio = 1.0 / n as f32;
        let ratios = vec![ratio; n];
        Self::VSplit { children, ratios }
    }
}

// =============================================================================
// DOCK REGION
// =============================================================================

/// A docking region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockRegion {
    /// Position of this region
    pub position: PanelPosition,
    /// Layout tree for this region
    pub layout: Option<LayoutNode>,
    /// Region size (pixels, for left/right = width, for top/bottom = height)
    pub size: u32,
    /// Whether region is collapsed
    pub collapsed: bool,
}

impl DockRegion {
    /// Create a new dock region.
    pub fn new(position: PanelPosition) -> Self {
        Self {
            position,
            layout: None,
            size: match position {
                PanelPosition::Left | PanelPosition::Right => 250,
                PanelPosition::Top | PanelPosition::Bottom => 200,
                _ => 0,
            },
            collapsed: false,
        }
    }

    /// Set layout.
    pub fn with_layout(mut self, layout: LayoutNode) -> Self {
        self.layout = Some(layout);
        self
    }

    /// Set size.
    pub fn with_size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }

    /// Set collapsed.
    pub fn collapsed(mut self) -> Self {
        self.collapsed = true;
        self
    }
}

// =============================================================================
// WORKSPACE LAYOUT
// =============================================================================

/// Complete workspace layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLayout {
    /// Layout name
    pub name: String,

    /// Left dock region
    pub left: DockRegion,

    /// Right dock region
    pub right: DockRegion,

    /// Top dock region
    pub top: DockRegion,

    /// Bottom dock region
    pub bottom: DockRegion,

    /// Center content layout
    pub center: LayoutNode,
}

impl WorkspaceLayout {
    /// Create a new workspace layout.
    pub fn new(name: impl Into<String>, center: LayoutNode) -> Self {
        Self {
            name: name.into(),
            left: DockRegion::new(PanelPosition::Left),
            right: DockRegion::new(PanelPosition::Right),
            top: DockRegion::new(PanelPosition::Top),
            bottom: DockRegion::new(PanelPosition::Bottom),
            center,
        }
    }

    /// Builder: set left dock.
    pub fn with_left(mut self, region: DockRegion) -> Self {
        self.left = region;
        self
    }

    /// Builder: set right dock.
    pub fn with_right(mut self, region: DockRegion) -> Self {
        self.right = region;
        self
    }

    /// Builder: set bottom dock.
    pub fn with_bottom(mut self, region: DockRegion) -> Self {
        self.bottom = region;
        self
    }

    /// Create default editing layout.
    pub fn default_editing() -> Self {
        use super::panel::panels;

        Self::new(
            "Editing",
            LayoutNode::vsplit(
                vec![
                    // Top: Preview
                    LayoutNode::panel(panels::preview().id),
                    // Bottom: Timeline
                    LayoutNode::panel(panels::timeline().id),
                ],
                vec![0.6, 0.4],
            ),
        )
        .with_left(
            DockRegion::new(PanelPosition::Left)
                .with_layout(LayoutNode::panel(panels::media_browser().id))
                .with_size(280),
        )
        .with_right(
            DockRegion::new(PanelPosition::Right)
                .with_layout(LayoutNode::tabs(vec![
                    panels::properties().id,
                    panels::effects().id,
                ]))
                .with_size(280),
        )
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_node() {
        let node =
            LayoutNode::hsplit_equal(vec![LayoutNode::panel("left"), LayoutNode::panel("right")]);

        match node {
            LayoutNode::HSplit { children, ratios } => {
                assert_eq!(children.len(), 2);
                assert_eq!(ratios.len(), 2);
                assert!((ratios[0] - 0.5).abs() < 0.01);
            }
            _ => panic!("Expected HSplit"),
        }
    }

    #[test]
    fn test_layout_serializable() {
        let layout = WorkspaceLayout::default_editing();

        let json = serde_json::to_string(&layout).unwrap();
        let deserialized: WorkspaceLayout = serde_json::from_str(&json).unwrap();

        assert_eq!(layout.name, deserialized.name);
    }

    #[test]
    fn test_dock_region() {
        let region = DockRegion::new(PanelPosition::Left)
            .with_layout(LayoutNode::panel("test"))
            .with_size(300);

        assert_eq!(region.position, PanelPosition::Left);
        assert_eq!(region.size, 300);
        assert!(region.layout.is_some());
    }

    #[test]
    fn test_default_editing_layout() {
        let layout = WorkspaceLayout::default_editing();

        assert_eq!(layout.name, "Editing");
        assert!(layout.left.layout.is_some());
        assert!(layout.right.layout.is_some());
    }
}
