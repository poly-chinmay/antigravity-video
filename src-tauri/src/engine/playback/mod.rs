//! Playback module - Timeline playback and preview scheduling.
//!
//! # Architecture
//!
//! This module provides a clean separation of concerns for playback:
//!
//! - `clock` - Wall-clock time abstraction (Instant-based)
//! - `playhead` - Timeline position tracking (MediaTime)
//! - `transport` - Play/pause/seek state machine
//! - `timeline_view` - Query clips at position via TimelineIndex
//! - `scheduler` - Coordinated playback scheduling
//!
//! # Threading Model
//!
//! All components are designed for single-threaded use by default.
//! For multi-threaded access, wrap in `Arc<RwLock<>>`.
//!
//! # Invariants
//!
//! 1. All timeline time uses MediaTime (integer nanoseconds)
//! 2. Wall-clock time (Instant) is ONLY used inside Clock
//! 3. TimelineIndex is used for all clip queries
//! 4. No mutations to TimelineState
//! 5. No unsafe code

pub mod clock;
pub mod playhead;
pub mod scheduler;
pub mod timeline_view;
pub mod transport;

// Re-exports for convenience
pub use clock::{Clock, PlaybackRate};
pub use playhead::Playhead;
pub use scheduler::{FrameInfo, PlaybackScheduler, SchedulerConfig};
pub use timeline_view::{TimelineView, VisibleClip};
pub use transport::{Transport, TransportCommand, TransportState};
