//! InvariantValidator - Timeline state validation rules.
//!
//! # Design
//!
//! All invariants are pure predicates with no side effects.
//! Validation is stateless and deterministic.
//!
//! # Performance
//!
//! When a TimelineIndex is provided, overlap detection runs in O(n log n).
//! Without an index, it falls back to O(n²) (sorting per track).

use crate::engine::interval_tree::TimeRange;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::{ClipId, TimelineState, TrackId};
use std::collections::HashSet;

/// Types of invariant violations.
///
/// Each variant describes a specific rule that was broken.
#[derive(Debug, Clone, PartialEq)]
pub enum InvariantViolation {
    /// Two clips overlap on the same track
    OverlappingClips {
        clip_a: ClipId,
        clip_b: ClipId,
        track: TrackId,
    },

    /// Clip has zero or negative duration
    InvalidDuration {
        clip_id: ClipId,
        duration_nanos: i64,
    },

    /// Clip starts before timeline origin
    NegativeStart { clip_id: ClipId, start_nanos: i64 },

    /// Clip ID is duplicated
    DuplicateClipId { clip_id: ClipId },

    /// Clip has empty source file
    EmptySourceFile { clip_id: ClipId },

    /// Index is out of sync with clips vector
    IndexDesync { description: String },
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OverlappingClips {
                clip_a,
                clip_b,
                track,
            } => {
                write!(
                    f,
                    "Clips {} and {} overlap on track {}",
                    clip_a, clip_b, track
                )
            }
            Self::InvalidDuration {
                clip_id,
                duration_nanos,
            } => {
                write!(
                    f,
                    "Clip {} has invalid duration: {} nanos",
                    clip_id, duration_nanos
                )
            }
            Self::NegativeStart {
                clip_id,
                start_nanos,
            } => {
                write!(
                    f,
                    "Clip {} has negative start: {} nanos",
                    clip_id, start_nanos
                )
            }
            Self::DuplicateClipId { clip_id } => {
                write!(f, "Duplicate clip ID: {}", clip_id)
            }
            Self::EmptySourceFile { clip_id } => {
                write!(f, "Clip {} has empty source file", clip_id)
            }
            Self::IndexDesync { description } => {
                write!(f, "Index desync: {}", description)
            }
        }
    }
}

/// Invariant validation system.
///
/// # Thread Safety
///
/// InvariantValidator is stateless and can be called from any thread.
pub struct InvariantValidator;

impl InvariantValidator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self
    }

    /// Validate all invariants on the given state.
    ///
    /// # Arguments
    ///
    /// * `state` - The timeline state to validate
    /// * `index` - Optional TimelineIndex for O(n log n) overlap detection
    ///
    /// # Invariants Checked
    ///
    /// 1. No two clips on the same track may overlap in time
    /// 2. All clips must have duration > 0
    /// 3. All clips must have start >= 0
    /// 4. All clip IDs must be unique
    /// 5. All clips must have non-empty source file
    /// 6. Indices must be consistent with clips vector
    ///
    /// # Returns
    ///
    /// `Ok(())` if all invariants hold, `Err(violation)` on first failure.
    pub fn validate(
        &self,
        state: &TimelineState,
        index: Option<&TimelineIndex>,
    ) -> Result<(), InvariantViolation> {
        if let Some(idx) = index {
            self.validate_with_index(state, idx)
        } else {
            self.validate_linear(state)
        }
    }

    /// Validate using TimelineIndex for O(n log n) overlap detection.
    fn validate_with_index(
        &self,
        state: &TimelineState,
        index: &TimelineIndex,
    ) -> Result<(), InvariantViolation> {
        // Non-overlap checks first (same for both paths)
        self.check_unique_ids(state)?;
        self.check_valid_durations(state)?;
        self.check_valid_starts(state)?;

        // O(n log n) overlap detection using index
        self.check_no_overlaps_with_index(state, index)?;

        // Remaining checks
        self.check_non_empty_sources(state)?;
        self.check_index_consistency(state)?;

        Ok(())
    }

    /// Validate using linear O(n²) overlap detection (fallback).
    fn validate_linear(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        self.check_unique_ids(state)?;
        self.check_valid_durations(state)?;
        self.check_valid_starts(state)?;
        self.check_no_overlaps_linear(state)?;
        self.check_non_empty_sources(state)?;
        self.check_index_consistency(state)?;
        Ok(())
    }

    /// Check that all clip IDs are unique.
    fn check_unique_ids(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(state.clips.len());

        for clip in &state.clips {
            if !seen.insert(&clip.id) {
                return Err(InvariantViolation::DuplicateClipId {
                    clip_id: clip.id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Check that all durations are positive.
    fn check_valid_durations(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        for clip in &state.clips {
            if !clip.duration.is_positive() {
                return Err(InvariantViolation::InvalidDuration {
                    clip_id: clip.id.clone(),
                    duration_nanos: clip.duration.as_nanos(),
                });
            }
        }
        Ok(())
    }

    /// Check that all start times are non-negative.
    fn check_valid_starts(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        for clip in &state.clips {
            if clip.start.is_negative() {
                return Err(InvariantViolation::NegativeStart {
                    clip_id: clip.id.clone(),
                    start_nanos: clip.start.as_nanos(),
                });
            }
        }
        Ok(())
    }

    /// Check overlaps using TimelineIndex (O(n log n)).
    fn check_no_overlaps_with_index(
        &self,
        state: &TimelineState,
        index: &TimelineIndex,
    ) -> Result<(), InvariantViolation> {
        for clip in &state.clips {
            let range = TimeRange::new(clip.start, clip.end());

            // Check if any OTHER clip overlaps this one on the same track
            if index.has_overlap_on_track(&clip.track_id, range, Some(&clip.id)) {
                // Find the overlapping clip for error reporting
                let overlapping = index.clips_on_track_in_range(&clip.track_id, range);
                let other_id = overlapping
                    .into_iter()
                    .find(|id| id != &clip.id)
                    .unwrap_or_else(|| "unknown".to_string());

                return Err(InvariantViolation::OverlappingClips {
                    clip_a: clip.id.clone(),
                    clip_b: other_id,
                    track: clip.track_id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Check overlaps using linear scan (O(n²) fallback).
    fn check_no_overlaps_linear(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        // Group clips by track
        use std::collections::HashMap;
        let mut by_track: HashMap<&str, Vec<&crate::engine::timeline_state::Clip>> = HashMap::new();

        for clip in &state.clips {
            by_track.entry(&clip.track_id).or_default().push(clip);
        }

        // Check each track for overlaps
        for (track_id, clips) in by_track {
            let mut sorted = clips.clone();
            sorted.sort_by_key(|c| c.start.as_nanos());

            for window in sorted.windows(2) {
                let a = window[0];
                let b = window[1];

                // Overlap if a.end > b.start
                if a.end() > b.start {
                    return Err(InvariantViolation::OverlappingClips {
                        clip_a: a.id.clone(),
                        clip_b: b.id.clone(),
                        track: track_id.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Check that all source files are non-empty.
    fn check_non_empty_sources(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        for clip in &state.clips {
            if clip.source_file.is_empty() {
                return Err(InvariantViolation::EmptySourceFile {
                    clip_id: clip.id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Check that indices are consistent with clips vector.
    fn check_index_consistency(&self, state: &TimelineState) -> Result<(), InvariantViolation> {
        // Skip if indices are empty (not yet built)
        if state.clip_id_index.is_empty() && !state.clips.is_empty() {
            // This is a warning state, not an error — indices may not be built yet
            return Ok(());
        }

        for (id, &idx) in &state.clip_id_index {
            if idx >= state.clips.len() {
                return Err(InvariantViolation::IndexDesync {
                    description: format!(
                        "clip_id_index[{}] = {} but only {} clips exist",
                        id,
                        idx,
                        state.clips.len()
                    ),
                });
            }

            if state.clips[idx].id != *id {
                return Err(InvariantViolation::IndexDesync {
                    description: format!(
                        "clip_id_index[{}] = {} but clips[{}].id = {}",
                        id, idx, idx, state.clips[idx].id
                    ),
                });
            }
        }

        Ok(())
    }
}

impl Default for InvariantValidator {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;

    fn make_clip(id: &str, track: &str, start_secs: f64, duration_secs: f64) -> Clip {
        Clip::new(
            id,
            track,
            MediaTime::from_seconds(start_secs),
            MediaTime::from_seconds(duration_secs),
            "test.mp4",
        )
    }

    fn make_state(clips: Vec<Clip>) -> TimelineState {
        let mut state = TimelineState::new();
        state.clips = clips;
        state.rebuild_indices();
        state
    }

    // =========================================================================
    // EXISTING TESTS (using linear fallback via None)
    // =========================================================================

    #[test]
    fn test_valid_state() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 5.0),
            make_clip("c2", "t1", 5.0, 5.0),
        ]);

        let validator = InvariantValidator::new();
        assert!(validator.validate(&state, None).is_ok());
    }

    #[test]
    fn test_overlapping_clips() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 10.0),
            make_clip("c2", "t1", 5.0, 10.0), // Overlaps!
        ]);

        let validator = InvariantValidator::new();
        let result = validator.validate(&state, None);

        assert!(matches!(
            result,
            Err(InvariantViolation::OverlappingClips { .. })
        ));
    }

    #[test]
    fn test_duplicate_id() {
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 5.0),
            make_clip("c1", "t1", 10.0, 5.0), // Duplicate ID!
        ]);

        let validator = InvariantValidator::new();
        let result = validator.validate(&state, None);

        assert!(matches!(
            result,
            Err(InvariantViolation::DuplicateClipId { .. })
        ));
    }

    #[test]
    fn test_negative_start() {
        let mut state = TimelineState::new();
        let mut clip = make_clip("c1", "t1", 0.0, 5.0);
        clip.start = MediaTime::from_seconds(-1.0);
        state.clips.push(clip);

        let validator = InvariantValidator::new();
        let result = validator.validate(&state, None);

        assert!(matches!(
            result,
            Err(InvariantViolation::NegativeStart { .. })
        ));
    }

    #[test]
    fn test_zero_duration() {
        let mut state = TimelineState::new();
        let mut clip = make_clip("c1", "t1", 0.0, 5.0);
        clip.duration = MediaTime::ZERO;
        state.clips.push(clip);

        let validator = InvariantValidator::new();
        let result = validator.validate(&state, None);

        assert!(matches!(
            result,
            Err(InvariantViolation::InvalidDuration { .. })
        ));
    }

    // =========================================================================
    // NEW TESTS FOR INDEX-BACKED VALIDATION
    // =========================================================================

    #[test]
    fn test_validate_with_index_matches_linear() {
        // Create a state with various clips
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 5.0),
            make_clip("c2", "t1", 5.0, 5.0),
            make_clip("c3", "t2", 0.0, 10.0),
            make_clip("c4", "t2", 10.0, 5.0),
        ]);

        let index = TimelineIndex::build(&state);
        let validator = InvariantValidator::new();

        // Both paths should succeed
        let linear_result = validator.validate(&state, None);
        let index_result = validator.validate(&state, Some(&index));

        assert!(linear_result.is_ok());
        assert!(index_result.is_ok());
    }

    #[test]
    fn test_no_false_overlap() {
        // Adjacent clips should NOT overlap (end-exclusive)
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 5.0),  // [0, 5)
            make_clip("c2", "t1", 5.0, 5.0),  // [5, 10)
            make_clip("c3", "t1", 10.0, 5.0), // [10, 15)
        ]);

        let index = TimelineIndex::build(&state);
        let validator = InvariantValidator::new();

        // Should pass - no overlaps
        assert!(validator.validate(&state, Some(&index)).is_ok());
    }

    #[test]
    fn test_index_overlap_detected() {
        // Create overlapping clips
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 10.0), // [0, 10)
            make_clip("c2", "t1", 5.0, 10.0), // [5, 15) - overlaps!
        ]);

        let index = TimelineIndex::build(&state);
        let validator = InvariantValidator::new();

        let result = validator.validate(&state, Some(&index));

        assert!(matches!(
            result,
            Err(InvariantViolation::OverlappingClips { .. })
        ));
    }

    #[test]
    fn test_index_missing_fallback() {
        // When index is None, should use linear fallback
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 5.0),
            make_clip("c2", "t1", 5.0, 5.0),
        ]);

        let validator = InvariantValidator::new();

        // Should work with None (fallback to linear)
        assert!(validator.validate(&state, None).is_ok());
    }

    #[test]
    fn test_index_and_linear_same_error() {
        // Both paths should detect the same overlap
        let state = make_state(vec![
            make_clip("c1", "t1", 0.0, 10.0),
            make_clip("c2", "t1", 5.0, 10.0), // Overlaps!
        ]);

        let index = TimelineIndex::build(&state);
        let validator = InvariantValidator::new();

        let linear_result = validator.validate(&state, None);
        let index_result = validator.validate(&state, Some(&index));

        // Both should be errors
        assert!(linear_result.is_err());
        assert!(index_result.is_err());

        // Both should be OverlappingClips
        assert!(matches!(
            linear_result,
            Err(InvariantViolation::OverlappingClips { .. })
        ));
        assert!(matches!(
            index_result,
            Err(InvariantViolation::OverlappingClips { .. })
        ));
    }
}
