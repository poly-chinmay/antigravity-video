//! Transport - Play/pause/seek state machine.
//!
//! # Design
//!
//! The Transport encapsulates the playback state machine with clear transitions:
//!
//! - Stopped → Playing (play)
//! - Playing → Paused (pause)
//! - Paused → Playing (play)
//! - Any → Stopped (stop)
//!
//! # Thread Safety
//!
//! Transport provides atomic state transitions. For multi-threaded access,
//! wrap in Arc<RwLock<Transport>>.

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;

use super::clock::PlaybackRate;
use super::playhead::Playhead;

// =============================================================================
// TRANSPORT STATE
// =============================================================================

/// Current transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportState {
    /// Not playing, position at 0.
    Stopped,
    /// Actively playing.
    Playing,
    /// Paused at current position.
    Paused,
}

// =============================================================================
// TRANSPORT COMMAND
// =============================================================================

/// Commands that can be sent to the transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportCommand {
    /// Start playback.
    Play,
    /// Pause playback.
    Pause,
    /// Stop and reset to beginning.
    Stop,
    /// Seek to absolute position.
    Seek(MediaTime),
    /// Seek relative to current position.
    SeekRelative(MediaTime),
    /// Set playback rate.
    SetRate(PlaybackRate),
    /// Toggle play/pause.
    Toggle,
}

// =============================================================================
// TRANSPORT
// =============================================================================

/// Playback transport controller.
///
/// # State Machine
///
/// ```text
///                  play()
///     ┌─────────────────────────┐
///     │                         ▼
/// ┌───────┐              ┌──────────┐
/// │Stopped│              │ Playing  │
/// └───────┘              └──────────┘
///     ▲                         │
///     │         pause()         ▼
///     │                   ┌──────────┐
///     └───────────────────│  Paused  │
///           stop()        └──────────┘
/// ```
#[derive(Debug)]
pub struct Transport {
    /// Internal playhead
    playhead: Playhead,

    /// Current state
    state: TransportState,

    /// Loop mode enabled
    loop_enabled: bool,

    /// Loop in point (if looping)
    loop_in: MediaTime,

    /// Loop out point (if looping)
    loop_out: MediaTime,
}

impl Transport {
    /// Create a new transport with given duration.
    pub fn new(duration: MediaTime) -> Self {
        Self {
            playhead: Playhead::with_duration(duration),
            state: TransportState::Stopped,
            loop_enabled: false,
            loop_in: MediaTime::ZERO,
            loop_out: duration,
        }
    }

    /// Get current transport state.
    pub fn state(&self) -> TransportState {
        self.state
    }

    /// Check if playing.
    pub fn is_playing(&self) -> bool {
        self.state == TransportState::Playing
    }

    /// Get current position.
    pub fn position(&self) -> MediaTime {
        let raw = self.playhead.position();

        // Handle loop wraparound
        if self.loop_enabled && self.is_playing() && raw >= self.loop_out {
            self.loop_in
        } else {
            raw
        }
    }

    /// Get timeline duration.
    pub fn duration(&self) -> MediaTime {
        self.playhead.duration()
    }

    /// Get playback rate.
    pub fn rate(&self) -> PlaybackRate {
        self.playhead.clock().rate()
    }

    // =========================================================================
    // COMMANDS
    // =========================================================================

    /// Execute a transport command.
    pub fn execute(&mut self, cmd: TransportCommand) {
        match cmd {
            TransportCommand::Play => self.play(),
            TransportCommand::Pause => self.pause(),
            TransportCommand::Stop => self.stop(),
            TransportCommand::Seek(pos) => self.seek(pos),
            TransportCommand::SeekRelative(delta) => self.seek_relative(delta),
            TransportCommand::SetRate(rate) => self.set_rate(rate),
            TransportCommand::Toggle => self.toggle(),
        }
    }

    /// Start playback.
    pub fn play(&mut self) {
        match self.state {
            TransportState::Stopped => {
                self.playhead.seek(MediaTime::ZERO);
                self.playhead.play();
                self.state = TransportState::Playing;
            }
            TransportState::Paused => {
                self.playhead.play();
                self.state = TransportState::Playing;
            }
            TransportState::Playing => {
                // Already playing, no-op
            }
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if self.state == TransportState::Playing {
            self.playhead.pause();
            self.state = TransportState::Paused;
        }
    }

    /// Stop playback and reset to beginning.
    pub fn stop(&mut self) {
        self.playhead.pause();
        self.playhead.seek(MediaTime::ZERO);
        self.state = TransportState::Stopped;
    }

    /// Toggle between play and pause.
    pub fn toggle(&mut self) {
        match self.state {
            TransportState::Stopped | TransportState::Paused => self.play(),
            TransportState::Playing => self.pause(),
        }
    }

    /// Seek to absolute position.
    pub fn seek(&mut self, position: MediaTime) {
        self.playhead.seek(position);

        // If stopped, transition to paused
        if self.state == TransportState::Stopped {
            self.state = TransportState::Paused;
        }
    }

    /// Seek relative to current position.
    pub fn seek_relative(&mut self, delta: MediaTime) {
        self.playhead.seek_relative(delta);

        if self.state == TransportState::Stopped {
            self.state = TransportState::Paused;
        }
    }

    /// Set playback rate.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        self.playhead.clock_mut().set_rate(rate);
    }

    // =========================================================================
    // LOOP CONTROL
    // =========================================================================

    /// Enable or disable loop mode.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
    }

    /// Check if loop mode is enabled.
    pub fn is_loop_enabled(&self) -> bool {
        self.loop_enabled
    }

    /// Set loop in point.
    pub fn set_loop_in(&mut self, point: MediaTime) {
        self.loop_in = point;
    }

    /// Set loop out point.
    pub fn set_loop_out(&mut self, point: MediaTime) {
        self.loop_out = point;
    }

    /// Get loop region.
    pub fn loop_region(&self) -> (MediaTime, MediaTime) {
        (self.loop_in, self.loop_out)
    }

    // =========================================================================
    // INTERNAL
    // =========================================================================

    /// Update duration (called when timeline changes).
    pub fn set_duration(&mut self, duration: MediaTime) {
        self.playhead.set_duration(duration);
        self.loop_out = duration;
    }

    /// Check if at end of timeline.
    pub fn is_at_end(&self) -> bool {
        self.playhead.is_at_end()
    }

    /// Tick function for loop handling.
    ///
    /// Should be called periodically to handle loop wraparound.
    pub fn tick(&mut self) {
        if !self.is_playing() {
            return;
        }

        // Handle end of timeline
        if self.playhead.is_at_end() {
            if self.loop_enabled {
                self.playhead.seek(self.loop_in);
            } else {
                self.pause();
            }
        }

        // Handle loop region
        if self.loop_enabled && self.playhead.position() >= self.loop_out {
            self.playhead.seek(self.loop_in);
        }
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(MediaTime::ZERO)
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
    fn test_transport_new() {
        let transport = Transport::new(ms(10000));

        assert_eq!(transport.state(), TransportState::Stopped);
        assert_eq!(transport.position(), MediaTime::ZERO);
        assert!(!transport.is_playing());
    }

    #[test]
    fn test_transport_play_pause() {
        let mut transport = Transport::new(ms(10000));

        transport.play();
        assert_eq!(transport.state(), TransportState::Playing);
        assert!(transport.is_playing());

        transport.pause();
        assert_eq!(transport.state(), TransportState::Paused);
        assert!(!transport.is_playing());
    }

    #[test]
    fn test_transport_stop() {
        let mut transport = Transport::new(ms(10000));

        transport.play();
        sleep(Duration::from_millis(10));
        transport.stop();

        assert_eq!(transport.state(), TransportState::Stopped);
        assert_eq!(transport.position(), MediaTime::ZERO);
    }

    #[test]
    fn test_transport_toggle() {
        let mut transport = Transport::new(ms(10000));

        transport.toggle();
        assert!(transport.is_playing());

        transport.toggle();
        assert!(!transport.is_playing());
        assert_eq!(transport.state(), TransportState::Paused);
    }

    #[test]
    fn test_seek_accuracy() {
        let mut transport = Transport::new(ms(10000));

        transport.seek(ms(5000));
        assert_eq!(transport.position(), ms(5000));

        transport.seek(ms(7500));
        assert_eq!(transport.position(), ms(7500));

        transport.seek(ms(2500));
        assert_eq!(transport.position(), ms(2500));
    }

    #[test]
    fn test_seek_clamps() {
        let mut transport = Transport::new(ms(10000));

        transport.seek(ms(-1000));
        assert_eq!(transport.position(), MediaTime::ZERO);

        transport.seek(ms(20000));
        assert_eq!(transport.position(), ms(10000));
    }

    #[test]
    fn test_seek_relative() {
        let mut transport = Transport::new(ms(10000));

        transport.seek(ms(3000));
        transport.seek_relative(ms(2000));

        assert_eq!(transport.position(), ms(5000));
    }

    #[test]
    fn test_set_rate() {
        let mut transport = Transport::new(ms(10000));

        transport.set_rate(PlaybackRate::DOUBLE);
        assert_eq!(transport.rate(), PlaybackRate::DOUBLE);
    }

    #[test]
    fn test_loop_region() {
        let mut transport = Transport::new(ms(10000));

        transport.set_loop_in(ms(2000));
        transport.set_loop_out(ms(8000));
        transport.set_loop_enabled(true);

        assert!(transport.is_loop_enabled());
        assert_eq!(transport.loop_region(), (ms(2000), ms(8000)));
    }
}
