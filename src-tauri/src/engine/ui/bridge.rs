//! UIBridge - Engine-to-UI bridge.
//!
//! # Design
//!
//! UIBridge connects the engine to the UI layer. It:
//! - Listens to engine state changes
//! - Listens to playback ticks
//! - Emits UIEvents to React
//!
//! # One-Way Flow
//!
//! Data flows engine → UI only. The UI sends commands back via Tauri commands,
//! NOT through the bridge.
//!
//! # Thread Safety
//!
//! UIBridge is designed for single-threaded use on the engine side.
//! Events are sent via channels that are safe across threads.

use crate::engine::media_time::MediaTime;
use crate::engine::playback::PlaybackScheduler;
use crate::engine::timeline_state::TimelineState;

use super::timeline_view_model::{build_view, TimelineViewModel};
use super::ui_events::{UIEventSender, UpdateReason};

// =============================================================================
// BRIDGE CONFIG
// =============================================================================

/// Configuration for the UI bridge.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Minimum interval between playhead updates (nanos)
    pub playhead_throttle_ns: i64,

    /// Whether to send full view model on playhead tick
    pub full_update_on_tick: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            playhead_throttle_ns: 16_666_667, // ~60fps
            full_update_on_tick: false,
        }
    }
}

// =============================================================================
// BRIDGE STATS
// =============================================================================

/// Statistics about bridge operation.
#[derive(Debug, Clone, Default)]
pub struct BridgeStats {
    /// Total state updates sent
    pub state_updates: u64,

    /// Total playhead updates sent
    pub playhead_updates: u64,

    /// Updates throttled (skipped)
    pub throttled: u64,

    /// Current version
    pub version: u64,
}

// =============================================================================
// UI BRIDGE
// =============================================================================

/// Bridge between engine and UI.
///
/// # Usage
///
/// ```ignore
/// let (tx, rx) = ui_event_channel();
/// let mut bridge = UIBridge::new(tx, config);
///
/// // After state mutation
/// bridge.on_state_changed(&state, UpdateReason::ClipAdded);
///
/// // On playback tick
/// bridge.on_playhead_tick(&scheduler);
/// ```
#[derive(Debug)]
pub struct UIBridge {
    /// Event sender
    sender: UIEventSender,

    /// Configuration
    config: BridgeConfig,

    /// Statistics
    stats: BridgeStats,

    /// Last playhead position sent
    last_playhead_ns: i64,

    /// Last update time (nanos)
    last_update_ns: i64,

    /// Cached duration for normalized calculations
    cached_duration: MediaTime,
}

impl UIBridge {
    /// Create a new UI bridge.
    pub fn new(sender: UIEventSender, config: BridgeConfig) -> Self {
        Self {
            sender,
            config,
            stats: BridgeStats::default(),
            last_playhead_ns: 0,
            last_update_ns: 0,
            cached_duration: MediaTime::ZERO,
        }
    }

    /// Create with default config.
    pub fn with_sender(sender: UIEventSender) -> Self {
        Self::new(sender, BridgeConfig::default())
    }

    // =========================================================================
    // STATE CHANGES
    // =========================================================================

    /// Called when state changes.
    ///
    /// Builds a new view model and emits StateUpdated event.
    pub fn on_state_changed(
        &mut self,
        state: &TimelineState,
        playback: &PlaybackScheduler,
        reason: UpdateReason,
    ) {
        self.stats.version += 1;
        self.stats.state_updates += 1;

        // Cache duration for playhead calculations
        self.cached_duration = state.duration;

        // Build view model
        let view_model = build_view(
            state,
            playback.position(),
            playback.is_playing(),
            playback.rate().to_f64(),
            self.stats.version,
        );

        // Emit event
        self.sender.emit_state_updated(view_model, reason);
    }

    /// Called after any mutation commits.
    pub fn on_mutation_committed(
        &mut self,
        state: &TimelineState,
        playback: &PlaybackScheduler,
        reason: UpdateReason,
    ) {
        self.on_state_changed(state, playback, reason);
    }

    // =========================================================================
    // PLAYBACK
    // =========================================================================

    /// Called on playback tick.
    ///
    /// Emits PlayheadMoved event if enough time has passed.
    pub fn on_playhead_tick(&mut self, playback: &PlaybackScheduler) {
        let position = playback.position();
        let position_ns = position.as_nanos();

        // Throttle updates
        let delta = (position_ns - self.last_playhead_ns).abs();
        if delta < self.config.playhead_throttle_ns {
            self.stats.throttled += 1;
            return;
        }

        self.last_playhead_ns = position_ns;
        self.stats.playhead_updates += 1;

        // Emit playhead event
        self.sender
            .emit_playhead_moved(position, self.cached_duration, playback.is_playing());
    }

    /// Force a playhead update (ignores throttle).
    pub fn force_playhead_update(&mut self, playback: &PlaybackScheduler) {
        let position = playback.position();
        self.last_playhead_ns = position.as_nanos();
        self.stats.playhead_updates += 1;

        self.sender
            .emit_playhead_moved(position, self.cached_duration, playback.is_playing());
    }

    /// Called when playback state changes.
    pub fn on_playback_state_changed(&mut self, is_playing: bool, rate: f64) {
        self.sender.emit_playback_state_changed(is_playing, rate);
    }

    /// Called when seek completes.
    pub fn on_seek_completed(&mut self, position: MediaTime) {
        self.last_playhead_ns = position.as_nanos();
        self.sender.emit_seek_completed(position);
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Get current view model (snapshot).
    pub fn get_view_model(
        &self,
        state: &TimelineState,
        playback: &PlaybackScheduler,
    ) -> TimelineViewModel {
        build_view(
            state,
            playback.position(),
            playback.is_playing(),
            playback.rate().to_f64(),
            self.stats.version,
        )
    }

    /// Get statistics.
    pub fn stats(&self) -> &BridgeStats {
        &self.stats
    }

    /// Get current version.
    pub fn version(&self) -> u64 {
        self.stats.version
    }

    /// Reset throttle state.
    pub fn reset_throttle(&mut self) {
        self.last_playhead_ns = 0;
        self.last_update_ns = 0;
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;
    use crate::engine::ui::ui_events::{ui_event_channel, UIEvent};

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
        state.recalculate_duration();
        state
    }

    #[test]
    fn test_bridge_creation() {
        let (tx, _rx) = ui_event_channel();
        let bridge = UIBridge::with_sender(tx);

        assert_eq!(bridge.version(), 0);
    }

    #[test]
    fn test_mutation_emits_event() {
        let (tx, rx) = ui_event_channel();
        let mut bridge = UIBridge::with_sender(tx);

        let state = make_state(vec![make_clip("c1", "t1", 0, 5000)]);
        let playback = PlaybackScheduler::with_duration(ms(5000));

        // Emit state changed
        bridge.on_mutation_committed(&state, &playback, UpdateReason::ClipAdded);

        // Check event received
        let event = rx.recv().unwrap();

        match event {
            UIEvent::StateUpdated { view_model, reason } => {
                assert_eq!(view_model.clip_count, 1);
                assert_eq!(reason, UpdateReason::ClipAdded);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_playhead_tick_emits_event() {
        let (tx, rx) = ui_event_channel();
        let config = BridgeConfig {
            playhead_throttle_ns: 0, // Disable throttle for test
            ..Default::default()
        };
        let mut bridge = UIBridge::new(tx, config);

        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let mut playback = PlaybackScheduler::with_duration(ms(10000));
        playback.seek(ms(5000));

        // Set cached duration
        bridge.cached_duration = state.duration;

        // Tick
        bridge.on_playhead_tick(&playback);

        // Check event
        let event = rx.recv().unwrap();

        match event {
            UIEvent::PlayheadMoved {
                position_ns,
                normalized,
                ..
            } => {
                assert_eq!(position_ns, 5_000_000_000);
                assert!((normalized - 0.5).abs() < 0.01);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_throttling() {
        let (tx, rx) = ui_event_channel();
        let config = BridgeConfig {
            playhead_throttle_ns: 100_000_000, // 100ms
            ..Default::default()
        };
        let mut bridge = UIBridge::new(tx, config);

        let state = make_state(vec![make_clip("c1", "t1", 0, 10000)]);
        let mut playback = PlaybackScheduler::with_duration(ms(10000));

        bridge.cached_duration = state.duration;

        // First tick at 1 second - should emit (delta from 0 is 1s > 100ms)
        playback.seek(ms(1000));
        bridge.on_playhead_tick(&playback);

        // Second tick at 1.05 second - should be throttled (delta 50ms < 100ms)
        playback.seek(ms(1050));
        bridge.on_playhead_tick(&playback);

        // Only one event should be received
        assert!(rx.try_recv().is_some());
        assert!(rx.try_recv().is_none());
        assert_eq!(bridge.stats().throttled, 1);
    }

    #[test]
    fn test_no_mutations_from_ui() {
        // This test verifies the design: UIBridge only has read access to state
        let (tx, _rx) = ui_event_channel();
        let bridge = UIBridge::with_sender(tx);

        // Bridge takes &TimelineState (immutable reference)
        // There is no method that takes &mut TimelineState
        // This is enforced by the type system

        // The only way to mutate state is through TimelineEngine,
        // which is accessed via Tauri commands, not the bridge

        assert_eq!(bridge.version(), 0);
    }

    #[test]
    fn test_get_view_model() {
        let (tx, _rx) = ui_event_channel();
        let bridge = UIBridge::with_sender(tx);

        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
        ]);
        let playback = PlaybackScheduler::with_duration(ms(10000));

        let view = bridge.get_view_model(&state, &playback);

        assert_eq!(view.clip_count, 2);
        assert_eq!(view.track_count, 1);
    }
}
