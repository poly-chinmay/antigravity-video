//! Event Store - Append-only durable event log.
//!
//! # Design Principles
//!
//! 1. Events are written BEFORE state mutation
//! 2. Events are marked committed only AFTER successful mutation
//! 3. All writes are fsync'd for durability
//! 4. Never rewrite or delete events
//!
//! # Crash Safety
//!
//! On crash: uncommitted events are discarded during recovery.
//! This ensures only successfully applied mutations survive.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::edit_action::EditAction;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Current schema version for event records
const SCHEMA_VERSION: u32 = 1;

/// Minimum disk space required before write (1MB)
const MIN_DISK_SPACE_BYTES: u64 = 1_048_576;

// =============================================================================
// EVENT RECORD
// =============================================================================

/// A single event in the append-only log.
///
/// # Invariants
///
/// - `schema_version` allows future format evolution
/// - `event_version` is strictly monotonic
/// - `committed = false` until mutation succeeds
/// - `timestamp` is UTC nanoseconds since epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    /// Schema version for future compatibility
    pub schema_version: u32,

    /// Version number of this event (monotonically increasing)
    pub event_version: u64,

    /// Whether mutation completed successfully
    pub committed: bool,

    /// The action that was applied
    pub action: EditAction,

    /// UTC timestamp (nanoseconds since epoch)
    pub timestamp: u64,

    /// CRC32 checksum for corruption detection
    pub checksum: u32,
}

impl EventRecord {
    /// Create a new uncommitted event record
    pub fn new(event_version: u64, action: EditAction) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut record = Self {
            schema_version: SCHEMA_VERSION,
            event_version,
            committed: false,
            action,
            timestamp,
            checksum: 0,
        };

        record.checksum = record.compute_checksum();
        record
    }

    /// Compute CRC32 checksum of record (excluding checksum field)
    fn compute_checksum(&self) -> u32 {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&self.schema_version.to_le_bytes());
        hasher.update(&self.event_version.to_le_bytes());
        hasher.update(&[self.committed as u8]);
        hasher.update(&self.timestamp.to_le_bytes());
        // Include action ID for basic action integrity
        hasher.update(self.action.id.as_bytes());
        hasher.finalize()
    }

    /// Verify checksum is valid
    pub fn verify_checksum(&self) -> bool {
        let expected = {
            let mut copy = self.clone();
            copy.checksum = 0;
            copy.compute_checksum()
        };
        self.checksum == expected
    }
}

// =============================================================================
// EVENT STORE ERRORS
// =============================================================================

/// Errors that can occur in event store operations
#[derive(Debug)]
pub enum EventStoreError {
    /// Failed to create storage directory
    CreateDirFailed(io::Error),

    /// Failed to open event file
    OpenFailed(io::Error),

    /// Failed to write event
    WriteFailed(io::Error),

    /// Failed to sync to disk
    SyncFailed(io::Error),

    /// Insufficient disk space
    InsufficientSpace { required: u64, available: u64 },

    /// Failed to serialize event
    SerializeFailed(String),

    /// Failed to deserialize event
    DeserializeFailed { line: usize, error: String },

    /// Checksum verification failed
    ChecksumFailed { event_version: u64 },

    /// Event version mismatch
    VersionMismatch { expected: u64, found: u64 },

    /// Cannot mark event committed (not found or already committed)
    CommitFailed { event_version: u64 },
}

impl std::fmt::Display for EventStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirFailed(e) => write!(f, "Failed to create event store directory: {}", e),
            Self::OpenFailed(e) => write!(f, "Failed to open event file: {}", e),
            Self::WriteFailed(e) => write!(f, "Failed to write event: {}", e),
            Self::SyncFailed(e) => write!(f, "Failed to sync event file: {}", e),
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
            Self::SerializeFailed(e) => write!(f, "Failed to serialize event: {}", e),
            Self::DeserializeFailed { line, error } => {
                write!(f, "Failed to deserialize event at line {}: {}", line, error)
            }
            Self::ChecksumFailed { event_version } => {
                write!(
                    f,
                    "Checksum verification failed for event {}",
                    event_version
                )
            }
            Self::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "Event version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            Self::CommitFailed { event_version } => {
                write!(f, "Failed to mark event {} as committed", event_version)
            }
        }
    }
}

impl std::error::Error for EventStoreError {}

// =============================================================================
// EVENT STORE
// =============================================================================

/// Append-only event store for crash-safe durability.
///
/// # Thread Safety
///
/// EventStore is designed to be used from a single thread (wrapped in Mutex by engine).
pub struct EventStore {
    /// Base directory for event storage
    base_path: PathBuf,

    /// Path to the active event log file
    log_path: PathBuf,

    /// Cached events (for fast lookup during commit)
    events: Vec<EventRecord>,

    /// Next event version to assign
    next_version: u64,
}

impl EventStore {
    /// Create or open an event store at the given path.
    pub fn new(base_path: PathBuf) -> Result<Self, EventStoreError> {
        // Ensure directory exists
        fs::create_dir_all(&base_path).map_err(EventStoreError::CreateDirFailed)?;

        let log_path = base_path.join("events.jsonl");

        // Load existing events
        let (events, next_version) = Self::load_events_from_file(&log_path)?;

        Ok(Self {
            base_path,
            log_path,
            events,
            next_version,
        })
    }

    /// Append a new event (uncommitted).
    ///
    /// # Durability
    ///
    /// Event is fsync'd to disk before returning.
    /// This ensures the event survives power failure.
    pub fn append_event(&mut self, action: &EditAction) -> Result<u64, EventStoreError> {
        let event_version = self.next_version;

        // Check disk space before writing
        self.check_disk_space()?;

        // Create event record
        let record = EventRecord::new(event_version, action.clone());

        // Serialize to JSON line
        let json = serde_json::to_string(&record)
            .map_err(|e| EventStoreError::SerializeFailed(e.to_string()))?;

        // Append to file with fsync
        self.append_to_log(&json)?;

        // Update in-memory state
        self.events.push(record);
        self.next_version += 1;

        Ok(event_version)
    }

    /// Mark an event as committed.
    ///
    /// # Implementation
    ///
    /// To avoid rewriting the log file, we maintain a separate "commits" file
    /// that lists committed event versions. On load, we filter events by this.
    ///
    /// Alternative: Use a manifest file that tracks the last committed version.
    pub fn mark_committed(&mut self, event_version: u64) -> Result<(), EventStoreError> {
        // Find event in memory
        let event = self
            .events
            .iter_mut()
            .find(|e| e.event_version == event_version)
            .ok_or(EventStoreError::CommitFailed { event_version })?;

        if event.committed {
            // Already committed - idempotent
            return Ok(());
        }

        // Mark as committed in memory
        event.committed = true;

        // Write to commits manifest
        let commits_path = self.base_path.join("commits.txt");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&commits_path)
            .map_err(EventStoreError::OpenFailed)?;

        writeln!(file, "{}", event_version).map_err(EventStoreError::WriteFailed)?;

        file.sync_all().map_err(EventStoreError::SyncFailed)?;

        // Sync parent directory
        Self::sync_directory(&self.base_path)?;

        Ok(())
    }

    /// Rollback the last appended event (if uncommitted).
    ///
    /// # Note
    ///
    /// This only removes from memory. The event remains in the log file
    /// but will be ignored on recovery (not in commits.txt).
    pub fn rollback_last(&mut self) {
        if let Some(last) = self.events.last() {
            if !last.committed {
                self.events.pop();
                // Note: next_version stays incremented to avoid version reuse
            }
        }
    }

    /// Get committed events after a given version.
    pub fn get_committed_events_after(&self, version: u64) -> Vec<&EventRecord> {
        self.events
            .iter()
            .filter(|e| e.committed && e.event_version > version)
            .collect()
    }

    /// Get the highest committed version.
    pub fn last_committed_version(&self) -> u64 {
        self.events
            .iter()
            .filter(|e| e.committed)
            .map(|e| e.event_version)
            .max()
            .unwrap_or(0)
    }

    /// Get all committed events.
    pub fn committed_events(&self) -> Vec<&EventRecord> {
        self.events.iter().filter(|e| e.committed).collect()
    }

    /// Get path for external use.
    pub fn path(&self) -> &Path {
        &self.base_path
    }

    // =========================================================================
    // PRIVATE METHODS
    // =========================================================================

    /// Load events from log file.
    fn load_events_from_file(log_path: &Path) -> Result<(Vec<EventRecord>, u64), EventStoreError> {
        if !log_path.exists() {
            return Ok((Vec::new(), 1));
        }

        // Load commits manifest
        let commits_path = log_path.parent().unwrap().join("commits.txt");
        let committed_versions = Self::load_committed_versions(&commits_path)?;

        let file = File::open(log_path).map_err(EventStoreError::OpenFailed)?;
        let reader = BufReader::new(file);

        let mut events = Vec::new();
        let mut max_version = 0u64;

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| EventStoreError::DeserializeFailed {
                line: line_num + 1,
                error: e.to_string(),
            })?;

            if line.trim().is_empty() {
                continue;
            }

            let mut record: EventRecord =
                serde_json::from_str(&line).map_err(|e| EventStoreError::DeserializeFailed {
                    line: line_num + 1,
                    error: e.to_string(),
                })?;

            // Verify checksum
            if !record.verify_checksum() {
                return Err(EventStoreError::ChecksumFailed {
                    event_version: record.event_version,
                });
            }

            // Apply committed status from manifest
            record.committed = committed_versions.contains(&record.event_version);

            max_version = max_version.max(record.event_version);
            events.push(record);
        }

        Ok((events, max_version + 1))
    }

    /// Load committed versions from manifest.
    fn load_committed_versions(
        commits_path: &Path,
    ) -> Result<std::collections::HashSet<u64>, EventStoreError> {
        use std::collections::HashSet;

        if !commits_path.exists() {
            return Ok(HashSet::new());
        }

        let file = File::open(commits_path).map_err(EventStoreError::OpenFailed)?;
        let reader = BufReader::new(file);

        let mut versions = HashSet::new();

        for line_result in reader.lines() {
            if let Ok(line) = line_result {
                if let Ok(version) = line.trim().parse::<u64>() {
                    versions.insert(version);
                }
            }
        }

        Ok(versions)
    }

    /// Append a line to the log file with fsync.
    fn append_to_log(&self, json: &str) -> Result<(), EventStoreError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(EventStoreError::OpenFailed)?;

        writeln!(file, "{}", json).map_err(EventStoreError::WriteFailed)?;

        // Sync file to disk
        file.sync_all().map_err(EventStoreError::SyncFailed)?;

        // Sync parent directory (for filesystem metadata)
        Self::sync_directory(&self.base_path)?;

        Ok(())
    }

    /// Check available disk space.
    fn check_disk_space(&self) -> Result<(), EventStoreError> {
        // Use fs2 for cross-platform disk space check
        match fs2::available_space(&self.base_path) {
            Ok(available) => {
                if available < MIN_DISK_SPACE_BYTES {
                    return Err(EventStoreError::InsufficientSpace {
                        required: MIN_DISK_SPACE_BYTES,
                        available,
                    });
                }
                Ok(())
            }
            Err(_) => {
                // If we can't check, proceed with caution
                Ok(())
            }
        }
    }

    /// Sync directory for filesystem metadata durability.
    fn sync_directory(dir: &Path) -> Result<(), EventStoreError> {
        #[cfg(unix)]
        {
            if let Ok(dir_file) = File::open(dir) {
                // sync_all() on directory ensures rename atomicity on POSIX
                let _ = dir_file.sync_all();
            }
        }

        Ok(())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::edit_action::{ActionType, EditAction};
    use tempfile::TempDir;

    fn make_action() -> EditAction {
        EditAction::new(ActionType::AddClip)
    }

    #[test]
    fn test_append_and_commit() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        let action = make_action();
        let version = store.append_event(&action).unwrap();

        assert_eq!(version, 1);
        assert_eq!(store.events.len(), 1);
        assert!(!store.events[0].committed);

        store.mark_committed(version).unwrap();
        assert!(store.events[0].committed);
    }

    #[test]
    fn test_rollback() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        store.append_event(&make_action()).unwrap();
        assert_eq!(store.events.len(), 1);

        store.rollback_last();
        assert_eq!(store.events.len(), 0);
    }

    #[test]
    fn test_reload_preserves_committed() {
        let temp_dir = TempDir::new().unwrap();

        // First session: append and commit
        {
            let mut store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();
            let v1 = store.append_event(&make_action()).unwrap();
            store.mark_committed(v1).unwrap();
            let _v2 = store.append_event(&make_action()).unwrap();
            // v2 left uncommitted
        }

        // Second session: reload
        {
            let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();
            let committed = store.committed_events();

            assert_eq!(committed.len(), 1);
            assert_eq!(committed[0].event_version, 1);
        }
    }

    #[test]
    fn test_checksum_verification() {
        let record = EventRecord::new(1, make_action());
        assert!(record.verify_checksum());

        // Corrupt the record
        let mut corrupted = record.clone();
        corrupted.event_version = 999;
        assert!(!corrupted.verify_checksum());
    }
}
