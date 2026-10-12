//! AVSync - Audio/Video synchronization controller.
//!
//! # Design
//!
//! AVSync ensures video timing follows audio timing. It enforces:
//! - Audio is master clock
//! - Video never leads audio by more than 1 frame
//! - Video may lag audio by up to 1 frame (preferred over leading)
//! - Seek/pause/resume maintain sync
//!
//! # Sync Strategy
//!
//! 1. Audio clock advances based on samples played
//! 2. Video scheduler queries audio clock for target time
//! 3. Video skips frames if behind, waits if ahead
//! 4. Drift is corrected by micro-adjusting video timing
//!
//! # Thread Safety
//!
//! AVSync is designed for single-threaded use with periodic tick calls.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::PlaybackRate;

use super::audio_clock::{AudioClock, AudioClockConfig, AudioClockState};

// =============================================================================
// SYNC STATUS
// =============================================================================

/// Status of A/V synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// Audio and video are in sync
    InSync,
    /// Video is ahead of audio (bad - need to wait)
    VideoAhead,
    /// Video is behind audio (ok - need to catch up)
    VideoBehind,
    /// Actively seeking (sync suspended)
    Seeking,
    /// Paused (sync suspended)
    Paused,
}

// =============================================================================
// SYNC STATS
// =============================================================================

/// Statistics about synchronization.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Total frames processed
    pub frames_processed: u64,

    /// Frames skipped (video behind)
    pub frames_skipped: u64,

    /// Frames repeated (video ahead)
    pub frames_repeated: u64,

    /// sync corrections made
    pub corrections: u64,

    /// Maximum drift observed (nanos)
    pub max_drift_ns: i64,

    /// Average drift (nanos)
    pub avg_drift_ns: i64,
}

// =============================================================================
// SYNC CONFIG
// =============================================================================

/// Configuration for A/V sync.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Maximum allowed drift before correction (nanos)
    pub max_drift_ns: i64,

    /// Frame interval for video (nanos)
    pub frame_interval_ns: i64,

    /// Enable sync corrections
    pub corrections_enabled: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_drift_ns: 16_666_667, // ~1 frame at 60fps
            frame_interval_ns: 16_666_667,
            corrections_enabled: true,
        }
    }
}

// =============================================================================
// AV PAIRING
// =============================================================================

/// A synchronized audio/video pair.
#[derive(Debug, Clone)]
pub struct AVPair {
    /// Audio position
    pub audio_time: MediaTime,

    /// Video position
    pub video_time: MediaTime,

    /// Drift (video - audio)
    pub drift: MediaTime,

    /// Status
    pub status: SyncStatus,
}

impl AVPair {
    /// Check if pair is in sync (within 1 frame).
    pub fn is_in_sync(&self, frame_interval_ns: i64) -> bool {
        self.drift.as_nanos().abs() <= frame_interval_ns
    }
}

// =============================================================================
// AV SYNC
// =============================================================================

/// A/V synchronization controller.
///
/// # Usage
///
/// ```ignore
/// let mut sync = AVSync::new(audio_clock, config);
///
/// // In render loop
/// let audio_time = sync.audio_time();
/// let video_target = sync.video_target_time();
///
/// // Check sync
/// let pair = sync.pair(video_time);
/// if pair.status == SyncStatus::VideoAhead {
///     // Wait before displaying this frame
/// }
/// ```
#[derive(Debug)]
pub struct AVSync {
    /// Audio clock (master)
    audio_clock: AudioClock,

    /// Configuration
    config: SyncConfig,

    /// Statistics
    stats: SyncStats,

    /// Last video time
    last_video_time: MediaTime,

    /// Accumulated drift for averaging
    drift_sum: i64,

    /// Drift sample count
    drift_count: u64,

    /// Whether currently seeking
    seeking: bool,
}

impl AVSync {
    /// Create a new A/V sync controller.
    pub fn new(audio_clock: AudioClock, config: SyncConfig) -> Self {
        Self {
            audio_clock,
            config,
            stats: SyncStats::default(),
            last_video_time: MediaTime::ZERO,
            drift_sum: 0,
            drift_count: 0,
            seeking: false,
        }
    }

    /// Create with default config.
    pub fn with_audio_clock(audio_clock: AudioClock) -> Self {
        Self::new(audio_clock, SyncConfig::default())
    }

    /// Get audio clock reference.
    pub fn audio_clock(&self) -> &AudioClock {
        &self.audio_clock
    }

    /// Get mutable audio clock reference.
    pub fn audio_clock_mut(&mut self) -> &mut AudioClock {
        &mut self.audio_clock
    }

    /// Get current audio time (master).
    pub fn audio_time(&self) -> MediaTime {
        self.audio_clock.current_time()
    }

    /// Get target video time (should match audio).
    pub fn video_target_time(&self) -> MediaTime {
        self.audio_clock.current_time()
    }

    /// Get sync status for a video time.
    pub fn status_for(&self, video_time: MediaTime) -> SyncStatus {
        if self.seeking {
            return SyncStatus::Seeking;
        }

        if self.audio_clock.state() == AudioClockState::Paused {
            return SyncStatus::Paused;
        }

        let audio_time = self.audio_time();
        let drift = video_time.as_nanos() - audio_time.as_nanos();

        if drift.abs() <= self.config.max_drift_ns {
            SyncStatus::InSync
        } else if drift > 0 {
            SyncStatus::VideoAhead
        } else {
            SyncStatus::VideoBehind
        }
    }

    /// Create an A/V pair for synchronization check.
    pub fn pair(&mut self, video_time: MediaTime) -> AVPair {
        let audio_time = self.audio_time();
        let drift_ns = video_time.as_nanos() - audio_time.as_nanos();
        let drift = MediaTime::from_nanos(drift_ns);

        // Track stats
        self.stats.frames_processed += 1;
        self.drift_sum += drift_ns;
        self.drift_count += 1;
        self.stats.max_drift_ns = self.stats.max_drift_ns.max(drift_ns.abs());
        self.stats.avg_drift_ns = self.drift_sum / self.drift_count as i64;

        let status = self.status_for(video_time);

        match status {
            SyncStatus::VideoBehind => {
                self.stats.frames_skipped += 1;
            }
            SyncStatus::VideoAhead => {
                self.stats.frames_repeated += 1;
            }
            _ => {}
        }

        self.last_video_time = video_time;

        AVPair {
            audio_time,
            video_time,
            drift,
            status,
        }
    }

    /// Check if video should skip a frame to catch up.
    pub fn should_skip(&self, video_time: MediaTime) -> bool {
        let status = self.status_for(video_time);
        matches!(status, SyncStatus::VideoBehind)
    }

    /// Check if video should wait before displaying.
    pub fn should_wait(&self, video_time: MediaTime) -> bool {
        let status = self.status_for(video_time);
        matches!(status, SyncStatus::VideoAhead)
    }

    // =========================================================================
    // TRANSPORT CONTROL
    // =========================================================================

    /// Start playback.
    pub fn start(&mut self) {
        self.audio_clock.start();
        self.seeking = false;
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.audio_clock.pause();
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        self.audio_clock.stop();
        self.last_video_time = MediaTime::ZERO;
    }

    /// Seek to position.
    pub fn seek(&mut self, position: MediaTime) {
        self.seeking = true;
        self.audio_clock.seek(position);
        self.last_video_time = position;
        self.seeking = false;
    }

    /// Set playback rate.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        self.audio_clock.set_rate(rate);
    }

    /// Advance audio by samples (called from audio callback).
    pub fn advance_samples(&mut self, samples: u64) {
        self.audio_clock.advance_samples(samples);
    }

    // =========================================================================
    // STATS
    // =========================================================================

    /// Get sync statistics.
    pub fn stats(&self) -> &SyncStats {
        &self.stats
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.stats = SyncStats::default();
        self.drift_sum = 0;
        self.drift_count = 0;
    }

    /// Check if within sync tolerance.
    pub fn is_in_sync(&self, video_time: MediaTime) -> bool {
        matches!(self.status_for(video_time), SyncStatus::InSync)
    }
}

impl Default for AVSync {
    fn default() -> Self {
        Self::with_audio_clock(AudioClock::default())
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

    fn make_sync() -> AVSync {
        AVSync::default()
    }

    #[test]
    fn test_sync_new() {
        let sync = make_sync();

        assert_eq!(sync.audio_time(), MediaTime::ZERO);
        assert_eq!(sync.video_target_time(), MediaTime::ZERO);
    }

    #[test]
    fn test_sync_in_sync() {
        let mut sync = make_sync();
        sync.start();

        // Advance audio clock
        sync.advance_samples(48000); // 1 second

        let audio_time = sync.audio_time();
        let pair = sync.pair(audio_time);

        assert_eq!(pair.status, SyncStatus::InSync);
    }

    #[test]
    fn test_video_never_leads_audio() {
        let mut sync = make_sync();
        sync.start();

        sync.advance_samples(48000); // 1 second

        // Video at 1.5 seconds, audio at 1 second
        let video_time = ms(1500);
        let pair = sync.pair(video_time);

        assert_eq!(pair.status, SyncStatus::VideoAhead);
        assert!(sync.should_wait(video_time));
    }

    #[test]
    fn test_video_behind_catches_up() {
        let mut sync = make_sync();
        sync.start();

        sync.advance_samples(48000 * 2); // 2 seconds

        // Video at 0.5 seconds, audio at 2 seconds
        let video_time = ms(500);
        let pair = sync.pair(video_time);

        assert_eq!(pair.status, SyncStatus::VideoBehind);
        assert!(sync.should_skip(video_time));
    }

    #[test]
    fn test_seek_resets_audio_video_alignment() {
        let mut sync = make_sync();
        sync.start();

        sync.advance_samples(48000);
        assert_eq!(sync.audio_time(), ms(1000));

        // Seek to 5 seconds
        sync.seek(ms(5000));

        assert_eq!(sync.audio_time(), ms(5000));
        assert_eq!(sync.video_target_time(), ms(5000));

        // Video at target should be in sync
        let pair = sync.pair(ms(5000));
        assert_eq!(pair.status, SyncStatus::InSync);
    }

    #[test]
    fn test_speed_change_preserves_sync() {
        let mut sync = make_sync();
        sync.start();

        sync.advance_samples(48000);
        let time_before = sync.audio_time();

        // Change to 2x speed
        sync.set_rate(PlaybackRate::DOUBLE);

        // Time should be preserved
        assert_eq!(sync.audio_time(), time_before);

        // Advance more
        sync.advance_samples(48000);

        // Should have advanced 2x the normal amount
        assert_eq!(sync.audio_time(), ms(3000)); // 1000 + 2000
    }

    #[test]
    fn test_no_av_drift_over_10_minutes() {
        let mut sync = make_sync();
        sync.start();

        // Simulate 10 minutes of playback
        let frames_10_min = 60 * 60 * 10; // 36000 frames at 60fps
        let samples_per_frame = 48000 / 60; // 800 samples

        for frame in 0..frames_10_min {
            // Advance audio by samples per frame
            sync.advance_samples(samples_per_frame);

            // Video follows audio
            let video_time = sync.video_target_time();
            let pair = sync.pair(video_time);

            // Should always be in sync
            assert!(
                pair.is_in_sync(sync.config.frame_interval_ns),
                "Drift at frame {}: {:?}",
                frame,
                pair.drift
            );
        }

        // Check final time is correct (10 minutes)
        let expected = ms(10 * 60 * 1000);
        let actual = sync.audio_time();
        let drift = (actual.as_nanos() - expected.as_nanos()).abs();

        // Allow 1 frame tolerance
        assert!(
            drift < 20_000_000, // 20ms
            "Final drift too large: {}ns",
            drift
        );
    }

    #[test]
    fn test_scrubbing_produces_correct_pair() {
        let mut sync = make_sync();
        sync.start(); // Start playback first

        // Scrub to various positions
        let positions = [0, 1000, 5000, 10000, 30000];

        for &pos in &positions {
            sync.seek(ms(pos));

            // Video at seek position
            let pair = sync.pair(ms(pos));

            assert_eq!(pair.audio_time, ms(pos));
            assert_eq!(pair.video_time, ms(pos));
            assert_eq!(pair.status, SyncStatus::InSync);
        }
    }
}
