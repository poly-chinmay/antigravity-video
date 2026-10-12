//! Workspace module - Workspace engine and UI state system.
//!
//! # Architecture
//!
//! ```text
//! WorkspaceEngine (sole owner of mutable state)
//!         │
//!         ├── state: RwLock<WorkspaceState>
//!         │
//!         ├── apply_command(cmd) → Result     ← ONLY mutation path
//!         │
//!         └── snapshot() → WorkspaceState    ← returns CLONE
//!
//! WorkspaceState (pure data)
//!     ├── projects: HashMap<ProjectState>
//!     ├── panels: HashMap<PanelState>
//!     ├── window: WindowState
//!     └── focused_panel: Option<PanelId>
//! ```
//!
//! # Invariants
//!
//! | Rule | Enforcement |
//! |------|-------------|
//! | WorkspaceState is pure data | No methods on state types |
//! | WorkspaceEngine sole owner | RwLock<WorkspaceState> |
//! | All mutations via apply_command() | No other mutation paths |
//! | Engine returns cloned snapshots | snapshot() clones state |
//! | No UI dependencies | Types are UI-agnostic |
//! | All types Serialize/Deserialize | derive macros |
//! | Deterministic behavior | Same commands → same state |

// Core types (pure data, no methods)
pub mod workspace_types;

// Command enum (all mutations)
pub mod workspace_command;

// Error types
pub mod workspace_error;

// Engine (sole owner of state)
pub mod workspace_engine_v2;

// Legacy modules (to be deprecated)
pub mod ui_diff;
pub mod workspace_engine;
pub mod workspace_persistence;
pub mod workspace_state;

// Re-exports (new API)
pub use workspace_command::WorkspaceCommand;
pub use workspace_engine_v2::WorkspaceEngine as WorkspaceEngineV2;
pub use workspace_error::{WorkspaceError, WorkspaceResult};
pub use workspace_types::{
    calculate_checksum, create_default_workspace, create_panel, create_project, PanelId,
    PanelPosition, PanelState, ProjectId, ProjectState, WindowMode, WindowPosition, WindowSize,
    WindowState, WorkspaceId, WorkspaceState as WorkspaceStateV2, WORKSPACE_FORMAT_VERSION,
};

// Legacy re-exports (for backward compatibility)
pub use ui_diff::{UIDiff, UIDiffSet, UIDiffer, UISnapshot};
pub use workspace_engine::WorkspaceEngine;
pub use workspace_persistence::{
    JournalEntry, PersistenceError, PersistenceResult, RecoveryJournal, WorkspacePersistence,
};
pub use workspace_state::{
    PanelState as LegacyPanelState, WorkspaceCollection, WorkspaceState, WORKSPACE_VERSION,
};
