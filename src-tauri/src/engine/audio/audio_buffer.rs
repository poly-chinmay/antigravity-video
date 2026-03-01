//! AudioBuffer - Audio sample buffer management.
//!
//! # Design
//!
//! AudioBuffer manages audio samples for playback scheduling.
//! It provides ring-buffer semantics with underrun protection.
//!
//! # Memory
//!
//! Buffers are pre-allocated to avoid allocations in audio callbacks.

use std::collections::VecDeque;

use crate::engine::media_time::MediaTime;

use super::audio_clock::AudioClockConfig;

// =============================================================================
// BUFFER STATS
// =============================================================================

/// Statistics about buffer operation.
#[derive(Debug, Clone, Default)]
pub struct BufferStats {
    /// Total samples written
    pub samples_written: u64,

    /// Total samples read
    pub samples_read: u64,

    /// Underrun count (buffer empty when read needed)
    pub underruns: u64,

    /// Overrun count (buffer full when write needed)
    pub overruns: u64,

    /// Current fill level (samples)
    pub fill_level: usize,

    /// High water mark
    pub max_fill: usize,
}

// =============================================================================
// AUDIO CHUNK
// =============================================================================

/// A chunk of audio samples with timeline position.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// Timeline position of first sample
    pub position: MediaTime,

    /// Sample data (interleaved channels)
    pub samples: Vec<f32>,

    /// Number of frames (samples per channel)
    pub frames: usize,
}

impl AudioChunk {
    /// Create a new audio chunk.
    pub fn new(position: MediaTime, samples: Vec<f32>, channels: u32) -> Self {
        let frames = samples.len() / channels as usize;
        Self {
            position,
            samples,
            frames,
        }
    }

    /// Create silence chunk.
    pub fn silence(position: MediaTime, frames: usize, channels: u32) -> Self {
        let samples = vec![0.0f32; frames * channels as usize];
        Self {
            position,
            samples,
            frames,
        }
    }

    /// Get duration in samples.
    pub fn duration_samples(&self) -> usize {
        self.frames
    }

    /// Get duration as MediaTime.
    pub fn duration(&self, config: &AudioClockConfig) -> MediaTime {
        config.samples_to_time(self.frames as u64)
    }

    /// Get end position.
    pub fn end_position(&self, config: &AudioClockConfig) -> MediaTime {
        self.position + self.duration(config)
    }
}

// =============================================================================
// AUDIO BUFFER
// =============================================================================

/// Ring buffer for audio chunks.
///
/// # Usage
///
/// ```ignore
/// let mut buffer = AudioBuffer::new(config, target_fill);
///
/// // Producer side
/// buffer.push(chunk);
///
/// // Consumer side (audio callback)
/// if let Some(chunk) = buffer.pop() {
///     output.write(chunk.samples);
/// }
/// ```
#[derive(Debug)]
pub struct AudioBuffer {
    /// Audio configuration
    config: AudioClockConfig,

    /// Buffer storage
    chunks: VecDeque<AudioChunk>,

    /// Target fill level in frames
    target_fill: usize,

    /// Maximum capacity in frames
    max_frames: usize,

    /// Current fill in frames
    current_frames: usize,

    /// Statistics
    stats: BufferStats,
}

impl AudioBuffer {
    /// Create a new audio buffer.
    pub fn new(config: AudioClockConfig, target_frames: usize, max_frames: usize) -> Self {
        Self {
            config,
            chunks: VecDeque::new(),
            target_fill: target_frames,
            max_frames,
            current_frames: 0,
            stats: BufferStats::default(),
        }
    }

    /// Create with default sizes (~100ms buffer).
    pub fn with_config(config: AudioClockConfig) -> Self {
        let sample_rate = config.sample_rate as usize;
        let target = sample_rate / 10; // 100ms
        let max = sample_rate / 2; // 500ms
        Self::new(config, target, max)
    }

    /// Get current fill level in frames.
    pub fn fill_level(&self) -> usize {
        self.current_frames
    }

    /// Get fill level as duration.
    pub fn fill_duration(&self) -> MediaTime {
        self.config.samples_to_time(self.current_frames as u64)
    }

    /// Check if buffer needs more data.
    pub fn needs_fill(&self) -> bool {
        self.current_frames < self.target_fill
    }

    /// Check if buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Check if buffer is full.
    pub fn is_full(&self) -> bool {
        self.current_frames >= self.max_frames
    }

    /// Push audio chunk.
    pub fn push(&mut self, chunk: AudioChunk) -> bool {
        let frames = chunk.frames;

        // Check overflow
        if self.current_frames + frames > self.max_frames {
            self.stats.overruns += 1;
            return false;
        }

        self.chunks.push_back(chunk);
        self.current_frames += frames;
        self.stats.samples_written += frames as u64;
        self.stats.fill_level = self.current_frames;
        self.stats.max_fill = self.stats.max_fill.max(self.current_frames);

        true
    }

    /// Pop next chunk for playback.
    pub fn pop(&mut self) -> Option<AudioChunk> {
        let chunk = self.chunks.pop_front()?;
        self.current_frames -= chunk.frames;
        self.stats.samples_read += chunk.frames as u64;
        self.stats.fill_level = self.current_frames;
        Some(chunk)
    }

    /// Peek at next chunk without removing.
    pub fn peek(&self) -> Option<&AudioChunk> {
        self.chunks.front()
    }

    /// Record an underrun (called when pop needed but buffer empty).
    pub fn record_underrun(&mut self) {
        self.stats.underruns += 1;
    }

    /// Clear all buffered audio.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.current_frames = 0;
        self.stats.fill_level = 0;
    }

    /// Get statistics.
    pub fn stats(&self) -> &BufferStats {
        &self.stats
    }

    /// Get number of chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Get position of next chunk to play.
    pub fn next_position(&self) -> Option<MediaTime> {
        self.chunks.front().map(|c| c.position)
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::with_config(AudioClockConfig::default())
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

    fn make_chunk(position_ms: i64, frames: usize) -> AudioChunk {
        AudioChunk::silence(ms(position_ms), frames, 2)
    }

    #[test]
    fn test_buffer_new() {
        let buffer = AudioBuffer::default();

        assert!(buffer.is_empty());
        assert!(buffer.needs_fill());
    }

    #[test]
    fn test_buffer_push_pop() {
        let mut buffer = AudioBuffer::default();

        buffer.push(make_chunk(0, 512));
        buffer.push(make_chunk(512, 512));

        assert_eq!(buffer.fill_level(), 1024);
        assert!(!buffer.is_empty());

        let chunk = buffer.pop().unwrap();
        assert_eq!(chunk.position, ms(0));
        assert_eq!(chunk.frames, 512);

        assert_eq!(buffer.fill_level(), 512);
    }

    #[test]
    fn test_buffer_overflow() {
        let config = AudioClockConfig::PROFESSIONAL;
        let mut buffer = AudioBuffer::new(config, 512, 1024);

        // Fill to max
        assert!(buffer.push(make_chunk(0, 512)));
        assert!(buffer.push(make_chunk(0, 512)));

        // Should reject overflow
        assert!(!buffer.push(make_chunk(0, 512)));
        assert_eq!(buffer.stats().overruns, 1);
    }

    #[test]
    fn test_buffer_underrun() {
        let mut buffer = AudioBuffer::default();

        assert!(buffer.pop().is_none());
        buffer.record_underrun();

        assert_eq!(buffer.stats().underruns, 1);
    }

    #[test]
    fn test_audio_never_underruns() {
        // Test that properly filled buffer never underruns
        let config = AudioClockConfig::PROFESSIONAL;
        let mut buffer = AudioBuffer::new(config, 1024, 4096);

        // Pre-fill
        for i in 0..4 {
            buffer.push(make_chunk(i * 512, 512));
        }

        // Simulate playback loop: consume and produce
        for i in 0..100 {
            // Consumer - always succeeds if buffer was filled
            if buffer.pop().is_none() {
                buffer.record_underrun();
            }

            // Producer - refill
            buffer.push(make_chunk((i + 4) * 512, 512));
        }

        assert_eq!(buffer.stats().underruns, 0);
    }
}
