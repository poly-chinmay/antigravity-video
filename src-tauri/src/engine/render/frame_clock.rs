//! FrameClock - Frame timing and vsync coordination.
//!
//! # Design
//!
//! FrameClock provides deterministic frame timing independent of wall-clock.
//! It tracks frame numbers and calculates the expected timeline position
//! for each frame based on target FPS.
//!
//! # Determinism
//!
//! For testing, frame advancement can be done manually without real time.

use crate::engine::media_time::MediaTime;

// =============================================================================
// FRAME ID
// =============================================================================

/// Unique identifier for a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameId(pub u64);

impl FrameId {
    /// Create a new frame ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the next frame ID.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Get the raw value.
    pub fn value(self) -> u64 {
        self.0
    }
}

impl Default for FrameId {
    fn default() -> Self {
        Self(0)
    }
}

impl std::fmt::Display for FrameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({})", self.0)
    }
}

// =============================================================================
// FRAME CLOCK
// =============================================================================

/// Frame timing controller.
///
/// # Usage
///
/// ```ignore
/// let mut clock = FrameClock::new(60.0);  // 60 FPS
///
/// loop {
///     let frame = clock.next_frame();
///     render(frame);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct FrameClock {
    /// Target frames per second
    target_fps: f64,

    /// Frame interval in nanoseconds
    frame_interval_ns: i64,

    /// Current frame number
    current_frame: FrameId,

    /// Total frames produced
    total_frames: u64,

    /// Dropped frames (when behind)
    dropped_frames: u64,
}

impl FrameClock {
    /// Create a new frame clock with target FPS.
    pub fn new(target_fps: f64) -> Self {
        assert!(target_fps > 0.0, "Target FPS must be positive");

        let frame_interval_ns = (1_000_000_000.0 / target_fps) as i64;

        Self {
            target_fps,
            frame_interval_ns,
            current_frame: FrameId::default(),
            total_frames: 0,
            dropped_frames: 0,
        }
    }

    /// Create a clock for 60 FPS.
    pub fn at_60fps() -> Self {
        Self::new(60.0)
    }

    /// Create a clock for 30 FPS.
    pub fn at_30fps() -> Self {
        Self::new(30.0)
    }

    /// Get target FPS.
    pub fn target_fps(&self) -> f64 {
        self.target_fps
    }

    /// Get frame interval as MediaTime.
    pub fn frame_interval(&self) -> MediaTime {
        MediaTime::from_nanos(self.frame_interval_ns)
    }

    /// Get current frame ID.
    pub fn current_frame(&self) -> FrameId {
        self.current_frame
    }

    /// Advance to next frame.
    pub fn advance(&mut self) -> FrameId {
        self.current_frame = self.current_frame.next();
        self.total_frames += 1;
        self.current_frame
    }

    /// Reset to frame 0.
    pub fn reset(&mut self) {
        self.current_frame = FrameId::default();
        self.total_frames = 0;
        self.dropped_frames = 0;
    }

    /// Calculate timeline position for a frame.
    ///
    /// Given a base position and frame offset, returns the expected
    /// timeline position.
    pub fn frame_to_time(&self, base: MediaTime, frame_offset: u64) -> MediaTime {
        let offset_ns = self.frame_interval_ns * frame_offset as i64;
        base + MediaTime::from_nanos(offset_ns)
    }

    /// Calculate which frame a timeline position falls into.
    pub fn time_to_frame(&self, base: MediaTime, position: MediaTime) -> u64 {
        if position <= base {
            return 0;
        }
        let delta_ns = (position - base).as_nanos();
        (delta_ns / self.frame_interval_ns) as u64
    }

    /// Record a dropped frame.
    pub fn record_drop(&mut self) {
        self.dropped_frames += 1;
    }

    /// Get total frames produced.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get dropped frame count.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Get drop rate (0.0 - 1.0).
    pub fn drop_rate(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            self.dropped_frames as f64 / self.total_frames as f64
        }
    }

    /// Set new target FPS.
    pub fn set_target_fps(&mut self, fps: f64) {
        assert!(fps > 0.0, "Target FPS must be positive");
        self.target_fps = fps;
        self.frame_interval_ns = (1_000_000_000.0 / fps) as i64;
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::at_60fps()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    #[test]
    fn test_frame_clock_new() {
        let clock = FrameClock::new(60.0);

        assert_eq!(clock.target_fps(), 60.0);
        assert_eq!(clock.current_frame(), FrameId(0));
        assert_eq!(clock.total_frames(), 0);
    }

    #[test]
    fn test_frame_interval_60fps() {
        let clock = FrameClock::at_60fps();

        // 60 FPS = ~16.67ms per frame
        let interval = clock.frame_interval();
        assert!(interval.as_nanos() > 16_000_000);
        assert!(interval.as_nanos() < 17_000_000);
    }

    #[test]
    fn test_frame_advance() {
        let mut clock = FrameClock::new(60.0);

        assert_eq!(clock.current_frame(), FrameId(0));

        let f1 = clock.advance();
        assert_eq!(f1, FrameId(1));
        assert_eq!(clock.total_frames(), 1);

        let f2 = clock.advance();
        assert_eq!(f2, FrameId(2));
        assert_eq!(clock.total_frames(), 2);
    }

    #[test]
    fn test_frame_to_time() {
        let clock = FrameClock::new(30.0); // 30 FPS = 33.33ms per frame

        let base = ms(1000);

        let t0 = clock.frame_to_time(base, 0);
        assert_eq!(t0, ms(1000));

        let t30 = clock.frame_to_time(base, 30);
        // 30 frames at 30fps ≈ 1 second (allow 10ns truncation error)
        let expected = ms(2000).as_nanos();
        let actual = t30.as_nanos();
        assert!(
            (actual - expected).abs() < 100,
            "Expected ~{}, got {}",
            expected,
            actual
        );
    }

    #[test]
    fn test_time_to_frame() {
        let clock = FrameClock::new(60.0);

        let base = ms(0);

        // At exactly base, frame 0
        assert_eq!(clock.time_to_frame(base, ms(0)), 0);

        // At ~16.67ms, still frame 0
        assert_eq!(clock.time_to_frame(base, ms(16)), 0);

        // At ~17ms, frame 1
        assert_eq!(clock.time_to_frame(base, ms(17)), 1);

        // At 1 second, frame 60
        assert_eq!(clock.time_to_frame(base, ms(1000)), 60);
    }

    #[test]
    fn test_drop_rate() {
        let mut clock = FrameClock::new(60.0);

        for _ in 0..100 {
            clock.advance();
        }

        for _ in 0..10 {
            clock.record_drop();
        }

        assert_eq!(clock.dropped_frames(), 10);
        assert!((clock.drop_rate() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_stable_frame_rate_60fps() {
        let clock = FrameClock::at_60fps();

        // Over 1 second, we should have exactly 60 frames
        let frames_per_second = 1_000_000_000 / clock.frame_interval().as_nanos();
        assert_eq!(frames_per_second, 60);
    }
}
