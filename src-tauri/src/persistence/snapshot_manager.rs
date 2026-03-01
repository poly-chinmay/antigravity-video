// src-tauri/src/persistence/snapshot_manager.rs
//! Snapshot Manager - Automatic snapshot creation with intelligent triggers

use crate::persistence::snapshot_store::{SnapshotStore, SNAPSHOT_INTERVAL};
use crate::timeline::TimelineState;
use std::time::Instant;

/// Manager for automatic snapshot creation
pub struct SnapshotManager {
    snapshot_store: SnapshotStore,
    last_snapshot_version: u64,
    last_snapshot_time: Option<Instant>,
}

impl SnapshotManager {
    /// Create a new SnapshotManager
    pub fn new(snapshot_store: SnapshotStore) -> Self {
        Self {
            snapshot_store,
            last_snapshot_version: 0,
            last_snapshot_time: None,
        }
    }

    /// Check if snapshot should be created and create if needed
    ///
    /// Triggers:
    /// 1. Event count % SNAPSHOT_INTERVAL == 0
    /// 2. Estimated replay time > 500ms
    /// 3. Estimated dirty state size > 10MB
    pub fn maybe_create_snapshot(
        &mut self,
        current_version: u64,
        state: &TimelineState,
    ) -> Result<bool, String> {
        let events_since_snapshot = current_version - self.last_snapshot_version;

        // Trigger 1: Interval-based (every 50 events)
        let interval_trigger = current_version % SNAPSHOT_INTERVAL == 0 && current_version > 0;

        // Trigger 2: Replay time threshold (> 500ms)
        // Estimate: ~1ms per event replay
        let estimated_replay_ms = events_since_snapshot;
        let replay_time_trigger = estimated_replay_ms > 500;

        // Trigger 3: Dirty size threshold (> 10MB)
        // Estimate: ~5KB per clip
        let estimated_size_kb = state.clips.len() * 5;
        let estimated_size_mb = estimated_size_kb / 1024;
        let size_trigger = estimated_size_mb > 10;

        let should_snapshot = interval_trigger || replay_time_trigger || size_trigger;

        if should_snapshot {
            println!(
                "📸 [SnapshotManager] Creating snapshot at v{} (interval: {}, replay: {}ms, size: {}MB)",
                current_version,
                interval_trigger,
                estimated_replay_ms,
                estimated_size_mb
            );

            self.snapshot_store
                .save(current_version, state)
                .map_err(|e| format!("Failed to save snapshot: {}", e))?;

            self.last_snapshot_version = current_version;
            self.last_snapshot_time = Some(Instant::now());

            return Ok(true);
        }

        Ok(false)
    }

    /// Get the last snapshot version
    pub fn last_snapshot_version(&self) -> u64 {
        self.last_snapshot_version
    }

    /// Get the snapshot store reference
    pub fn snapshot_store(&self) -> &SnapshotStore {
        &self.snapshot_store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;
    use tempfile::TempDir;

    fn make_clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_snapshot_at_interval() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let mut manager = SnapshotManager::new(store);

        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1"));

        // Should NOT trigger at version 1
        let created = manager.maybe_create_snapshot(1, &state).unwrap();
        assert!(!created);

        // Should trigger at version 50 (SNAPSHOT_INTERVAL)
        let created = manager.maybe_create_snapshot(50, &state).unwrap();
        assert!(created);
        assert_eq!(manager.last_snapshot_version(), 50);

        // Should trigger at version 100
        let created = manager.maybe_create_snapshot(100, &state).unwrap();
        assert!(created);
        assert_eq!(manager.last_snapshot_version(), 100);
    }

    #[test]
    fn test_snapshot_on_replay_threshold() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let mut manager = SnapshotManager::new(store);

        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1"));

        // Create initial snapshot at v50
        manager.maybe_create_snapshot(50, &state).unwrap();

        // Jump to v600 (550 events since last snapshot)
        // Estimated replay time: 550ms > 500ms threshold
        let created = manager.maybe_create_snapshot(600, &state).unwrap();
        assert!(created, "Should trigger on replay time threshold");
        assert_eq!(manager.last_snapshot_version(), 600);
    }

    #[test]
    fn test_snapshot_on_size_threshold() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let mut manager = SnapshotManager::new(store);

        let mut state = TimelineState::new();

        // Add 2500 clips (estimated 12.5 MB > 10MB threshold)
        for i in 0..2500 {
            state.add_clip(make_clip(&format!("clip-{}", i)));
        }

        // Should trigger on size threshold
        let created = manager.maybe_create_snapshot(10, &state).unwrap();
        assert!(created, "Should trigger on size threshold");
    }

    #[test]
    fn test_no_snapshot_below_thresholds() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();
        let mut manager = SnapshotManager::new(store);

        let mut state = TimelineState::new();
        state.add_clip(make_clip("clip-1"));

        // Version 10: not at interval, low replay time, small size
        let created = manager.maybe_create_snapshot(10, &state).unwrap();
        assert!(!created, "Should not trigger below all thresholds");
    }
}
