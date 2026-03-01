//! AudioScheduler - Audio scheduling and clip mixing.
//!
//! # Design
//!
//! AudioScheduler produces audio chunks at timeline positions, similar to
//! how FrameScheduler produces video frames. It queries TimelineView for
//! clips and mixes audio from active clips.
//!
//! # Determinism
//!
//! All scheduling is deterministic and testable without real audio hardware.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::VisibleClip;
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::TimelineState;

use super::audio_buffer::{AudioBuffer, AudioChunk};
use super::audio_clock::{AudioClock, AudioClockConfig, AudioClockState};

// =============================================================================
// SCHEDULER CONFIG
// =============================================================================

/// Configuration for the audio scheduler.
#[derive(Debug, Clone)]
pub struct AudioSchedulerConfig {
    /// Audio configuration
    pub audio_config: AudioClockConfig,

    /// Chunk size in frames
    pub chunk_frames: usize,

    /// Target buffer fill in frames
    pub target_fill: usize,
}

impl Default for AudioSchedulerConfig {
    fn default() -> Self {
        let audio_config = AudioClockConfig::PROFESSIONAL;
        Self {
            audio_config,
            chunk_frames: 512,
            target_fill: audio_config.sample_rate as usize / 10, // 100ms
        }
    }
}

// =============================================================================
// AUDIO REQUEST
// =============================================================================

/// Request for audio samples at a position.
#[derive(Debug, Clone)]
pub struct AudioRequest {
    /// Timeline position
    pub position: MediaTime,

    /// Frames needed
    pub frames: usize,

    /// Expected clips at this position
    pub clips: Vec<String>,
}

// =============================================================================
// AUDIO SCHEDULER
// =============================================================================

/// Audio scheduling and production.
///
/// # Usage
///
/// ```ignore
/// let mut scheduler = AudioScheduler::new(config);
///
/// // In audio loop
/// while scheduler.buffer_needs_fill() {
///     scheduler.produce_chunk(&clock, &index, &state);
/// }
///
/// // Get next chunk for device
/// if let Some(chunk) = scheduler.pop_chunk() {
///     device.write(chunk);
/// }
/// ```
#[derive(Debug)]
pub struct AudioScheduler {
    /// Configuration
    config: AudioSchedulerConfig,

    /// Audio buffer
    buffer: AudioBuffer,

    /// Next position to produce
    next_position: MediaTime,

    /// Total chunks produced
    chunks_produced: u64,
}

impl AudioScheduler {
    /// Create a new audio scheduler.
    pub fn new(config: AudioSchedulerConfig) -> Self {
        let buffer = AudioBuffer::new(
            config.audio_config,
            config.target_fill,
            config.target_fill * 4,
        );

        Self {
            config,
            buffer,
            next_position: MediaTime::ZERO,
            chunks_produced: 0,
        }
    }

    /// Check if buffer needs more chunks.
    pub fn buffer_needs_fill(&self) -> bool {
        self.buffer.needs_fill()
    }

    /// Check if buffer is empty.
    pub fn buffer_is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get buffer fill level.
    pub fn buffer_fill(&self) -> usize {
        self.buffer.fill_level()
    }

    /// Produce an audio chunk.
    ///
    /// Returns the produced chunk if successful.
    pub fn produce_chunk(
        &mut self,
        clock: &AudioClock,
        index: &TimelineIndex,
        state: &TimelineState,
    ) -> Option<AudioChunk> {
        if !self.buffer.needs_fill() {
            return None;
        }

        // Use next_position for consistent scheduling
        let position = self.next_position;

        // Query clips at this position
        let _clips = index.clips_at(position);

        // Produce silence or mixed audio
        // (In real implementation, would decode and mix audio from clips)
        let chunk = AudioChunk::silence(
            position,
            self.config.chunk_frames,
            self.config.audio_config.channels,
        );

        // Advance position
        let chunk_duration = self
            .config
            .audio_config
            .samples_to_time(self.config.chunk_frames as u64);
        self.next_position = position + chunk_duration;

        // Buffer the chunk
        if self.buffer.push(chunk.clone()) {
            self.chunks_produced += 1;
            Some(chunk)
        } else {
            None
        }
    }

    /// Pop next chunk for playback.
    pub fn pop_chunk(&mut self) -> Option<AudioChunk> {
        self.buffer.pop()
    }

    /// Peek at next chunk.
    pub fn peek_chunk(&self) -> Option<&AudioChunk> {
        self.buffer.peek()
    }

    /// Seek to position.
    pub fn seek(&mut self, position: MediaTime) {
        self.buffer.clear();
        self.next_position = position;
    }

    /// Reset scheduler.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.next_position = MediaTime::ZERO;
        self.chunks_produced = 0;
    }

    /// Get chunks produced.
    pub fn chunks_produced(&self) -> u64 {
        self.chunks_produced
    }

    /// Get buffer stats.
    pub fn buffer_stats(&self) -> &super::audio_buffer::BufferStats {
        self.buffer.stats()
    }

    /// Get next position to produce.
    pub fn next_position(&self) -> MediaTime {
        self.next_position
    }

    /// Get audio configuration.
    pub fn audio_config(&self) -> &AudioClockConfig {
        &self.config.audio_config
    }
}

impl Default for AudioScheduler {
    fn default() -> Self {
        Self::new(AudioSchedulerConfig::default())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
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
        let scheduler = AudioScheduler::default();

        assert!(scheduler.buffer_needs_fill());
        assert!(scheduler.buffer_is_empty());
    }

    #[test]
    fn test_produce_chunk() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let index = TimelineIndex::build(&state);
        let clock = AudioClock::default();

        let mut scheduler = AudioScheduler::default();

        let chunk = scheduler.produce_chunk(&clock, &index, &state);

        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.position, MediaTime::ZERO);
        assert_eq!(chunk.frames, 512);
    }

    #[test]
    fn test_seek() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let index = TimelineIndex::build(&state);
        let clock = AudioClock::default();

        let mut scheduler = AudioScheduler::default();

        // Produce some chunks
        scheduler.produce_chunk(&clock, &index, &state);
        scheduler.produce_chunk(&clock, &index, &state);

        assert!(!scheduler.buffer_is_empty());

        // Seek clears buffer
        scheduler.seek(ms(5000));

        assert!(scheduler.buffer_is_empty());
        assert_eq!(scheduler.next_position(), ms(5000));
    }

    #[test]
    fn test_position_advances() {
        let state = make_state(vec![]);
        let index = TimelineIndex::build(&state);
        let clock = AudioClock::default();

        let config = AudioSchedulerConfig {
            chunk_frames: 512,
            target_fill: 2048,
            ..Default::default()
        };
        let mut scheduler = AudioScheduler::new(config);

        let mut positions = Vec::new();
        for _ in 0..4 {
            if let Some(chunk) = scheduler.produce_chunk(&clock, &index, &state) {
                positions.push(chunk.position);
            }
        }

        // Positions should be strictly increasing
        for i in 1..positions.len() {
            assert!(positions[i] > positions[i - 1]);
        }
    }
}
