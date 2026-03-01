// src-tauri/src/persistence/event_store.rs
//! Event Store for persistent event sourcing
//!
//! Stores all successful edit operations as immutable events.
//! Uses atomic file writes with fsync for durability.

use crate::edit_plan::EditPlan;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single event representing a successful mutation
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Event {
    /// Monotonically increasing version number
    pub version: u64,
    /// Unix timestamp in milliseconds
    pub timestamp: u64,
    /// The edit plan that was executed
    pub edit_plan: EditPlan,
    /// Original user instruction (if from AI)
    pub user_intent: Option<String>,
    /// AI confidence score (if from AI)
    pub ai_confidence: Option<f32>,
    /// Time taken to execute the plan in milliseconds
    pub execution_time_ms: u64,
    /// Whether the execution succeeded
    pub success: bool,
}

impl Event {
    /// Create a new event with current timestamp
    pub fn new(
        version: u64,
        edit_plan: EditPlan,
        user_intent: Option<String>,
        ai_confidence: Option<f32>,
        execution_time_ms: u64,
        success: bool,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            version,
            timestamp,
            edit_plan,
            user_intent,
            ai_confidence,
            execution_time_ms,
            success,
        }
    }
}

/// Persistent event store with atomic writes
pub struct EventStore {
    /// Base path for event storage
    base_path: PathBuf,
    /// Path to events directory
    events_dir: PathBuf,
}

impl EventStore {
    /// Create a new EventStore at the given base path
    ///
    /// Creates the events directory if it doesn't exist.
    pub fn new(base_path: PathBuf) -> std::io::Result<Self> {
        let events_dir = base_path.join("events");

        // Create events directory if it doesn't exist
        if !events_dir.exists() {
            fs::create_dir_all(&events_dir)?;
        }

        Ok(Self {
            base_path,
            events_dir,
        })
    }

    /// Generate filename for a given version
    fn event_filename(&self, version: u64) -> PathBuf {
        self.events_dir.join(format!("{:08}.json", version))
    }

    /// Generate temp filename for atomic writes
    fn temp_filename(&self, version: u64) -> PathBuf {
        self.events_dir.join(format!("{:08}.json.tmp", version))
    }

    /// Append an event to the store with atomic write
    ///
    /// Process:
    /// 1. Write to temp file
    /// 2. fsync temp file
    /// 3. Atomic rename to final path
    /// 4. fsync directory
    pub fn append(&self, event: &Event) -> std::io::Result<()> {
        let final_path = self.event_filename(event.version);
        let temp_path = self.temp_filename(event.version);

        // 1. Serialize event to JSON
        let json = serde_json::to_string_pretty(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 2. Write to temp file
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;

            file.write_all(json.as_bytes())?;

            // 3. fsync temp file
            file.sync_all()?;
        }

        // 4. Atomic rename
        fs::rename(&temp_path, &final_path)?;

        // 5. fsync directory (platform-specific)
        #[cfg(unix)]
        {
            let dir = File::open(&self.events_dir)?;
            dir.sync_all()?;
        }

        println!(
            "📝 [EventStore] Appended event v{} to {}",
            event.version,
            final_path.display()
        );

        Ok(())
    }

    /// Load all events from the store, sorted by version
    pub fn load_all(&self) -> std::io::Result<Vec<Event>> {
        let mut events = Vec::new();

        if !self.events_dir.exists() {
            return Ok(events);
        }

        let mut entries: Vec<_> = fs::read_dir(&self.events_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .collect();

        // Sort by filename (which is the version number)
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            let mut file = File::open(&path)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;

            match serde_json::from_str::<Event>(&content) {
                Ok(event) => events.push(event),
                Err(e) => {
                    eprintln!("⚠️ [EventStore] Failed to parse {}: {}", path.display(), e);
                }
            }
        }

        println!(
            "📂 [EventStore] Loaded {} events from {}",
            events.len(),
            self.events_dir.display()
        );

        Ok(events)
    }

    /// Get events in a version range (inclusive)
    pub fn get_range(&self, start: u64, end: u64) -> std::io::Result<Vec<Event>> {
        let mut events = Vec::new();

        for version in start..=end {
            let path = self.event_filename(version);
            if path.exists() {
                let mut file = File::open(&path)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;

                match serde_json::from_str::<Event>(&content) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        eprintln!("⚠️ [EventStore] Failed to parse {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(events)
    }

    /// Get the highest version number in the store
    pub fn get_latest_version(&self) -> std::io::Result<Option<u64>> {
        if !self.events_dir.exists() {
            return Ok(None);
        }

        let mut max_version: Option<u64> = None;

        for entry in fs::read_dir(&self.events_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    if let Ok(version) = stem.to_string_lossy().parse::<u64>() {
                        max_version = Some(max_version.map_or(version, |m| m.max(version)));
                    }
                }
            }
        }

        Ok(max_version)
    }

    /// Get the path to the events directory
    pub fn events_path(&self) -> &PathBuf {
        &self.events_dir
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
                thought_process: Some("Test thought".to_string()),
                confidence: Some(0.9),
            },
            Some("Delete the test clip".to_string()),
            Some(0.9),
            50,
            true,
        )
    }

    #[test]
    fn test_event_store_append_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Append events
        let event1 = create_test_event(1);
        let event2 = create_test_event(2);

        store.append(&event1).unwrap();
        store.append(&event2).unwrap();

        // Load all
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].version, 1);
        assert_eq!(loaded[1].version, 2);
    }

    #[test]
    fn test_event_store_get_range() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Append events 1-5
        for i in 1..=5 {
            store.append(&create_test_event(i)).unwrap();
        }

        // Get range 2-4
        let range = store.get_range(2, 4).unwrap();
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].version, 2);
        assert_eq!(range[2].version, 4);
    }

    #[test]
    fn test_event_store_latest_version() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::new(temp_dir.path().to_path_buf()).unwrap();

        // Empty store
        assert_eq!(store.get_latest_version().unwrap(), None);

        // Add events
        store.append(&create_test_event(1)).unwrap();
        store.append(&create_test_event(5)).unwrap();
        store.append(&create_test_event(3)).unwrap();

        // Latest should be 5
        assert_eq!(store.get_latest_version().unwrap(), Some(5));
    }
}
