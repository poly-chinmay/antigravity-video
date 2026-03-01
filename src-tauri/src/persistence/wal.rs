// src-tauri/src/persistence/wal.rs
//! Write-Ahead Log (WAL) for crash-safe mutations
//!
//! Ensures durability by writing mutations to disk BEFORE applying them.
//! Uses checksums to detect partial/corrupted entries.

use super::Event;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single WAL entry with checksum for integrity verification
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WALEntry {
    /// Version number of this entry
    pub version: u64,
    /// Unix timestamp in milliseconds
    pub timestamp: u64,
    /// The event being logged
    pub event: Event,
    /// CRC-style checksum for corruption detection
    pub checksum: u64,
}

impl WALEntry {
    /// Create a new WAL entry with computed checksum
    pub fn new(version: u64, event: Event) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut entry = Self {
            version,
            timestamp,
            event,
            checksum: 0, // Placeholder
        };

        // Compute checksum over all fields except checksum itself
        entry.checksum = entry.compute_checksum();
        entry
    }

    /// Compute checksum of entry (excluding checksum field)
    fn compute_checksum(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.version.hash(&mut hasher);
        self.timestamp.hash(&mut hasher);
        // Hash the serialized event for consistency
        if let Ok(event_json) = serde_json::to_string(&self.event) {
            event_json.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Verify entry integrity
    pub fn verify_checksum(&self) -> bool {
        let expected = self.compute_checksum();
        self.checksum == expected
    }
}

/// Write-Ahead Log for crash-safe mutations
///
/// WAL entries are stored in a single append-only file.
/// Each entry is a JSON line followed by a newline.
pub struct WriteAheadLog {
    /// Path to the WAL file
    wal_path: PathBuf,
    /// Current file handle for appending
    file: Option<File>,
}

impl WriteAheadLog {
    /// Create or open a WAL at the given base path
    pub fn new(base_path: PathBuf) -> std::io::Result<Self> {
        // Ensure directory exists
        if !base_path.exists() {
            fs::create_dir_all(&base_path)?;
        }

        let wal_path = base_path.join("wal.log");

        // Open file for appending
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        println!("📝 [WAL] Opened at {:?}", wal_path);

        Ok(Self {
            wal_path,
            file: Some(file),
        })
    }

    /// Append an entry to the WAL with fsync
    ///
    /// This MUST complete before the mutation is applied.
    /// Returns error if fsync fails - caller should NOT proceed with mutation.
    pub fn append(&mut self, entry: &WALEntry) -> std::io::Result<()> {
        let file = self.file.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotConnected, "WAL file not open")
        })?;

        // Serialize entry as single JSON line
        let json = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Write entry + newline
        writeln!(file, "{}", json)?;

        // CRITICAL: fsync before returning to ensure durability
        file.sync_all()?;

        println!(
            "📝 [WAL] Appended entry v{} (checksum: {:016x})",
            entry.version, entry.checksum
        );

        Ok(())
    }

    /// Load all entries since a given version (inclusive)
    pub fn load_since(&self, since_version: u64) -> std::io::Result<Vec<WALEntry>> {
        if !self.wal_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.wal_path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut line_num = 0;

        for line in reader.lines() {
            line_num += 1;
            let line = line?;

            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<WALEntry>(&line) {
                Ok(entry) => {
                    // Verify checksum
                    if !entry.verify_checksum() {
                        eprintln!(
                            "⚠️ [WAL] Corrupted entry at line {} (checksum mismatch), skipping",
                            line_num
                        );
                        continue;
                    }

                    if entry.version >= since_version {
                        entries.push(entry);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ [WAL] Failed to parse entry at line {}: {} (partial write?)",
                        line_num, e
                    );
                    // Partial entry at end of file is expected after crash
                    // Just skip it
                }
            }
        }

        println!(
            "📂 [WAL] Loaded {} entries since v{}",
            entries.len(),
            since_version
        );

        Ok(entries)
    }

    /// Truncate WAL up to a given version (exclusive)
    ///
    /// Removes all entries with version < truncate_version.
    /// Used after checkpoint to reclaim space.
    pub fn truncate_up_to(&mut self, truncate_version: u64) -> std::io::Result<()> {
        // Close current file handle
        self.file = None;

        if !self.wal_path.exists() {
            // Reopen for appending
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.wal_path)?,
            );
            return Ok(());
        }

        // Load entries to keep
        let entries_to_keep = self.load_since(truncate_version)?;

        // Write to temp file
        let temp_path = self.wal_path.with_extension("log.tmp");
        {
            let mut temp_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)?;

            for entry in &entries_to_keep {
                let json = serde_json::to_string(entry)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                writeln!(temp_file, "{}", json)?;
            }

            temp_file.sync_all()?;
        }

        // Atomic rename
        fs::rename(&temp_path, &self.wal_path)?;

        // Fsync directory
        #[cfg(unix)]
        {
            if let Some(parent) = self.wal_path.parent() {
                let dir = File::open(parent)?;
                dir.sync_all()?;
            }
        }

        // Reopen for appending
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.wal_path)?,
        );

        println!(
            "🗑️ [WAL] Truncated entries before v{}, {} entries remaining",
            truncate_version,
            entries_to_keep.len()
        );

        Ok(())
    }

    /// Get the path to the WAL file
    pub fn wal_path(&self) -> &PathBuf {
        &self.wal_path
    }

    /// Check if WAL has any entries
    pub fn is_empty(&self) -> std::io::Result<bool> {
        if !self.wal_path.exists() {
            return Ok(true);
        }

        let metadata = fs::metadata(&self.wal_path)?;
        Ok(metadata.len() == 0)
    }

    /// Get the latest version in the WAL
    pub fn get_latest_version(&self) -> std::io::Result<Option<u64>> {
        let entries = self.load_since(0)?;
        Ok(entries.last().map(|e| e.version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_plan::{ActionType, EditAction, EditPlan};
    use tempfile::TempDir;

    fn create_test_event(version: u64) -> Event {
        Event::new(
            version,
            EditPlan {
                actions: vec![EditAction {
                    action_type: ActionType::Delete,
                    target_clip_id: "test-clip".to_string(),
                    parameters: None,
                }],
                thought_process: Some("Test".to_string()),
                confidence: Some(0.9),
            },
            Some("Test instruction".to_string()),
            Some(0.9),
            50,
            true,
        )
    }

    #[test]
    fn test_wal_entry_checksum() {
        let event = create_test_event(1);
        let entry = WALEntry::new(1, event);

        assert!(entry.verify_checksum(), "Checksum should verify");

        // Tamper with entry
        let mut tampered = entry.clone();
        tampered.version = 999;
        assert!(
            !tampered.verify_checksum(),
            "Tampered entry should fail verification"
        );
    }

    #[test]
    fn test_wal_append_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let mut wal = WriteAheadLog::new(temp_dir.path().to_path_buf()).unwrap();

        // Append entries
        let entry1 = WALEntry::new(1, create_test_event(1));
        let entry2 = WALEntry::new(2, create_test_event(2));

        wal.append(&entry1).unwrap();
        wal.append(&entry2).unwrap();

        // Load all
        let loaded = wal.load_since(0).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].version, 1);
        assert_eq!(loaded[1].version, 2);

        // Load since version 2
        let loaded_since_2 = wal.load_since(2).unwrap();
        assert_eq!(loaded_since_2.len(), 1);
        assert_eq!(loaded_since_2[0].version, 2);
    }

    #[test]
    fn test_wal_truncate() {
        let temp_dir = TempDir::new().unwrap();
        let mut wal = WriteAheadLog::new(temp_dir.path().to_path_buf()).unwrap();

        // Append entries 1-5
        for i in 1..=5 {
            let entry = WALEntry::new(i, create_test_event(i));
            wal.append(&entry).unwrap();
        }

        // Truncate up to version 3 (keep 3, 4, 5)
        wal.truncate_up_to(3).unwrap();

        let remaining = wal.load_since(0).unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(remaining[0].version, 3);
        assert_eq!(remaining[2].version, 5);
    }

    #[test]
    fn test_wal_latest_version() {
        let temp_dir = TempDir::new().unwrap();
        let mut wal = WriteAheadLog::new(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(wal.get_latest_version().unwrap(), None);

        wal.append(&WALEntry::new(5, create_test_event(5))).unwrap();
        wal.append(&WALEntry::new(10, create_test_event(10)))
            .unwrap();

        assert_eq!(wal.get_latest_version().unwrap(), Some(10));
    }
}
