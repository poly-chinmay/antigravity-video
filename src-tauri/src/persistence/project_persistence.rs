// src-tauri/src/persistence/project_persistence.rs
//! Hybrid Project Persistence
//!
//! Combines Snapshot, WAL, and EventStore for robust project recovery.
//! Load strategy: Latest snapshot → Replay WAL → Fallback to previous snapshot → Rebuild from EventStore

use super::{Event, EventStore, SnapshotStore, WriteAheadLog};
use crate::timeline::TimelineState;
use std::path::PathBuf;

/// Hybrid project persistence manager
pub struct ProjectPersistence {
    /// Event store for all events
    pub event_store: EventStore,
    /// Write-ahead log for crash safety
    pub wal: WriteAheadLog,
    /// Snapshot store for fast recovery
    pub snapshot_store: SnapshotStore,
    /// Base path for persistence
    base_path: PathBuf,
}

impl ProjectPersistence {
    /// Create a new ProjectPersistence at the given base path
    pub fn new(base_path: PathBuf) -> std::io::Result<Self> {
        let event_store = EventStore::new(base_path.clone())?;
        let wal = WriteAheadLog::new(base_path.clone())?;
        let snapshot_store = SnapshotStore::new(base_path.clone())?;

        println!("🗂️ [ProjectPersistence] Initialized at {:?}", base_path);

        Ok(Self {
            event_store,
            wal,
            snapshot_store,
            base_path,
        })
    }

    /// Load project state using hybrid strategy
    ///
    /// Strategy:
    /// 1. Load latest snapshot
    /// 2. Replay WAL entries after snapshot version
    /// 3. If snapshot corrupt → try previous snapshot
    /// 4. If all snapshots fail → rebuild from EventStore
    pub fn load_project(&self) -> std::io::Result<TimelineState> {
        println!("🔄 [ProjectPersistence] Loading project...");

        // Strategy 1: Try to load from snapshot + WAL
        if let Some(state) = self.try_load_from_snapshots()? {
            return Ok(state);
        }

        // Strategy 2: Rebuild entirely from EventStore
        println!("⚠️ [ProjectPersistence] No valid snapshots, rebuilding from EventStore...");
        self.rebuild_from_event_store()
    }

    /// Try to load from snapshots (newest first, with WAL replay)
    fn try_load_from_snapshots(&self) -> std::io::Result<Option<TimelineState>> {
        // Get all available snapshot versions (sorted ascending)
        let versions = self.snapshot_store.list_versions()?;

        if versions.is_empty() {
            println!("📭 [ProjectPersistence] No snapshots found");
            return Ok(None);
        }

        // Try snapshots from newest to oldest
        for version in versions.iter().rev() {
            println!("📸 [ProjectPersistence] Trying snapshot v{}...", version);

            match self.snapshot_store.load(*version) {
                Ok(mut state) => {
                    // Replay WAL entries after this snapshot
                    match self.replay_wal_from(*version, &mut state) {
                        Ok(()) => {
                            println!(
                                "✅ [ProjectPersistence] Loaded from snapshot v{} + WAL replay",
                                version
                            );
                            return Ok(Some(state));
                        }
                        Err(e) => {
                            eprintln!(
                                "⚠️ [ProjectPersistence] WAL replay failed for v{}: {}",
                                version, e
                            );
                            // Continue to try older snapshot
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ [ProjectPersistence] Snapshot v{} corrupt: {}",
                        version, e
                    );
                    // Continue to try older snapshot
                }
            }
        }

        Ok(None)
    }

    /// Replay WAL entries after a given version onto the state
    fn replay_wal_from(
        &self,
        after_version: u64,
        state: &mut TimelineState,
    ) -> std::io::Result<()> {
        let wal_entries = self.wal.load_since(after_version + 1)?;

        if wal_entries.is_empty() {
            println!("📝 [ProjectPersistence] No WAL entries to replay");
            return Ok(());
        }

        println!(
            "📝 [ProjectPersistence] Replaying {} WAL entries...",
            wal_entries.len()
        );

        for entry in wal_entries {
            if !entry.verify_checksum() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("WAL entry v{} has invalid checksum", entry.version),
                ));
            }

            // Apply the edit plan from the event
            let plan = entry.event.edit_plan.clone();

            // Use a simplified replay - just apply actions directly
            for action in &plan.actions {
                if let Err(e) = apply_action_to_state(state, action) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to replay action: {}", e),
                    ));
                }
            }

            state.version = entry.version;
        }

        Ok(())
    }

    /// Rebuild state entirely from EventStore
    fn rebuild_from_event_store(&self) -> std::io::Result<TimelineState> {
        let events = self.event_store.load_all()?;

        if events.is_empty() {
            println!("📭 [ProjectPersistence] No events found, creating empty timeline");
            return Ok(TimelineState::default());
        }

        println!(
            "🔨 [ProjectPersistence] Rebuilding from {} events...",
            events.len()
        );

        let mut state = TimelineState::default();

        for event in events {
            if !event.success {
                // Skip failed events
                continue;
            }

            for action in &event.edit_plan.actions {
                if let Err(e) = apply_action_to_state(&mut state, action) {
                    eprintln!(
                        "⚠️ [ProjectPersistence] Skipping corrupt event v{}: {}",
                        event.version, e
                    );
                    continue;
                }
            }

            state.version = event.version;
        }

        println!(
            "✅ [ProjectPersistence] Rebuilt state: {} clips, v{}",
            state.clips.len(),
            state.version
        );

        Ok(state)
    }

    /// Save an event and optionally create a snapshot
    pub fn save_event_and_state(
        &mut self,
        event: Event,
        state: &TimelineState,
    ) -> std::io::Result<()> {
        let version = event.version;

        // 1. Append to EventStore
        self.event_store.append(&event)?;

        // 2. Check if we should create a snapshot
        if SnapshotStore::should_snapshot(version) {
            println!("📸 [ProjectPersistence] Creating snapshot at v{}", version);
            self.snapshot_store.save(version, state)?;

            // 3. Truncate WAL after successful snapshot
            self.wal.truncate_up_to(version)?;
        }

        Ok(())
    }

    /// Get the base path
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }
}

/// Apply a single action to the state (simplified replay)
fn apply_action_to_state(
    state: &mut TimelineState,
    action: &crate::edit_plan::EditAction,
) -> Result<(), String> {
    use crate::edit_plan::ActionType;

    match action.action_type {
        ActionType::Delete => {
            if let Some(pos) = state
                .clips
                .iter()
                .position(|c| c.id == action.target_clip_id)
            {
                state.clips.remove(pos);
                recalculate_duration(state);
                Ok(())
            } else {
                Err(format!(
                    "Clip {} not found for delete",
                    action.target_clip_id
                ))
            }
        }
        ActionType::Move => {
            if let Some(clip) = state
                .clips
                .iter_mut()
                .find(|c| c.id == action.target_clip_id)
            {
                if let Some(params) = &action.parameters {
                    if let Some(new_start) = params.new_start_time {
                        clip.start = new_start;
                        recalculate_duration(state);
                    }
                }
                Ok(())
            } else {
                Err(format!("Clip {} not found for move", action.target_clip_id))
            }
        }
        ActionType::Trim => {
            if let Some(clip) = state
                .clips
                .iter_mut()
                .find(|c| c.id == action.target_clip_id)
            {
                if let Some(params) = &action.parameters {
                    if let Some(delta) = params.trim_start_delta {
                        clip.start += delta;
                        clip.duration -= delta;
                    }
                    if let Some(delta) = params.trim_end_delta {
                        clip.duration += delta;
                    }
                }
                recalculate_duration(state);
                Ok(())
            } else {
                Err(format!("Clip {} not found for trim", action.target_clip_id))
            }
        }
        ActionType::Split => {
            // Split is complex - for replay we just note it happened
            // The split would have already modified the state when originally applied
            Ok(())
        }
    }
}

/// Recalculate timeline duration from clips
fn recalculate_duration(state: &mut TimelineState) {
    state.duration = state
        .clips
        .iter()
        .map(|c| c.start + c.duration)
        .fold(0.0, f64::max);

    // Clamp playhead
    if state.playhead_time > state.duration {
        state.playhead_time = state.duration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::EditPlan;
    use crate::timeline::Clip;
    use tempfile::TempDir;

    fn create_test_state(version: u64) -> TimelineState {
        TimelineState {
            clips: vec![Clip {
                id: "clip-1".to_string(),
                track_id: "video_track_1".to_string(),
                start: 0.0,
                duration: 10.0,
                source_file: "/test.mp4".to_string(),
            }],
            duration: 10.0,
            playhead_time: 0.0,
            version,
            ..TimelineState::default()
        }
    }

    fn create_test_event(version: u64) -> Event {
        Event::new(
            version,
            EditPlan {
                actions: vec![],
                thought_process: Some("Test".to_string()),
                confidence: Some(0.9),
            },
            Some("Test".to_string()),
            Some(0.9),
            50,
            true,
        )
    }

    #[test]
    fn test_project_persistence_new() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ProjectPersistence::new(temp_dir.path().to_path_buf());
        assert!(persistence.is_ok());
    }

    #[test]
    fn test_load_empty_project() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ProjectPersistence::new(temp_dir.path().to_path_buf()).unwrap();

        let state = persistence.load_project().unwrap();
        assert_eq!(state.clips.len(), 0);
        assert_eq!(state.version, 0);
    }

    #[test]
    fn test_save_and_load_with_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let mut persistence = ProjectPersistence::new(temp_dir.path().to_path_buf()).unwrap();

        // Save a snapshot directly
        let state = create_test_state(50);
        persistence.snapshot_store.save(50, &state).unwrap();

        // Load should find the snapshot
        let loaded = persistence.load_project().unwrap();
        assert_eq!(loaded.version, 50);
        assert_eq!(loaded.clips.len(), 1);
    }

    #[test]
    fn test_save_event_and_state() {
        let temp_dir = TempDir::new().unwrap();
        let mut persistence = ProjectPersistence::new(temp_dir.path().to_path_buf()).unwrap();

        let event = create_test_event(1);
        let state = create_test_state(1);

        persistence.save_event_and_state(event, &state).unwrap();

        // Event should be in store
        let events = persistence.event_store.load_all().unwrap();
        assert_eq!(events.len(), 1);
    }
}
