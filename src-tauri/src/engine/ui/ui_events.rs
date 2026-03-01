//! UIEvents - Event types and channels for engine-to-UI communication.
//!
//! # Design
//!
//! UIEvents are emitted by the engine and consumed by the UI layer.
//! They are one-way: engine → UI only.
//!
//! # Thread Safety
//!
//! Events use crossbeam channels for safe cross-thread communication.

use std::sync::mpsc;

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;

use super::timeline_view_model::TimelineViewModel;

// =============================================================================
// UI EVENT
// =============================================================================

/// Events emitted from engine to UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UIEvent {
    /// Timeline state was updated
    StateUpdated {
        /// New view model
        view_model: TimelineViewModel,
        /// What caused the update
        reason: UpdateReason,
    },
    
    /// Playhead position changed
    PlayheadMoved {
        /// New position (nanoseconds)
        position_ns: i64,
        /// Normalized position (0.0-1.0)
        normalized: f64,
        /// Is playing
        is_playing: bool,
    },
    
    /// Playback state changed
    PlaybackStateChanged {
        /// Is now playing
        is_playing: bool,
        /// Current rate
        rate: f64,
    },
    
    /// Seek occurred
    SeekCompleted {
        /// New position (nanoseconds)
        position_ns: i64,
    },
    
    /// Selection changed
    SelectionChanged {
        /// Selected clip IDs
        selected_clips: Vec<String>,
        /// Selected track IDs
        selected_tracks: Vec<String>,
    },
    
    /// Error occurred
    Error {
        /// Error message
        message: String,
        /// Error code
        code: String,
    },
}

// =============================================================================
// UPDATE REASON
// =============================================================================

/// Reason for a state update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateReason {
    /// Initial load
    Initial,
    /// Clip was added
    ClipAdded,
    /// Clip was removed
    ClipRemoved,
    /// Clip was moved
    ClipMoved,
    /// Clip was trimmed
    ClipTrimmed,
    /// Undo operation
    Undo,
    /// Redo operation
    Redo,
    /// External sync (recovery, etc)
    External,
}

// =============================================================================
// EVENT SENDER
// =============================================================================

/// Sender for UI events.
#[derive(Debug, Clone)]
pub struct UIEventSender {
    /// Channel sender
    tx: mpsc::Sender<UIEvent>,
}

impl UIEventSender {
    /// Create a new sender.
    pub fn new(tx: mpsc::Sender<UIEvent>) -> Self {
        Self { tx }
    }
    
    /// Send an event.
    pub fn send(&self, event: UIEvent) -> Result<(), mpsc::SendError<UIEvent>> {
        self.tx.send(event)
    }
    
    /// Send state updated event.
    pub fn emit_state_updated(&self, view_model: TimelineViewModel, reason: UpdateReason) {
        let _ = self.send(UIEvent::StateUpdated { view_model, reason });
    }
    
    /// Send playhead moved event.
    pub fn emit_playhead_moved(&self, position: MediaTime, duration: MediaTime, is_playing: bool) {
        let normalized = if duration.is_zero() {
            0.0
        } else {
            (position.as_nanos() as f64 / duration.as_nanos() as f64).clamp(0.0, 1.0)
        };
        
        let _ = self.send(UIEvent::PlayheadMoved {
            position_ns: position.as_nanos(),
            normalized,
            is_playing,
        });
    }
    
    /// Send playback state changed event.
    pub fn emit_playback_state_changed(&self, is_playing: bool, rate: f64) {
        let _ = self.send(UIEvent::PlaybackStateChanged { is_playing, rate });
    }
    
    /// Send seek completed event.
    pub fn emit_seek_completed(&self, position: MediaTime) {
        let _ = self.send(UIEvent::SeekCompleted {
            position_ns: position.as_nanos(),
        });
    }
    
    /// Send error event.
    pub fn emit_error(&self, message: String, code: String) {
        let _ = self.send(UIEvent::Error { message, code });
    }
}

// =============================================================================
// EVENT RECEIVER
// =============================================================================

/// Receiver for UI events.
#[derive(Debug)]
pub struct UIEventReceiver {
    /// Channel receiver
    rx: mpsc::Receiver<UIEvent>,
}

impl UIEventReceiver {
    /// Create a new receiver.
    pub fn new(rx: mpsc::Receiver<UIEvent>) -> Self {
        Self { rx }
    }
    
    /// Try to receive an event (non-blocking).
    pub fn try_recv(&self) -> Option<UIEvent> {
        self.rx.try_recv().ok()
    }
    
    /// Receive an event (blocking).
    pub fn recv(&self) -> Option<UIEvent> {
        self.rx.recv().ok()
    }
    
    /// Drain all pending events.
    pub fn drain(&self) -> Vec<UIEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.try_recv() {
            events.push(event);
        }
        events
    }
}

// =============================================================================
// CHANNEL CREATION
// =============================================================================

/// Create a new UI event channel.
pub fn ui_event_channel() -> (UIEventSender, UIEventReceiver) {
    let (tx, rx) = mpsc::channel();
    (UIEventSender::new(tx), UIEventReceiver::new(rx))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ui::timeline_view_model::TimelineViewModel;
    
    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }
    
    #[test]
    fn test_channel_creation() {
        let (tx, rx) = ui_event_channel();
        
        // Send an event
        tx.emit_playhead_moved(ms(1000), ms(10000), true);
        
        // Receive it
        let event = rx.recv().unwrap();
        
        match event {
            UIEvent::PlayheadMoved { position_ns, normalized, is_playing } => {
                assert_eq!(position_ns, 1_000_000_000);
                assert!((normalized - 0.1).abs() < 0.001);
                assert!(is_playing);
            }
            _ => panic!("Wrong event type"),
        }
    }
    
    #[test]
    fn test_state_updated_event() {
        let (tx, rx) = ui_event_channel();
        
        let view = TimelineViewModel::empty();
        tx.emit_state_updated(view.clone(), UpdateReason::ClipAdded);
        
        let event = rx.recv().unwrap();
        
        match event {
            UIEvent::StateUpdated { view_model, reason } => {
                assert_eq!(view_model, view);
                assert_eq!(reason, UpdateReason::ClipAdded);
            }
            _ => panic!("Wrong event type"),
        }
    }
    
    #[test]
    fn test_drain_events() {
        let (tx, rx) = ui_event_channel();
        
        tx.emit_playhead_moved(ms(100), ms(1000), false);
        tx.emit_playhead_moved(ms(200), ms(1000), false);
        tx.emit_playhead_moved(ms(300), ms(1000), false);
        
        let events = rx.drain();
        
        assert_eq!(events.len(), 3);
    }
    
    #[test]
    fn test_event_serializable() {
        let event = UIEvent::PlayheadMoved {
            position_ns: 1_000_000_000,
            normalized: 0.5,
            is_playing: true,
        };
        
        let json = serde_json::to_string(&event).unwrap();
        let restored: UIEvent = serde_json::from_str(&json).unwrap();
        
        match restored {
            UIEvent::PlayheadMoved { position_ns, .. } => {
                assert_eq!(position_ns, 1_000_000_000);
            }
            _ => panic!("Wrong event type"),
        }
    }
}
