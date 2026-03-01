//! TimelineView - Query clips at current position.
//!
//! # Design
//!
//! TimelineView provides a read-only view of clips at a given timeline position.
//! It uses TimelineIndex for O(log n) queries and does NOT mutate TimelineState.
//!
//! # Thread Safety
//!
//! TimelineView is immutable after creation and can be shared across threads.

use std::sync::Arc;

use crate::engine::interval_tree::TimeRange;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::{Clip, ClipId, TimelineState, TrackId};

// =============================================================================
// VISIBLE CLIP
// =============================================================================

/// Information about a clip visible at a specific time.
#[derive(Debug, Clone)]
pub struct VisibleClip {
    /// Clip ID
    pub id: ClipId,

    /// Track ID
    pub track_id: TrackId,

    /// Clip start on timeline
    pub start: MediaTime,

    /// Clip duration
    pub duration: MediaTime,

    /// Clip end on timeline
    pub end: MediaTime,

    /// Source file path
    pub source_file: String,

    /// Offset into clip (how far into the clip we are)
    pub playback_offset: MediaTime,
}

impl VisibleClip {
    /// Create from Clip and current position.
    pub fn from_clip(clip: &Clip, current_time: MediaTime) -> Self {
        let end = clip.start + clip.duration;
        let playback_offset = current_time - clip.start;

        Self {
            id: clip.id.clone(),
            track_id: clip.track_id.clone(),
            start: clip.start,
            duration: clip.duration,
            end,
            source_file: clip.source_file.clone(),
            playback_offset,
        }
    }
}

// =============================================================================
// TIMELINE VIEW
// =============================================================================

/// Read-only view of timeline clips at a position.
///
/// # Performance
///
/// Uses TimelineIndex for O(log n + k) queries where k = number of clips visible.
#[derive(Debug, Clone)]
pub struct TimelineView {
    /// Current position being viewed
    position: MediaTime,

    /// Clips visible at this position
    visible_clips: Vec<VisibleClip>,

    /// Clip IDs visible at this position (for quick lookup)
    visible_ids: Vec<ClipId>,
}

impl TimelineView {
    /// Create a view at the given position using index and state.
    pub fn at_position(position: MediaTime, index: &TimelineIndex, state: &TimelineState) -> Self {
        // Query index for clip IDs at this position
        let visible_ids = index.clips_at(position);

        // Build visible clips from IDs
        let visible_clips: Vec<_> = visible_ids
            .iter()
            .filter_map(|id| {
                state
                    .get_clip(id)
                    .map(|clip| VisibleClip::from_clip(clip, position))
            })
            .collect();

        Self {
            position,
            visible_clips,
            visible_ids,
        }
    }

    /// Create a view over a range using index and state.
    pub fn in_range(range: TimeRange, index: &TimelineIndex, state: &TimelineState) -> Self {
        let visible_ids = index.clips_in_range(range);

        let visible_clips: Vec<_> = visible_ids
            .iter()
            .filter_map(|id| {
                state
                    .get_clip(id)
                    .map(|clip| VisibleClip::from_clip(clip, range.start))
            })
            .collect();

        Self {
            position: range.start,
            visible_clips,
            visible_ids,
        }
    }

    /// Create a view on a specific track at position.
    pub fn on_track_at_position(
        track_id: &TrackId,
        position: MediaTime,
        index: &TimelineIndex,
        state: &TimelineState,
    ) -> Self {
        // Create a tiny range for point query
        let range = TimeRange::new(position, position + MediaTime::from_nanos(1));
        let visible_ids = index.clips_on_track_in_range(track_id, range);

        let visible_clips: Vec<_> = visible_ids
            .iter()
            .filter_map(|id| {
                state
                    .get_clip(id)
                    .map(|clip| VisibleClip::from_clip(clip, position))
            })
            .collect();

        Self {
            position,
            visible_clips,
            visible_ids,
        }
    }

    /// Get the viewing position.
    pub fn position(&self) -> MediaTime {
        self.position
    }

    /// Get all visible clips.
    pub fn clips(&self) -> &[VisibleClip] {
        &self.visible_clips
    }

    /// Get visible clip IDs.
    pub fn clip_ids(&self) -> &[ClipId] {
        &self.visible_ids
    }

    /// Get number of visible clips.
    pub fn len(&self) -> usize {
        self.visible_clips.len()
    }

    /// Check if no clips are visible.
    pub fn is_empty(&self) -> bool {
        self.visible_clips.is_empty()
    }

    /// Get clip by ID from this view.
    pub fn get(&self, id: &ClipId) -> Option<&VisibleClip> {
        self.visible_clips.iter().find(|c| &c.id == id)
    }

    /// Get clips grouped by track.
    pub fn by_track(&self) -> std::collections::HashMap<TrackId, Vec<&VisibleClip>> {
        use std::collections::HashMap;

        let mut by_track: HashMap<TrackId, Vec<&VisibleClip>> = HashMap::new();

        for clip in &self.visible_clips {
            by_track
                .entry(clip.track_id.clone())
                .or_default()
                .push(clip);
        }

        by_track
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
        state
    }

    #[test]
    fn test_timeline_view_at_position() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
            make_clip("c3", "t2", 0, 10000),
        ]);
        let index = TimelineIndex::build(&state);

        // At position 2500ms, c1 and c3 should be visible
        let view = TimelineView::at_position(ms(2500), &index, &state);

        assert_eq!(view.len(), 2);
        assert!(view.clip_ids().contains(&"c1".to_string()));
        assert!(view.clip_ids().contains(&"c3".to_string()));
    }

    #[test]
    fn test_timeline_view_playback_offset() {
        let state = make_state(vec![make_clip("c1", "t1", 1000, 5000)]);
        let index = TimelineIndex::build(&state);

        // At position 3000ms, we're 2000ms into clip c1
        let view = TimelineView::at_position(ms(3000), &index, &state);

        assert_eq!(view.len(), 1);
        let clip = view.get(&"c1".to_string()).unwrap();
        assert_eq!(clip.playback_offset, ms(2000));
    }

    #[test]
    fn test_timeline_view_empty() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 5000)]);
        let index = TimelineIndex::build(&state);

        // At position 10000ms, no clips
        let view = TimelineView::at_position(ms(10000), &index, &state);

        assert!(view.is_empty());
    }

    #[test]
    fn test_timeline_view_matches_linear_scan() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t1", 5000, 5000),
            make_clip("c3", "t2", 2000, 6000),
            make_clip("c4", "t3", 0, 3000),
        ]);
        let index = TimelineIndex::build(&state);

        // Test many positions
        for t in (0..12000).step_by(500) {
            let position = ms(t);

            // Index-based view
            let view = TimelineView::at_position(position, &index, &state);
            let index_ids: std::collections::HashSet<_> = view.clip_ids().iter().cloned().collect();

            // Linear scan
            let linear_ids: std::collections::HashSet<_> = state
                .clips
                .iter()
                .filter(|c| {
                    let end = c.start + c.duration;
                    position >= c.start && position < end
                })
                .map(|c| c.id.clone())
                .collect();

            assert_eq!(index_ids, linear_ids, "Mismatch at position {}ms", t);
        }
    }

    #[test]
    fn test_timeline_view_by_track() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 5000),
            make_clip("c2", "t2", 0, 5000),
            make_clip("c3", "t1", 0, 3000),
        ]);
        let index = TimelineIndex::build(&state);

        let view = TimelineView::at_position(ms(1000), &index, &state);
        let by_track = view.by_track();

        assert_eq!(by_track.get("t1").map(|v| v.len()), Some(2));
        assert_eq!(by_track.get("t2").map(|v| v.len()), Some(1));
    }
}
