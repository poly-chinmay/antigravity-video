//! TimelineState - Pure data structures for timeline representation.
//!
//! # Architectural Invariant
//!
//! TimelineState contains DATA ONLY. No business logic.
//! All mutations must go through TimelineEngine.

use crate::engine::media_time::MediaTime;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Unique identifier for a clip. Generated once, never reused.
pub type ClipId = String;

/// Unique identifier for a track.
pub type TrackId = String;

/// A single media segment on the timeline.
///
/// # Invariants
///
/// - `id` is unique across all clips in the timeline
/// - `start >= MediaTime::ZERO`
/// - `duration > MediaTime::ZERO`
/// - `source_file` is non-empty
/// - `source_in >= MediaTime::ZERO`
/// - `source_out <= source_duration`
/// - `source_out > source_in`
/// - `duration == source_out - source_in`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    /// Immutable unique identifier (UUID v7)
    pub id: ClipId,

    /// Track this clip belongs to
    pub track_id: TrackId,

    /// Start position on timeline (integer nanoseconds)
    /// INVARIANT: start >= 0
    pub start: MediaTime,

    /// Duration of clip (integer nanoseconds)
    /// INVARIANT: duration > 0
    /// NOTE: Always equals (source_out - source_in)
    pub duration: MediaTime,

    /// Path to source media file
    /// INVARIANT: Non-empty string
    pub source_file: String,

    // ===== Source Metadata (for trim bounds) =====
    /// Total duration of the source media file
    /// Used to enforce trim boundaries
    pub source_duration: MediaTime,

    /// In-point within source media (where clip starts in source)
    /// INVARIANT: source_in >= 0
    pub source_in: MediaTime,

    /// Out-point within source media (where clip ends in source)
    /// INVARIANT: source_out <= source_duration, source_out > source_in
    pub source_out: MediaTime,
}

impl Clip {
    /// Create a new clip with basic parameters.
    ///
    /// Sets source bounds to use full duration (source_in=0, source_out=duration).
    pub fn new(
        id: impl Into<String>,
        track_id: impl Into<String>,
        start: MediaTime,
        duration: MediaTime,
        source_file: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            track_id: track_id.into(),
            start,
            duration,
            source_file: source_file.into(),
            source_duration: duration,
            source_in: MediaTime::ZERO,
            source_out: duration,
        }
    }

    /// Create a clip from a media source with full duration.
    ///
    /// The clip will use the entire source from 0 to source_duration.
    pub fn from_source(
        track_id: impl Into<String>,
        start: MediaTime,
        source_file: impl Into<String>,
        source_duration: MediaTime,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: track_id.into(),
            start,
            duration: source_duration,
            source_file: source_file.into(),
            source_duration,
            source_in: MediaTime::ZERO,
            source_out: source_duration,
        }
    }

    /// Create a clip from a media source with custom in/out points.
    ///
    /// # Panics
    /// Panics if source_in >= source_out or source_out > source_duration.
    pub fn from_source_with_range(
        track_id: impl Into<String>,
        start: MediaTime,
        source_file: impl Into<String>,
        source_duration: MediaTime,
        source_in: MediaTime,
        source_out: MediaTime,
    ) -> Self {
        assert!(source_out > source_in, "source_out must be > source_in");
        assert!(
            source_out <= source_duration,
            "source_out must be <= source_duration"
        );
        assert!(!source_in.is_negative(), "source_in must be >= 0");

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: track_id.into(),
            start,
            duration: source_out - source_in,
            source_file: source_file.into(),
            source_duration,
            source_in,
            source_out,
        }
    }

    /// End time of this clip (start + duration).
    ///
    /// # Note
    /// This is computed, not stored, to avoid sync issues.
    #[inline]
    pub fn end(&self) -> MediaTime {
        self.start + self.duration
    }

    /// Check if this clip overlaps with a time range.
    #[inline]
    pub fn overlaps(&self, start: MediaTime, end: MediaTime) -> bool {
        self.start < end && self.end() > start
    }

    /// Check if this clip overlaps with another clip.
    #[inline]
    pub fn overlaps_clip(&self, other: &Clip) -> bool {
        self.track_id == other.track_id && self.overlaps(other.start, other.end())
    }
}

/// The God State — single source of truth for all timeline data.
///
/// # Architectural Invariants
///
/// 1. This struct contains DATA ONLY. No mutation methods.
/// 2. All fields must be serializable for snapshots.
/// 3. Indices are derived and rebuildable from `clips`.
/// 4. Only TimelineEngine may modify this struct.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelineState {
    // ===== Primary Data =====
    /// All clips in the timeline (authoritative source)
    pub clips: Vec<Clip>,

    /// Computed total duration (updated after mutations)
    pub duration: MediaTime,

    /// Current version number (incremented on each mutation)
    pub version: u64,

    // ===== Indices (Derived, Rebuildable) =====
    /// ClipId → index in clips vector
    #[serde(skip)]
    pub clip_id_index: HashMap<ClipId, usize>,

    /// TrackId → (start_nanos → clip_index)
    #[serde(skip)]
    pub track_index: HashMap<TrackId, BTreeMap<i64, usize>>,
}

impl TimelineState {
    /// Create empty timeline state.
    pub fn new() -> Self {
        Self {
            clips: Vec::new(),
            duration: MediaTime::ZERO,
            version: 0,
            clip_id_index: HashMap::new(),
            track_index: HashMap::new(),
        }
    }

    /// Rebuild all indices from clips vector.
    ///
    /// MUST be called after:
    /// - Deserialization
    /// - Any mutation that changes clip positions or count
    pub fn rebuild_indices(&mut self) {
        self.clip_id_index.clear();
        self.track_index.clear();

        for (idx, clip) in self.clips.iter().enumerate() {
            self.clip_id_index.insert(clip.id.clone(), idx);
            self.track_index
                .entry(clip.track_id.clone())
                .or_default()
                .insert(clip.start.as_nanos(), idx);
        }
    }

    /// Recalculate duration from clips.
    pub fn recalculate_duration(&mut self) {
        self.duration = self
            .clips
            .iter()
            .map(|c| c.end())
            .fold(MediaTime::ZERO, MediaTime::max);
    }

    /// Get a clip by ID.
    pub fn get_clip(&self, id: &ClipId) -> Option<&Clip> {
        self.clip_id_index
            .get(id)
            .and_then(|&idx| self.clips.get(idx))
    }

    /// Get a mutable reference to a clip by ID.
    pub fn get_clip_mut(&mut self, id: &ClipId) -> Option<&mut Clip> {
        self.clip_id_index
            .get(id)
            .cloned()
            .and_then(move |idx| self.clips.get_mut(idx))
    }

    /// Get all clips on a specific track.
    pub fn clips_on_track(&self, track_id: &TrackId) -> Vec<&Clip> {
        self.clips
            .iter()
            .filter(|c| c.track_id == *track_id)
            .collect()
    }

    /// Get clip count.
    #[inline]
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clip(id: &str, start_secs: f64, duration_secs: f64) -> Clip {
        Clip::new(
            id,
            "track1",
            MediaTime::from_seconds(start_secs),
            MediaTime::from_seconds(duration_secs),
            "test.mp4",
        )
    }

    #[test]
    fn test_clip_end() {
        let clip = make_clip("c1", 10.0, 5.0);
        assert_eq!(clip.end().to_seconds(), 15.0);
    }

    #[test]
    fn test_clip_overlap() {
        let clip = make_clip("c1", 10.0, 5.0);

        // Overlapping cases
        assert!(clip.overlaps(MediaTime::from_seconds(12.0), MediaTime::from_seconds(20.0)));
        assert!(clip.overlaps(MediaTime::from_seconds(5.0), MediaTime::from_seconds(12.0)));
        assert!(clip.overlaps(MediaTime::from_seconds(11.0), MediaTime::from_seconds(14.0)));

        // Non-overlapping cases
        assert!(!clip.overlaps(MediaTime::from_seconds(0.0), MediaTime::from_seconds(10.0)));
        assert!(!clip.overlaps(MediaTime::from_seconds(15.0), MediaTime::from_seconds(20.0)));
    }

    #[test]
    fn test_rebuild_indices() {
        let mut state = TimelineState::new();
        state.clips.push(make_clip("c1", 0.0, 5.0));
        state.clips.push(make_clip("c2", 10.0, 5.0));

        state.rebuild_indices();

        assert_eq!(state.clip_id_index.get("c1"), Some(&0));
        assert_eq!(state.clip_id_index.get("c2"), Some(&1));
    }

    #[test]
    fn test_recalculate_duration() {
        let mut state = TimelineState::new();
        state.clips.push(make_clip("c1", 0.0, 5.0));
        state.clips.push(make_clip("c2", 10.0, 5.0));

        state.recalculate_duration();

        assert_eq!(state.duration.to_seconds(), 15.0);
    }
}
