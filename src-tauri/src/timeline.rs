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
}

// 2. THE ENGINE (Holds the State safely)
pub struct TimelineEngine {
    // Mutex allows safe access from multiple threads (UI + AI)
    pub state: Mutex<TimelineState>,
}

impl TimelineEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(TimelineState {
                clips: vec![], // Start empty
                duration: 0.0,
            }),
        }
    }

    // Helper to print current state (for debugging)
    #[allow(dead_code)]
    pub fn log_state(&self) {
        let state = self.state.lock().unwrap();
        println!(
            "🎥 CURRENT STATE: {} clips, {:.2}s duration",
            state.clips.len(),
            state.duration
        );
    }
}
