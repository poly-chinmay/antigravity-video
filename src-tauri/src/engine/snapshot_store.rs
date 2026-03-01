//! Snapshot Store - Atomic point-in-time state snapshots.
//!
//! # Design Principles
//!
//! 1. Snapshots are atomic (write temp → fsync → rename)
//! 2. Never overwrite previous snapshots (versioned)
//! 3. Checksummed for corruption detection
//! 4. Compressed for storage efficiency
//!
//! # Recovery Role
//!
//! Snapshots reduce recovery time by providing a starting point
//! closer to the current state than replaying all events.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::engine::timeline_state::TimelineState;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Minimum disk space required before snapshot write (10MB)
const MIN_DISK_SPACE_BYTES: u64 = 10_485_760;

/// Snapshot file extension
const SNAPSHOT_EXTENSION: &str = "snapshot";

/// Maximum number of snapshots to retain
const MAX_SNAPSHOTS: usize = 10;

// =============================================================================
// SNAPSHOT
// =============================================================================

/// A point-in-time snapshot of timeline state.
///
/// # Invariants
///
/// - `event_version` matches the last applied event
/// - `checksum` is verified on load
/// - State is valid (invariants satisfied at snapshot time)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// The timeline state at this point
    pub state: TimelineState,

    /// Version of the last event applied before this snapshot
    pub event_version: u64,

    /// UTC timestamp when snapshot was created (nanoseconds)
    pub timestamp: u64,

    /// CRC32 checksum of serialized state
    pub checksum: u32,

    /// Schema version for future compatibility
    pub schema_version: u32,
}

impl Snapshot {
    /// Create a new snapshot from current state.
    pub fn new(state: TimelineState, event_version: u64) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut snapshot = Self {
            state,
            event_version,
            timestamp,
            checksum: 0,
            schema_version: 1,
        };

        snapshot.checksum = snapshot.compute_checksum();
        snapshot
    }

    /// Compute checksum of snapshot (excluding checksum field).
    fn compute_checksum(&self) -> u32 {
        // Serialize state for hashing
        let state_bytes = serde_json::to_vec(&self.state).unwrap_or_default();

        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&state_bytes);
        hasher.update(&self.event_version.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize()
    }

    /// Verify checksum is valid.
    pub fn verify_checksum(&self) -> bool {
        let expected = self.compute_checksum();
        self.checksum == expected
    }
}

// =============================================================================
// SNAPSHOT STORE ERRORS
// =============================================================================

/// Errors that can occur in snapshot store operations
#[derive(Debug)]
pub enum SnapshotStoreError {
    /// Failed to create storage directory
    CreateDirFailed(io::Error),

    /// Failed to write snapshot
    WriteFailed(io::Error),

    /// Failed to read snapshot
    ReadFailed(io::Error),

    /// Failed to sync to disk
    SyncFailed(io::Error),

    /// Failed to rename temp file
    RenameFailed(io::Error),

    /// Insufficient disk space
    InsufficientSpace { required: u64, available: u64 },

    /// Failed to serialize snapshot
    SerializeFailed(String),

    /// Failed to deserialize snapshot
    DeserializeFailed(String),

    /// Checksum verification failed
    ChecksumFailed,

    /// No snapshots found
    NoSnapshots,

    /// Snapshot corrupted
    Corrupted(String),
}

impl std::fmt::Display for SnapshotStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirFailed(e) => write!(f, "Failed to create snapshot directory: {}", e),
            Self::WriteFailed(e) => write!(f, "Failed to write snapshot: {}", e),
            Self::ReadFailed(e) => write!(f, "Failed to read snapshot: {}", e),
            Self::SyncFailed(e) => write!(f, "Failed to sync snapshot: {}", e),
            Self::RenameFailed(e) => write!(f, "Failed to rename snapshot: {}", e),
            Self::InsufficientSpace {
                required,
                available,
            } => {
                write!(
                    f,
                    "Insufficient disk space: need {} bytes, have {}",
                    required, available
                )
            }
            Self::SerializeFailed(e) => write!(f, "Failed to serialize snapshot: {}", e),
            Self::DeserializeFailed(e) => write!(f, "Failed to deserialize snapshot: {}", e),
            Self::ChecksumFailed => write!(f, "Snapshot checksum verification failed"),
            Self::NoSnapshots => write!(f, "No snapshots found"),
            Self::Corrupted(msg) => write!(f, "Snapshot corrupted: {}", msg),
        }
    }
}

impl std::error::Error for SnapshotStoreError {}

// =============================================================================
// SNAPSHOT STORE
// =============================================================================

/// Storage for point-in-time timeline snapshots.
pub struct SnapshotStore {
    /// Base directory for snapshot storage
    base_path: PathBuf,
}

impl SnapshotStore {
    /// Create or open a snapshot store at the given path.
    pub fn new(base_path: PathBuf) -> Result<Self, SnapshotStoreError> {
        fs::create_dir_all(&base_path).map_err(SnapshotStoreError::CreateDirFailed)?;

        Ok(Self { base_path })
    }

    /// Write a new snapshot atomically.
    ///
    /// # Atomicity
    ///
    /// 1. Write to temp file
    /// 2. Fsync temp file
    /// 3. Rename to final location
    /// 4. Fsync directory
    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<PathBuf, SnapshotStoreError> {
        // Check disk space
        self.check_disk_space()?;

        // Serialize with compression
        let data = self.serialize_snapshot(snapshot)?;

        // Generate filename
        let filename = format!(
            "v{:08}_{}.{}",
            snapshot.event_version, snapshot.timestamp, SNAPSHOT_EXTENSION
        );
        let final_path = self.base_path.join(&filename);
        let temp_path = self.base_path.join(format!(".{}.tmp", filename));

        // Write to temp file
        {
            let mut file = File::create(&temp_path).map_err(SnapshotStoreError::WriteFailed)?;

            file.write_all(&data)
                .map_err(SnapshotStoreError::WriteFailed)?;

            file.sync_all().map_err(SnapshotStoreError::SyncFailed)?;
        }

        // Atomic rename
        fs::rename(&temp_path, &final_path).map_err(SnapshotStoreError::RenameFailed)?;

        // Sync directory
        Self::sync_directory(&self.base_path)?;

        // Cleanup old snapshots
        self.cleanup_old_snapshots();

        Ok(final_path)
    }

    /// Load the latest valid snapshot.
    pub fn load_latest_snapshot(&self) -> Result<Snapshot, SnapshotStoreError> {
        let snapshots = self.list_snapshots()?;

        if snapshots.is_empty() {
            return Err(SnapshotStoreError::NoSnapshots);
        }

        // Try snapshots from newest to oldest
        for path in snapshots.into_iter().rev() {
            match self.load_snapshot(&path) {
                Ok(snapshot) => return Ok(snapshot),
                Err(e) => {
                    eprintln!("Warning: Snapshot {:?} corrupted: {}", path, e);
                    // Continue to next snapshot
                }
            }
        }

        Err(SnapshotStoreError::NoSnapshots)
    }

    /// Load a specific snapshot file.
    pub fn load_snapshot(&self, path: &Path) -> Result<Snapshot, SnapshotStoreError> {
        // Read file
        let mut file = File::open(path).map_err(SnapshotStoreError::ReadFailed)?;

        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(SnapshotStoreError::ReadFailed)?;

        // Deserialize
        let mut snapshot = self.deserialize_snapshot(&data)?;

        // Verify checksum
        if !snapshot.verify_checksum() {
            return Err(SnapshotStoreError::ChecksumFailed);
        }

        // Rebuild indices (not serialized)
        snapshot.state.rebuild_indices();

        Ok(snapshot)
    }

    /// Load snapshot at or before a specific event version.
    pub fn load_snapshot_at_version(
        &self,
        max_version: u64,
    ) -> Result<Snapshot, SnapshotStoreError> {
        let snapshots = self.list_snapshots()?;

        // Filter to snapshots at or before requested version
        for path in snapshots.into_iter().rev() {
            if let Some(version) = self.extract_version_from_path(&path) {
                if version <= max_version {
                    return self.load_snapshot(&path);
                }
            }
        }

        Err(SnapshotStoreError::NoSnapshots)
    }

    /// List all snapshot files, sorted by version (oldest first).
    pub fn list_snapshots(&self) -> Result<Vec<PathBuf>, SnapshotStoreError> {
        let mut snapshots: Vec<PathBuf> = fs::read_dir(&self.base_path)
            .map_err(SnapshotStoreError::ReadFailed)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext == SNAPSHOT_EXTENSION)
                    .unwrap_or(false)
            })
            .collect();

        // Sort by filename (version is first in name)
        snapshots.sort();

        Ok(snapshots)
    }

    /// Get path for external use.
    pub fn path(&self) -> &Path {
        &self.base_path
    }

    // =========================================================================
    // PRIVATE METHODS
    // =========================================================================

    /// Serialize snapshot with optional compression.
    fn serialize_snapshot(&self, snapshot: &Snapshot) -> Result<Vec<u8>, SnapshotStoreError> {
        let json = serde_json::to_vec(snapshot)
            .map_err(|e| SnapshotStoreError::SerializeFailed(e.to_string()))?;

        // Compress with zstd (if available) or return raw JSON
        #[cfg(feature = "compression")]
        {
            zstd::encode_all(&json[..], 3)
                .map_err(|e| SnapshotStoreError::SerializeFailed(e.to_string()))
        }

        #[cfg(not(feature = "compression"))]
        Ok(json)
    }

    /// Deserialize snapshot.
    fn deserialize_snapshot(&self, data: &[u8]) -> Result<Snapshot, SnapshotStoreError> {
        // Try decompression first (if compressed)
        #[cfg(feature = "compression")]
        let json = match zstd::decode_all(data) {
            Ok(decompressed) => decompressed,
            Err(_) => data.to_vec(), // Try as raw JSON
        };

        #[cfg(not(feature = "compression"))]
        let json = data;

        serde_json::from_slice(&json)
            .map_err(|e| SnapshotStoreError::DeserializeFailed(e.to_string()))
    }

    /// Extract event version from snapshot filename.
    fn extract_version_from_path(&self, path: &Path) -> Option<u64> {
        path.file_name()?
            .to_str()?
            .strip_prefix('v')?
            .split('_')
            .next()?
            .parse()
            .ok()
    }

    /// Check available disk space.
    fn check_disk_space(&self) -> Result<(), SnapshotStoreError> {
        match fs2::available_space(&self.base_path) {
            Ok(available) => {
                if available < MIN_DISK_SPACE_BYTES {
                    return Err(SnapshotStoreError::InsufficientSpace {
                        required: MIN_DISK_SPACE_BYTES,
                        available,
                    });
                }
                Ok(())
            }
            Err(_) => Ok(()), // Proceed with caution
        }
    }

    /// Sync directory for filesystem metadata durability.
    fn sync_directory(dir: &Path) -> Result<(), SnapshotStoreError> {
        #[cfg(unix)]
        {
            if let Ok(dir_file) = File::open(dir) {
                let _ = dir_file.sync_all();
            }
        }

        Ok(())
    }

    /// Remove old snapshots, keeping only the newest MAX_SNAPSHOTS.
    fn cleanup_old_snapshots(&self) {
        if let Ok(snapshots) = self.list_snapshots() {
            if snapshots.len() > MAX_SNAPSHOTS {
                let to_remove = snapshots.len() - MAX_SNAPSHOTS;
                for path in snapshots.into_iter().take(to_remove) {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::media_time::MediaTime;
    use crate::engine::timeline_state::Clip;
    use tempfile::TempDir;

    fn make_state() -> TimelineState {
        let mut state = TimelineState::new();
        state.clips.push(Clip::new(
            "c1",
            "t1",
            MediaTime::ZERO,
            MediaTime::from_seconds(5.0),
            "test.mp4",
        ));
        state.version = 5;
        state.rebuild_indices();
        state
    }

    #[test]
    fn test_write_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        let state = make_state();
        let snapshot = Snapshot::new(state.clone(), 5);

        let path = store.write_snapshot(&snapshot).unwrap();
        assert!(path.exists());

        let loaded = store.load_snapshot(&path).unwrap();
        assert_eq!(loaded.event_version, 5);
        assert_eq!(loaded.state.clips.len(), 1);
        assert!(loaded.verify_checksum());
    }

    #[test]
    fn test_load_latest() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Write multiple snapshots
        for version in [1, 5, 10] {
            let mut state = make_state();
            state.version = version;
            let snapshot = Snapshot::new(state, version);
            store.write_snapshot(&snapshot).unwrap();
        }

        let latest = store.load_latest_snapshot().unwrap();
        assert_eq!(latest.event_version, 10);
    }

    #[test]
    fn test_checksum_detects_corruption() {
        let state = make_state();
        let snapshot = Snapshot::new(state, 5);

        assert!(snapshot.verify_checksum());

        // Corrupt the snapshot
        let mut corrupted = snapshot.clone();
        corrupted.event_version = 999;
        assert!(!corrupted.verify_checksum());
    }

    #[test]
    fn test_no_snapshots_error() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        let result = store.load_latest_snapshot();
        assert!(matches!(result, Err(SnapshotStoreError::NoSnapshots)));
    }
}
