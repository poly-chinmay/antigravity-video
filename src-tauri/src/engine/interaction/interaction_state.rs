//! InteractionState - State machine for editor interactions.
//!
//! # Design
//!
//! Interactions follow a strict state machine:
//!
//! ```text
//! Idle ──▶ Hovering ──▶ Dragging ──▶ Committed
//!  ▲           │            │            │
//!  └───────────┴────────────┴────────────┘
//! ```
//!
//! # Invariants
//!
//! - Only one active interaction at a time
//! - Dragging produces preview only
//! - Commit only on explicit mouse_up
//! - All transitions are explicit

use serde::{Deserialize, Serialize};

use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::ClipId;

// =============================================================================
// INTERACTION PHASE
// =============================================================================

/// Current phase of an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionPhase {
    /// No active interaction
    Idle,
    /// Hovering over an interactable element
    Hovering,
    /// Actively dragging
    Dragging,
    /// Interaction committed (final)
    Committed,
    /// Interaction cancelled
    Cancelled,
}

impl Default for InteractionPhase {
    fn default() -> Self {
        Self::Idle
    }
}

// =============================================================================
// DRAG ORIGIN
// =============================================================================

/// Where a drag started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragOrigin {
    /// Timeline position where drag started
    pub timeline_position: MediaTime,

    /// Screen X coordinate
    pub screen_x: f64,

    /// Screen Y coordinate
    pub screen_y: f64,

    /// Target clip (if any)
    pub clip_id: Option<ClipId>,

    /// Original clip start position (for move)
    pub original_start: Option<MediaTime>,

    /// Original clip duration (for trim)
    pub original_duration: Option<MediaTime>,
}

impl DragOrigin {
    /// Create a new drag origin.
    pub fn new(timeline_position: MediaTime, screen_x: f64, screen_y: f64) -> Self {
        Self {
            timeline_position,
            screen_x,
            screen_y,
            clip_id: None,
            original_start: None,
            original_duration: None,
        }
    }

    /// Create for a specific clip.
    pub fn for_clip(
        clip_id: ClipId,
        timeline_position: MediaTime,
        screen_x: f64,
        screen_y: f64,
        original_start: MediaTime,
        original_duration: MediaTime,
    ) -> Self {
        Self {
            timeline_position,
            screen_x,
            screen_y,
            clip_id: Some(clip_id),
            original_start: Some(original_start),
            original_duration: Some(original_duration),
        }
    }
}

// =============================================================================
// DRAG DELTA
// =============================================================================

/// Current drag offset from origin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DragDelta {
    /// Timeline offset
    pub timeline_offset: MediaTime,

    /// Screen X offset
    pub screen_dx: f64,

    /// Screen Y offset
    pub screen_dy: f64,
}

impl DragDelta {
    /// Create from current position and origin.
    pub fn from_positions(
        current_timeline: MediaTime,
        current_x: f64,
        current_y: f64,
        origin: &DragOrigin,
    ) -> Self {
        let timeline_offset = if current_timeline >= origin.timeline_position {
            current_timeline - origin.timeline_position
        } else {
            // Negative offset (moving left)
            MediaTime::from_nanos(-(origin.timeline_position - current_timeline).as_nanos())
        };

        Self {
            timeline_offset,
            screen_dx: current_x - origin.screen_x,
            screen_dy: current_y - origin.screen_y,
        }
    }
}

// =============================================================================
// PREVIEW STATE
// =============================================================================

/// Preview state during drag (before commit).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewState {
    /// Clip being previewed
    pub clip_id: ClipId,

    /// Preview start position
    pub preview_start: MediaTime,

    /// Preview duration
    pub preview_duration: MediaTime,

    /// Whether preview is valid
    pub is_valid: bool,

    /// Reason if invalid
    pub invalid_reason: Option<String>,
}

impl PreviewState {
    /// Create a valid preview.
    pub fn valid(clip_id: ClipId, start: MediaTime, duration: MediaTime) -> Self {
        Self {
            clip_id,
            preview_start: start,
            preview_duration: duration,
            is_valid: true,
            invalid_reason: None,
        }
    }

    /// Create an invalid preview.
    pub fn invalid(clip_id: ClipId, start: MediaTime, duration: MediaTime, reason: String) -> Self {
        Self {
            clip_id,
            preview_start: start,
            preview_duration: duration,
            is_valid: false,
            invalid_reason: Some(reason),
        }
    }
}

// =============================================================================
// INTERACTION STATE
// =============================================================================

/// Complete interaction state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InteractionState {
    /// Current phase
    pub phase: InteractionPhase,

    /// Drag origin (if dragging)
    pub drag_origin: Option<DragOrigin>,

    /// Current drag delta
    pub drag_delta: DragDelta,

    /// Preview state (if any)
    pub preview: Option<PreviewState>,

    /// Selected clips
    pub selected_clips: Vec<ClipId>,

    /// Hovered clip (if any)
    pub hovered_clip: Option<ClipId>,
}

impl InteractionState {
    /// Create idle state.
    pub fn idle() -> Self {
        Self::default()
    }

    /// Check if idle.
    pub fn is_idle(&self) -> bool {
        self.phase == InteractionPhase::Idle
    }

    /// Check if dragging.
    pub fn is_dragging(&self) -> bool {
        self.phase == InteractionPhase::Dragging
    }

    /// Check if has preview.
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }

    /// Check if preview is valid.
    pub fn is_preview_valid(&self) -> bool {
        self.preview.as_ref().map(|p| p.is_valid).unwrap_or(false)
    }

    // =========================================================================
    // TRANSITIONS
    // =========================================================================

    /// Transition to hovering.
    pub fn hover(&mut self, clip_id: Option<ClipId>) {
        if self.is_idle() {
            self.phase = InteractionPhase::Hovering;
            self.hovered_clip = clip_id;
        }
    }

    /// Transition to idle.
    pub fn to_idle(&mut self) {
        self.phase = InteractionPhase::Idle;
        self.drag_origin = None;
        self.drag_delta = DragDelta::default();
        self.preview = None;
        self.hovered_clip = None;
    }

    /// Start dragging.
    pub fn start_drag(&mut self, origin: DragOrigin) {
        self.phase = InteractionPhase::Dragging;
        self.drag_origin = Some(origin);
        self.drag_delta = DragDelta::default();
    }

    /// Update drag position.
    pub fn update_drag(&mut self, current_timeline: MediaTime, current_x: f64, current_y: f64) {
        if let Some(ref origin) = self.drag_origin {
            self.drag_delta =
                DragDelta::from_positions(current_timeline, current_x, current_y, origin);
        }
    }

    /// Set preview.
    pub fn set_preview(&mut self, preview: PreviewState) {
        self.preview = Some(preview);
    }

    /// Clear preview.
    pub fn clear_preview(&mut self) {
        self.preview = None;
    }

    /// Commit interaction.
    pub fn commit(&mut self) {
        self.phase = InteractionPhase::Committed;
    }

    /// Cancel interaction.
    pub fn cancel(&mut self) {
        self.phase = InteractionPhase::Cancelled;
        self.preview = None;
        self.drag_origin = None;
        self.drag_delta = DragDelta::default();
    }

    /// Reset to idle.
    pub fn reset(&mut self) {
        *self = Self::idle();
    }

    // =========================================================================
    // SELECTION
    // =========================================================================

    /// Select a clip.
    pub fn select(&mut self, clip_id: ClipId) {
        if !self.selected_clips.contains(&clip_id) {
            self.selected_clips.push(clip_id);
        }
    }

    /// Deselect a clip.
    pub fn deselect(&mut self, clip_id: &ClipId) {
        self.selected_clips.retain(|id| id != clip_id);
    }

    /// Clear selection.
    pub fn clear_selection(&mut self) {
        self.selected_clips.clear();
    }

    /// Set selection to single clip.
    pub fn select_only(&mut self, clip_id: ClipId) {
        self.selected_clips.clear();
        self.selected_clips.push(clip_id);
    }

    /// Check if clip is selected.
    pub fn is_selected(&self, clip_id: &ClipId) -> bool {
        self.selected_clips.contains(clip_id)
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
    fn test_state_new() {
        let state = InteractionState::idle();

        assert!(state.is_idle());
        assert!(!state.is_dragging());
        assert!(!state.has_preview());
    }

    #[test]
    fn test_hover_transition() {
        let mut state = InteractionState::idle();

        state.hover(Some("clip1".to_string()));

        assert_eq!(state.phase, InteractionPhase::Hovering);
        assert_eq!(state.hovered_clip, Some("clip1".to_string()));
    }

    #[test]
    fn test_drag_transition() {
        let mut state = InteractionState::idle();

        let origin = DragOrigin::new(ms(1000), 100.0, 50.0);
        state.start_drag(origin);

        assert!(state.is_dragging());
        assert!(state.drag_origin.is_some());
    }

    #[test]
    fn test_drag_update() {
        let mut state = InteractionState::idle();

        let origin = DragOrigin::new(ms(1000), 100.0, 50.0);
        state.start_drag(origin);

        // Update to new position
        state.update_drag(ms(2000), 200.0, 50.0);

        assert_eq!(state.drag_delta.timeline_offset, ms(1000));
        assert_eq!(state.drag_delta.screen_dx, 100.0);
    }

    #[test]
    fn test_preview_state() {
        let mut state = InteractionState::idle();

        let preview = PreviewState::valid("clip1".to_string(), ms(2000), ms(5000));
        state.set_preview(preview);

        assert!(state.has_preview());
        assert!(state.is_preview_valid());
    }

    #[test]
    fn test_commit_transition() {
        let mut state = InteractionState::idle();

        state.start_drag(DragOrigin::new(ms(0), 0.0, 0.0));
        state.commit();

        assert_eq!(state.phase, InteractionPhase::Committed);
    }

    #[test]
    fn test_cancel_clears_preview() {
        let mut state = InteractionState::idle();

        state.start_drag(DragOrigin::new(ms(0), 0.0, 0.0));
        state.set_preview(PreviewState::valid("clip1".to_string(), ms(0), ms(1000)));

        state.cancel();

        assert_eq!(state.phase, InteractionPhase::Cancelled);
        assert!(!state.has_preview());
    }

    #[test]
    fn test_selection() {
        let mut state = InteractionState::idle();

        state.select("clip1".to_string());
        state.select("clip2".to_string());

        assert!(state.is_selected(&"clip1".to_string()));
        assert!(state.is_selected(&"clip2".to_string()));

        state.deselect(&"clip1".to_string());

        assert!(!state.is_selected(&"clip1".to_string()));
        assert!(state.is_selected(&"clip2".to_string()));
    }

    #[test]
    fn test_select_only() {
        let mut state = InteractionState::idle();

        state.select("clip1".to_string());
        state.select("clip2".to_string());

        state.select_only("clip3".to_string());

        assert!(!state.is_selected(&"clip1".to_string()));
        assert!(!state.is_selected(&"clip2".to_string()));
        assert!(state.is_selected(&"clip3".to_string()));
    }
}
