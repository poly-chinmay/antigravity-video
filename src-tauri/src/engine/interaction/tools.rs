//! Tools - Editor tool types and behaviors.
//!
//! # Design
//!
//! Each tool defines:
//! - What it can interact with
//! - How it handles mouse events
//! - What preview/commit actions it generates
//!
//! # Available Tools
//!
//! - Select: Click to select, drag for marquee
//! - Move: Drag clips to new position
//! - TrimStart: Drag clip start edge
//! - TrimEnd: Drag clip end edge
//! - Razor: Click to split clip
//! - Playhead: Drag timeline playhead

use serde::{Deserialize, Serialize};

use crate::engine::edit_action::EditAction;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::ClipId;

use super::interaction_state::{DragDelta, DragOrigin, PreviewState};

// =============================================================================
// TOOL TYPE
// =============================================================================

/// Available editor tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolType {
    /// Selection tool
    Select,
    /// Move clip tool
    Move,
    /// Trim clip start
    TrimStart,
    /// Trim clip end
    TrimEnd,
    /// Razor/split tool
    Razor,
    /// Playhead scrub
    Playhead,
}

impl Default for ToolType {
    fn default() -> Self {
        Self::Select
    }
}

// =============================================================================
// TOOL CONTEXT
// =============================================================================

/// Context for tool operations.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Current tool
    pub tool: ToolType,

    /// Snap enabled
    pub snap_enabled: bool,

    /// Snap threshold (nanos)
    pub snap_threshold_ns: i64,

    /// Minimum clip duration (nanos)
    pub min_clip_duration_ns: i64,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            tool: ToolType::Select,
            snap_enabled: true,
            snap_threshold_ns: 100_000_000,    // 100ms
            min_clip_duration_ns: 100_000_000, // 100ms minimum
        }
    }
}

// =============================================================================
// TOOL RESULT
// =============================================================================

/// Result of a tool operation.
#[derive(Debug, Clone)]
pub enum ToolResult {
    /// No action needed
    None,

    /// Preview updated
    Preview(PreviewState),

    /// Selection changed
    Selection(Vec<ClipId>),

    /// Playhead moved
    PlayheadMoved(MediaTime),

    /// Generate edit action
    EditAction(EditAction),
}

// =============================================================================
// MOVE TOOL
// =============================================================================

/// Move tool logic.
pub struct MoveTool;

impl MoveTool {
    /// Calculate preview for move.
    pub fn calculate_preview(
        origin: &DragOrigin,
        delta: &DragDelta,
        snapped_position: MediaTime,
    ) -> Option<PreviewState> {
        let clip_id = origin.clip_id.clone()?;
        let original_duration = origin.original_duration?;

        // Don't allow negative positions
        let preview_start = if snapped_position.as_nanos() < 0 {
            MediaTime::ZERO
        } else {
            snapped_position
        };

        Some(PreviewState::valid(
            clip_id,
            preview_start,
            original_duration,
        ))
    }

    /// Generate edit action for move commit.
    pub fn generate_action(clip_id: &ClipId, new_start: MediaTime) -> EditAction {
        EditAction::move_clip(clip_id.clone(), new_start, None)
    }
}

// =============================================================================
// TRIM TOOL
// =============================================================================

/// Trim tool logic.
pub struct TrimTool;

impl TrimTool {
    /// Calculate preview for trim start.
    pub fn calculate_trim_start_preview(
        origin: &DragOrigin,
        delta: &DragDelta,
        snapped_position: MediaTime,
        min_duration_ns: i64,
    ) -> Option<PreviewState> {
        let clip_id = origin.clip_id.clone()?;
        let original_start = origin.original_start?;
        let original_duration = origin.original_duration?;
        let original_end = original_start + original_duration;

        // New start position
        let new_start = snapped_position.max(MediaTime::ZERO);

        // Calculate new duration
        let new_duration = if original_end > new_start {
            original_end - new_start
        } else {
            MediaTime::from_nanos(min_duration_ns)
        };

        // Enforce minimum duration
        if new_duration.as_nanos() < min_duration_ns {
            return Some(PreviewState::invalid(
                clip_id,
                new_start,
                new_duration,
                "Duration too short".to_string(),
            ));
        }

        Some(PreviewState::valid(clip_id, new_start, new_duration))
    }

    /// Calculate preview for trim end.
    pub fn calculate_trim_end_preview(
        origin: &DragOrigin,
        delta: &DragDelta,
        snapped_position: MediaTime,
        min_duration_ns: i64,
    ) -> Option<PreviewState> {
        let clip_id = origin.clip_id.clone()?;
        let original_start = origin.original_start?;

        // New end position = snapped_position
        // Calculate new duration based on user intent
        let new_duration = if snapped_position > original_start {
            snapped_position - original_start
        } else {
            MediaTime::ZERO
        };

        // Check if duration is below minimum
        if new_duration.as_nanos() < min_duration_ns {
            return Some(PreviewState::invalid(
                clip_id,
                original_start,
                new_duration,
                "Duration too short".to_string(),
            ));
        }

        Some(PreviewState::valid(clip_id, original_start, new_duration))
    }

    /// Generate edit action for trim.
    pub fn generate_action(
        clip_id: &ClipId,
        new_start: MediaTime,
        new_duration: MediaTime,
    ) -> EditAction {
        // Trim uses delta from original position
        // For now, we calculate deltas based on preview values
        // The caller should track original values to compute proper deltas
        let start_delta = Some(new_start); // This is simplified - needs refactoring
        EditAction::trim_clip(clip_id.clone(), start_delta, None)
    }
}

// =============================================================================
// SELECT TOOL
// =============================================================================

/// Select tool logic.
pub struct SelectTool;

impl SelectTool {
    /// Handle click selection.
    pub fn handle_click(
        clip_id: Option<ClipId>,
        shift_held: bool,
        current_selection: &[ClipId],
    ) -> ToolResult {
        match clip_id {
            Some(id) => {
                if shift_held {
                    // Toggle selection
                    let mut new_selection = current_selection.to_vec();
                    if new_selection.contains(&id) {
                        new_selection.retain(|c| c != &id);
                    } else {
                        new_selection.push(id);
                    }
                    ToolResult::Selection(new_selection)
                } else {
                    // Replace selection
                    ToolResult::Selection(vec![id])
                }
            }
            None => {
                // Click on empty space
                if shift_held {
                    ToolResult::None
                } else {
                    ToolResult::Selection(vec![])
                }
            }
        }
    }
}

// =============================================================================
// RAZOR TOOL
// =============================================================================

/// Razor tool logic.
pub struct RazorTool;

impl RazorTool {
    /// Generate split action.
    pub fn generate_action(clip_id: &ClipId, split_position: MediaTime) -> EditAction {
        EditAction::split_clip(clip_id.clone(), split_position)
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

    #[test]
    fn test_move_preview() {
        let origin = DragOrigin::for_clip(
            "clip1".to_string(),
            ms(1000),
            100.0,
            50.0,
            ms(1000),
            ms(5000),
        );
        let delta = DragDelta {
            timeline_offset: ms(500),
            screen_dx: 50.0,
            screen_dy: 0.0,
        };

        let preview = MoveTool::calculate_preview(&origin, &delta, ms(1500)).unwrap();

        assert_eq!(preview.preview_start, ms(1500));
        assert_eq!(preview.preview_duration, ms(5000));
        assert!(preview.is_valid);
    }

    #[test]
    fn test_move_prevents_negative() {
        let origin = DragOrigin::for_clip(
            "clip1".to_string(),
            ms(1000),
            100.0,
            50.0,
            ms(1000),
            ms(5000),
        );
        let delta = DragDelta::default();

        let preview = MoveTool::calculate_preview(&origin, &delta, ms(-500)).unwrap();

        assert_eq!(preview.preview_start, MediaTime::ZERO);
    }

    #[test]
    fn test_trim_start_preview() {
        let origin = DragOrigin::for_clip(
            "clip1".to_string(),
            ms(1000),
            100.0,
            50.0,
            ms(1000),
            ms(5000),
        );
        let delta = DragDelta::default();

        let preview =
            TrimTool::calculate_trim_start_preview(&origin, &delta, ms(2000), 100_000_000).unwrap();

        // Original end was 6000ms, new start is 2000ms
        assert_eq!(preview.preview_start, ms(2000));
        assert_eq!(preview.preview_duration, ms(4000)); // 6000 - 2000
        assert!(preview.is_valid);
    }

    #[test]
    fn test_trim_end_preview() {
        let origin = DragOrigin::for_clip(
            "clip1".to_string(),
            ms(1000),
            100.0,
            50.0,
            ms(1000),
            ms(5000),
        );
        let delta = DragDelta::default();

        let preview =
            TrimTool::calculate_trim_end_preview(&origin, &delta, ms(4000), 100_000_000).unwrap();

        assert_eq!(preview.preview_start, ms(1000));
        assert_eq!(preview.preview_duration, ms(3000)); // 4000 - 1000
        assert!(preview.is_valid);
    }

    #[test]
    fn test_trim_enforces_minimum() {
        let origin = DragOrigin::for_clip(
            "clip1".to_string(),
            ms(1000),
            100.0,
            50.0,
            ms(1000),
            ms(5000),
        );
        let delta = DragDelta::default();

        // Try to trim to 50ms duration (less than 100ms minimum)
        let preview =
            TrimTool::calculate_trim_end_preview(&origin, &delta, ms(1050), 100_000_000).unwrap();

        // Should be clamped to minimum
        assert!(!preview.is_valid);
    }

    #[test]
    fn test_select_click() {
        let result = SelectTool::handle_click(Some("clip1".to_string()), false, &[]);

        match result {
            ToolResult::Selection(clips) => {
                assert_eq!(clips, vec!["clip1".to_string()]);
            }
            _ => panic!("Expected selection"),
        }
    }

    #[test]
    fn test_select_shift_toggle() {
        let result =
            SelectTool::handle_click(Some("clip2".to_string()), true, &["clip1".to_string()]);

        match result {
            ToolResult::Selection(clips) => {
                assert!(clips.contains(&"clip1".to_string()));
                assert!(clips.contains(&"clip2".to_string()));
            }
            _ => panic!("Expected selection"),
        }
    }
}
