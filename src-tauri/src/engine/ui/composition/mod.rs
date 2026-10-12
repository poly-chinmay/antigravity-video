//! UI Composition - Declarative UI model for React.
//!
//! # Architecture
//!
//! ```text
//! CommandRegistry + Keymap
//!         ↓
//!    UIModel.build()
//!         ↓
//! ┌───────────────────────────────────┐
//! │ UIModel                           │
//! │ ├── MenuBar (from commands)       │
//! │ ├── Toolbars (from commands)      │
//! │ ├── Panels (descriptors)          │
//! │ ├── Layout (workspace)            │
//! │ ├── Theme                         │
//! │ └── Preferences                   │
//! └───────────────────────────────────┘
//!         ↓
//!    serialize to JSON
//!         ↓
//!    React render
//! ```
//!
//! # Invariants
//!
//! - No engine references in UI model
//! - All types are Serialize + Deserialize
//! - UI built purely from CommandRegistry + Keymap

pub mod layout;
pub mod menu;
pub mod panel;
pub mod toolbar;
pub mod ui_model;

// Re-exports
pub use layout::{DockRegion, LayoutNode, WorkspaceLayout};
pub use menu::{Menu, MenuBar, MenuItem};
pub use panel::{panels, PanelDescriptor, PanelId, PanelPosition, PanelType};
pub use toolbar::{Toolbar, ToolbarItem};
pub use ui_model::{Theme, UIModel, UIModelBuilder, UIPreferences};
