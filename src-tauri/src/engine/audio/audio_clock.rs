//! AudioClock - Master audio clock for A/V synchronization.
//!
//! # Design
//!
//! AudioClock is the authoritative time source for playback. Video timing
//! follows audio timing, not the other way around. This ensures lip-sync
//! accuracy and prevents perceptible A/V drift.
//!
//! The clock tracks:
//! - Samples played (converted to time via sample rate)
//! - Accumulated time from previous segments
//! - Playback rate for variable speed
//!
//! # Precision
//!
//! Audio timing is sample-accurate. At 48kHz, this is ~21µs resolution.
//!
//! # Thread Safety
//!
//! AudioClock is designed for single-threaded use. Wrap in Arc<RwLock> for
//! multi-threaded access.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::PlaybackRate;

// =============================================================================
// AUDIO CLOCK CONFIG
// =============================================================================

/// Configuration for the audio clock.
#[derive(Debug, Clone, Copy)]
pub struct AudioClockConfig {
    /// Sample rate in Hz (e.g., 48000)
    pub sample_rate: u32,

    /// Channels (e.g., 2 for stereo)
    pub channels: u32,

    /// Bits per sample (e.g., 16, 24, 32)
    pub bits_per_sample: u32,
}

impl AudioClockConfig {
    /// CD quality: 44.1kHz stereo 16-bit
    pub const CD_QUALITY: Self = Self {
        sample_rate: 44100,
        channels: 2,
        bits_per_sample: 16,
    };

    /// Professional: 48kHz stereo 24-bit
    pub const PROFESSIONAL: Self = Self {
        sample_rate: 48000,
        channels: 2,
        bits_per_sample: 24,
    };

    /// Samples to nanoseconds.
    pub fn samples_to_nanos(&self, samples: u64) -> i64 {
        // nanos = samples * 1_000_000_000 / sample_rate
        (samples as i128 * 1_000_000_000 / self.sample_rate as i128) as i64
    }

    /// Nanoseconds to samples.
    pub fn nanos_to_samples(&self, nanos: i64) -> u64 {
        // samples = nanos * sample_rate / 1_000_000_000
        ((nanos as i128 * self.sample_rate as i128) / 1_000_000_000) as u64
    }

    /// MediaTime to samples.
    pub fn time_to_samples(&self, time: MediaTime) -> u64 {
        self.nanos_to_samples(time.as_nanos())
    }

    /// Samples to MediaTime.
    pub fn samples_to_time(&self, samples: u64) -> MediaTime {
        MediaTime::from_nanos(self.samples_to_nanos(samples))
    }

    /// Bytes per sample (all channels).
    pub fn bytes_per_frame(&self) -> usize {
        (self.channels * self.bits_per_sample / 8) as usize
    }

    /// Bytes for duration.
    pub fn bytes_for_duration(&self, duration: MediaTime) -> usize {
        let samples = self.time_to_samples(duration);
        samples as usize * self.bytes_per_frame()
    }
}

impl Default for AudioClockConfig {
    fn default() -> Self {
        Self::PROFESSIONAL
    }
}

// =============================================================================
// AUDIO CLOCK STATE
// =============================================================================

/// Internal state of the audio clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioClockState {
    /// Clock is stopped
    Stopped,
    /// Clock is running
    Running,
    /// Clock is paused
    Paused,
}

// =============================================================================
// AUDIO CLOCK
// =============================================================================

/// Master audio clock for A/V synchronization.
///
/// # Usage
///
/// ```ignore
/// let mut clock = AudioClock::new(AudioClockConfig::PROFESSIONAL);
///
/// // Start playback
/// clock.start();
///
/// // Audio callback: advance by samples played
/// clock.advance_samples(buffer_size);
///
/// // Get current time
/// let time = clock.current_time();
/// ```
#[derive(Debug)]
pub struct AudioClock {
    /// Configuration
    config: AudioClockConfig,

    /// Current state
    state: AudioClockState,

    /// Base timeline position (from seeks)
    base_position: MediaTime,

    /// Samples played since last seek
    samples_since_base: u64,

    /// Playback rate
    rate: PlaybackRate,

    /// Total samples played (for statistics)
    total_samples: u64,
}

impl AudioClock {
    /// Create a new audio clock.
    pub fn new(config: AudioClockConfig) -> Self {
        Self {
            config,
            state: AudioClockState::Stopped,
            base_position: MediaTime::ZERO,
            samples_since_base: 0,
            rate: PlaybackRate::NORMAL,
            total_samples: 0,
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &AudioClockConfig {
        &self.config
    }

    /// Get current state.
    pub fn state(&self) -> AudioClockState {
        self.state
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.state == AudioClockState::Running
    }

    /// Get current playback position.
    ///
    /// This is the authoritative time source.
    pub fn current_time(&self) -> MediaTime {
        let elapsed = self.config.samples_to_time(self.samples_since_base);
        let scaled = self.rate.scale_media_time(elapsed);
        self.base_position + scaled
    }

    /// Get current sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// Get current playback rate.
    pub fn rate(&self) -> PlaybackRate {
        self.rate
    }

    // =========================================================================
    // CONTROL
    // =========================================================================

    /// Start or resume playback.
    pub fn start(&mut self) {
        match self.state {
            AudioClockState::Stopped => {
                self.base_position = MediaTime::ZERO;
                self.samples_since_base = 0;
                self.state = AudioClockState::Running;
            }
            AudioClockState::Paused => {
                self.state = AudioClockState::Running;
            }
            AudioClockState::Running => {
                // Already running
            }
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if self.state == AudioClockState::Running {
            // Consolidate current position as base
            self.base_position = self.current_time();
            self.samples_since_base = 0;
            self.state = AudioClockState::Paused;
        }
    }

    /// Stop and reset to beginning.
    pub fn stop(&mut self) {
        self.base_position = MediaTime::ZERO;
        self.samples_since_base = 0;
        self.state = AudioClockState::Stopped;
    }

    /// Seek to position.
    pub fn seek(&mut self, position: MediaTime) {
        self.base_position = position;
        self.samples_since_base = 0;

        // If stopped, transition to paused
        if self.state == AudioClockState::Stopped {
            self.state = AudioClockState::Paused;
        }
    }

    /// Set playback rate.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        // Consolidate current position before rate change
        self.base_position = self.current_time();
        self.samples_since_base = 0;
        self.rate = rate;
    }

    // =========================================================================
    // SAMPLE TRACKING
    // =========================================================================

    /// Advance clock by number of samples played.
    ///
    /// Called from audio callback when samples are written to device.
    pub fn advance_samples(&mut self, samples: u64) {
        if self.state == AudioClockState::Running {
            self.samples_since_base += samples;
            self.total_samples += samples;
        }
    }

    /// Advance clock by buffer duration.
    pub fn advance_duration(&mut self, duration: MediaTime) {
        let samples = self.config.time_to_samples(duration);
        self.advance_samples(samples);
    }

    /// Get samples since base.
    pub fn samples_since_base(&self) -> u64 {
        self.samples_since_base
    }

    /// Get total samples played.
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    // =========================================================================
    // CONVERSIONS
    // =========================================================================

    /// Convert samples to MediaTime.
    pub fn samples_to_time(&self, samples: u64) -> MediaTime {
        self.config.samples_to_time(samples)
    }

    /// Convert MediaTime to samples.
    pub fn time_to_samples(&self, time: MediaTime) -> u64 {
        self.config.time_to_samples(time)
    }

    /// Get sample-accurate position (samples from start).
    pub fn current_sample(&self) -> u64 {
        self.config.time_to_samples(self.current_time())
    }
}

impl Default for AudioClock {
    fn default() -> Self {
        Self::new(AudioClockConfig::default())
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
    fn test_clock_new() {
        let clock = AudioClock::default();

        assert_eq!(clock.state(), AudioClockState::Stopped);
        assert_eq!(clock.current_time(), MediaTime::ZERO);
        assert!(!clock.is_running());
    }

    #[test]
    fn test_samples_to_time() {
        let config = AudioClockConfig::PROFESSIONAL; // 48kHz

        // 48000 samples = 1 second
        let time = config.samples_to_time(48000);
        assert_eq!(time, ms(1000));

        // 24000 samples = 0.5 seconds
        let time = config.samples_to_time(24000);
        assert_eq!(time, ms(500));
    }

    #[test]
    fn test_time_to_samples() {
        let config = AudioClockConfig::PROFESSIONAL;

        let samples = config.time_to_samples(ms(1000));
        assert_eq!(samples, 48000);
    }

    #[test]
    fn test_clock_advance() {
        let mut clock = AudioClock::default();
        clock.start();

        // Advance 48000 samples (1 second)
        clock.advance_samples(48000);

        assert_eq!(clock.current_time(), ms(1000));
    }

    #[test]
    fn test_clock_seek() {
        let mut clock = AudioClock::default();
        clock.start();

        clock.advance_samples(48000);
        assert_eq!(clock.current_time(), ms(1000));

        clock.seek(ms(5000));
        assert_eq!(clock.current_time(), ms(5000));
    }

    #[test]
    fn test_clock_pause_resume() {
        let mut clock = AudioClock::default();
        clock.start();

        clock.advance_samples(48000);
        clock.pause();

        let paused_time = clock.current_time();

        // Advancing while paused should not change time
        clock.advance_samples(48000);
        assert_eq!(clock.current_time(), paused_time);

        // Resume
        clock.start();
        clock.advance_samples(24000);

        // Should be 1.5 seconds now
        assert_eq!(clock.current_time(), ms(1500));
    }

    #[test]
    fn test_rate_change() {
        let mut clock = AudioClock::default();
        clock.start();

        clock.advance_samples(48000);
        assert_eq!(clock.current_time(), ms(1000));

        // Double speed
        clock.set_rate(PlaybackRate::DOUBLE);
        clock.advance_samples(48000);

        // 48000 samples at 2x = 2 seconds of content
        assert_eq!(clock.current_time(), ms(3000));
    }

    #[test]
    fn test_bytes_per_frame() {
        let config = AudioClockConfig::PROFESSIONAL; // 48kHz stereo 24-bit

        // 2 channels * 24 bits / 8 = 6 bytes per frame
        assert_eq!(config.bytes_per_frame(), 6);

        let cd = AudioClockConfig::CD_QUALITY; // 44.1kHz stereo 16-bit
                                               // 2 channels * 16 bits / 8 = 4 bytes per frame
        assert_eq!(cd.bytes_per_frame(), 4);
    }
}
