//! Orchestrator module - Central application coordination.
//!
//! # Architecture
//!
//! ```text
//! Tauri Commands
//!         │
//!         ▼
//! ┌─────────────────────────────────────────────────────┐
//! │              AppOrchestrator                        │
//! │  (single authority, all commands flow through)      │
//! └─────────────────────────────────────────────────────┘
//!         │              │              │
//!         ▼              ▼              ▼
//! ┌───────────┐  ┌─────────────┐  ┌──────────────┐
//! │ Workspace │  │  Timeline   │  │   Playback   │
//! │  Engine   │  │   Engine    │  │  Scheduler   │
//! └───────────┘  └─────────────┘  └──────────────┘
//!         │              │              │
//!         └──────────────┴──────────────┘
//!                        │
//!                        ▼
//!               AppSnapshot ─────────▶ React
//! ```
//!
//! # Invariants
//!
//! 1. All cross-engine effects are atomic
//! 2. Failures roll back safely
//! 3. Orchestrator owns sequencing
//! 4. UI receives updates only from orchestrator
//! 5. Deterministic replay

pub mod app_command;
pub mod app_state;
pub mod orchestrator;

pub use app_command::{AppCommand, SystemCommand, TimelineCommand};
pub use app_state::{
    AppSnapshot, PanelInfo, PlaybackSnapshot, ProjectInfo, TimelineSnapshot, WorkspaceSnapshot,
};
pub use orchestrator::{
    AppOrchestrator, OrchestratorCommandResult, OrchestratorError, OrchestratorResult,
};
