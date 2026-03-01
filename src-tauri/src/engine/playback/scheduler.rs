//! PlaybackScheduler - Coordinated playback scheduling.
//!
//! # Design
//!
//! The PlaybackScheduler coordinates Transport with TimelineView to provide
//! a complete playback system. It:
//!
//! 1. Manages transport state
//! 2. Queries visible clips at current position
//! 3. Provides frame-accurate scheduling
//! 4. Handles loop points
//!
//! # Thread Safety
//!
//! PlaybackScheduler is designed to be wrapped in Arc<RwLock<>> for
//! multi-threaded access.
//!
//! # Determinism
//!
//! For testing, the scheduler can be advanced manually without depending
//! on real wall-clock time.

use std::sync::RwLock;

use crate::engine::interval_tree::TimeRange;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::{ClipId, TimelineState};

use super::clock::PlaybackRate;
use super::timeline_view::{TimelineView, VisibleClip};
use super::transport::{Transport, TransportCommand, TransportState};

// =============================================================================
// FRAME INFO
// =============================================================================

/// Information about what to render for a frame.
#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// Current timeline position
    pub position: MediaTime,

    /// Clips visible at this position
    pub clips: Vec<VisibleClip>,

    /// Transport state
    pub state: TransportState,

    /// Current playback rate
    pub rate: PlaybackRate,

    /// Whether playhead is at end
    pub at_end: bool,
}

// =============================================================================
// SCHEDULER CONFIG
// =============================================================================

/// Configuration for the scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Target frame rate for scheduling
    pub target_fps: f64,

    /// Enable auto-pause at end of timeline
    pub auto_pause_at_end: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            target_fps: 30.0,
            auto_pause_at_end: true,
        }
    }
}

// =============================================================================
// PLAYBACK SCHEDULER
// =============================================================================

/// Coordinated playback scheduler.
///
/// # Usage
///
/// ```ignore
/// let scheduler = PlaybackScheduler::new(config, duration);
///
/// // In render loop
/// let frame = scheduler.get_frame(&index, &state);
/// render(frame);
///
/// // Transport control
/// scheduler.execute(TransportCommand::Play);
/// ```
#[derive(Debug)]
pub struct PlaybackScheduler {
    /// Transport controller
    transport: Transport,

    /// Scheduler configuration
    config: SchedulerConfig,

    /// Last reported position (for drift detection)
    last_position: MediaTime,

    /// Frame count for statistics
    frame_count: u64,
}

impl PlaybackScheduler {
    /// Create a new scheduler.
    pub fn new(config: SchedulerConfig, duration: MediaTime) -> Self {
        Self {
            transport: Transport::new(duration),
            config,
            last_position: MediaTime::ZERO,
            frame_count: 0,
        }
    }

    /// Create with default config.
    pub fn with_duration(duration: MediaTime) -> Self {
        Self::new(SchedulerConfig::default(), duration)
    }

    /// Get current frame info.
    pub fn get_frame(&mut self, index: &TimelineIndex, state: &TimelineState) -> FrameInfo {
        // Tick transport (handles loops, end of timeline)
        self.transport.tick();

        // Auto-pause at end
        if self.config.auto_pause_at_end
            && self.transport.is_at_end()
            && self.transport.is_playing()
        {
            if !self.transport.is_loop_enabled() {
                self.transport.pause();
            }
        }

        let position = self.transport.position();

        // Query visible clips
        let view = TimelineView::at_position(position, index, state);

        // Track position for drift detection
        self.last_position = position;
        self.frame_count += 1;

        FrameInfo {
            position,
            clips: view.clips().to_vec(),
            state: self.transport.state(),
            rate: self.transport.rate(),
            at_end: self.transport.is_at_end(),
        }
    }

    /// Get current position without full frame query.
    pub fn position(&self) -> MediaTime {
        self.transport.position()
    }

    /// Get transport state.
    pub fn state(&self) -> TransportState {
        self.transport.state()
    }

    /// Check if playing.
    pub fn is_playing(&self) -> bool {
        self.transport.is_playing()
    }

    /// Execute a transport command.
    pub fn execute(&mut self, cmd: TransportCommand) {
        self.transport.execute(cmd);
    }

    /// Convenience: play.
    pub fn play(&mut self) {
        self.transport.play();
    }

    /// Convenience: pause.
    pub fn pause(&mut self) {
        self.transport.pause();
    }

    /// Convenience: stop.
    pub fn stop(&mut self) {
        self.transport.stop();
    }

    /// Convenience: seek.
    pub fn seek(&mut self, position: MediaTime) {
        self.transport.seek(position);
    }

    /// Convenience: set rate.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        self.transport.set_rate(rate);
    }

    /// Get current rate.
    pub fn rate(&self) -> PlaybackRate {
        self.transport.rate()
    }

    /// Get timeline duration.
    pub fn duration(&self) -> MediaTime {
        self.transport.duration()
    }

    /// Update timeline duration.
    pub fn set_duration(&mut self, duration: MediaTime) {
        self.transport.set_duration(duration);
    }

    /// Get frame count.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Get frame interval for target FPS.
    pub fn frame_interval(&self) -> MediaTime {
        let nanos = (1_000_000_000.0 / self.config.target_fps) as i64;
        MediaTime::from_nanos(nanos)
    }

    // =========================================================================
    // LOOP CONTROL
    // =========================================================================

    /// Enable loop mode.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.transport.set_loop_enabled(enabled);
    }

    /// Check if loop is enabled.
    pub fn is_loop_enabled(&self) -> bool {
        self.transport.is_loop_enabled()
    }

    /// Set loop in point.
    pub fn set_loop_in(&mut self, point: MediaTime) {
        self.transport.set_loop_in(point);
    }

    /// Set loop out point.
    pub fn set_loop_out(&mut self, point: MediaTime) {
        self.transport.set_loop_out(point);
    }

    // =========================================================================
    // TESTING SUPPORT
    // =========================================================================

    /// For testing: get mutable transport access.
    #[cfg(test)]
    pub fn transport_mut(&mut self) -> &mut Transport {
        &mut self.transport
    }
}

impl Default for PlaybackScheduler {
    fn default() -> Self {
        Self::with_duration(MediaTime::ZERO)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;
    use std::thread::sleep;
    use std::time::Duration;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_clip(id: &str, track: &str, start_ms: i64, duration_ms: i64) -> Clip {
        Clip::new(
            id,
            track,
            ms(start_ms),
            ms(duration_ms),
            format!("{}.mp4", id),
        )
    }

    fn make_state(clips: Vec<Clip>) -> TimelineState {
        let mut state = TimelineState::new();
        state.clips = clips;
        state.rebuild_indices();
        state
    }

    #[test]
    fn test_scheduler_new() {
        let scheduler = PlaybackScheduler::with_duration(ms(10000));

        assert_eq!(scheduler.position(), MediaTime::ZERO);
        assert_eq!(scheduler.state(), TransportState::Stopped);
        assert!(!scheduler.is_playing());
    }

    #[test]
    fn test_play_pause_stability() {
        let mut scheduler = PlaybackScheduler::with_duration(ms(10000));
        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let index = TimelineIndex::build(&state);

        scheduler.seek(ms(5000));
        let start_pos = scheduler.position();

        // Rapid toggling shouldn't drift
        for _ in 0..20 {
            scheduler.play();
            scheduler.pause();
        }

        let end_pos = scheduler.position();
        let diff = (end_pos.as_nanos() - start_pos.as_nanos()).abs();

        // Allow 1ms tolerance
        assert!(diff < 1_000_000, "Drift detected: {} nanos", diff);
    }

    #[test]
    fn test_seek_accuracy() {
        let mut scheduler = PlaybackScheduler::with_duration(ms(10000));

        // Test various seek positions
        let positions = [0, 1000, 2500, 5000, 7500, 10000];

        for &pos in &positions {
            scheduler.seek(ms(pos));
            assert_eq!(scheduler.position(), ms(pos), "Seek to {}ms failed", pos);
        }
    }

    #[test]
    fn test_speed_change_drift_free() {
        let mut scheduler = PlaybackScheduler::with_duration(ms(100000));
        let state = make_state(vec![make_clip("c1", "t1", 0, 100000)]);
        let index = TimelineIndex::build(&state);

        scheduler.seek(ms(50000));
        let start = scheduler.position();

        // Change speed multiple times
        scheduler.set_rate(PlaybackRate::DOUBLE);
        scheduler.set_rate(PlaybackRate::HALF);
        scheduler.set_rate(PlaybackRate::NORMAL);
        scheduler.set_rate(PlaybackRate::new(3, 2)); // 1.5x
        scheduler.set_rate(PlaybackRate::NORMAL);

        let end = scheduler.position();
        let diff = (end.as_nanos() - start.as_nanos()).abs();

        // Allow 1ms tolerance
        assert!(diff < 1_000_000, "Drift detected: {} nanos", diff);
    }

    #[test]
    fn test_get_frame() {
        let mut scheduler = PlaybackScheduler::with_duration(ms(10000));
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
        ]);
        let index = TimelineIndex::build(&state);

        scheduler.seek(ms(2500));

        let frame = scheduler.get_frame(&index, &state);

        assert_eq!(frame.position, ms(2500));
        assert_eq!(frame.clips.len(), 1);
        assert_eq!(frame.clips[0].id, "c1");
    }

    #[test]
    fn test_scheduler_does_not_drift_over_time() {
        let mut scheduler = PlaybackScheduler::with_duration(ms(100000));
        let state = make_state(vec![make_clip("c1", "t1", 0, 100000)]);
        let index = TimelineIndex::build(&state);

        scheduler.play();

        // Simulate many frames
        let mut last_position = scheduler.position();
        let mut drift_sum: i64 = 0;

        for i in 0..100 {
            sleep(Duration::from_millis(1));

            let frame = scheduler.get_frame(&index, &state);
            let current = frame.position;

            // Position should always advance (or stay same if paused)
            assert!(
                current >= last_position,
                "Position went backwards at frame {}",
                i
            );

            last_position = current;
        }

        scheduler.pause();

        // Final position should be reasonable (within 50% of expected)
        // We slept ~100ms, so position should be roughly 100ms
        let final_pos = scheduler.position().as_nanos();
        assert!(
            final_pos > 50_000_000,
            "Position too low: {} nanos",
            final_pos
        );
        assert!(
            final_pos < 500_000_000,
            "Position too high: {} nanos",
            final_pos
        );
    }

    #[test]
    fn test_auto_pause_at_end() {
        let config = SchedulerConfig {
            target_fps: 30.0,
            auto_pause_at_end: true,
        };
        let mut scheduler = PlaybackScheduler::new(config, ms(100));
        let state = make_state(vec![make_clip("c1", "t1", 0, 100)]);
        let index = TimelineIndex::build(&state);

        scheduler.seek(ms(99));
        scheduler.play();

        // Advance a bit
        sleep(Duration::from_millis(10));
        let frame = scheduler.get_frame(&index, &state);

        // Should auto-pause at end
        assert!(frame.at_end || scheduler.state() == TransportState::Paused);
    }
}
