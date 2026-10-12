//! EditAction - Command representation for all timeline mutations.
//!
//! # Design Decision
//!
//! EditAction is a value type representing intent. It is:
//! - Immutable after creation
//! - Serializable for event sourcing
//! - Self-describing (contains all data needed to execute AND reverse)

use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::{Clip, ClipId, TrackId};
use serde::{Deserialize, Serialize};

/// Parameters for each action type.
///
/// Uses Option<T> to allow sparse parameter sets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ActionParameters {
    /// New start time for move operations
    pub new_start_time: Option<MediaTime>,

    /// New track for move operations
    pub new_track_id: Option<TrackId>,

    /// Trim delta for start edge (negative = extend, positive = shrink)
    pub trim_start_delta: Option<MediaTime>,

    /// Trim delta for end edge (negative = shrink, positive = extend)
    pub trim_end_delta: Option<MediaTime>,

    /// Split position (time within clip, not timeline position)
    pub split_time: Option<MediaTime>,
}

/// Enumeration of all possible edit operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ActionType {
    /// Add a new clip to the timeline
    AddClip,

    /// Remove a clip from the timeline
    DeleteClip,

    /// Move a clip to a new position/track
    MoveClip,

    /// Trim clip edges
    TrimClip,

    /// Split clip into two at a given time
    SplitClip,
}

impl ActionType {
    /// Check if this action type is destructive (modifies/removes existing data).
    #[inline]
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::DeleteClip | Self::TrimClip | Self::SplitClip)
    }
}

/// A single atomic edit command.
///
/// # Invariants
///
/// - An EditAction must be fully self-describing
/// - No external state is needed to interpret an EditAction
/// - After execution, must be reversible using only data in this struct
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditAction {
    /// Unique identifier for this action (for event store)
    pub id: String,

    /// Type of operation
    pub action_type: ActionType,

    /// Target clip (required for most operations, None for AddClip)
    pub clip_id: Option<ClipId>,

    /// Full clip data (for AddClip, or for undo reconstruction)
    pub clip_data: Option<Clip>,

    /// Operation parameters
    pub parameters: ActionParameters,

    /// Timestamp when action was created (UTC nanoseconds since epoch)
    pub timestamp: u64,

    /// Optional: AI reasoning that produced this action
    pub thought_process: Option<String>,

    /// Optional: Confidence score from AI (0.0 - 1.0)
    pub confidence: Option<f32>,
}

impl EditAction {
    /// Create a new action with current timestamp.
    pub fn new(action_type: ActionType) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            action_type,
            clip_id: None,
            clip_data: None,
            parameters: ActionParameters::default(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            thought_process: None,
            confidence: None,
        }
    }

    /// Create an AddClip action.
    pub fn add_clip(clip: Clip) -> Self {
        let mut action = Self::new(ActionType::AddClip);
        action.clip_id = Some(clip.id.clone());
        action.clip_data = Some(clip);
        action
    }

    /// Create a DeleteClip action.
    pub fn delete_clip(clip_id: ClipId) -> Self {
        let mut action = Self::new(ActionType::DeleteClip);
        action.clip_id = Some(clip_id);
        action
    }

    /// Create a MoveClip action.
    pub fn move_clip(clip_id: ClipId, new_start: MediaTime, new_track: Option<TrackId>) -> Self {
        let mut action = Self::new(ActionType::MoveClip);
        action.clip_id = Some(clip_id);
        action.parameters.new_start_time = Some(new_start);
        action.parameters.new_track_id = new_track;
        action
    }

    /// Create a TrimClip action.
    pub fn trim_clip(
        clip_id: ClipId,
        start_delta: Option<MediaTime>,
        end_delta: Option<MediaTime>,
    ) -> Self {
        let mut action = Self::new(ActionType::TrimClip);
        action.clip_id = Some(clip_id);
        action.parameters.trim_start_delta = start_delta;
        action.parameters.trim_end_delta = end_delta;
        action
    }

    /// Create a SplitClip action.
    pub fn split_clip(clip_id: ClipId, split_time: MediaTime) -> Self {
        let mut action = Self::new(ActionType::SplitClip);
        action.clip_id = Some(clip_id);
        action.parameters.split_time = Some(split_time);
        action
    }

    // =========================================================================
    // MEDIA-BACKED CLIP CREATION
    // =========================================================================

    /// Create an AddClip action from a MediaSource with full duration.
    ///
    /// This creates a clip that uses the entire source media from 0 to source_duration.
    /// The clip embeds all source metadata for self-sufficient recovery.
    ///
    /// # Arguments
    ///
    /// * `source` - Verified MediaSource with probed metadata
    /// * `track_id` - Target track for the clip
    /// * `start` - Timeline position where clip should be placed
    ///
    /// # Example
    ///
    /// ```ignore
    /// let source = import_media(path).await?;
    /// let action = EditAction::add_clip_from_source(&source, "track0", MediaTime::ZERO);
    /// engine.apply_action(action)?;
    /// ```
    pub fn add_clip_from_source(
        source: &crate::media::MediaSource,
        track_id: impl Into<TrackId>,
        start: MediaTime,
    ) -> Self {
        let source_duration = MediaTime::from_seconds(source.duration_secs);

        let clip = Clip {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: track_id.into(),
            start,
            duration: source_duration,
            source_file: source.path.to_string_lossy().to_string(),
            // Embedded source metadata for self-sufficient recovery
            source_duration,
            source_in: MediaTime::ZERO,
            source_out: source_duration,
        };

        let mut action = Self::new(ActionType::AddClip);
        action.clip_id = Some(clip.id.clone());
        action.clip_data = Some(clip);
        action
    }

    /// Create an AddClip action from a MediaSource with custom in/out points.
    ///
    /// # Arguments
    ///
    /// * `source` - Verified MediaSource with probed metadata
    /// * `track_id` - Target track for the clip
    /// * `start` - Timeline position where clip should be placed
    /// * `source_in` - In-point within the source media
    /// * `source_out` - Out-point within the source media
    ///
    /// # Panics
    ///
    /// Panics if source_in >= source_out or source_out > source_duration.
    pub fn add_clip_from_source_range(
        source: &crate::media::MediaSource,
        track_id: impl Into<TrackId>,
        start: MediaTime,
        source_in: MediaTime,
        source_out: MediaTime,
    ) -> Self {
        let source_duration = MediaTime::from_seconds(source.duration_secs);

        assert!(source_out > source_in, "source_out must be > source_in");
        assert!(
            source_out <= source_duration,
            "source_out must be <= source_duration"
        );
        assert!(!source_in.is_negative(), "source_in must be >= 0");

        let clip_duration = source_out - source_in;

        let clip = Clip {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: track_id.into(),
            start,
            duration: clip_duration,
            source_file: source.path.to_string_lossy().to_string(),
            source_duration,
            source_in,
            source_out,
        };

        let mut action = Self::new(ActionType::AddClip);
        action.clip_id = Some(clip.id.clone());
        action.clip_data = Some(clip);
        action
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_creation() {
        let action = EditAction::new(ActionType::AddClip);
        assert_eq!(action.action_type, ActionType::AddClip);
        assert!(action.timestamp > 0);
        assert!(!action.id.is_empty());
    }

    #[test]
    fn test_is_destructive() {
        assert!(!ActionType::AddClip.is_destructive());
        assert!(!ActionType::MoveClip.is_destructive());
        assert!(ActionType::DeleteClip.is_destructive());
        assert!(ActionType::TrimClip.is_destructive());
        assert!(ActionType::SplitClip.is_destructive());
    }

    #[test]
    fn test_add_clip_action() {
        let clip = Clip::new(
            "c1",
            "t1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );

        let action = EditAction::add_clip(clip.clone());
        assert_eq!(action.action_type, ActionType::AddClip);
        assert_eq!(action.clip_id, Some("c1".to_string()));
        assert_eq!(action.clip_data, Some(clip));
    }

    #[test]
    fn test_add_clip_from_source() {
        use crate::media::MediaSource;
        use std::path::PathBuf;

        let source = MediaSource {
            id: "src1".to_string(),
            path: PathBuf::from("/test/video.mp4"),
            duration_secs: 10.0,
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            file_size: 1_000_000,
            display_name: "video.mp4".to_string(),
        };

        let action = EditAction::add_clip_from_source(&source, "track0", MediaTime::ZERO);

        assert_eq!(action.action_type, ActionType::AddClip);
        assert!(action.clip_id.is_some());

        let clip = action.clip_data.unwrap();
        assert_eq!(clip.track_id, "track0");
        assert_eq!(clip.start, MediaTime::ZERO);
        assert_eq!(clip.source_file, "/test/video.mp4");

        // Verify source metadata is embedded
        let expected_duration = MediaTime::from_seconds(10.0);
        assert_eq!(clip.duration, expected_duration);
        assert_eq!(clip.source_duration, expected_duration);
        assert_eq!(clip.source_in, MediaTime::ZERO);
        assert_eq!(clip.source_out, expected_duration);
    }

    #[test]
    fn test_add_clip_from_source_range() {
        use crate::media::MediaSource;
        use std::path::PathBuf;

        let source = MediaSource {
            id: "src1".to_string(),
            path: PathBuf::from("/test/video.mp4"),
            duration_secs: 10.0,
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            file_size: 1_000_000,
            display_name: "video.mp4".to_string(),
        };

        // Create clip from 2s to 7s of source
        let source_in = MediaTime::from_seconds(2.0);
        let source_out = MediaTime::from_seconds(7.0);

        let action = EditAction::add_clip_from_source_range(
            &source,
            "track0",
            MediaTime::from_seconds(5.0),
            source_in,
            source_out,
        );

        let clip = action.clip_data.unwrap();

        // Clip duration should be 5s (7s - 2s)
        assert_eq!(clip.duration, MediaTime::from_seconds(5.0));
        assert_eq!(clip.source_duration, MediaTime::from_seconds(10.0));
        assert_eq!(clip.source_in, source_in);
        assert_eq!(clip.source_out, source_out);
        assert_eq!(clip.start, MediaTime::from_seconds(5.0)); // timeline position
    }

    #[test]
    #[should_panic(expected = "source_out must be > source_in")]
    fn test_add_clip_from_source_range_invalid_range() {
        use crate::media::MediaSource;
        use std::path::PathBuf;

        let source = MediaSource {
            id: "src1".to_string(),
            path: PathBuf::from("/test/video.mp4"),
            duration_secs: 10.0,
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            file_size: 1_000_000,
            display_name: "video.mp4".to_string(),
        };

        // source_in >= source_out should panic
        EditAction::add_clip_from_source_range(
            &source,
            "track0",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            MediaTime::from_seconds(3.0), // out < in
        );
    }
}
