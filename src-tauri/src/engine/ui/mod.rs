//! UI module - View model and engine-to-UI bridge.
//!
//! # Architecture
//!
//! This module provides the connection between engine and UI:
//!
//! - `timeline_view_model` - Read-only, serializable view models
//! - `ui_events` - Event types and channels
//! - `bridge` - Engine-to-UI event bridge
//! - `composition` - Declarative UI model for React
//! - `media_pool` - Media pool view model
//!
//! # One-Way Data Flow
//!
//! ```text
//! TimelineEngine ─────────────────────────────────────▶ UIBridge
//!     │                                                     │
//!     │ mutation committed                                  │ UIEvent
//!     │ playhead tick                                       ▼
//!     │                                                   React
//!     │
//!     ◀───────────────────────────────────────────────────────
//!                    Tauri Commands (NOT bridge)
//! ```
//!
//! # Invariants
//!
//! 1. UI NEVER mutates engine state
//! 2. Engine is single source of truth
//! 3. No business logic in UI layer
//! 4. No unsafe code
//! 5. Deterministic & testable
//! 6. Use MediaTime everywhere

pub mod bridge;
pub mod composition;
pub mod media_pool;
pub mod timeline_view_model;
pub mod ui_events;

// Re-exports
pub use bridge::{BridgeConfig, BridgeStats, UIBridge};
pub use composition::{
    panels, DockRegion, LayoutNode, Menu, MenuBar, MenuItem, PanelDescriptor, PanelId,
    PanelPosition, PanelType, Theme, Toolbar, ToolbarItem, UIModel, UIModelBuilder, UIPreferences,
    WorkspaceLayout,
};
pub use media_pool::{MediaPoolItem, MediaPoolViewModel, MediaStatus};
pub use timeline_view_model::{build_view, ClipView, PlayheadView, TimelineViewModel, TrackView};
pub use ui_events::{ui_event_channel, UIEvent, UIEventReceiver, UIEventSender, UpdateReason};
