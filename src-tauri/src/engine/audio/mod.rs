//! Audio module - Audio clock and A/V synchronization.
//!
//! # Architecture
//!
//! This module provides professional-grade audio synchronization:
//!
//! - `audio_clock` - Master audio clock (samples-based timing)
//! - `audio_device` - Mockable audio device interface
//! - `audio_buffer` - Audio sample buffer management
//! - `audio_scheduler` - Audio chunk scheduling
//! - `av_sync` - A/V synchronization controller
//!
//! # Sync Model
//!
//! Audio is the master clock. Video timing follows audio:
//!
//! ```text
//! AudioClock ──────────────────────────────────────────▶ Time
//!     │
//!     │ samples_played
//!     ▼
//! AVSync ──────────────────────────────────────────────▶ video_target_time
//!     │
//!     │ should_skip / should_wait
//!     ▼
//! FrameScheduler ──────────────────────────────────────▶ frames
//! ```
//!
//! # Threading Model
//!
//! - AudioClock: Single-threaded, wrap in Arc<RwLock>
//! - AudioScheduler: Single-threaded producer
//! - AVSync: Single-threaded coordinator
//!
//! # Guarantees
//!
//! 1. Audio is master clock
//! 2. Video stays in sync within 1 frame
//! 3. No drift over long playback
//! 4. Seek/pause/resume preserve sync
//! 5. No unsafe code

pub mod audio_buffer;
pub mod audio_clock;
pub mod audio_device;
pub mod audio_scheduler;
pub mod av_sync;

// Re-exports
pub use audio_buffer::{AudioBuffer, AudioChunk, BufferStats};
pub use audio_clock::{AudioClock, AudioClockConfig, AudioClockState};
pub use audio_device::{AudioDevice, DeviceState, MockAudioDevice, NullAudioDevice};
pub use audio_scheduler::{AudioScheduler, AudioSchedulerConfig};
pub use av_sync::{AVPair, AVSync, SyncConfig, SyncStats, SyncStatus};
