//! TimelineIndex - High-performance timeline query engine.
//!
//! # Design
//!
//! This module provides O(log n) queries and overlap detection by wrapping
//! the IntervalTree with timeline-specific logic.
//!
//! # Invariants
//!
//! - Index is DERIVED, never authoritative
//! - Index can be rebuilt from TimelineState at any time
//! - Index updates happen AFTER state mutation succeeds
//! - If index update fails, full rebuild is triggered

use std::collections::HashMap;
use std::time::Instant;

use crate::engine::interval_tree::{IntervalEntry, IntervalTree, TimeRange};
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::{Clip, ClipId, TimelineState, TrackId};

// =============================================================================
// INDEX STATISTICS
// =============================================================================

/// Statistics for monitoring index performance.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Total clips indexed
    pub total_clips: usize,

    /// Total tracks indexed
    pub total_tracks: usize,

    /// Last rebuild duration in nanoseconds
    pub last_rebuild_ns: u64,

    /// Number of incremental updates since last rebuild
    pub incremental_updates: u64,
}

// =============================================================================
// INDEX ERROR
// =============================================================================

/// Errors that can occur during index operations.
#[derive(Debug, Clone)]
pub enum IndexError {
    /// Clip not found for removal
    ClipNotFound(ClipId),

    /// Track not found
    TrackNotFound(TrackId),

    /// Index is in an inconsistent state
    InconsistentState(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClipNotFound(id) => write!(f, "Clip not found: {}", id),
            Self::TrackNotFound(id) => write!(f, "Track not found: {}", id),
            Self::InconsistentState(msg) => write!(f, "Inconsistent state: {}", msg),
        }
    }
}

impl std::error::Error for IndexError {}

// =============================================================================
// TIMELINE INDEX
// =============================================================================

/// High-performance timeline index for O(log n) queries.
///
/// # Usage
///
/// ```ignore
/// let index = TimelineIndex::build(&state);
///
/// // Query clips at a specific time
/// let clips = index.clips_at(MediaTime::from_seconds(5.0));
///
/// // Check for overlaps before adding
/// let range = TimeRange::new(start, end);
/// if index.has_overlap_on_track(&track_id, range, None) {
///     return Err("Clips would overlap");
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct TimelineIndex {
    /// Global interval tree (all clips, all tracks)
    all_clips: IntervalTree,

    /// Per-track interval trees for isolated overlap detection
    track_trees: HashMap<TrackId, IntervalTree>,

    /// Whether index may be out of sync with state
    dirty: bool,

    /// Performance statistics
    stats: IndexStats,
}

impl TimelineIndex {
    // =========================================================================
    // CONSTRUCTION
    // =========================================================================

    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build index from timeline state.
    ///
    /// Complexity: O(n log n) where n = number of clips
    ///
    /// Note: Invalid clips (zero/negative duration) are skipped. They will be
    /// caught by InvariantValidator but shouldn't prevent other clips from
    /// being indexed.
    pub fn build(state: &TimelineState) -> Self {
        let start = Instant::now();

        let mut index = Self::new();
        let mut indexed_count = 0;

        for clip in &state.clips {
            // Skip clips with invalid duration (would panic in TimeRange)
            if !clip.duration.is_positive() {
                continue;
            }

            let entry = Self::clip_to_entry(clip);

            // Insert into global tree
            index.all_clips.insert(entry.clone());

            // Insert into track-specific tree
            index
                .track_trees
                .entry(clip.track_id.clone())
                .or_insert_with(IntervalTree::new)
                .insert(entry);

            indexed_count += 1;
        }

        index.stats = IndexStats {
            total_clips: indexed_count,
            total_tracks: index.track_trees.len(),
            last_rebuild_ns: start.elapsed().as_nanos() as u64,
            incremental_updates: 0,
        };

        index.dirty = false;
        index
    }

    /// Convert a Clip to an IntervalEntry.
    #[inline]
    fn clip_to_entry(clip: &Clip) -> IntervalEntry {
        let end = clip.start + clip.duration;
        IntervalEntry::new(clip.id.clone(), clip.start, end)
    }

    /// Convert a Clip to a TimeRange.
    #[inline]
    pub fn clip_to_range(clip: &Clip) -> TimeRange {
        TimeRange::new(clip.start, clip.start + clip.duration)
    }

    // =========================================================================
    // INCREMENTAL UPDATES
    // =========================================================================

    /// Insert a new clip into the index.
    ///
    /// Complexity: O(log n)
    pub fn insert(&mut self, clip: &Clip) -> Result<(), IndexError> {
        let entry = Self::clip_to_entry(clip);

        // Insert into global tree
        self.all_clips.insert(entry.clone());

        // Insert into track tree
        self.track_trees
            .entry(clip.track_id.clone())
            .or_insert_with(IntervalTree::new)
            .insert(entry);

        self.stats.total_clips += 1;
        self.stats.incremental_updates += 1;

        Ok(())
    }

    /// Remove a clip from the index.
    ///
    /// Complexity: O(log n)
    pub fn remove(
        &mut self,
        clip_id: &ClipId,
        old_range: TimeRange,
        old_track: &TrackId,
    ) -> Result<(), IndexError> {
        // Remove from global tree
        if !self.all_clips.remove(clip_id, old_range) {
            return Err(IndexError::ClipNotFound(clip_id.clone()));
        }

        // Remove from track tree
        if let Some(track_tree) = self.track_trees.get_mut(old_track) {
            track_tree.remove(clip_id, old_range);

            // Remove empty track tree
            if track_tree.is_empty() {
                self.track_trees.remove(old_track);
            }
        }

        self.stats.total_clips = self.stats.total_clips.saturating_sub(1);
        self.stats.incremental_updates += 1;

        Ok(())
    }

    /// Update a clip after move/trim.
    ///
    /// Complexity: O(log n)
    pub fn update(
        &mut self,
        clip_id: &ClipId,
        old_range: TimeRange,
        old_track: &TrackId,
        new_range: TimeRange,
        new_track: &TrackId,
    ) -> Result<(), IndexError> {
        // Remove old entry
        self.remove(clip_id, old_range, old_track)?;

        // Insert new entry
        let entry = IntervalEntry::new(clip_id.clone(), new_range.start, new_range.end);

        self.all_clips.insert(entry.clone());
        self.track_trees
            .entry(new_track.clone())
            .or_insert_with(IntervalTree::new)
            .insert(entry);

        self.stats.total_clips += 1; // Re-add after remove decremented
        self.stats.incremental_updates += 1;

        Ok(())
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Find all clips containing a specific point in time.
    ///
    /// Complexity: O(log n + k) where k = result count
    pub fn clips_at(&self, time: MediaTime) -> Vec<ClipId> {
        self.all_clips.query_point(time)
    }

    /// Find all clips overlapping a time range.
    ///
    /// Complexity: O(log n + k) where k = result count
    pub fn clips_in_range(&self, range: TimeRange) -> Vec<ClipId> {
        self.all_clips.query_range(range)
    }

    /// Find clips on a specific track overlapping a time range.
    ///
    /// Complexity: O(log m + k) where m = clips on track, k = result count
    pub fn clips_on_track_in_range(&self, track_id: &TrackId, range: TimeRange) -> Vec<ClipId> {
        self.track_trees
            .get(track_id)
            .map(|tree| tree.query_range(range))
            .unwrap_or_default()
    }

    /// Check if any clip overlaps a range on a track.
    ///
    /// Complexity: O(log m) where m = clips on track
    pub fn has_overlap_on_track(
        &self,
        track_id: &TrackId,
        range: TimeRange,
        exclude: Option<&ClipId>,
    ) -> bool {
        self.track_trees
            .get(track_id)
            .map(|tree| tree.has_overlap(range, exclude))
            .unwrap_or(false)
    }

    // =========================================================================
    // MAINTENANCE
    // =========================================================================

    /// Mark index as potentially inconsistent.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if index needs rebuild.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Full rebuild from state.
    ///
    /// Complexity: O(n log n)
    pub fn rebuild(&mut self, state: &TimelineState) {
        *self = Self::build(state);
    }

    /// Get index statistics.
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Get total number of indexed clips.
    pub fn len(&self) -> usize {
        self.all_clips.len()
    }

    /// Check if index is empty.
    pub fn is_empty(&self) -> bool {
        self.all_clips.is_empty()
    }

    /// Get number of indexed tracks.
    pub fn track_count(&self) -> usize {
        self.track_trees.len()
    }

    /// Validate index against state for debugging.
    pub fn validate_against_state(&self, state: &TimelineState) -> Result<(), String> {
        // Check clip count matches
        if self.all_clips.len() != state.clips.len() {
            return Err(format!(
                "Clip count mismatch: index has {}, state has {}",
                self.all_clips.len(),
                state.clips.len()
            ));
        }

        // Check all state clips are in index
        for clip in &state.clips {
            let range = Self::clip_to_range(clip);
            let clips_at_start = self.all_clips.query_point(clip.start);
            if !clips_at_start.contains(&clip.id) {
                return Err(format!("Clip {} not found in index", clip.id));
            }

            // Check track tree
            if !self.has_overlap_on_track(&clip.track_id, range, None) {
                return Err(format!(
                    "Clip {} not found in track tree {}",
                    clip.id, clip.track_id
                ));
            }
        }

        Ok(())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> MediaTime {
        MediaTime::from_nanos(millis as i64 * 1_000_000)
    }

    fn make_clip(id: &str, track: &str, start_ms: u64, duration_ms: u64) -> Clip {
        Clip::new(id, track, ms(start_ms), ms(duration_ms), "test.mp4")
    }

    fn make_state(clips: Vec<Clip>) -> TimelineState {
        let mut state = TimelineState::new();
        state.clips = clips;
        state
    }

    #[test]
    fn test_build_empty() {
        let state = TimelineState::new();
        let index = TimelineIndex::build(&state);

        assert!(index.is_empty());
        assert_eq!(index.track_count(), 0);
        assert!(!index.is_dirty());
    }

    #[test]
    fn test_build_single_clip() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 1000)]);
        let index = TimelineIndex::build(&state);

        assert_eq!(index.len(), 1);
        assert_eq!(index.track_count(), 1);
    }

    #[test]
    fn test_build_multiple_clips_multiple_tracks() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t1", 1000, 1000),
            make_clip("c3", "t2", 0, 2000),
        ]);
        let index = TimelineIndex::build(&state);

        assert_eq!(index.len(), 3);
        assert_eq!(index.track_count(), 2);
    }

    #[test]
    fn test_clips_at() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t1", 1000, 1000),
        ]);
        let index = TimelineIndex::build(&state);

        // At start of c1
        let hits = index.clips_at(ms(0));
        assert_eq!(hits, vec!["c1".to_string()]);

        // At middle of c1
        let hits = index.clips_at(ms(500));
        assert_eq!(hits, vec!["c1".to_string()]);

        // At start of c2 (end of c1 is exclusive)
        let hits = index.clips_at(ms(1000));
        assert_eq!(hits, vec!["c2".to_string()]);
    }

    #[test]
    fn test_clips_in_range() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t1", 1000, 1000),
            make_clip("c3", "t1", 2000, 1000),
        ]);
        let index = TimelineIndex::build(&state);

        // Range overlapping c1 and c2
        let range = TimeRange::new(ms(500), ms(1500));
        let hits: std::collections::HashSet<_> = index.clips_in_range(range).into_iter().collect();

        assert_eq!(hits.len(), 2);
        assert!(hits.contains("c1"));
        assert!(hits.contains("c2"));
    }

    #[test]
    fn test_clips_on_track_in_range() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t2", 0, 1000), // Same time, different track
        ]);
        let index = TimelineIndex::build(&state);

        let range = TimeRange::new(ms(0), ms(500));

        // Track 1
        let hits = index.clips_on_track_in_range(&"t1".to_string(), range);
        assert_eq!(hits, vec!["c1".to_string()]);

        // Track 2
        let hits = index.clips_on_track_in_range(&"t2".to_string(), range);
        assert_eq!(hits, vec!["c2".to_string()]);

        // Nonexistent track
        let hits = index.clips_on_track_in_range(&"t3".to_string(), range);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_has_overlap_on_track() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t1", 2000, 1000),
        ]);
        let index = TimelineIndex::build(&state);

        // Overlaps c1
        assert!(index.has_overlap_on_track(
            &"t1".to_string(),
            TimeRange::new(ms(500), ms(1500)),
            None
        ));

        // Gap between c1 and c2
        assert!(!index.has_overlap_on_track(
            &"t1".to_string(),
            TimeRange::new(ms(1000), ms(2000)),
            None
        ));

        // Exclude c1
        assert!(!index.has_overlap_on_track(
            &"t1".to_string(),
            TimeRange::new(ms(500), ms(1500)),
            Some(&"c1".to_string())
        ));
    }

    #[test]
    fn test_insert() {
        let mut index = TimelineIndex::new();
        let clip = make_clip("c1", "t1", 0, 1000);

        index.insert(&clip).unwrap();

        assert_eq!(index.len(), 1);
        assert_eq!(index.track_count(), 1);
        assert_eq!(index.clips_at(ms(500)), vec!["c1".to_string()]);
    }

    #[test]
    fn test_remove() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 1000)]);
        let mut index = TimelineIndex::build(&state);

        let range = TimeRange::new(ms(0), ms(1000));
        index
            .remove(&"c1".to_string(), range, &"t1".to_string())
            .unwrap();

        assert!(index.is_empty());
        assert_eq!(index.track_count(), 0);
    }

    #[test]
    fn test_update() {
        let state = make_state(vec![make_clip("c1", "t1", 0, 1000)]);
        let mut index = TimelineIndex::build(&state);

        // Move clip from 0-1000 to 2000-3000
        let old_range = TimeRange::new(ms(0), ms(1000));
        let new_range = TimeRange::new(ms(2000), ms(3000));

        index
            .update(
                &"c1".to_string(),
                old_range,
                &"t1".to_string(),
                new_range,
                &"t1".to_string(),
            )
            .unwrap();

        // Should not be at old position
        assert!(index.clips_at(ms(500)).is_empty());

        // Should be at new position
        assert_eq!(index.clips_at(ms(2500)), vec!["c1".to_string()]);
    }

    #[test]
    fn test_rebuild() {
        let state1 = make_state(vec![make_clip("c1", "t1", 0, 1000)]);
        let mut index = TimelineIndex::build(&state1);

        index.mark_dirty();
        assert!(index.is_dirty());

        let state2 = make_state(vec![
            make_clip("c2", "t2", 0, 500),
            make_clip("c3", "t2", 500, 500),
        ]);

        index.rebuild(&state2);

        assert!(!index.is_dirty());
        assert_eq!(index.len(), 2);
        assert_eq!(index.track_count(), 1);
    }

    #[test]
    fn test_validate_against_state() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0, 1000),
            make_clip("c2", "t2", 1000, 1000),
        ]);
        let index = TimelineIndex::build(&state);

        assert!(index.validate_against_state(&state).is_ok());
    }

    #[test]
    fn test_index_vs_linear_scan() {
        // Create 100 clips
        let clips: Vec<Clip> = (0..100)
            .map(|i| make_clip(&format!("c{}", i), "t1", i * 100, 100))
            .collect();

        let state = make_state(clips.clone());
        let index = TimelineIndex::build(&state);

        // Test many query points
        for t in (0..10000).step_by(50) {
            let time = ms(t);

            // Index query
            let index_hits: std::collections::HashSet<_> =
                index.clips_at(time).into_iter().collect();

            // Linear scan
            let linear_hits: std::collections::HashSet<_> = clips
                .iter()
                .filter(|c| {
                    let end = c.start + c.duration;
                    time >= c.start && time < end
                })
                .map(|c| c.id.clone())
                .collect();

            assert_eq!(index_hits, linear_hits, "Mismatch at t={}", t);
        }
    }

    #[test]
    fn test_performance_1000_clips() {
        // Create 1000 clips
        let clips: Vec<Clip> = (0..1000)
            .map(|i| make_clip(&format!("c{}", i), &format!("t{}", i % 10), i * 10, 10))
            .collect();

        let state = make_state(clips);

        // Time the build
        let start = std::time::Instant::now();
        let index = TimelineIndex::build(&state);
        let build_time = start.elapsed();

        // Time 1000 overlap checks
        let start = std::time::Instant::now();
        for i in 0..1000 {
            let range = TimeRange::new(ms(i * 10), ms(i * 10 + 10));
            let _ = index.has_overlap_on_track(&format!("t{}", i % 10), range, None);
        }
        let query_time = start.elapsed();

        // Build should be < 100ms
        assert!(
            build_time.as_millis() < 100,
            "Build took {}ms",
            build_time.as_millis()
        );

        // 1000 queries should be < 10ms (averaging < 10µs each)
        assert!(
            query_time.as_millis() < 10,
            "Queries took {}ms",
            query_time.as_millis()
        );

        println!("Build: {:?}, 1000 queries: {:?}", build_time, query_time);
    }
}
