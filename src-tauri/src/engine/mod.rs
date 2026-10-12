//! Engine module - Core systems for Antigravity timeline management.
//!
//! # Architecture
//!
//! This module implements the God State pattern where TimelineEngine is the
//! sole owner of all mutable application state.
//!
//! # Module Structure
//!
//! - `media_time` - Integer-precision time representation
//! - `timeline_state` - Pure data structures (no business logic)
//! - `edit_action` - Command representation for all mutations
//! - `timeline_engine` - State owner with single mutation path
//! - `invariants` - Validation rules
//! - `errors` - Error types
//! - `event_store` - Append-only durable event log
//! - `snapshot_store` - Atomic point-in-time snapshots
//! - `recovery` - Crash-safe state reconstruction
//! - `ai_pipeline` - Hardened AI control surface
//! - `interval_tree` - AVL-balanced interval tree for O(log n) queries
//! - `timeline_index` - High-performance timeline query engine
//! - `playback` - Timeline playback and preview scheduling
//! - `render` - Frame and render orchestration
//! - `audio` - Audio clock and A/V synchronization
//! - `ui` - View model and UI bridge
//! - `interaction` - Editor tools and interaction model

pub mod ai_pipeline;
pub mod audio;
pub mod commands;
pub mod edit_action;
pub mod errors;
pub mod event_store;
pub mod interaction;
pub mod interval_tree;
pub mod invariants;
pub mod media_time;
pub mod orchestrator;
pub mod playback;
pub mod preview;
pub mod recovery;
pub mod render;
pub mod snapshot_store;
pub mod timeline_engine;
pub mod timeline_index;
pub mod timeline_state;
pub mod ui;
pub mod workspace;

// Re-exports for convenience
pub use ai_pipeline::{AIFailure, AIPipeline, AIResult, SafetyRule, UntrustedAIResponse};
pub use audio::{
    AVPair, AVSync, AudioBuffer, AudioChunk, AudioClock, AudioClockConfig, AudioClockState,
    AudioDevice, AudioScheduler, DeviceState, MockAudioDevice, SyncConfig, SyncStatus,
};
pub use commands::{
    commands as command_ids, CommandCategory, CommandContext, CommandDescriptor, CommandId,
    CommandRegistry, CommandResult, CommandRouter, CommandSnapshot, KeyBinding, Keymap,
    MutableContext, RouterResult,
};
pub use edit_action::{ActionParameters, ActionType, EditAction};
pub use errors::EngineError;
pub use event_store::{EventRecord, EventStore, EventStoreError};
pub use interaction::{
    ControllerConfig, DragDelta, DragOrigin, InteractionController, InteractionPhase,
    InteractionResult, InteractionState, MouseInput, PreviewState, SnapConfig, SnapResult, Snapper,
    ToolType,
};
pub use interval_tree::{IntervalEntry, IntervalTree, TimeRange};
pub use invariants::{InvariantValidator, InvariantViolation};
pub use media_time::MediaTime;
pub use orchestrator::{
    AppCommand, AppOrchestrator, AppSnapshot, OrchestratorCommandResult, OrchestratorError,
    OrchestratorResult, PanelInfo, PlaybackSnapshot, ProjectInfo, SystemCommand, TimelineCommand,
    TimelineSnapshot, WorkspaceSnapshot,
};
pub use playback::{
    Clock, FrameInfo, PlaybackRate, PlaybackScheduler, Playhead, SchedulerConfig, TimelineView,
    Transport, TransportCommand, TransportState, VisibleClip,
};
pub use recovery::{RecoveryEngine, RecoveryError, RecoveryResult};
pub use render::{
    CacheKey, CacheStats, ClipRenderInfo, FrameCache, FrameClock, FrameId, FrameQueue,
    FrameScheduler, OrchestratorConfig, RenderCommand, RenderOrchestrator, RenderPriority,
    RenderResult,
};
pub use snapshot_store::{Snapshot, SnapshotStore, SnapshotStoreError};
pub use timeline_engine::TimelineEngine;
pub use timeline_index::{IndexStats, TimelineIndex};
pub use timeline_state::{Clip, ClipId, TimelineState, TrackId};
pub use ui::{
    build_view, panels, ui_event_channel, BridgeConfig, ClipView, DockRegion, LayoutNode, Menu,
    MenuBar, MenuItem, PanelDescriptor, PanelId, PanelPosition, PanelType, PlayheadView, Theme,
    TimelineViewModel, Toolbar, ToolbarItem, TrackView, UIBridge, UIEvent, UIEventReceiver,
    UIEventSender, UIModel, UIModelBuilder, UIPreferences, UpdateReason, WorkspaceLayout,
};
pub use workspace::{
    JournalEntry, PanelState, PersistenceError, PersistenceResult, RecoveryJournal, UIDiff,
    UIDiffSet, UIDiffer, UISnapshot, WorkspaceCollection, WorkspaceEngine, WorkspacePersistence,
    WorkspaceState, WORKSPACE_VERSION,
};
