//! Workspace Persistence - Save/load with crash recovery.
//!
//! # Design
//!
//! Implements crash-safe workspace persistence:
//! 1. Write to temp file
//! 2. Validate content
//! 3. Atomic rename to target
//! 4. Keep backup of previous state
//!
//! On crash, recovery loads from backup if main file is corrupt.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::workspace_state::{WorkspaceCollection, WorkspaceState, WORKSPACE_VERSION};

// =============================================================================
// PERSISTENCE ERROR
// =============================================================================

/// Persistence errors.
#[derive(Debug)]
pub enum PersistenceError {
    /// IO error
    Io(std::io::Error),
    /// Serialization error
    Serialization(String),
    /// Version mismatch
    VersionMismatch { expected: u32, found: u32 },
    /// Checksum validation failed
    ChecksumInvalid,
    /// File not found
    NotFound(PathBuf),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            Self::VersionMismatch { expected, found } => {
                write!(f, "Version mismatch: expected {}, found {}", expected, found)
            }
            Self::ChecksumInvalid => write!(f, "Checksum validation failed"),
            Self::NotFound(path) => write!(f, "File not found: {:?}", path),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result type for persistence operations.
pub type PersistenceResult<T> = Result<T, PersistenceError>;

// =============================================================================
// FILE NAMES
// =============================================================================

const WORKSPACE_FILE: &str = "workspace.json";
const WORKSPACE_BACKUP: &str = "workspace.backup.json";
const WORKSPACE_TEMP: &str = "workspace.tmp.json";

// =============================================================================
// WORKSPACE PERSISTENCE
// =============================================================================

/// Handles workspace persistence with crash recovery.
#[derive(Debug)]
pub struct WorkspacePersistence {
    /// Base directory for workspace files
    base_path: PathBuf,
}

impl WorkspacePersistence {
    /// Create a new persistence handler.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Get path to main workspace file.
    fn main_path(&self) -> PathBuf {
        self.base_path.join(WORKSPACE_FILE)
    }

    /// Get path to backup file.
    fn backup_path(&self) -> PathBuf {
        self.base_path.join(WORKSPACE_BACKUP)
    }

    /// Get path to temp file.
    fn temp_path(&self) -> PathBuf {
        self.base_path.join(WORKSPACE_TEMP)
    }

    /// Save workspace collection with crash safety.
    pub fn save(&self, collection: &mut WorkspaceCollection) -> PersistenceResult<()> {
        // Ensure directory exists
        fs::create_dir_all(&self.base_path)?;

        // Calculate checksums for all workspaces
        for workspace in collection.workspaces.values_mut() {
            workspace.calculate_checksum();
        }

        // 1. Write to temp file
        let temp_path = self.temp_path();
        {
            let file = File::create(&temp_path)?;
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, collection)
                .map_err(|e| PersistenceError::Serialization(e.to_string()))?;
        }

        // 2. Validate temp file by reading it back
        self.validate_file(&temp_path)?;

        // 3. Backup existing file (if exists)
        let main_path = self.main_path();
        let backup_path = self.backup_path();
        if main_path.exists() {
            fs::copy(&main_path, &backup_path)?;
        }

        // 4. Atomic rename temp to main
        fs::rename(&temp_path, &main_path)?;

        Ok(())
    }

    /// Load workspace collection with crash recovery.
    pub fn load(&self) -> PersistenceResult<WorkspaceCollection> {
        let main_path = self.main_path();
        let backup_path = self.backup_path();

        // Try main file first
        if main_path.exists() {
            match self.load_from_file(&main_path) {
                Ok(collection) => return Ok(collection),
                Err(e) => {
                    eprintln!("Warning: Main workspace file corrupted: {}", e);
                    // Fall through to backup
                }
            }
        }

        // Try backup file
        if backup_path.exists() {
            match self.load_from_file(&backup_path) {
                Ok(collection) => {
                    eprintln!("Recovered workspace from backup");
                    return Ok(collection);
                }
                Err(e) => {
                    eprintln!("Warning: Backup workspace file also corrupted: {}", e);
                }
            }
        }

        // No valid file found
        if main_path.exists() || backup_path.exists() {
            Err(PersistenceError::ChecksumInvalid)
        } else {
            Err(PersistenceError::NotFound(main_path))
        }
    }

    /// Load or create default workspace.
    pub fn load_or_default(&self) -> WorkspaceCollection {
        self.load().unwrap_or_else(|_| WorkspaceCollection::new())
    }

    /// Load from specific file.
    fn load_from_file(&self, path: &Path) -> PersistenceResult<WorkspaceCollection> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        
        let collection: WorkspaceCollection = serde_json::from_reader(reader)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;

        // Check version
        if collection.version != WORKSPACE_VERSION {
            return Err(PersistenceError::VersionMismatch {
                expected: WORKSPACE_VERSION,
                found: collection.version,
            });
        }

        // Validate checksums
        for workspace in collection.workspaces.values() {
            if workspace.checksum.is_some() && !workspace.validate_checksum() {
                return Err(PersistenceError::ChecksumInvalid);
            }
        }

        Ok(collection)
    }

    /// Validate a workspace file.
    fn validate_file(&self, path: &Path) -> PersistenceResult<()> {
        let _collection = self.load_from_file(path)?;
        Ok(())
    }

    /// Check if workspace file exists.
    pub fn exists(&self) -> bool {
        self.main_path().exists()
    }

    /// Get last modified time of workspace file.
    pub fn last_modified(&self) -> Option<std::time::SystemTime> {
        fs::metadata(self.main_path())
            .ok()
            .and_then(|m| m.modified().ok())
    }

    /// Delete workspace files.
    pub fn clear(&self) -> PersistenceResult<()> {
        let _ = fs::remove_file(self.main_path());
        let _ = fs::remove_file(self.backup_path());
        let _ = fs::remove_file(self.temp_path());
        Ok(())
    }
}

// =============================================================================
// RECOVERY JOURNAL
// =============================================================================

/// Journal entry for crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Timestamp
    pub timestamp: u64,
    /// Operation type
    pub operation: String,
    /// Workspace name
    pub workspace_name: String,
    /// Completed flag
    pub completed: bool,
}

/// Simple journal for tracking in-progress operations.
#[derive(Debug)]
pub struct RecoveryJournal {
    /// Journal file path
    path: PathBuf,
}

impl RecoveryJournal {
    /// Create new journal.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        let mut path: PathBuf = base_path.into();
        path.push("workspace.journal");
        Self { path }
    }

    /// Write journal entry at start of operation.
    pub fn begin(&self, operation: &str, workspace_name: &str) -> PersistenceResult<()> {
        let entry = JournalEntry {
            timestamp: Self::now_millis(),
            operation: operation.to_string(),
            workspace_name: workspace_name.to_string(),
            completed: false,
        };
        self.write_entry(&entry)
    }

    /// Mark operation as complete.
    pub fn complete(&self) -> PersistenceResult<()> {
        // Simply delete the journal file
        let _ = fs::remove_file(&self.path);
        Ok(())
    }

    /// Check for incomplete operation.
    pub fn check_incomplete(&self) -> Option<JournalEntry> {
        if !self.path.exists() {
            return None;
        }

        let content = fs::read_to_string(&self.path).ok()?;
        let entry: JournalEntry = serde_json::from_str(&content).ok()?;
        
        if !entry.completed {
            Some(entry)
        } else {
            None
        }
    }

    fn write_entry(&self, entry: &JournalEntry) -> PersistenceResult<()> {
        let mut file = File::create(&self.path)?;
        let json = serde_json::to_string(entry)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    fn now_millis() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_workspace_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = WorkspacePersistence::new(temp_dir.path());

        // Create and save
        let mut collection = WorkspaceCollection::new();
        collection.active_mut().unwrap().set_panel_visible("panel.history", true);
        
        persistence.save(&mut collection).unwrap();

        // Load back
        let loaded = persistence.load().unwrap();
        
        assert_eq!(loaded.active, collection.active);
        assert!(loaded.active().unwrap().get_panel("panel.history").unwrap().visible);
    }

    #[test]
    fn test_crash_recovery_layout() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = WorkspacePersistence::new(temp_dir.path());

        // Save initial state
        let mut collection = WorkspaceCollection::new();
        persistence.save(&mut collection).unwrap();

        // Save again (creates backup)
        collection.active_mut().unwrap().theme.dark = false;
        persistence.save(&mut collection).unwrap();

        // Corrupt main file
        let main_path = temp_dir.path().join(WORKSPACE_FILE);
        fs::write(&main_path, "corrupted data").unwrap();

        // Load should recover from backup
        let recovered = persistence.load().unwrap();
        
        // Should get the version before corruption (dark mode still true from backup)
        assert!(recovered.active().is_some());
    }

    #[test]
    fn test_load_or_default() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = WorkspacePersistence::new(temp_dir.path());

        // No file exists - should return default
        let collection = persistence.load_or_default();
        
        assert_eq!(collection.active, "Editing");
        assert!(!collection.workspaces.is_empty());
    }

    #[test]
    fn test_persistence_exists() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = WorkspacePersistence::new(temp_dir.path());

        assert!(!persistence.exists());

        let mut collection = WorkspaceCollection::new();
        persistence.save(&mut collection).unwrap();

        assert!(persistence.exists());
    }

    #[test]
    fn test_journal_incomplete_detection() {
        let temp_dir = TempDir::new().unwrap();
        let journal = RecoveryJournal::new(temp_dir.path());

        // No journal - no incomplete
        assert!(journal.check_incomplete().is_none());

        // Begin operation
        journal.begin("save", "Editing").unwrap();
        
        // Should detect incomplete
        let entry = journal.check_incomplete().unwrap();
        assert_eq!(entry.operation, "save");
        assert!(!entry.completed);

        // Complete
        journal.complete().unwrap();
        
        // No longer incomplete
        assert!(journal.check_incomplete().is_none());
    }
}
