//! Render module - Frame and render orchestration.
//!
//! # Architecture
//!
//! This module provides the visual frame pipeline:
//!
//! - `frame_clock` - Frame timing and vsync
//! - `render_command` - Render instructions
//! - `frame_queue` - Bounded frame buffer
//! - `frame_scheduler` - Frame production
//! - `render_orchestrator` - Pipeline coordinator
//! - `frame_cache` - LRU frame cache
//!
//! # Threading Model
//!
//! - FrameScheduler runs in engine thread
//! - RenderOrchestrator coordinates with renderer thread(s)
//! - All components are single-threaded, wrap in Arc<RwLock> for multi-threading
//!
//! # Performance Characteristics
//!
//! - Frame queue prevents starvation and overflow
//! - Cache provides O(1) frame reuse
//! - Non-blocking: slow renderer drops frames, never stalls engine
//!
//! # Invariants
//!
//! 1. Frame scheduling decoupled from playback
//! 2. PlaybackScheduler is only source of timeline time
//! 3. No blocking calls in scheduler loop
//! 4. Renderer never blocks engine
//! 5. No unsafe code

pub mod frame_cache;
pub mod frame_clock;
pub mod frame_queue;
pub mod frame_scheduler;
pub mod render_command;
pub mod render_orchestrator;

// Re-exports for convenience
pub use frame_cache::{CacheKey, CacheStats, CachedFrame, FrameCache};
pub use frame_clock::{FrameClock, FrameId};
pub use frame_queue::{FrameQueue, FrameQueueConfig, QueueStats};
pub use frame_scheduler::{FrameScheduler, FrameSchedulerConfig, SchedulerStats};
pub use render_command::{ClipRenderInfo, RenderCommand, RenderPriority, RenderResult};
pub use render_orchestrator::{OrchestratorConfig, OrchestratorStats, RenderOrchestrator};
