//! AudioDevice - Mockable audio device interface.
//!
//! # Design
//!
//! AudioDevice abstracts the hardware audio interface for testability.
//! Two implementations are provided:
//! - MockAudioDevice for deterministic testing
//! - (Future) Real device via cpal or platform APIs
//!
//! # Thread Safety
//!
//! Audio devices are typically accessed from a dedicated audio thread.
//! The interface is designed for single-threaded callbacks.

use crate::engine::media_time::MediaTime;

use super::audio_clock::AudioClockConfig;

// =============================================================================
// DEVICE STATE
// =============================================================================

/// Audio device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Device not initialized
    Uninitialized,
    /// Device ready but not playing
    Ready,
    /// Device actively playing
    Playing,
    /// Device error
    Error,
}

// =============================================================================
// AUDIO DEVICE TRAIT
// =============================================================================

/// Trait for audio output devices.
pub trait AudioDevice: std::fmt::Debug + Send {
    /// Get device state.
    fn state(&self) -> DeviceState;

    /// Get audio configuration.
    fn config(&self) -> &AudioClockConfig;

    /// Start playback.
    fn start(&mut self) -> Result<(), String>;

    /// Stop playback.
    fn stop(&mut self) -> Result<(), String>;

    /// Get buffer size in samples.
    fn buffer_size(&self) -> usize;

    /// Get current latency in samples.
    fn latency_samples(&self) -> u64;

    /// Get latency as MediaTime.
    fn latency(&self) -> MediaTime {
        self.config().samples_to_time(self.latency_samples())
    }
}

// =============================================================================
// MOCK AUDIO DEVICE
// =============================================================================

/// Mock audio device for testing.
///
/// Provides deterministic behavior without real audio hardware.
#[derive(Debug)]
pub struct MockAudioDevice {
    /// Configuration
    config: AudioClockConfig,

    /// Current state
    state: DeviceState,

    /// Buffer size in samples per channel
    buffer_size: usize,

    /// Simulated latency in samples
    latency: u64,

    /// Total samples "played"
    total_samples: u64,

    /// Buffers written
    buffers_written: u64,
}

impl MockAudioDevice {
    /// Create a new mock device.
    pub fn new(config: AudioClockConfig) -> Self {
        Self {
            config,
            state: DeviceState::Ready,
            buffer_size: 512,
            latency: 1024,
            total_samples: 0,
            buffers_written: 0,
        }
    }

    /// Set buffer size.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set latency.
    pub fn with_latency(mut self, samples: u64) -> Self {
        self.latency = samples;
        self
    }

    /// Simulate writing a buffer.
    pub fn write_buffer(&mut self, samples: usize) {
        if self.state == DeviceState::Playing {
            self.total_samples += samples as u64;
            self.buffers_written += 1;
        }
    }

    /// Get total samples played.
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Get number of buffers written.
    pub fn buffers_written(&self) -> u64 {
        self.buffers_written
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.total_samples = 0;
        self.buffers_written = 0;
    }
}

impl AudioDevice for MockAudioDevice {
    fn state(&self) -> DeviceState {
        self.state
    }

    fn config(&self) -> &AudioClockConfig {
        &self.config
    }

    fn start(&mut self) -> Result<(), String> {
        self.state = DeviceState::Playing;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        self.state = DeviceState::Ready;
        Ok(())
    }

    fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    fn latency_samples(&self) -> u64 {
        self.latency
    }
}

impl Default for MockAudioDevice {
    fn default() -> Self {
        Self::new(AudioClockConfig::default())
    }
}

// =============================================================================
// NULL DEVICE (for offline mode)
// =============================================================================

/// Null audio device for offline rendering.
///
/// Consumes audio data without outputting anything.
#[derive(Debug)]
pub struct NullAudioDevice {
    config: AudioClockConfig,
    state: DeviceState,
}

impl NullAudioDevice {
    /// Create a new null device.
    pub fn new(config: AudioClockConfig) -> Self {
        Self {
            config,
            state: DeviceState::Ready,
        }
    }
}

impl AudioDevice for NullAudioDevice {
    fn state(&self) -> DeviceState {
        self.state
    }

    fn config(&self) -> &AudioClockConfig {
        &self.config
    }

    fn start(&mut self) -> Result<(), String> {
        self.state = DeviceState::Playing;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        self.state = DeviceState::Ready;
        Ok(())
    }

    fn buffer_size(&self) -> usize {
        1024
    }

    fn latency_samples(&self) -> u64 {
        0 // No latency in offline mode
    }
}

impl Default for NullAudioDevice {
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

    #[test]
    fn test_mock_device_new() {
        let device = MockAudioDevice::default();

        assert_eq!(device.state(), DeviceState::Ready);
        assert_eq!(device.buffer_size(), 512);
    }

    #[test]
    fn test_mock_device_start_stop() {
        let mut device = MockAudioDevice::default();

        device.start().unwrap();
        assert_eq!(device.state(), DeviceState::Playing);

        device.stop().unwrap();
        assert_eq!(device.state(), DeviceState::Ready);
    }

    #[test]
    fn test_mock_device_write() {
        let mut device = MockAudioDevice::default();
        device.start().unwrap();

        device.write_buffer(512);
        device.write_buffer(512);

        assert_eq!(device.total_samples(), 1024);
        assert_eq!(device.buffers_written(), 2);
    }

    #[test]
    fn test_null_device() {
        let mut device = NullAudioDevice::default();

        assert_eq!(device.latency_samples(), 0);

        device.start().unwrap();
        assert_eq!(device.state(), DeviceState::Playing);
    }
}
