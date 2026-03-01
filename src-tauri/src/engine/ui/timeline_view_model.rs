//! TimelineViewModel - Read-only, serializable view model for React.
//!
//! # Design
//!
//! TimelineViewModel is a pure projection of TimelineState for UI consumption.
//! It is:
//! - Immutable after creation
//! - Serializable to JSON for React
//! - Decoupled from engine internals
//!
//! # Thread Safety
//!
//! ViewModels are Clone and can be freely passed to UI thread.
//!
//! # No Business Logic
//!
//! This module contains NO business logic. It is purely structural.

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::{Clip, TimelineState, TrackId};

// =============================================================================
// CLIP VIEW
// =============================================================================

/// Read-only view of a clip for UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipView {
    /// Unique clip identifier
    pub id: String,

    /// Track this clip belongs to
    pub track_id: String,

    /// Start position on timeline (nanoseconds as i64 for JS)
    pub start_ns: i64,

    /// Duration (nanoseconds as i64 for JS)
    pub duration_ns: i64,

    /// End position on timeline
    pub end_ns: i64,

    /// Source file path
    pub source_file: String,

    /// Visual position for UI rendering (normalized 0.0-1.0)
    pub normalized_start: f64,

    /// Visual width for UI rendering (normalized 0.0-1.0)
    pub normalized_width: f64,
}

impl ClipView {
    /// Create from engine Clip.
    pub fn from_clip(clip: &Clip, timeline_duration: MediaTime) -> Self {
        let end = clip.start + clip.duration;

        let (normalized_start, normalized_width) = if timeline_duration.is_zero() {
            (0.0, 0.0)
        } else {
            let duration_ns = timeline_duration.as_nanos() as f64;
            (
                clip.start.as_nanos() as f64 / duration_ns,
                clip.duration.as_nanos() as f64 / duration_ns,
            )
        };

        Self {
            id: clip.id.clone(),
            track_id: clip.track_id.clone(),
            start_ns: clip.start.as_nanos(),
            duration_ns: clip.duration.as_nanos(),
            end_ns: end.as_nanos(),
            source_file: clip.source_file.clone(),
            normalized_start,
            normalized_width,
        }
    }
}

// =============================================================================
// TRACK VIEW
// =============================================================================

/// Read-only view of a track for UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackView {
    /// Track identifier
    pub id: String,

    /// Display index (0 = top track)
    pub index: usize,

    /// Clips on this track
    pub clips: Vec<ClipView>,

    /// Track duration (nanoseconds)
    pub duration_ns: i64,
}

impl TrackView {
    /// Create a new track view.
    pub fn new(id: String, index: usize) -> Self {
        Self {
            id,
            index,
            clips: Vec::new(),
            duration_ns: 0,
        }
    }

    /// Add a clip to this track.
    pub fn add_clip(&mut self, clip: ClipView) {
        let clip_end = clip.end_ns;
        self.clips.push(clip);
        self.duration_ns = self.duration_ns.max(clip_end);
    }

    /// Get clip count.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }
}

// =============================================================================
// PLAYHEAD VIEW
// =============================================================================

/// Read-only view of playhead state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayheadView {
    /// Current position (nanoseconds)
    pub position_ns: i64,

    /// Normalized position (0.0-1.0)
    pub normalized_position: f64,

    /// Whether playback is active
    pub is_playing: bool,

    /// Current playback rate (1.0 = normal)
    pub rate: f64,
}

impl PlayheadView {
    /// Create from position and duration.
    pub fn new(position: MediaTime, duration: MediaTime, is_playing: bool, rate: f64) -> Self {
        let normalized = if duration.is_zero() {
            0.0
        } else {
            position.as_nanos() as f64 / duration.as_nanos() as f64
        };

        Self {
            position_ns: position.as_nanos(),
            normalized_position: normalized.clamp(0.0, 1.0),
            is_playing,
            rate,
        }
    }
}

// =============================================================================
// TIMELINE VIEW MODEL
// =============================================================================

/// Complete timeline view model for React.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineViewModel {
    /// All tracks
    pub tracks: Vec<TrackView>,

    /// All clips (flat list for quick access)
    pub clips: Vec<ClipView>,

    /// Timeline duration (nanoseconds)
    pub duration_ns: i64,

    /// Playhead state
    pub playhead: PlayheadView,

    /// Total clip count
    pub clip_count: usize,

    /// Total track count
    pub track_count: usize,

    /// Version counter for change detection
    pub version: u64,
}

impl TimelineViewModel {
    /// Create an empty view model.
    pub fn empty() -> Self {
        Self {
            tracks: Vec::new(),
            clips: Vec::new(),
            duration_ns: 0,
            playhead: PlayheadView::new(MediaTime::ZERO, MediaTime::ZERO, false, 1.0),
            clip_count: 0,
            track_count: 0,
            version: 0,
        }
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Get clip by ID.
    pub fn get_clip(&self, id: &str) -> Option<&ClipView> {
        self.clips.iter().find(|c| c.id == id)
    }

    /// Get track by ID.
    pub fn get_track(&self, id: &str) -> Option<&TrackView> {
        self.tracks.iter().find(|t| t.id == id)
    }
}

// =============================================================================
// BUILD FUNCTION
// =============================================================================

/// Pure function to build view model from state.
///
/// This is the ONLY way to create a TimelineViewModel.
/// It is deterministic: same inputs → same outputs.
pub fn build_view(
    state: &TimelineState,
    playhead_position: MediaTime,
    is_playing: bool,
    rate: f64,
    version: u64,
) -> TimelineViewModel {
    use std::collections::HashMap;

    let duration = state.duration;

    // Build clips
    let clips: Vec<ClipView> = state
        .clips
        .iter()
        .map(|c| ClipView::from_clip(c, duration))
        .collect();

    // Group by track
    let mut track_map: HashMap<String, TrackView> = HashMap::new();
    let mut track_order: Vec<String> = Vec::new();

    for clip in &clips {
        if !track_map.contains_key(&clip.track_id) {
            let index = track_order.len();
            track_order.push(clip.track_id.clone());
            track_map.insert(
                clip.track_id.clone(),
                TrackView::new(clip.track_id.clone(), index),
            );
        }

        if let Some(track) = track_map.get_mut(&clip.track_id) {
            track.add_clip(clip.clone());
        }
    }

    // Convert to ordered vec
    let tracks: Vec<TrackView> = track_order
        .iter()
        .filter_map(|id| track_map.remove(id))
        .collect();

    let playhead = PlayheadView::new(playhead_position, duration, is_playing, rate);

    TimelineViewModel {
        track_count: tracks.len(),
        tracks,
        clip_count: clips.len(),
        clips,
        duration_ns: duration.as_nanos(),
        playhead,
        version,
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
    fn test_clip_view_from_clip() {
        let clip = make_clip("c1", "t1", 0, 5000);
        let duration = ms(10000);

        let view = ClipView::from_clip(&clip, duration);

        assert_eq!(view.id, "c1");
        assert_eq!(view.start_ns, 0);
        assert_eq!(view.duration_ns, 5_000_000_000);
        assert_eq!(view.normalized_start, 0.0);
        assert_eq!(view.normalized_width, 0.5); // 5000ms / 10000ms
    }

    #[test]
    fn test_build_view_matches_state() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
            make_clip("c3", "t2", 0, 10000),
        ]);

        let view = build_view(&state, ms(0), false, 1.0, 1);

        // Check clip count matches
        assert_eq!(view.clip_count, state.clips.len());

        // Check all clips present
        assert!(view.get_clip("c1").is_some());
        assert!(view.get_clip("c2").is_some());
        assert!(view.get_clip("c3").is_some());

        // Check tracks
        assert_eq!(view.track_count, 2); // t1 and t2

        // Check duration
        assert_eq!(view.duration_ns, ms(10000).as_nanos());
    }

    #[test]
    fn test_view_model_serializable() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 5000)]);
        let view = build_view(&state, ms(1000), true, 1.5, 42);

        // Serialize to JSON
        let json = serde_json::to_string(&view).unwrap();

        // Deserialize back
        let restored: TimelineViewModel = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, view);
        assert_eq!(restored.playhead.position_ns, 1_000_000_000);
        assert_eq!(restored.playhead.is_playing, true);
        assert_eq!(restored.playhead.rate, 1.5);
        assert_eq!(restored.version, 42);
    }

    #[test]
    fn test_empty_view_model() {
        let view = TimelineViewModel::empty();

        assert!(view.is_empty());
        assert_eq!(view.clip_count, 0);
        assert_eq!(view.track_count, 0);
    }

    #[test]
    fn test_playhead_view() {
        let playhead = PlayheadView::new(ms(2500), ms(10000), true, 2.0);

        assert_eq!(playhead.position_ns, 2_500_000_000);
        assert_eq!(playhead.normalized_position, 0.25);
        assert!(playhead.is_playing);
        assert_eq!(playhead.rate, 2.0);
    }

    #[test]
    fn test_deterministic_build() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t2", 0, 5000),
        ]);

        // Build twice with same inputs
        let view1 = build_view(&state, ms(1000), false, 1.0, 1);
        let view2 = build_view(&state, ms(1000), false, 1.0, 1);

        // Should be identical
        assert_eq!(view1, view2);
    }
}
