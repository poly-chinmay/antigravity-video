// src-tauri/src/persistence/snapshot_store.rs
//! Snapshot Store with zstd compression
//!
//! Periodically saves compressed snapshots of TimelineState for fast recovery.
//! Snapshots are taken every 50 events to balance recovery time and storage.

use crate::timeline::TimelineState;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Snapshot interval - take a snapshot every N events
pub const SNAPSHOT_INTERVAL: u64 = 50;

/// Magic bytes to identify snapshot files
const SNAPSHOT_MAGIC: &[u8] = b"GHST";
const SNAPSHOT_VERSION: u8 = 1;

/// Header for snapshot files
#[derive(Serialize, Deserialize, Debug)]
struct SnapshotHeader {
    /// Magic bytes for file identification
    magic: [u8; 4],
    /// File format version
    version: u8,
    /// Version of the timeline state
    state_version: u64,
    /// Uncompressed size of the state data
    uncompressed_size: u64,
    /// Compressed size of the state data
    compressed_size: u64,
    /// Checksum of the uncompressed data
    checksum: u64,
}

impl SnapshotHeader {
    fn new(
        state_version: u64,
        uncompressed_size: u64,
        compressed_size: u64,
        checksum: u64,
    ) -> Self {
        Self {
            magic: [
                SNAPSHOT_MAGIC[0],
                SNAPSHOT_MAGIC[1],
                SNAPSHOT_MAGIC[2],
                SNAPSHOT_MAGIC[3],
            ],
            version: SNAPSHOT_VERSION,
            state_version,
            uncompressed_size,
            compressed_size,
            checksum,
        }
    }

    fn verify_magic(&self) -> bool {
        &self.magic == SNAPSHOT_MAGIC
    }
}

/// Snapshot Store for compressed timeline state
pub struct SnapshotStore {
    /// Directory for snapshot files
    snapshots_dir: PathBuf,
}

impl SnapshotStore {
    /// Create a new SnapshotStore at the given base path
    pub fn new(base_path: PathBuf) -> std::io::Result<Self> {
        let snapshots_dir = base_path.join("snapshots");

        if !snapshots_dir.exists() {
            fs::create_dir_all(&snapshots_dir)?;
        }

        println!("📸 [SnapshotStore] Initialized at {:?}", snapshots_dir);

        Ok(Self { snapshots_dir })
    }

    /// Generate filename for a snapshot at given version
    fn snapshot_filename(&self, version: u64) -> PathBuf {
        self.snapshots_dir
            .join(format!("snapshot_{:08}.zst", version))
    }

    /// Generate temp filename for atomic writes
    fn temp_filename(&self, version: u64) -> PathBuf {
        self.snapshots_dir
            .join(format!("snapshot_{:08}.zst.tmp", version))
    }

    /// Compute checksum for data
    fn compute_checksum(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    /// Save a snapshot of the timeline state
    ///
    /// Uses zstd compression with atomic file writes.
    pub fn save(&self, version: u64, state: &TimelineState) -> std::io::Result<()> {
        let final_path = self.snapshot_filename(version);
        let temp_path = self.temp_filename(version);

        // 1. Serialize state to JSON
        let json_data = serde_json::to_vec(state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let uncompressed_size = json_data.len() as u64;
        let checksum = Self::compute_checksum(&json_data);

        // 2. Compress with zstd (level 3 for good balance)
        let compressed_data = zstd::encode_all(&json_data[..], 3)?;
        let compressed_size = compressed_data.len() as u64;

        // 3. Create header
        let header = SnapshotHeader::new(version, uncompressed_size, compressed_size, checksum);
        let header_bytes = serde_json::to_vec(&header)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 4. Write to temp file: [header_len (4 bytes)] + [header] + [compressed_data]
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;

            // Write header length as u32
            let header_len = header_bytes.len() as u32;
            file.write_all(&header_len.to_le_bytes())?;

            // Write header
            file.write_all(&header_bytes)?;

            // Write compressed data
            file.write_all(&compressed_data)?;

            // 5. fsync temp file
            file.sync_all()?;
        }

        // 6. Atomic rename
        fs::rename(&temp_path, &final_path)?;

        // 7. fsync directory
        #[cfg(unix)]
        {
            let dir = File::open(&self.snapshots_dir)?;
            dir.sync_all()?;
        }

        let ratio = (compressed_size as f64 / uncompressed_size as f64) * 100.0;
        println!(
            "📸 [SnapshotStore] Saved v{} ({} -> {} bytes, {:.1}% ratio)",
            version, uncompressed_size, compressed_size, ratio
        );

        Ok(())
    }

    /// Load a snapshot at a specific version
    pub fn load(&self, version: u64) -> std::io::Result<TimelineState> {
        let path = self.snapshot_filename(version);

        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Snapshot v{} not found", version),
            ));
        }

        let mut file = File::open(&path)?;

        // 1. Read header length
        let mut header_len_bytes = [0u8; 4];
        file.read_exact(&mut header_len_bytes)?;
        let header_len = u32::from_le_bytes(header_len_bytes) as usize;

        // 2. Read header
        let mut header_bytes = vec![0u8; header_len];
        file.read_exact(&mut header_bytes)?;

        let header: SnapshotHeader = serde_json::from_slice(&header_bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 3. Verify magic
        if !header.verify_magic() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid snapshot magic bytes",
            ));
        }

        // 4. Read compressed data
        let mut compressed_data = vec![0u8; header.compressed_size as usize];
        file.read_exact(&mut compressed_data)?;

        // 5. Decompress
        let decompressed = zstd::decode_all(&compressed_data[..])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 6. Verify checksum
        let actual_checksum = Self::compute_checksum(&decompressed);
        if actual_checksum != header.checksum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Checksum mismatch: expected {:016x}, got {:016x}",
                    header.checksum, actual_checksum
                ),
            ));
        }

        // 7. Deserialize
        let mut state: TimelineState = serde_json::from_slice(&decompressed)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 8. Rebuild indices (they are not serialized)
        state.rebuild_indices();

        println!(
            "📸 [SnapshotStore] Loaded v{} ({} bytes)",
            version,
            decompressed.len()
        );

        Ok(state)
    }

    /// Get the latest snapshot
    pub fn latest(&self) -> std::io::Result<Option<(u64, TimelineState)>> {
        if !self.snapshots_dir.exists() {
            return Ok(None);
        }

        // Find all snapshot files and get the highest version
        let mut max_version: Option<u64> = None;

        for entry in fs::read_dir(&self.snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("snapshot_") && name.ends_with(".zst") {
                    // Extract version from "snapshot_00000050.zst"
                    let version_str = name
                        .trim_start_matches("snapshot_")
                        .trim_end_matches(".zst");

                    if let Ok(version) = version_str.parse::<u64>() {
                        max_version = Some(max_version.map_or(version, |m| m.max(version)));
                    }
                }
            }
        }

        match max_version {
            Some(version) => {
                let state = self.load(version)?;
                Ok(Some((version, state)))
            }
            None => Ok(None),
        }
    }

    /// Check if a snapshot should be taken at this version
    pub fn should_snapshot(version: u64) -> bool {
        version > 0 && version % SNAPSHOT_INTERVAL == 0
    }

    /// Get the path to snapshots directory
    pub fn snapshots_path(&self) -> &PathBuf {
        &self.snapshots_dir
    }

    /// List all available snapshot versions
    pub fn list_versions(&self) -> std::io::Result<Vec<u64>> {
        let mut versions = Vec::new();

        if !self.snapshots_dir.exists() {
            return Ok(versions);
        }

        for entry in fs::read_dir(&self.snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("snapshot_") && name.ends_with(".zst") {
                    let version_str = name
                        .trim_start_matches("snapshot_")
                        .trim_end_matches(".zst");

                    if let Ok(version) = version_str.parse::<u64>() {
                        versions.push(version);
                    }
                }
            }
        }

        versions.sort();
        Ok(versions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;
    use tempfile::TempDir;

    fn create_test_state(version: u64) -> TimelineState {
        TimelineState {
            clips: vec![
                Clip {
                    id: "clip-1".to_string(),
                    track_id: "video_track_1".to_string(),
                    start: 0.0,
                    duration: 10.0,
                    source_file: "/path/to/video1.mp4".to_string(),
                },
                Clip {
                    id: "clip-2".to_string(),
                    track_id: "video_track_1".to_string(),
                    start: 10.0,
                    duration: 5.0,
                    source_file: "/path/to/video2.mp4".to_string(),
                },
            ],
            duration: 15.0,
            playhead_time: 5.0,
            version,
            ..TimelineState::default()
        }
    }

    #[test]
    fn test_snapshot_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        let state = create_test_state(50);
        store.save(50, &state).unwrap();

        let loaded = store.load(50).unwrap();
        assert_eq!(loaded.version, 50);
        assert_eq!(loaded.clips.len(), 2);
        assert_eq!(loaded.duration, 15.0);
    }

    #[test]
    fn test_snapshot_latest() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Save multiple snapshots
        store.save(50, &create_test_state(50)).unwrap();
        store.save(100, &create_test_state(100)).unwrap();
        store.save(75, &create_test_state(75)).unwrap();

        // Latest should be 100
        let (version, state) = store.latest().unwrap().unwrap();
        assert_eq!(version, 100);
        assert_eq!(state.version, 100);
    }

    #[test]
    fn test_snapshot_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        let result = store.load(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_snapshot() {
        assert!(!SnapshotStore::should_snapshot(0));
        assert!(!SnapshotStore::should_snapshot(1));
        assert!(!SnapshotStore::should_snapshot(49));
        assert!(SnapshotStore::should_snapshot(50));
        assert!(!SnapshotStore::should_snapshot(51));
        assert!(SnapshotStore::should_snapshot(100));
        assert!(SnapshotStore::should_snapshot(150));
    }

    #[test]
    fn test_list_versions() {
        let temp_dir = TempDir::new().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_path_buf()).unwrap();

        store.save(50, &create_test_state(50)).unwrap();
        store.save(100, &create_test_state(100)).unwrap();
        store.save(150, &create_test_state(150)).unwrap();

        let versions = store.list_versions().unwrap();
        assert_eq!(versions, vec![50, 100, 150]);
    }
}
