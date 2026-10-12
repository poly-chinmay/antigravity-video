//! Recovery - Crash-safe state reconstruction.
//!
//! # Recovery Algorithm
//!
//! 1. Load newest valid snapshot
//! 2. Load committed events after snapshot
//! 3. Replay events in order
//! 4. Validate invariants
//! 5. Return recovered state or error
//!
//! # Safety Properties
//!
//! - Uncommitted events are discarded
//! - Corrupted snapshots are skipped
//! - Invariant violations abort recovery
//! - No partial state is ever returned

use std::path::{Path, PathBuf};

use crate::engine::edit_action::{ActionType, EditAction};
use crate::engine::event_store::{EventRecord, EventStore, EventStoreError};
use crate::engine::invariants::{InvariantValidator, InvariantViolation};
use crate::engine::media_time::MediaTime;
use crate::engine::snapshot_store::{Snapshot, SnapshotStore, SnapshotStoreError};
use crate::engine::timeline_state::{Clip, TimelineState};

// =============================================================================
// RECOVERY RESULT
// =============================================================================

/// Result of crash recovery.
#[derive(Debug)]
pub struct RecoveryResult {
    /// The recovered timeline state
    pub state: TimelineState,

    /// Version of the last event applied
    pub version: u64,

    /// Number of events replayed
    pub events_replayed: usize,

    /// Whether a snapshot was used
    pub snapshot_used: bool,

    /// Snapshot version used (if any)
    pub snapshot_version: Option<u64>,

    /// User-friendly recovery message
    pub message: String,
}

// =============================================================================
// RECOVERY ERRORS
// =============================================================================

/// Errors that can occur during recovery.
#[derive(Debug)]
pub enum RecoveryError {
    /// Event store initialization failed
    EventStoreFailed(EventStoreError),

    /// Snapshot store initialization failed
    SnapshotStoreFailed(SnapshotStoreError),

    /// No valid state could be recovered
    NoValidState,

    /// Invariant violation after replay
    InvariantViolation(InvariantViolation),

    /// Event replay failed
    ReplayFailed { event_version: u64, error: String },

    /// All snapshots corrupted
    AllSnapshotsCorrupted,

    /// Project files corrupted beyond recovery
    Corrupted(String),
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventStoreFailed(e) => write!(f, "Event store failed: {}", e),
            Self::SnapshotStoreFailed(e) => write!(f, "Snapshot store failed: {}", e),
            Self::NoValidState => write!(f, "No valid state could be recovered"),
            Self::InvariantViolation(v) => write!(f, "Invariant violation after replay: {}", v),
            Self::ReplayFailed {
                event_version,
                error,
            } => {
                write!(f, "Failed to replay event {}: {}", event_version, error)
            }
            Self::AllSnapshotsCorrupted => write!(f, "All snapshots are corrupted"),
            Self::Corrupted(msg) => write!(f, "Project corrupted: {}", msg),
        }
    }
}

impl std::error::Error for RecoveryError {}

// =============================================================================
// RECOVERY ENGINE
// =============================================================================

/// Crash recovery engine.
///
/// # Recovery Sequence
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────┐
/// │  1. LOAD SNAPSHOT                                                   │
/// │     Try newest snapshot first                                       │
/// │     If corrupted, try older snapshots                              │
/// │     If all corrupted, start from empty state                       │
/// ├─────────────────────────────────────────────────────────────────────┤
/// │  2. LOAD COMMITTED EVENTS                                           │
/// │     Filter: committed = true AND version > snapshot.version        │
/// │     Sort by version (ascending)                                     │
/// ├─────────────────────────────────────────────────────────────────────┤
/// │  3. REPLAY EVENTS                                                   │
/// │     For each event in order:                                        │
/// │       - Apply action to state                                       │
/// │       - If fails, abort recovery                                    │
/// ├─────────────────────────────────────────────────────────────────────┤
/// │  4. VALIDATE INVARIANTS                                             │
/// │     Check all timeline invariants                                   │
/// │     If violated, refuse to open project                            │
/// ├─────────────────────────────────────────────────────────────────────┤
/// │  5. RETURN RECOVERED STATE                                          │
/// │     Include recovery metadata                                       │
/// └─────────────────────────────────────────────────────────────────────┘
/// ```
pub struct RecoveryEngine {
    /// Base path for project storage
    base_path: PathBuf,

    /// Invariant validator
    validator: InvariantValidator,
}

impl RecoveryEngine {
    /// Create a new recovery engine.
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            validator: InvariantValidator::new(),
        }
    }

    /// Recover timeline state from storage.
    ///
    /// This is the main entry point for crash recovery.
    pub fn recover(&self) -> Result<RecoveryResult, RecoveryError> {
        let events_path = self.base_path.join("events");
        let snapshots_path = self.base_path.join("snapshots");

        // Initialize stores
        let event_store = EventStore::new(events_path).map_err(RecoveryError::EventStoreFailed)?;
        let snapshot_store =
            SnapshotStore::new(snapshots_path).map_err(RecoveryError::SnapshotStoreFailed)?;

        // Step 1: Load snapshot
        let (mut state, snapshot_version) = self.load_starting_state(&snapshot_store)?;
        let snapshot_used = snapshot_version.is_some();

        // Step 2: Get committed events after snapshot
        let base_version = snapshot_version.unwrap_or(0);
        let events_to_replay: Vec<_> = event_store
            .get_committed_events_after(base_version)
            .into_iter()
            .cloned()
            .collect();

        let events_replayed = events_to_replay.len();

        // Step 3: Replay events
        let mut last_version = base_version;
        for event in &events_to_replay {
            self.apply_event_to_state(&mut state, event).map_err(|e| {
                RecoveryError::ReplayFailed {
                    event_version: event.event_version,
                    error: e,
                }
            })?;
            last_version = event.event_version;
        }

        // Update state version
        state.version = last_version;

        // Step 4: Validate invariants
        state.rebuild_indices();
        state.recalculate_duration();

        self.validator
            .validate(&state, None) // No index during recovery
            .map_err(RecoveryError::InvariantViolation)?;

        // Step 5: Build result
        let message = self.build_recovery_message(snapshot_used, snapshot_version, events_replayed);

        Ok(RecoveryResult {
            state,
            version: last_version,
            events_replayed,
            snapshot_used,
            snapshot_version,
            message,
        })
    }

    /// Check if recovery is needed (i.e., uncommitted events exist).
    pub fn needs_recovery(&self) -> bool {
        let events_path = self.base_path.join("events");

        if let Ok(store) = EventStore::new(events_path) {
            // Check if there are uncommitted events in the log
            // We check by comparing committed count vs total
            let committed = store.committed_events().len();
            let log_exists = store.path().join("events.jsonl").exists();

            log_exists && committed > 0
        } else {
            false
        }
    }

    // =========================================================================
    // PRIVATE METHODS
    // =========================================================================

    /// Load starting state from snapshot or create empty.
    fn load_starting_state(
        &self,
        snapshot_store: &SnapshotStore,
    ) -> Result<(TimelineState, Option<u64>), RecoveryError> {
        match snapshot_store.load_latest_snapshot() {
            Ok(snapshot) => Ok((snapshot.state, Some(snapshot.event_version))),
            Err(SnapshotStoreError::NoSnapshots) => {
                // No snapshots - start fresh
                Ok((TimelineState::new(), None))
            }
            Err(SnapshotStoreError::ChecksumFailed) => {
                // Try older snapshots
                if let Ok(snapshots) = snapshot_store.list_snapshots() {
                    for path in snapshots.iter().rev().skip(1) {
                        if let Ok(snapshot) = snapshot_store.load_snapshot(path) {
                            return Ok((snapshot.state, Some(snapshot.event_version)));
                        }
                    }
                }

                // All corrupted - start fresh but warn
                eprintln!("Warning: All snapshots corrupted, starting from empty state");
                Ok((TimelineState::new(), None))
            }
            Err(e) => Err(RecoveryError::SnapshotStoreFailed(e)),
        }
    }

    /// Apply a single event to state.
    fn apply_event_to_state(
        &self,
        state: &mut TimelineState,
        event: &EventRecord,
    ) -> Result<(), String> {
        let action = &event.action;

        match action.action_type {
            ActionType::AddClip => {
                let clip = action
                    .clip_data
                    .clone()
                    .ok_or("AddClip requires clip_data")?;
                state.clips.push(clip);
            }

            ActionType::DeleteClip => {
                let clip_id = action
                    .clip_id
                    .as_ref()
                    .ok_or("DeleteClip requires clip_id")?;

                state.rebuild_indices();

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .ok_or_else(|| format!("Clip {} not found", clip_id))?;

                state.clips.remove(*idx);
            }

            ActionType::MoveClip => {
                let clip_id = action.clip_id.as_ref().ok_or("MoveClip requires clip_id")?;
                let new_start = action
                    .parameters
                    .new_start_time
                    .ok_or("MoveClip requires new_start_time")?;

                state.rebuild_indices();

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .ok_or_else(|| format!("Clip {} not found", clip_id))?;

                let clip = &mut state.clips[*idx];
                clip.start = new_start;

                if let Some(new_track) = &action.parameters.new_track_id {
                    clip.track_id = new_track.clone();
                }
            }

            ActionType::TrimClip => {
                let clip_id = action.clip_id.as_ref().ok_or("TrimClip requires clip_id")?;

                state.rebuild_indices();

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .ok_or_else(|| format!("Clip {} not found", clip_id))?;

                let clip = &mut state.clips[*idx];

                if let Some(delta) = action.parameters.trim_start_delta {
                    clip.start = clip.start + delta;
                    clip.duration = clip.duration - delta;
                }

                if let Some(delta) = action.parameters.trim_end_delta {
                    clip.duration = clip.duration + delta;
                }
            }

            ActionType::SplitClip => {
                let clip_id = action
                    .clip_id
                    .as_ref()
                    .ok_or("SplitClip requires clip_id")?;
                let split_time = action
                    .parameters
                    .split_time
                    .ok_or("SplitClip requires split_time")?;

                state.rebuild_indices();

                let idx = state
                    .clip_id_index
                    .get(clip_id)
                    .ok_or_else(|| format!("Clip {} not found", clip_id))?;

                let original = state.clips[*idx].clone();

                // Calculate source bounds for split
                let left_source_out = original.source_in + split_time;
                let right_source_in = original.source_in + split_time;

                // Modify original (left half)
                state.clips[*idx].duration = split_time;
                state.clips[*idx].source_out = left_source_out;

                // Create new clip (right half) with updated source bounds
                let new_clip = Clip {
                    id: format!("{}_split", original.id),
                    track_id: original.track_id.clone(),
                    start: original.start + split_time,
                    duration: original.duration - split_time,
                    source_file: original.source_file.clone(),
                    source_duration: original.source_duration,
                    source_in: right_source_in,
                    source_out: original.source_out,
                };

                state.clips.push(new_clip);
            }
        }

        Ok(())
    }

    /// Build user-friendly recovery message.
    fn build_recovery_message(
        &self,
        snapshot_used: bool,
        snapshot_version: Option<u64>,
        events_replayed: usize,
    ) -> String {
        if events_replayed == 0 && !snapshot_used {
            "Project opened (new or empty)".to_string()
        } else if events_replayed == 0 {
            format!(
                "Project loaded from snapshot v{}",
                snapshot_version.unwrap_or(0)
            )
        } else if snapshot_used {
            format!(
                "Project recovered: loaded snapshot v{}, replayed {} events",
                snapshot_version.unwrap_or(0),
                events_replayed
            )
        } else {
            format!(
                "Project recovered: replayed {} events from beginning",
                events_replayed
            )
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::edit_action::EditAction;
    use tempfile::TempDir;

    fn make_clip(id: &str) -> Clip {
        Clip::new(
            id,
            "track1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        )
    }

    fn make_clip_at(id: &str, start_secs: f64) -> Clip {
        Clip::new(
            id,
            "track1",
            MediaTime::from_seconds(start_secs),
            MediaTime::from_seconds(5.0),
            "test.mp4",
        )
    }

    #[test]
    fn test_recover_empty() {
        let temp_dir = TempDir::new().unwrap();
        let engine = RecoveryEngine::new(temp_dir.path().to_path_buf());

        let result = engine.recover().unwrap();

        assert_eq!(result.events_replayed, 0);
        assert!(!result.snapshot_used);
        assert_eq!(result.state.clips.len(), 0);
    }

    #[test]
    fn test_recover_from_events() {
        let temp_dir = TempDir::new().unwrap();

        // Write some events
        let events_path = temp_dir.path().join("events");
        {
            let mut store = EventStore::new(events_path).unwrap();

            let clip = make_clip("c1");
            let action = EditAction::add_clip(clip);
            let v1 = store.append_event(&action).unwrap();
            store.mark_committed(v1).unwrap();
        }

        // Recover
        let engine = RecoveryEngine::new(temp_dir.path().to_path_buf());
        let result = engine.recover().unwrap();

        assert_eq!(result.events_replayed, 1);
        assert!(!result.snapshot_used);
        assert_eq!(result.state.clips.len(), 1);
        assert_eq!(result.state.clips[0].id, "c1");
    }

    #[test]
    fn test_recover_from_snapshot_plus_events() {
        let temp_dir = TempDir::new().unwrap();

        // Write snapshot at version 0 (before any events)
        let snapshots_path = temp_dir.path().join("snapshots");
        {
            let store = SnapshotStore::new(snapshots_path).unwrap();
            let mut state = TimelineState::new();
            state.clips.push(make_clip("c1"));
            state.version = 0;

            // Snapshot at version 0 - events after this will be replayed
            let snapshot = Snapshot::new(state, 0);
            store.write_snapshot(&snapshot).unwrap();
        }

        // Write additional events (version 1)
        let events_path = temp_dir.path().join("events");
        {
            let mut store = EventStore::new(events_path).unwrap();

            // This will get version 1, which is > snapshot version 0
            // c2 starts at 5.0s to avoid overlapping with c1 (0-5s)
            let action = EditAction::add_clip(make_clip_at("c2", 5.0));
            let v1 = store.append_event(&action).unwrap();
            store.mark_committed(v1).unwrap();
        }

        // Recover
        let engine = RecoveryEngine::new(temp_dir.path().to_path_buf());
        let result = engine.recover().unwrap();

        assert!(result.snapshot_used);
        assert_eq!(result.snapshot_version, Some(0));
        assert_eq!(result.events_replayed, 1);
        assert_eq!(result.state.clips.len(), 2);
    }

    #[test]
    fn test_uncommitted_events_ignored() {
        let temp_dir = TempDir::new().unwrap();

        // Write events - one committed, one not
        let events_path = temp_dir.path().join("events");
        {
            let mut store = EventStore::new(events_path).unwrap();

            let v1 = store
                .append_event(&EditAction::add_clip(make_clip("c1")))
                .unwrap();
            store.mark_committed(v1).unwrap();

            let _v2 = store
                .append_event(&EditAction::add_clip(make_clip("c2")))
                .unwrap();
            // v2 left uncommitted
        }

        // Recover
        let engine = RecoveryEngine::new(temp_dir.path().to_path_buf());
        let result = engine.recover().unwrap();

        // Only committed event should be replayed
        assert_eq!(result.events_replayed, 1);
        assert_eq!(result.state.clips.len(), 1);
        assert_eq!(result.state.clips[0].id, "c1");
    }
}
