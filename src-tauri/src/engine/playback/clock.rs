//! Clock - Wall-clock time abstraction for playback.
//!
//! # Design
//!
//! This module provides a clean separation between:
//! - Wall-clock time (Instant) - for measuring real elapsed time
//! - Timeline time (MediaTime) - for timeline position
//!
//! The Clock is the ONLY place where Instant is used. All timeline positions
//! use MediaTime exclusively.
//!
//! # Thread Safety
//!
//! Clock is designed to be used from a single thread. For multi-threaded
//! access, wrap in Arc<RwLock<Clock>>.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;

// =============================================================================
// PLAYBACK RATE
// =============================================================================

/// Playback speed multiplier.
///
/// Stored as a fraction for precision.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaybackRate {
    /// Numerator of rate fraction (e.g., 1 for 1x, 2 for 2x)
    numerator: u32,
    /// Denominator of rate fraction (e.g., 2 for 0.5x)
    denominator: u32,
}

impl PlaybackRate {
    /// Normal playback speed (1x).
    pub const NORMAL: PlaybackRate = PlaybackRate {
        numerator: 1,
        denominator: 1,
    };

    /// Half speed (0.5x).
    pub const HALF: PlaybackRate = PlaybackRate {
        numerator: 1,
        denominator: 2,
    };

    /// Double speed (2x).
    pub const DOUBLE: PlaybackRate = PlaybackRate {
        numerator: 2,
        denominator: 1,
    };

    /// Create a new playback rate.
    ///
    /// # Panics
    /// Panics if numerator or denominator is zero.
    pub fn new(numerator: u32, denominator: u32) -> Self {
        assert!(numerator > 0, "PlaybackRate numerator must be positive");
        assert!(denominator > 0, "PlaybackRate denominator must be positive");
        Self {
            numerator,
            denominator,
        }
    }

    /// Create from float (approximate).
    pub fn from_f64(rate: f64) -> Self {
        assert!(rate > 0.0, "PlaybackRate must be positive");
        // Use simple approximation with denominator 1000
        let numerator = (rate * 1000.0).round() as u32;
        Self::new(numerator.max(1), 1000)
    }

    /// Convert to float.
    pub fn to_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    /// Scale a duration by this rate.
    pub fn scale_duration(self, duration: Duration) -> Duration {
        let nanos = duration.as_nanos() as u64;
        let scaled = (nanos as u128 * self.numerator as u128) / self.denominator as u128;
        Duration::from_nanos(scaled as u64)
    }

    /// Scale MediaTime by this rate.
    pub fn scale_media_time(self, time: MediaTime) -> MediaTime {
        let nanos = time.as_nanos();
        let scaled = (nanos as i128 * self.numerator as i128) / self.denominator as i128;
        MediaTime::from_nanos(scaled as i64)
    }
}

impl Default for PlaybackRate {
    fn default() -> Self {
        Self::NORMAL
    }
}

// =============================================================================
// CLOCK
// =============================================================================

/// High-precision wall-clock for playback timing.
///
/// # Usage
///
/// ```ignore
/// let mut clock = Clock::new();
/// clock.start();
///
/// // Later...
/// let elapsed = clock.elapsed();
/// ```
#[derive(Debug)]
pub struct Clock {
    /// The instant when playback started (or was last resumed)
    start_instant: Option<Instant>,

    /// Accumulated time from previous play segments
    accumulated: Duration,

    /// Current playback rate
    rate: PlaybackRate,

    /// Whether the clock is currently running
    running: bool,
}

impl Clock {
    /// Create a new stopped clock.
    pub fn new() -> Self {
        Self {
            start_instant: None,
            accumulated: Duration::ZERO,
            rate: PlaybackRate::NORMAL,
            running: false,
        }
    }

    /// Check if clock is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get current playback rate.
    pub fn rate(&self) -> PlaybackRate {
        self.rate
    }

    /// Set playback rate.
    ///
    /// If running, accumulates current elapsed time first.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        if self.running {
            // Accumulate current segment before changing rate
            self.accumulated += self.current_segment_elapsed();
            self.start_instant = Some(Instant::now());
        }
        self.rate = rate;
    }

    /// Start or resume the clock.
    pub fn start(&mut self) {
        if !self.running {
            self.start_instant = Some(Instant::now());
            self.running = true;
        }
    }

    /// Stop the clock, preserving accumulated time.
    pub fn stop(&mut self) {
        if self.running {
            self.accumulated += self.current_segment_elapsed();
            self.start_instant = None;
            self.running = false;
        }
    }

    /// Reset the clock to zero.
    pub fn reset(&mut self) {
        self.start_instant = None;
        self.accumulated = Duration::ZERO;
        self.running = false;
    }

    /// Get total elapsed time (rate-adjusted).
    pub fn elapsed(&self) -> Duration {
        let current = if self.running {
            self.current_segment_elapsed()
        } else {
            Duration::ZERO
        };
        self.accumulated + current
    }

    /// Get elapsed time as MediaTime.
    pub fn elapsed_media_time(&self) -> MediaTime {
        MediaTime::from_nanos(self.elapsed().as_nanos() as i64)
    }

    /// Set accumulated time (for seeking).
    pub fn set_accumulated(&mut self, time: Duration) {
        self.accumulated = time;
        if self.running {
            self.start_instant = Some(Instant::now());
        }
    }

    /// Set accumulated time from MediaTime.
    pub fn set_accumulated_media_time(&mut self, time: MediaTime) {
        self.set_accumulated(Duration::from_nanos(time.as_nanos() as u64));
    }

    /// Calculate elapsed time for current running segment with rate applied.
    fn current_segment_elapsed(&self) -> Duration {
        self.start_instant
            .map(|start| self.rate.scale_duration(start.elapsed()))
            .unwrap_or(Duration::ZERO)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_clock_new() {
        let clock = Clock::new();
        assert!(!clock.is_running());
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    #[test]
    fn test_clock_start_stop() {
        let mut clock = Clock::new();

        clock.start();
        assert!(clock.is_running());

        sleep(Duration::from_millis(10));

        clock.stop();
        assert!(!clock.is_running());

        let elapsed = clock.elapsed();
        assert!(elapsed >= Duration::from_millis(9)); // Allow some tolerance
    }

    #[test]
    fn test_clock_accumulates() {
        let mut clock = Clock::new();

        // First segment
        clock.start();
        sleep(Duration::from_millis(10));
        clock.stop();

        let first = clock.elapsed();

        // Second segment
        clock.start();
        sleep(Duration::from_millis(10));
        clock.stop();

        let total = clock.elapsed();

        assert!(total > first);
        assert!(total >= Duration::from_millis(18)); // Allow tolerance
    }

    #[test]
    fn test_clock_reset() {
        let mut clock = Clock::new();

        clock.start();
        sleep(Duration::from_millis(10));
        clock.stop();

        clock.reset();

        assert!(!clock.is_running());
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    #[test]
    fn test_playback_rate() {
        let rate = PlaybackRate::new(2, 1);
        assert_eq!(rate.to_f64(), 2.0);

        let duration = Duration::from_secs(1);
        let scaled = rate.scale_duration(duration);
        assert_eq!(scaled, Duration::from_secs(2));
    }

    #[test]
    fn test_playback_rate_half() {
        let rate = PlaybackRate::HALF;

        let duration = Duration::from_secs(2);
        let scaled = rate.scale_duration(duration);
        assert_eq!(scaled, Duration::from_secs(1));
    }

    #[test]
    fn test_clock_with_rate() {
        let mut clock = Clock::new();
        clock.set_rate(PlaybackRate::DOUBLE);

        clock.start();
        sleep(Duration::from_millis(10));
        clock.stop();

        let elapsed = clock.elapsed();
        // At 2x speed, 10ms wall time = ~20ms playback time
        // Allow wide tolerance due to sleep imprecision
        assert!(elapsed >= Duration::from_millis(15));
    }

    #[test]
    fn test_set_accumulated() {
        let mut clock = Clock::new();

        clock.set_accumulated(Duration::from_secs(5));

        assert_eq!(clock.elapsed(), Duration::from_secs(5));
        assert!(!clock.is_running());
    }
}
