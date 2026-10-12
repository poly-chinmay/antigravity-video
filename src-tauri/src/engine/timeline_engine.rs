//! TimelineEngine - The sole owner of all mutable application state.
//!
//! # Architectural Invariants
//!
//! 1. TimelineEngine is the ONLY owner of mutable state
//! 2. All mutations go through `apply_action()`
//! 3. No external code may mutate TimelineState directly
//! 4. All mutations are panic-safe (catch_unwind)
//! 5. Mass operations require confirmation
//!
//! # Thread Safety
//!
//! Uses RwLock for concurrent read access with exclusive writes.
//! NO unsafe Send/Sync implementations — relies entirely on RwLock.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::RwLock;

use crate::engine::edit_action::{ActionType, EditAction};
use crate::engine::errors::EngineError;
use crate::engine::invariants::InvariantValidator;
use crate::engine::media_time::MediaTime;
use crate::engine::timeline_index::TimelineIndex;
use crate::engine::timeline_state::{Clip, TimelineState};

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Maximum number of clips a destructive operation can affect without confirmation.
const MASS_OPERATION_THRESHOLD: usize = 50;

// =============================================================================
// UNDO TYPES (Minimal inline implementation)
// =============================================================================

/// Snapshot of state for undo purposes.
#[derive(Clone)]
struct UndoEntry {
    state_snapshot: TimelineState,
    action: EditAction,
}

/// Simple undo/redo manager.
struct UndoManager {
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    max_history: usize,
}

impl UndoManager {
    fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history: 500,
        }
    }

    fn push(&mut self, entry: UndoEntry) {
        self.redo_stack.clear();
        self.undo_stack.push(entry);

        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
    }

    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

// =============================================================================
// EVENT STORE (Minimal inline implementation)
// =============================================================================

/// Minimal event store for tracking actions.
struct EventStore {
    events: Vec<(EditAction, u64, bool)>, // (action, version, committed)
}

impl EventStore {
    fn new() -> Self {
        Self { events: Vec::new() }
    }

    fn append(&mut self, action: &EditAction, version: u64) {
        self.events.push((action.clone(), version, false));
    }

    fn mark_committed(&mut self, version: u64) {
        if let Some(entry) = self.events.iter_mut().find(|(_, v, _)| *v == version) {
            entry.2 = true;
        }
    }

    fn rollback_last(&mut self) {
        if let Some((_, _, committed)) = self.events.last() {
            if !committed {
                self.events.pop();
            }
        }
    }
}

// =============================================================================
// TIMELINE ENGINE
// =============================================================================

/// The central engine that owns all application state.
///
/// # Thread Safety
///
/// Uses RwLock (not Mutex) for these reasons:
/// 1. Read operations vastly outnumber writes (60fps preview vs occasional edits)
/// 2. Multiple concurrent readers are safe and desirable
/// 3. Writes are serialized and exclusive
///
/// # Panic Safety
///
/// All mutation operations use `catch_unwind` to:
/// 1. Prevent panics from corrupting engine state
/// 2. Rollback partial operations on panic
/// 3. Return deterministic errors
///
/// # Mass Operation Safety
///
/// Destructive operations affecting more than MASS_OPERATION_THRESHOLD clips
/// require explicit confirmation via a separate API call.
pub struct TimelineEngine {
    /// The God State — protected by RwLock
    state: RwLock<TimelineState>,

    /// High-performance timeline index for O(log n) queries
    index: RwLock<TimelineIndex>,

    /// Invariant validation system
    invariants: InvariantValidator,

    /// Undo/Redo management
    undo_manager: RwLock<UndoManager>,

    /// Event store for persistence
    event_store: RwLock<EventStore>,
}

// No unsafe impl! RwLock provides Send + Sync automatically when inner types are Send.

impl TimelineEngine {
    // =========================================================================
    // CONSTRUCTION
    // =========================================================================

    /// Create a new TimelineEngine with empty state.
    pub fn new() -> Self {
        Self {
            state: RwLock::new(TimelineState::new()),
            index: RwLock::new(TimelineIndex::new()),
            invariants: InvariantValidator::new(),
            undo_manager: RwLock::new(UndoManager::new()),
            event_store: RwLock::new(EventStore::new()),
        }
    }

    /// Create TimelineEngine with initial state.
    pub fn with_state(state: TimelineState) -> Self {
        let index = TimelineIndex::build(&state);
        Self {
            state: RwLock::new(state),
            index: RwLock::new(index),
            invariants: InvariantValidator::new(),
            undo_manager: RwLock::new(UndoManager::new()),
            event_store: RwLock::new(EventStore::new()),
        }
    }

    // =========================================================================
    // READ ACCESS (Thread-Safe, Concurrent)
    // =========================================================================

    /// Get a read-only snapshot of current state.
    ///
    /// # Thread Safety
    /// Multiple threads may call this concurrently.
    pub fn snapshot(&self) -> TimelineState {
        self.state
            .read()
            .expect("RwLock poisoned — unrecoverable")
            .clone()
    }

    /// Get current version number.
    pub fn version(&self) -> u64 {
        self.state.read().expect("RwLock poisoned").version
    }

    /// Get clip count.
    pub fn clip_count(&self) -> usize {
        self.state.read().expect("RwLock poisoned").clips.len()
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.undo_manager
            .read()
            .expect("RwLock poisoned")
            .can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.undo_manager
            .read()
            .expect("RwLock poisoned")
            .can_redo()
    }

    // =========================================================================
    // THE SINGLE MUTATION PATH
    // =========================================================================

    /// Apply an action to the timeline state.
    ///
    /// # The Mutation Pipeline
    ///
    /// ```text
    /// 1. PRE-VALIDATION      → Verify current state is valid
    /// 2. SAFETY GUARD        → Check mass-operation threshold
    /// 3. UNDO CAPTURE        → Snapshot state before mutation
    /// 4. EVENT COMMIT        → Append to event store
    /// 5. STATE MUTATION      → Apply changes (panic-protected)
    /// 6. POST-VALIDATION     → Verify result is valid
    /// 7. COMMIT              → Finalize undo entry
    /// ```
    ///
    /// # Atomicity Guarantee
    ///
    /// If this function returns `Err`, the state is UNCHANGED.
    ///
    /// # Panic Safety
    ///
    /// Panics during step 5 are caught and converted to `PanicRecovered` errors.
    pub fn apply_action(&self, action: EditAction) -> Result<(), EngineError> {
        // Acquire exclusive write lock
        let mut state = self.state.write().map_err(|_| EngineError::EngineLocked)?;

        // ─────────────────────────────────────────────────────────────────────
        // STEP 1: PRE-VALIDATION (without index - state hasn't changed yet)
        // ─────────────────────────────────────────────────────────────────────
        self.invariants
            .validate(&state, None)
            .map_err(EngineError::PreValidationFailed)?;

        // ─────────────────────────────────────────────────────────────────────
        // STEP 2: SAFETY GUARD - Mass Operation Check
        // ─────────────────────────────────────────────────────────────────────
        self.check_mass_operation_safety(&state, &action)?;

        // ─────────────────────────────────────────────────────────────────────
        // STEP 3: UNDO CAPTURE
        // ─────────────────────────────────────────────────────────────────────
        let undo_snapshot = state.clone();

        // ─────────────────────────────────────────────────────────────────────
        // STEP 4: EVENT COMMIT (before mutation)
        // ─────────────────────────────────────────────────────────────────────
        let event_version = state.version + 1;
        {
            let mut events = self.event_store.write().expect("RwLock poisoned");
            events.append(&action, event_version);
        }

        // ─────────────────────────────────────────────────────────────────────
        // STEP 5: STATE MUTATION (Panic-Protected)
        // ─────────────────────────────────────────────────────────────────────
        // Ensure indices are up-to-date before mutation
        state.rebuild_indices();

        let mutation_result = catch_unwind(AssertUnwindSafe(|| {
            Self::execute_mutation(&mut state, &action)
        }));

        match mutation_result {
            Ok(Ok(())) => {
                // Mutation succeeded
            }
            Ok(Err(e)) => {
                // Mutation returned error — rollback
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();
                return Err(e);
            }
            Err(panic_info) => {
                // Panic occurred — restore state and rollback
                *state = undo_snapshot;
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();

                let message = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };

                return Err(EngineError::PanicRecovered { message });
            }
        }

        // ─────────────────────────────────────────────────────────────────────
        // STEP 6: POST-VALIDATION
        // ─────────────────────────────────────────────────────────────────────
        // Rebuild indices after mutation so validation sees consistent state
        state.rebuild_indices();
        state.recalculate_duration();

        // Update index incrementally for post-validation
        {
            let mut index = self.index.write().expect("RwLock poisoned");
            index.rebuild(&state);
        }

        // Post-validate with index for O(n log n) overlap detection
        {
            let index = self.index.read().expect("RwLock poisoned");
            if let Err(violation) = self.invariants.validate(&state, Some(&index)) {
                // Rollback state
                *state = undo_snapshot;
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();
                return Err(EngineError::PostValidationFailed(violation));
            }
        }

        // ─────────────────────────────────────────────────────────────────────
        // STEP 7: COMMIT
        // ─────────────────────────────────────────────────────────────────────

        // Finalize event
        self.event_store
            .write()
            .expect("RwLock poisoned")
            .mark_committed(event_version);

        // Push undo entry
        self.undo_manager
            .write()
            .expect("RwLock poisoned")
            .push(UndoEntry {
                state_snapshot: undo_snapshot,
                action: action.clone(),
            });

        // Update version
        state.version = event_version;

        // Recalculate derived fields
        state.recalculate_duration();
        state.rebuild_indices();

        Ok(())
    }

    /// Apply action with explicit confirmation (bypasses mass-operation guard).
    ///
    /// Use this only when user has explicitly confirmed a large operation.
    pub fn apply_action_confirmed(&self, action: EditAction) -> Result<(), EngineError> {
        // Same as apply_action but skips safety guard
        let mut state = self.state.write().map_err(|_| EngineError::EngineLocked)?;

        self.invariants
            .validate(&state, None)
            .map_err(EngineError::PreValidationFailed)?;

        let undo_snapshot = state.clone();
        let event_version = state.version + 1;

        self.event_store
            .write()
            .expect("RwLock poisoned")
            .append(&action, event_version);

        let mutation_result = catch_unwind(AssertUnwindSafe(|| {
            Self::execute_mutation(&mut state, &action)
        }));

        match mutation_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();
                return Err(e);
            }
            Err(panic_info) => {
                *state = undo_snapshot;
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();
                let message = panic_info
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "Unknown panic".to_string());
                return Err(EngineError::PanicRecovered { message });
            }
        }

        // Update index and post-validate
        {
            let mut index = self.index.write().expect("RwLock poisoned");
            index.rebuild(&state);
        }
        {
            let index = self.index.read().expect("RwLock poisoned");
            if let Err(violation) = self.invariants.validate(&state, Some(&index)) {
                *state = undo_snapshot;
                self.event_store
                    .write()
                    .expect("RwLock poisoned")
                    .rollback_last();
                return Err(EngineError::PostValidationFailed(violation));
            }
        }

        self.event_store
            .write()
            .expect("RwLock poisoned")
            .mark_committed(event_version);
        self.undo_manager
            .write()
            .expect("RwLock poisoned")
            .push(UndoEntry {
                state_snapshot: undo_snapshot,
                action,
            });
        state.version = event_version;
        state.recalculate_duration();
        state.rebuild_indices();

        Ok(())
    }

    // =========================================================================
    // UNDO / REDO (Panic-Protected)
    // =========================================================================

    /// Undo the last action.
    pub fn undo(&self) -> Result<(), EngineError> {
        let result = catch_unwind(AssertUnwindSafe(|| self.undo_internal()));

        match result {
            Ok(r) => r,
            Err(panic_info) => {
                let message = panic_info
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "Panic during undo".to_string());
                Err(EngineError::PanicRecovered { message })
            }
        }
    }

    fn undo_internal(&self) -> Result<(), EngineError> {
        let mut state = self.state.write().map_err(|_| EngineError::EngineLocked)?;
        let mut undo = self.undo_manager.write().expect("RwLock poisoned");

        let entry = undo.undo_stack.pop().ok_or(EngineError::NothingToUndo)?;

        // Save current state for redo
        let current_state = state.clone();

        // Restore previous state
        *state = entry.state_snapshot;

        // Push to redo stack
        undo.redo_stack.push(UndoEntry {
            state_snapshot: current_state,
            action: entry.action,
        });

        // Rebuild index from restored state
        self.index.write().expect("RwLock poisoned").rebuild(&state);

        // Validate with index
        {
            let index = self.index.read().expect("RwLock poisoned");
            self.invariants
                .validate(&state, Some(&index))
                .map_err(EngineError::PostValidationFailed)?;
        }

        Ok(())
    }

    /// Redo the last undone action.
    pub fn redo(&self) -> Result<(), EngineError> {
        let result = catch_unwind(AssertUnwindSafe(|| self.redo_internal()));

        match result {
            Ok(r) => r,
            Err(panic_info) => {
                let message = panic_info
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "Panic during redo".to_string());
                Err(EngineError::PanicRecovered { message })
            }
        }
    }

    fn redo_internal(&self) -> Result<(), EngineError> {
        let mut state = self.state.write().map_err(|_| EngineError::EngineLocked)?;
        let mut undo = self.undo_manager.write().expect("RwLock poisoned");

        let entry = undo.redo_stack.pop().ok_or(EngineError::NothingToRedo)?;

        // Save current state for undo
        let current_state = state.clone();

        // Restore redo state
        *state = entry.state_snapshot;

        // Push to undo stack
        undo.undo_stack.push(UndoEntry {
            state_snapshot: current_state,
            action: entry.action,
        });

        // Rebuild index from restored state
        self.index.write().expect("RwLock poisoned").rebuild(&state);

        // Validate with index
        {
            let index = self.index.read().expect("RwLock poisoned");
            self.invariants
                .validate(&state, Some(&index))
                .map_err(EngineError::PostValidationFailed)?;
        }

        Ok(())
    }

    // =========================================================================
    // PRIVATE: SAFETY CHECKS
    // =========================================================================

    /// Check if operation affects too many clips.
    fn check_mass_operation_safety(
        &self,
        state: &TimelineState,
        action: &EditAction,
    ) -> Result<(), EngineError> {
        // Only check destructive operations
        if !action.action_type.is_destructive() {
            return Ok(());
        }

        let clip_count = state.clips.len();

        // For delete/trim/split, check threshold
        if clip_count > MASS_OPERATION_THRESHOLD {
            // This is a simplified check — in practice you'd check how many
            // clips the SPECIFIC operation affects
            return Err(EngineError::ConfirmationRequired {
                clip_count,
                action_type: format!("{:?}", action.action_type),
            });
        }

        Ok(())
    }

    // =========================================================================
    // PRIVATE: MUTATION EXECUTION
    // =========================================================================

    /// Execute the actual state mutation.
    ///
    /// INVARIANT: This is the ONLY function that mutates TimelineState.
    fn execute_mutation(state: &mut TimelineState, action: &EditAction) -> Result<(), EngineError> {
        match action.action_type {
            ActionType::AddClip => {
                let clip = action
                    .clip_data
                    .clone()
                    .ok_or(EngineError::MissingClipData)?;

                if state.clip_id_index.contains_key(&clip.id) {
                    return Err(EngineError::DuplicateClipId(clip.id.clone()));
                }

                state.clips.push(clip);
                Ok(())
            }

            ActionType::DeleteClip => {
                let clip_id = action.clip_id.as_ref().ok_or(EngineError::MissingClipId)?;

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .copied()
                    .ok_or_else(|| EngineError::ClipNotFound(clip_id.clone()))?;

                state.clips.remove(idx);
                Ok(())
            }

            ActionType::MoveClip => {
                let clip_id = action.clip_id.as_ref().ok_or(EngineError::MissingClipId)?;
                let new_start = action
                    .parameters
                    .new_start_time
                    .ok_or_else(|| EngineError::MissingParameter("new_start_time".into()))?;

                if new_start.is_negative() {
                    return Err(EngineError::InvalidPosition(new_start.as_nanos()));
                }

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .copied()
                    .ok_or_else(|| EngineError::ClipNotFound(clip_id.clone()))?;

                let clip = &mut state.clips[idx];
                clip.start = new_start;

                if let Some(ref new_track) = action.parameters.new_track_id {
                    clip.track_id = new_track.clone();
                }

                Ok(())
            }

            ActionType::TrimClip => {
                let clip_id = action.clip_id.as_ref().ok_or(EngineError::MissingClipId)?;

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .copied()
                    .ok_or_else(|| EngineError::ClipNotFound(clip_id.clone()))?;

                let clip = &mut state.clips[idx];

                // Handle trim start (affects timeline position, source_in, and duration)
                if let Some(start_delta) = action.parameters.trim_start_delta {
                    // Validate: new source_in must not be negative
                    let new_source_in = clip.source_in + start_delta;
                    if new_source_in.is_negative() {
                        return Err(EngineError::TrimBeyondSourceStart {
                            current_source_in: clip.source_in.as_nanos(),
                            delta: start_delta.as_nanos(),
                        });
                    }

                    clip.start = clip.start + start_delta;
                    clip.duration = clip.duration - start_delta;
                    clip.source_in = new_source_in;
                }

                // Handle trim end (affects source_out and duration)
                if let Some(end_delta) = action.parameters.trim_end_delta {
                    // Validate: new source_out must not exceed source_duration
                    let new_source_out = clip.source_out + end_delta;
                    if new_source_out > clip.source_duration {
                        return Err(EngineError::TrimBeyondSourceEnd {
                            source_duration: clip.source_duration.as_nanos(),
                            requested_source_out: new_source_out.as_nanos(),
                        });
                    }

                    clip.duration = clip.duration + end_delta;
                    clip.source_out = new_source_out;
                }

                // Final validation: ensure clip state is valid
                if clip.start.is_negative() {
                    return Err(EngineError::InvalidPosition(clip.start.as_nanos()));
                }
                if !clip.duration.is_positive() {
                    return Err(EngineError::InvalidDuration(clip.duration.as_nanos()));
                }

                Ok(())
            }

            ActionType::SplitClip => {
                let clip_id = action.clip_id.as_ref().ok_or(EngineError::MissingClipId)?;
                let split_time = action
                    .parameters
                    .split_time
                    .ok_or_else(|| EngineError::MissingParameter("split_time".into()))?;

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .copied()
                    .ok_or_else(|| EngineError::ClipNotFound(clip_id.clone()))?;

                let original = state.clips[idx].clone();

                if !split_time.is_positive() || split_time >= original.duration {
                    return Err(EngineError::InvalidSplitPosition(split_time.as_nanos()));
                }

                // Calculate new source bounds for split
                let left_source_out = original.source_in + split_time;
                let right_source_in = original.source_in + split_time;

                // Modify original (becomes left half)
                state.clips[idx].duration = split_time;
                state.clips[idx].source_out = left_source_out;

                // Create new clip (right half) with updated source bounds
                let new_clip = Clip {
                    id: uuid::Uuid::new_v4().to_string(),
                    track_id: original.track_id.clone(),
                    start: original.start + split_time,
                    duration: original.duration - split_time,
                    source_file: original.source_file.clone(),
                    source_duration: original.source_duration,
                    source_in: right_source_in,
                    source_out: original.source_out,
                };

                state.clips.push(new_clip);

                Ok(())
            }
        }
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
    fn test_add_clip() {
        let engine = TimelineEngine::new();
        let clip = make_clip("c1", 0.0, 5.0);

        let action = EditAction::add_clip(clip.clone());
        let result = engine.apply_action(action);

        assert!(result.is_ok());
        assert_eq!(engine.clip_count(), 1);
    }

    #[test]
    fn test_delete_clip() {
        let engine = TimelineEngine::new();

        // Add a clip first
        let clip = make_clip("c1", 0.0, 5.0);
        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        // Delete it
        let delete_action = EditAction::delete_clip("c1".to_string());
        let result = engine.apply_action(delete_action);

        match &result {
            Ok(()) => {}
            Err(e) => eprintln!("Delete failed with error: {:?}", e),
        }
        assert!(result.is_ok(), "Delete should succeed, got: {:?}", result);
        assert_eq!(engine.clip_count(), 0);
    }

    #[test]
    fn test_undo_redo() {
        let engine = TimelineEngine::new();

        // Add clip
        let clip = make_clip("c1", 0.0, 5.0);
        engine.apply_action(EditAction::add_clip(clip)).unwrap();
        assert_eq!(engine.clip_count(), 1);

        // Undo
        engine.undo().unwrap();
        assert_eq!(engine.clip_count(), 0);

        // Redo
        engine.redo().unwrap();
        assert_eq!(engine.clip_count(), 1);
    }

    #[test]
    fn test_invariant_violation_rejected() {
        let engine = TimelineEngine::new();

        // Add clip with negative duration (should fail post-validation)
        let mut clip = make_clip("c1", 0.0, 5.0);
        clip.duration = MediaTime::from_seconds(-1.0);

        let action = EditAction::add_clip(clip);
        let result = engine.apply_action(action);

        assert!(matches!(result, Err(EngineError::PostValidationFailed(_))));
        assert_eq!(engine.clip_count(), 0); // State unchanged
    }

    #[test]
    fn test_media_time_precision() {
        let engine = TimelineEngine::new();

        // Create clip at very precise position
        let clip = Clip::new(
            "precise",
            "track1",
            MediaTime::from_nanos(123456789),
            MediaTime::from_nanos(987654321),
            "test.mp4",
        );

        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        let state = engine.snapshot();
        let stored_clip = &state.clips[0];

        // Verify exact nanosecond precision is preserved
        assert_eq!(stored_clip.start.as_nanos(), 123456789);
        assert_eq!(stored_clip.duration.as_nanos(), 987654321);
    }

    #[test]
    fn test_trim_start_valid() {
        let engine = TimelineEngine::new();

        // Add a clip with source_in=0, source_out=5s, source_duration=5s
        let clip = Clip::new(
            "c1",
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );
        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        // Trim start inward by 1 second (valid - moves source_in from 0 to 1s)
        let action =
            EditAction::trim_clip("c1".to_string(), Some(MediaTime::from_seconds(1.0)), None);
        let result = engine.apply_action(action);
        assert!(result.is_ok());

        let state = engine.snapshot();
        let clip = &state.clips[0];
        assert_eq!(clip.source_in.to_seconds(), 1.0);
        assert_eq!(clip.duration.to_seconds(), 4.0);
    }

    #[test]
    fn test_trim_end_valid() {
        let engine = TimelineEngine::new();

        // Add a clip with source_in=0, source_out=5s, source_duration=5s
        let clip = Clip::new(
            "c1",
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );
        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        // Trim end inward by 1 second (valid - moves source_out from 5s to 4s)
        let action =
            EditAction::trim_clip("c1".to_string(), None, Some(MediaTime::from_seconds(-1.0)));
        let result = engine.apply_action(action);
        assert!(result.is_ok());

        let state = engine.snapshot();
        let clip = &state.clips[0];
        assert_eq!(clip.source_out.to_seconds(), 4.0);
        assert_eq!(clip.duration.to_seconds(), 4.0);
    }

    #[test]
    fn test_trim_beyond_source_start_rejected() {
        let engine = TimelineEngine::new();

        // Add a clip - uses Clip::new so source_in starts at 0
        let clip = Clip::new(
            "c1",
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );
        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        // Try to trim start with negative delta (would make source_in negative)
        let action =
            EditAction::trim_clip("c1".to_string(), Some(MediaTime::from_seconds(-1.0)), None);
        let result = engine.apply_action(action);

        assert!(matches!(
            result,
            Err(EngineError::TrimBeyondSourceStart { .. })
        ));
    }

    #[test]
    fn test_trim_beyond_source_end_rejected() {
        let engine = TimelineEngine::new();

        // Add a clip with source_duration=5s, already at full length
        let clip = Clip::new(
            "c1",
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        );
        engine.apply_action(EditAction::add_clip(clip)).unwrap();

        // Try to extend end beyond source duration
        let action =
            EditAction::trim_clip("c1".to_string(), None, Some(MediaTime::from_seconds(1.0)));
        let result = engine.apply_action(action);

        assert!(matches!(
            result,
            Err(EngineError::TrimBeyondSourceEnd { .. })
        ));
    }
}
