//! FrameScheduler - Frame production and scheduling.
//!
//! # Design
//!
//! FrameScheduler is responsible for producing RenderCommands based on
//! the current playback position. It queries PlaybackScheduler for time
//! and TimelineIndex for visible clips.
//!
//! # Separation
//!
//! Frame scheduling is decoupled from playback:
//! - PlaybackScheduler owns timeline time
//! - FrameScheduler owns frame production rate
//!
//! # Thread Safety
//!
//! FrameScheduler is single-threaded. For async operation, wrap in Arc<RwLock>.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::{PlaybackScheduler, TimelineView, VisibleClip};
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::TimelineState;

use super::frame_clock::{FrameClock, FrameId};
use super::frame_queue::{FrameQueue, FrameQueueConfig};
use super::render_command::{ClipRenderInfo, RenderCommand, RenderPriority};

// =============================================================================
// SCHEDULER CONFIG
// =============================================================================

/// Configuration for the frame scheduler.
#[derive(Debug, Clone)]
pub struct FrameSchedulerConfig {
    /// Target frames per second
    pub target_fps: f64,

    /// Frame queue configuration
    pub queue_config: FrameQueueConfig,

    /// Output width
    pub width: u32,

    /// Output height
    pub height: u32,

    /// Lookahead frames (pre-render)
    pub lookahead: u32,
}

impl Default for FrameSchedulerConfig {
    fn default() -> Self {
        Self {
            target_fps: 60.0,
            queue_config: FrameQueueConfig::default(),
            width: 1920,
            height: 1080,
            lookahead: 2,
        }
    }
}

// =============================================================================
// SCHEDULER STATS
// =============================================================================

/// Frame scheduler statistics.
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    /// Total frames scheduled
    pub frames_scheduled: u64,

    /// Frames skipped (behind schedule)
    pub frames_skipped: u64,

    /// Empty frames (no clips)
    pub empty_frames: u64,

    /// Seek frames produced
    pub seek_frames: u64,
}

// =============================================================================
// FRAME SCHEDULER
// =============================================================================

/// Frame production scheduler.
///
/// # Usage
///
/// ```ignore
/// let mut scheduler = FrameScheduler::new(config);
///
/// // In render loop
/// while scheduler.queue_needs_frames() {
///     if let Some(cmd) = scheduler.produce_frame(&playback, &index, &state) {
///         // Send to renderer
///     }
/// }
/// ```
#[derive(Debug)]
pub struct FrameScheduler {
    /// Frame timing clock
    clock: FrameClock,

    /// Frame queue
    queue: FrameQueue,

    /// Configuration
    config: FrameSchedulerConfig,

    /// Statistics
    stats: SchedulerStats,

    /// Last position for drift detection
    last_position: MediaTime,

    /// Whether we're in seek mode
    seeking: bool,
}

impl FrameScheduler {
    /// Create a new frame scheduler.
    pub fn new(config: FrameSchedulerConfig) -> Self {
        let clock = FrameClock::new(config.target_fps);
        let queue = FrameQueue::new(config.queue_config.clone());

        Self {
            clock,
            queue,
            config,
            stats: SchedulerStats::default(),
            last_position: MediaTime::ZERO,
            seeking: false,
        }
    }

    /// Create with default config.
    pub fn default_60fps() -> Self {
        Self::new(FrameSchedulerConfig::default())
    }

    /// Check if queue needs more frames.
    pub fn queue_needs_frames(&self) -> bool {
        self.queue.needs_frames()
    }

    /// Check if queue is ready for playback.
    pub fn is_ready(&self) -> bool {
        self.queue.is_ready()
    }

    /// Produce a single frame.
    ///
    /// Returns the render command if a frame was produced.
    pub fn produce_frame(
        &mut self,
        playback: &PlaybackScheduler,
        index: &TimelineIndex,
        state: &TimelineState,
    ) -> Option<RenderCommand> {
        if !self.queue.needs_frames() {
            return None;
        }

        let position = playback.position();
        let frame_id = self.clock.advance();

        // Query visible clips
        let view = TimelineView::at_position(position, index, state);

        // Build clip render infos
        let clips: Vec<ClipRenderInfo> = view
            .clips()
            .iter()
            .enumerate()
            .map(|(layer, clip)| ClipRenderInfo::from_visible_clip(clip, layer as u32))
            .collect();

        // Create command
        let mut cmd = RenderCommand::new(frame_id, position, clips)
            .with_dimensions(self.config.width, self.config.height);

        // Set priority
        if self.seeking {
            cmd.priority = RenderPriority::High;
            cmd.is_keyframe = true;
            self.seeking = false;
            self.stats.seek_frames += 1;
        }

        // Track stats
        self.stats.frames_scheduled += 1;
        if cmd.is_empty() {
            self.stats.empty_frames += 1;
        }

        // Detect large position jumps (seeks)
        let position_delta = if position >= self.last_position {
            position - self.last_position
        } else {
            self.last_position - position
        };

        // If position jumped more than 2 frames, treat as seek
        let two_frames = self.clock.frame_interval() + self.clock.frame_interval();
        if position_delta > two_frames {
            cmd.priority = RenderPriority::High;
            cmd.is_keyframe = true;
        }

        self.last_position = position;

        // Queue the frame
        self.queue.push(cmd.clone());

        Some(cmd)
    }

    /// Notify scheduler of a seek.
    pub fn notify_seek(&mut self) {
        self.seeking = true;
        self.queue.clear();
    }

    /// Get next frame from queue.
    pub fn pop_frame(&mut self) -> Option<RenderCommand> {
        self.queue.pop()
    }

    /// Peek at next frame.
    pub fn peek_frame(&self) -> Option<&RenderCommand> {
        self.queue.peek()
    }

    /// Get queue depth.
    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    /// Get statistics.
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Get frame clock.
    pub fn clock(&self) -> &FrameClock {
        &self.clock
    }

    /// Get frame clock mutably.
    pub fn clock_mut(&mut self) -> &mut FrameClock {
        &mut self.clock
    }

    /// Set target FPS.
    pub fn set_target_fps(&mut self, fps: f64) {
        self.config.target_fps = fps;
        self.clock.set_target_fps(fps);
    }

    /// Reset the scheduler.
    pub fn reset(&mut self) {
        self.clock.reset();
        self.queue.clear();
        self.stats = SchedulerStats::default();
        self.last_position = MediaTime::ZERO;
        self.seeking = false;
    }

    /// Clear expired frames.
    pub fn clear_expired(&mut self, current_time_ns: u64) -> usize {
        self.queue.clear_expired(current_time_ns)
    }
}

impl Default for FrameScheduler {
    fn default() -> Self {
        Self::default_60fps()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::playback::SchedulerConfig;
    use crate::engine::timeline_state::Clip;

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
        let scheduler = FrameScheduler::default_60fps();

        assert!(scheduler.queue_needs_frames());
        assert!(!scheduler.is_ready());
    }

    #[test]
    fn test_produce_frame() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let index = TimelineIndex::build(&state);
        let mut playback = PlaybackScheduler::with_duration(ms(10000));
        playback.seek(ms(1000));

        let mut scheduler = FrameScheduler::default_60fps();

        let cmd = scheduler.produce_frame(&playback, &index, &state);

        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert_eq!(cmd.frame_id, FrameId(1));
        assert_eq!(cmd.position, ms(1000));
        assert_eq!(cmd.clips.len(), 1);
    }

    #[test]
    fn test_frame_matches_timeline_view() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
        ]);
        let index = TimelineIndex::build(&state);
        let mut playback = PlaybackScheduler::with_duration(ms(10000));

        let mut scheduler = FrameScheduler::default_60fps();

        // Test at position 2500ms (only c1 visible)
        playback.seek(ms(2500));
        let cmd = scheduler.produce_frame(&playback, &index, &state).unwrap();

        let view = TimelineView::at_position(ms(2500), &index, &state);

        assert_eq!(cmd.clips.len(), view.clips().len());
        assert_eq!(cmd.clips[0].clip_id, view.clips()[0].id);

        // Test at position 7500ms (only c2 visible)
        playback.seek(ms(7500));
        scheduler.notify_seek();
        let cmd = scheduler.produce_frame(&playback, &index, &state).unwrap();

        let view = TimelineView::at_position(ms(7500), &index, &state);

        assert_eq!(cmd.clips.len(), view.clips().len());
        assert_eq!(cmd.clips[0].clip_id, view.clips()[0].id);
    }

    #[test]
    fn test_no_frame_drift_over_time() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 100000)]);
        let index = TimelineIndex::build(&state);
        let mut playback = PlaybackScheduler::with_duration(ms(100000));

        let mut scheduler = FrameScheduler::default_60fps();

        // Produce 60 frames (1 second at 60fps)
        let mut positions = Vec::new();
        for i in 0..60 {
            playback.seek(ms(i * 17)); // ~17ms per frame

            // Pop frames to make room
            while scheduler.queue_depth() >= 3 {
                scheduler.pop_frame();
            }

            if let Some(cmd) = scheduler.produce_frame(&playback, &index, &state) {
                positions.push(cmd.position);
            }
        }

        assert_eq!(positions.len(), 60, "Should produce 60 frames");

        // Verify positions are strictly increasing
        for i in 1..positions.len() {
            assert!(
                positions[i] >= positions[i - 1],
                "Position decreased at frame {}: {:?} < {:?}",
                i,
                positions[i],
                positions[i - 1]
            );
        }

        // Verify total span is approximately 1 second
        let total_span =
            positions.last().unwrap().as_nanos() - positions.first().unwrap().as_nanos();
        let expected_span = 59 * 17 * 1_000_000; // 59 * 17ms in nanos

        // Allow 10% tolerance
        assert!(
            (total_span - expected_span).abs() < expected_span / 10,
            "Drift detected: span {} vs expected {}",
            total_span,
            expected_span
        );
    }
}
