// src-tauri/src/timeline.rs
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// 1. THE DATA STRUCTURES (The Lego Blocks)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Clip {
    pub id: String,
    pub track_id: String,
    pub start: f64,    // Start time on timeline (seconds)
    pub duration: f64, // Length of clip (seconds)
    pub source_file: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimelineState {
    pub clips: Vec<Clip>,
    pub duration: f64,
    /// Current playhead position in seconds. Always in range [0, duration].
    pub playhead_time: f64,
    /// Version counter, incremented on every state mutation. Used for change detection.
    pub version: u64,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            clips: vec![],
            duration: 0.0,
            playhead_time: 0.0,
            version: 0,
        }
    }
}

// 2. THE ENGINE (Holds the State safely)
pub struct TimelineEngine {
    // Mutex allows safe access from multiple threads (UI + AI)
    pub state: Mutex<TimelineState>,
}

impl TimelineEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(TimelineState::default()),
        }
    }

    /// Seek to a specific time on the timeline.
    /// Clamps to valid range [0, duration].
    pub fn seek(&self, time: f64) -> f64 {
        let mut state = self.state.lock().unwrap();
        let clamped = time.max(0.0).min(state.duration);
        state.playhead_time = clamped;
        state.version += 1;
        clamped
    }

    /// Get the clip that is active at the given time.
    /// Returns None if no clip exists at that time (gap or empty timeline).
    pub fn get_active_clip(&self, time: f64) -> Option<Clip> {
        let state = self.state.lock().unwrap();
        state
            .clips
            .iter()
            .find(|clip| clip.start <= time && time < clip.start + clip.duration)
            .cloned()
    }

    /// Get the clip at the current playhead position.
    pub fn get_current_clip(&self) -> Option<Clip> {
        let state = self.state.lock().unwrap();
        let time = state.playhead_time;
        state
            .clips
            .iter()
            .find(|clip| clip.start <= time && time < clip.start + clip.duration)
            .cloned()
    }

    /// Increment the version counter. Call this after any state mutation.
    pub fn bump_version(&self) {
        let mut state = self.state.lock().unwrap();
        state.version += 1;
    }

    // Helper to print current state (for debugging)
    #[allow(dead_code)]
    pub fn log_state(&self) {
        let state = self.state.lock().unwrap();
        println!(
            "🎥 CURRENT STATE: {} clips, {:.2}s duration, playhead at {:.2}s, version {}",
            state.clips.len(),
            state.duration,
            state.playhead_time,
            state.version
        );
    }
}
