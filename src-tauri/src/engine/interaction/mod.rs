//! Interaction module - Editor tools and interaction model.
//!
//! # Architecture
//!
//! This module provides the interaction layer between React UI and engine:
//!
//! - `interaction_state` - State machine for interactions
//! - `tools` - Editor tool types (Select, Move, Trim, etc.)
//! - `snapping` - Snap-to-grid and snap-to-clip logic
//! - `interaction_controller` - Main coordinator
//!
//! # Flow
//!
//! ```text
//! React Input
//!    ↓
//! InteractionController
//!    ↓ (preview only)
//! UIBridge.build_view() ← adds preview overlay
//!    ↓
//! React Render
//!    ↓ (on commit)
//! InteractionController → EditActions → TimelineEngine.apply_action()
//!    ↓
//! UIBridge → UIEvent → React
//! ```
//!
//! # Invariants
//!
//! 1. UI never mutates engine state directly
//! 2. Only EditActions may mutate state
//! 3. Drag produces preview only
//! 4. Commit only on mouse_up
//! 5. All tools use snapping
//! 6. No unsafe code

pub mod interaction_controller;
pub mod interaction_state;
pub mod snapping;
pub mod tools;

// Re-exports
pub use interaction_controller::{
    ControllerConfig, InteractionController, InteractionResult, MouseInput,
};
pub use interaction_state::{
    DragDelta, DragOrigin, InteractionPhase, InteractionState, PreviewState,
};
pub use snapping::{SnapConfig, SnapResult, SnapTarget, Snapper};
pub use tools::{MoveTool, RazorTool, SelectTool, ToolContext, ToolResult, ToolType, TrimTool};
