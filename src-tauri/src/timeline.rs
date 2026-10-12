// src-tauri/src/timeline.rs
//! Timeline Core Data Structures
//!
//! This module defines the immutable core data contracts for the timeline.
//! All IDs are newtype wrappers around UUIDs for type safety.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use uuid::Uuid;

// =============================================================================
// NEWTYPE IDENTIFIERS - Immutable, Type-Safe IDs
// =============================================================================

/// Unique identifier for a Clip. Immutable after creation.
/// SPLIT operations must always create new ClipId values.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct ClipId(Uuid);

impl ClipId {
    /// Create a new random ClipId
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID (for deserialization)
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parse from string (for backward compatibility)
    pub fn from_string(s: &str) -> Option<Self> {
        Uuid::parse_str(s).ok().map(Self)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for ClipId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ClipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClipId({})", &self.0.to_string()[..8])
    }
}

impl fmt::Display for ClipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Hash for ClipId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Unique identifier for a Track. Immutable after creation.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct TrackId(Uuid);

impl TrackId {
    /// Create a new random TrackId
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parse from string (for backward compatibility)
    pub fn from_string(s: &str) -> Option<Self> {
        Uuid::parse_str(s).ok().map(Self)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }

    /// Create a default video track ID (for V1 single-track mode)
    pub fn default_video_track() -> Self {
        // Use a fixed UUID for the default video track
        Self(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
    }
}

impl Default for TrackId {
    fn default() -> Self {
        Self::default_video_track()
    }
}

impl fmt::Debug for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TrackId({})", &self.0.to_string()[..8])
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Hash for TrackId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Unique identifier for source media. Immutable after creation.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct MediaId(Uuid);

impl MediaId {
    /// Create a new random MediaId
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MediaId({})", &self.0.to_string()[..8])
    }
}

impl Hash for MediaId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

// =============================================================================
// PROJECT SETTINGS
// =============================================================================

/// Project-wide settings for the timeline
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectSettings {
    /// Timeline frame rate (frames per second)
    pub timeline_frame_rate: f32,
    /// Timeline resolution (width, height)
    pub timeline_resolution: (u32, u32),
    /// Audio sample rate (Hz)
    pub timeline_sample_rate: u32,
}

impl ProjectSettings {
    /// Create new project settings with validation
    pub fn new(
        timeline_frame_rate: f32,
        timeline_resolution: (u32, u32),
        timeline_sample_rate: u32,
    ) -> Result<Self, String> {
        if timeline_frame_rate <= 0.0 {
            return Err("timeline_frame_rate must be > 0".to_string());
        }
        if timeline_resolution.0 == 0 || timeline_resolution.1 == 0 {
            return Err("timeline_resolution must have non-zero dimensions".to_string());
        }
        if timeline_sample_rate == 0 {
            return Err("timeline_sample_rate must be > 0".to_string());
        }

        Ok(Self {
            timeline_frame_rate,
            timeline_resolution,
            timeline_sample_rate,
        })
    }

    /// Create with sensible defaults (1080p, 30fps, 48kHz)
    pub fn default_1080p() -> Self {
        Self {
            timeline_frame_rate: 30.0,
            timeline_resolution: (1920, 1080),
            timeline_sample_rate: 48000,
        }
    }

    /// Create with 4K defaults
    pub fn default_4k() -> Self {
        Self {
            timeline_frame_rate: 30.0,
            timeline_resolution: (3840, 2160),
            timeline_sample_rate: 48000,
        }
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self::default_1080p()
    }
}

// =============================================================================
// TRACK - Container for clips
// =============================================================================

/// A track on the timeline. Holds clips in vertical lanes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Track {
    /// Unique track identifier
    pub id: String,
    /// Display name (e.g., "V1", "V2")
    pub name: String,
    /// Vertical order (0 = topmost)
    pub index: usize,
}

impl Track {
    /// Create a new track with a fresh ID
    pub fn new(name: String, index: usize) -> Self {
        Self {
            id: TrackId::new().to_string(),
            name,
            index,
        }
    }

    /// Create default video track V1
    pub fn default_v1() -> Self {
        Self {
            id: TrackId::new().to_string(),
            name: "V1".to_string(),
            index: 0,
        }
    }
}

// =============================================================================
// CLIP - Core timeline element
// =============================================================================

/// A clip on the timeline. IDs are immutable after creation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Clip {
    /// Immutable unique identifier. SPLIT creates new ClipIds.
    pub id: String,
    /// Track this clip belongs to
    pub track_id: String,
    /// Start time on timeline (seconds)
    pub start: f64,
    /// Length of clip (seconds)
    pub duration: f64,
    /// Path to source media file
    pub source_file: String,
}

impl Clip {
    /// Create a new clip with a fresh ID
    pub fn new(track_id: String, start: f64, duration: f64, source_file: String) -> Self {
        Self {
            id: ClipId::new().to_string(),
            track_id,
            start,
            duration,
            source_file,
        }
    }

    /// Get the end time of this clip
    pub fn end(&self) -> f64 {
        self.start + self.duration
    }

    /// Check if a time falls within this clip
    pub fn contains_time(&self, time: f64) -> bool {
        time >= self.start && time < self.end()
    }

    /// Get typed ClipId
    pub fn clip_id(&self) -> Option<ClipId> {
        ClipId::from_string(&self.id)
    }

    /// Get typed TrackId
    pub fn track_id_typed(&self) -> Option<TrackId> {
        TrackId::from_string(&self.track_id)
    }
}

// =============================================================================
// TIMELINE STATE - High-Performance Indexed Version
// =============================================================================

use std::collections::{BTreeMap, HashMap};

/// The complete state of the timeline with performance indices
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimelineState {
    /// All clips in the timeline
    pub clips: Vec<Clip>,
    /// All tracks in the timeline
    pub tracks: Vec<Track>,
    /// Calculated content duration (max of clip ends)
    pub duration: f64,
    /// Project duration (may be >= content duration)
    pub project_duration: f64,
    /// Current playhead position in seconds. Always in range [0, duration].
    pub playhead_time: f64,
    /// Version counter, incremented on every state mutation.
    pub version: u64,
    /// Project-wide settings
    pub settings: ProjectSettings,

    // =========================================================================
    // PERFORMANCE INDICES - Rebuilt from clips on deserialization
    // =========================================================================
    /// O(1) lookup: ClipId -> index in clips Vec
    #[serde(skip)]
    pub clip_id_index: HashMap<String, usize>,

    /// O(log n) lookup: TrackId -> (start_time -> ClipId)
    /// Enables fast time-based queries within a track
    #[serde(skip)]
    pub track_index: HashMap<String, BTreeMap<OrderedFloat, String>>,
}

/// Wrapper for f64 that implements Ord for use in BTreeMap
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OrderedFloat(pub f64);

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl TimelineState {
    /// Create a new empty timeline state with default settings
    pub fn new() -> Self {
        Self {
            clips: vec![],
            tracks: vec![Track::default_v1()],
            duration: 0.0,
            project_duration: 0.0,
            playhead_time: 0.0,
            version: 0,
            settings: ProjectSettings::default(),
            clip_id_index: HashMap::new(),
            track_index: HashMap::new(),
        }
    }

    /// Create with custom settings
    pub fn with_settings(settings: ProjectSettings) -> Self {
        Self {
            clips: vec![],
            tracks: vec![Track::default_v1()],
            duration: 0.0,
            project_duration: 0.0,
            playhead_time: 0.0,
            version: 0,
            settings,
            clip_id_index: HashMap::new(),
            track_index: HashMap::new(),
        }
    }

    // =========================================================================
    // INDEX MANAGEMENT
    // =========================================================================

    /// Rebuild all indices from the clips vector
    /// Call this after deserialization or bulk modifications
    pub fn rebuild_indices(&mut self) {
        self.clip_id_index.clear();
        self.track_index.clear();

        for (idx, clip) in self.clips.iter().enumerate() {
            // Update clip_id_index
            self.clip_id_index.insert(clip.id.clone(), idx);

            // Update track_index
            self.track_index
                .entry(clip.track_id.clone())
                .or_insert_with(BTreeMap::new)
                .insert(OrderedFloat(clip.start), clip.id.clone());
        }
    }

    /// Add a clip and update indices
    pub fn add_clip(&mut self, clip: Clip) {
        let idx = self.clips.len();

        // Update indices before adding
        self.clip_id_index.insert(clip.id.clone(), idx);
        self.track_index
            .entry(clip.track_id.clone())
            .or_insert_with(BTreeMap::new)
            .insert(OrderedFloat(clip.start), clip.id.clone());

        self.clips.push(clip);
        self.recalculate_duration();
        self.version += 1;
    }

    /// Remove a clip by ID and update indices
    pub fn remove_clip(&mut self, clip_id: &str) -> Option<Clip> {
        let idx = self.clip_id_index.remove(clip_id)?;
        let clip = self.clips.remove(idx);

        // Remove from track_index
        if let Some(track_map) = self.track_index.get_mut(&clip.track_id) {
            track_map.remove(&OrderedFloat(clip.start));
        }

        // Rebuild clip_id_index since indices shifted
        self.rebuild_clip_id_index();
        self.recalculate_duration();
        self.version += 1;

        Some(clip)
    }

    /// Rebuild only the clip_id_index (after removal shifts indices)
    fn rebuild_clip_id_index(&mut self) {
        self.clip_id_index.clear();
        for (idx, clip) in self.clips.iter().enumerate() {
            self.clip_id_index.insert(clip.id.clone(), idx);
        }
    }

    // =========================================================================
    // O(1) LOOKUPS
    // =========================================================================

    /// Get a clip by ID in O(1) time
    pub fn get_clip_by_id(&self, clip_id: &str) -> Option<&Clip> {
        let idx = self.clip_id_index.get(clip_id)?;
        self.clips.get(*idx)
    }

    /// Get a mutable clip by ID in O(1) time
    pub fn get_clip_by_id_mut(&mut self, clip_id: &str) -> Option<&mut Clip> {
        let idx = *self.clip_id_index.get(clip_id)?;
        self.clips.get_mut(idx)
    }

    /// Check if a clip exists in O(1) time
    pub fn has_clip(&self, clip_id: &str) -> bool {
        self.clip_id_index.contains_key(clip_id)
    }

    // =========================================================================
    // O(log n) TIME-BASED LOOKUPS
    // =========================================================================

    /// Find the clip at a specific time on a specific track in O(log n) time
    pub fn find_clip_at_time_on_track(&self, track_id: &str, time: f64) -> Option<&Clip> {
        let track_map = self.track_index.get(track_id)?;

        // Find the clip whose start is <= time
        // Use range to find clips that could contain this time
        for (start, clip_id) in track_map.range(..=OrderedFloat(time)).rev() {
            if let Some(clip) = self.get_clip_by_id(clip_id) {
                if clip.contains_time(time) {
                    return Some(clip);
                }
                // If this clip ends before our time, no earlier clip will contain it
                if clip.end() < time {
                    break;
                }
            }
        }
        None
    }

    /// Find any clip at a specific time (across all tracks) in O(tracks * log n) time
    pub fn find_clip_at_time(&self, time: f64) -> Option<&Clip> {
        for track_id in self.track_index.keys() {
            if let Some(clip) = self.find_clip_at_time_on_track(track_id, time) {
                return Some(clip);
            }
        }
        None
    }

    /// Get all clips on a specific track, sorted by start time
    pub fn get_clips_on_track(&self, track_id: &str) -> Vec<&Clip> {
        self.track_index
            .get(track_id)
            .map(|track_map| {
                track_map
                    .values()
                    .filter_map(|id| self.get_clip_by_id(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get clips in a time range on a track in O(log n + k) where k = results
    pub fn get_clips_in_range(&self, track_id: &str, start: f64, end: f64) -> Vec<&Clip> {
        let Some(track_map) = self.track_index.get(track_id) else {
            return vec![];
        };

        let mut result = Vec::new();

        // Find clips that could overlap with [start, end]
        for (clip_start, clip_id) in track_map.range(..OrderedFloat(end)) {
            if let Some(clip) = self.get_clip_by_id(clip_id) {
                // Clip overlaps if: clip.start < end AND clip.end > start
                if clip_start.0 < end && clip.end() > start {
                    result.push(clip);
                }
            }
        }
        result
    }

    // =========================================================================
    // TRACK OPERATIONS
    // =========================================================================

    /// Get all unique track IDs
    pub fn get_track_ids(&self) -> Vec<String> {
        self.track_index.keys().cloned().collect()
    }

    /// Get clip count on a specific track
    pub fn clip_count_on_track(&self, track_id: &str) -> usize {
        self.track_index.get(track_id).map(|m| m.len()).unwrap_or(0)
    }

    /// Get the visible clip at a given time (topmost based on track index).
    ///
    /// When clips overlap across tracks, the clip on the highest-index track
    /// is considered "on top" and will be rendered. This matches visual stacking.
    ///
    /// Returns the clip from the highest-index track at the given time.
    pub fn get_visible_clip_at_time(&self, time: f64) -> Option<&Clip> {
        // Find all clips containing this time
        let mut candidates: Vec<(&Clip, usize)> = Vec::new();

        for clip in &self.clips {
            if clip.contains_time(time) {
                // Find the track index for this clip
                let track_index = self
                    .tracks
                    .iter()
                    .find(|t| t.id == clip.track_id)
                    .map(|t| t.index)
                    .unwrap_or(0);
                candidates.push((clip, track_index));
            }
        }

        // Select the clip with the highest track index (visually on top)
        candidates
            .into_iter()
            .max_by_key(|(_, idx)| *idx)
            .map(|(clip, _)| clip)
    }

    // =========================================================================
    // OVERLAP DETECTION - O(log n) per check instead of O(n²)
    // =========================================================================

    /// Check if a new clip would overlap with existing clips on the same track
    /// Uses track_index for O(log n) lookup instead of O(n) scan
    pub fn would_overlap(&self, track_id: &str, start: f64, duration: f64) -> bool {
        let end = start + duration;
        !self.get_clips_in_range(track_id, start, end).is_empty()
    }

    /// Find adjacent clips (previous and next) on the same track in O(log n)
    pub fn find_adjacent_clips(&self, clip_id: &str) -> (Option<&Clip>, Option<&Clip>) {
        let Some(clip) = self.get_clip_by_id(clip_id) else {
            return (None, None);
        };

        let Some(track_map) = self.track_index.get(&clip.track_id) else {
            return (None, None);
        };

        let key = OrderedFloat(clip.start);

        // Find previous clip
        let prev = track_map
            .range(..key)
            .next_back()
            .and_then(|(_, id)| self.get_clip_by_id(id));

        // Find next clip
        let next = track_map
            .range((std::ops::Bound::Excluded(key), std::ops::Bound::Unbounded))
            .next()
            .and_then(|(_, id)| self.get_clip_by_id(id));

        (prev, next)
    }

    /// Recalculate content duration from clips
    pub fn recalculate_duration(&mut self) {
        self.duration = self.clips.iter().map(|c| c.end()).fold(0.0, f64::max);

        // Ensure project_duration >= content duration
        if self.project_duration < self.duration {
            self.project_duration = self.duration;
        }
    }

    /// Validate all constitutional invariants of the timeline.
    ///
    /// This method enforces the Antigravity Invariants - the non-negotiable
    /// rules that must hold true for any valid timeline state.
    ///
    /// # Invariants Enforced
    /// 1. All ClipId values are globally unique
    /// 2. project_duration >= max(clip.start + clip.duration)
    /// 3. Exactly one master timeline_frame_rate exists (via ProjectSettings)
    /// 4. clip.duration > 0 for all clips
    /// 5. clip.start >= 0 for all clips
    /// 6. playhead_time ∈ [0, project_duration]
    ///
    /// # Returns
    /// - `Ok(())` if all invariants hold
    /// - `Err(String)` with description of the first violated invariant
    pub fn validate_invariants(&self) -> Result<(), String> {
        use std::collections::HashSet;

        // Invariant 1: All ClipId values are globally unique
        let mut seen_ids: HashSet<&str> = HashSet::new();
        for clip in &self.clips {
            if !seen_ids.insert(&clip.id) {
                return Err(format!(
                    "INVARIANT_VIOLATED: Duplicate ClipId '{}' detected",
                    clip.id
                ));
            }
        }

        // Invariant 2: project_duration >= content_duration
        let content_duration = self.clips.iter().map(|c| c.end()).fold(0.0, f64::max);

        if self.project_duration < content_duration - 0.001 {
            return Err(format!(
                "INVARIANT_VIOLATED: project_duration ({:.3}s) < content_duration ({:.3}s)",
                self.project_duration, content_duration
            ));
        }

        // Invariant 3: Exactly one master timeline_frame_rate exists
        // (Enforced by ProjectSettings struct - always has exactly one frame_rate)
        if self.settings.timeline_frame_rate <= 0.0 {
            return Err(format!(
                "INVARIANT_VIOLATED: timeline_frame_rate ({:.2}) must be > 0",
                self.settings.timeline_frame_rate
            ));
        }

        // Invariant 4 & 5: All clips have positive duration and non-negative start
        for clip in &self.clips {
            if clip.duration <= 0.0 {
                return Err(format!(
                    "INVARIANT_VIOLATED: Clip '{}' has invalid duration: {:.3}s (must be > 0)",
                    clip.id, clip.duration
                ));
            }
            if clip.start < 0.0 {
                return Err(format!(
                    "INVARIANT_VIOLATED: Clip '{}' has negative start: {:.3}s",
                    clip.id, clip.start
                ));
            }
        }

        // Invariant 6: playhead_time ∈ [0, project_duration]
        if self.playhead_time < 0.0 {
            return Err(format!(
                "INVARIANT_VIOLATED: playhead_time ({:.3}s) < 0",
                self.playhead_time
            ));
        }
        if self.playhead_time > self.project_duration + 0.001 {
            return Err(format!(
                "INVARIANT_VIOLATED: playhead_time ({:.3}s) > project_duration ({:.3}s)",
                self.playhead_time, self.project_duration
            ));
        }

        Ok(())
    }

    /// Validate invariants (alias for backward compatibility)
    pub fn validate(&self) -> Result<(), String> {
        self.validate_invariants()
    }
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TIMELINE ENGINE
// =============================================================================

/// The engine that holds the timeline state safely across threads
pub struct TimelineEngine {
    /// Thread-safe access to timeline state
    pub state: Mutex<TimelineState>,
}

impl TimelineEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(TimelineState::default()),
        }
    }

    /// Create with custom settings
    pub fn with_settings(settings: ProjectSettings) -> Self {
        Self {
            state: Mutex::new(TimelineState::with_settings(settings)),
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

    /// Get the visible clip at the given time (topmost based on track index).
    ///
    /// When clips overlap across tracks, the clip on the highest-index track
    /// is considered "on top" and will be rendered.
    pub fn get_active_clip(&self, time: f64) -> Option<Clip> {
        let state = self.state.lock().unwrap();
        state.get_visible_clip_at_time(time).cloned()
    }

    /// Get the visible clip at the current playhead position.
    pub fn get_current_clip(&self) -> Option<Clip> {
        let state = self.state.lock().unwrap();
        let time = state.playhead_time;
        state.get_visible_clip_at_time(time).cloned()
    }

    /// Increment the version counter.
    pub fn bump_version(&self) {
        let mut state = self.state.lock().unwrap();
        state.version += 1;
    }

    /// Helper to print current state (for debugging)
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

impl Default for TimelineEngine {
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

    #[test]
    fn test_clip_id_uniqueness() {
        let id1 = ClipId::new();
        let id2 = ClipId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_clip_id_from_string() {
        let id = ClipId::new();
        let s = id.to_string();
        let parsed = ClipId::from_string(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_track_id_default() {
        let track = TrackId::default_video_track();
        assert_eq!(track.to_string(), "00000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn test_project_settings_validation() {
        assert!(ProjectSettings::new(0.0, (1920, 1080), 48000).is_err());
        assert!(ProjectSettings::new(30.0, (0, 1080), 48000).is_err());
        assert!(ProjectSettings::new(30.0, (1920, 1080), 0).is_err());
        assert!(ProjectSettings::new(30.0, (1920, 1080), 48000).is_ok());
    }

    #[test]
    fn test_timeline_state_validation() {
        // Create state with a clip that extends beyond project_duration
        let mut state = TimelineState::new();
        state.clips.push(Clip {
            id: "clip-1".to_string(),
            track_id: "video_track_1".to_string(),
            start: 0.0,
            duration: 10.0, // Clip ends at 10s
            source_file: "/test.mp4".to_string(),
        });
        state.duration = 10.0;
        state.project_duration = 5.0; // Invalid: less than content (10s)
        assert!(state.validate().is_err());

        state.project_duration = 10.0;
        assert!(state.validate().is_ok());
    }

    #[test]
    fn test_clip_contains_time() {
        let clip = Clip::new(
            "video_track_1".to_string(),
            5.0,
            10.0,
            "/test.mp4".to_string(),
        );
        assert!(!clip.contains_time(4.9));
        assert!(clip.contains_time(5.0));
        assert!(clip.contains_time(10.0));
        assert!(!clip.contains_time(15.0));
    }
}
