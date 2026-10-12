//! Playhead - Current timeline position.
//!
//! # Design
//!
//! The Playhead represents the current position on the timeline in MediaTime.
//! It combines the Clock's wall-clock elapsed time with a base offset to
//! produce the current timeline position.
//!
//! # Invariants
//!
//! - Position is always >= 0 (clamped)
//! - Position is always <= timeline duration (clamped)

use crate::engine::media_time::MediaTime;

use super::clock::Clock;

/// Current playhead position on the timeline.
#[derive(Debug)]
pub struct Playhead {
    /// Clock for measuring elapsed time
    clock: Clock,

    /// Starting position offset on timeline
    offset: MediaTime,

    /// Maximum allowed position (timeline duration)
    duration: MediaTime,
}

impl Playhead {
    /// Create a new playhead at position 0.
    pub fn new() -> Self {
        Self {
            clock: Clock::new(),
            offset: MediaTime::ZERO,
            duration: MediaTime::ZERO,
        }
    }

    /// Create a playhead with a known timeline duration.
    pub fn with_duration(duration: MediaTime) -> Self {
        Self {
            clock: Clock::new(),
            offset: MediaTime::ZERO,
            duration,
        }
    }

    /// Get current position on timeline.
    pub fn position(&self) -> MediaTime {
        let elapsed = self.clock.elapsed_media_time();
        let raw = self.offset + elapsed;

        // Clamp to valid range
        if raw.is_negative() {
            MediaTime::ZERO
        } else if raw > self.duration && !self.duration.is_zero() {
            self.duration
        } else {
            raw
        }
    }

    /// Check if playhead is at the end of timeline.
    pub fn is_at_end(&self) -> bool {
        !self.duration.is_zero() && self.position() >= self.duration
    }

    /// Check if playing.
    pub fn is_playing(&self) -> bool {
        self.clock.is_running()
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.clock.start();
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        // Capture current position as new offset
        self.offset = self.position();
        self.clock.stop();
        self.clock.reset();
    }

    /// Seek to specific position.
    pub fn seek(&mut self, position: MediaTime) {
        let was_playing = self.is_playing();

        // Stop clock to reset it
        self.clock.stop();
        self.clock.reset();

        // Set new offset (clamped)
        self.offset = if position.is_negative() {
            MediaTime::ZERO
        } else if position > self.duration && !self.duration.is_zero() {
            self.duration
        } else {
            position
        };

        // Resume if was playing
        if was_playing {
            self.clock.start();
        }
    }

    /// Seek by relative amount.
    pub fn seek_relative(&mut self, delta: MediaTime) {
        let current = self.position();
        self.seek(current + delta);
    }

    /// Update timeline duration.
    pub fn set_duration(&mut self, duration: MediaTime) {
        self.duration = duration;

        // Clamp position if beyond new duration
        if self.position() > duration {
            self.seek(duration);
        }
    }

    /// Get timeline duration.
    pub fn duration(&self) -> MediaTime {
        self.duration
    }

    /// Get mutable access to clock (for rate changes).
    pub fn clock_mut(&mut self) -> &mut Clock {
        &mut self.clock
    }

    /// Get reference to clock.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }
}

impl Default for Playhead {
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
    use std::time::Duration;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    #[test]
    fn test_playhead_new() {
        let playhead = Playhead::new();
        assert_eq!(playhead.position(), MediaTime::ZERO);
        assert!(!playhead.is_playing());
    }

    #[test]
    fn test_playhead_play_pause() {
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.play();
        assert!(playhead.is_playing());

        sleep(Duration::from_millis(10));

        playhead.pause();
        assert!(!playhead.is_playing());

        let pos = playhead.position();
        assert!(pos > MediaTime::ZERO);
    }

    #[test]
    fn test_playhead_seek() {
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.seek(ms(5000));

        assert_eq!(playhead.position(), ms(5000));
    }

    #[test]
    fn test_playhead_seek_clamp_negative() {
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.seek(ms(-1000));

        assert_eq!(playhead.position(), MediaTime::ZERO);
    }

    #[test]
    fn test_playhead_seek_clamp_overflow() {
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.seek(ms(20000));

        assert_eq!(playhead.position(), ms(10000));
    }

    #[test]
    fn test_playhead_seek_relative() {
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.seek(ms(3000));
        playhead.seek_relative(ms(2000));

        assert_eq!(playhead.position(), ms(5000));
    }

    #[test]
    fn test_playhead_is_at_end() {
        let mut playhead = Playhead::with_duration(ms(10000));

        assert!(!playhead.is_at_end());

        playhead.seek(ms(10000));

        assert!(playhead.is_at_end());
    }

    #[test]
    fn test_play_pause_stability() {
        // Test that rapid play/pause doesn't drift
        let mut playhead = Playhead::with_duration(ms(10000));

        playhead.seek(ms(5000));
        let start_pos = playhead.position();

        // Rapid toggling
        for _ in 0..10 {
            playhead.play();
            playhead.pause();
        }

        let end_pos = playhead.position();

        // Should be at same position (or within microsecond tolerance)
        let diff = if end_pos > start_pos {
            end_pos - start_pos
        } else {
            start_pos - end_pos
        };

        // Allow 1ms tolerance
        assert!(
            diff.as_nanos().abs() < 1_000_000,
            "Drift detected: {:?}",
            diff
        );
    }
}
