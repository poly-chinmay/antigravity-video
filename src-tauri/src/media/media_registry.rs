//! MediaRegistry - Non-authoritative session cache for media sources.
//!
//! # Design Philosophy
//!
//! The MediaRegistry is a SESSION CACHE, not authoritative storage.
//!
//! **Key Invariants:**
//! - Clips are SELF-SUFFICIENT: All source metadata (duration, path) is stored in the Clip itself
//! - Registry can be rebuilt at any time from disk/import operations
//! - Missing registry entries are NOT fatal - clips still have all data needed for playback
//! - Registry is used for LOOKUP OPTIMIZATION, not source-of-truth
//!
//! **Failure Handling:**
//! - If source file is deleted: Clip remains valid, playback returns error
//! - If registry is cleared: Clips continue working with embedded metadata
//! - Registry is purely for avoiding re-probe on repeated operations
//!
//! # Thread Safety
//!
//! MediaRegistry uses interior mutability with RwLock for safe concurrent access.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::media::MediaSource;

/// Unique identifier for a registered media source.
pub type SourceId = String;

// =============================================================================
// REGISTRY ERROR
// =============================================================================

/// Errors from MediaRegistry operations.
#[derive(Debug, Clone)]
pub enum RegistryError {
    /// Source not found in registry (caller should re-import)
    SourceNotFound(SourceId),

    /// Source file no longer exists on disk
    SourceUnavailable {
        source_id: SourceId,
        path: PathBuf,
        reason: String,
    },

    /// Lock acquisition failed
    LockError(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceNotFound(id) => write!(
                f,
                "Source not in registry: {} (re-import may be needed)",
                id
            ),
            Self::SourceUnavailable {
                source_id,
                path,
                reason,
            } => {
                write!(
                    f,
                    "Source unavailable: {} at {:?} - {}",
                    source_id, path, reason
                )
            }
            Self::LockError(msg) => write!(f, "Registry lock error: {}", msg),
        }
    }
}

impl std::error::Error for RegistryError {}

// =============================================================================
// MEDIA REGISTRY
// =============================================================================

/// Non-authoritative session cache for imported media sources.
///
/// # Usage
///
/// ```ignore
/// let registry = MediaRegistry::new();
///
/// // After importing a file
/// let source = import_media(path).await?;
/// let source_id = registry.register(source);
///
/// // Later, when creating a clip
/// if let Some(source) = registry.get(&source_id) {
///     let clip = Clip::from_source(track_id, start, &source.path, source.duration());
/// }
///
/// // If registry is cleared, clips still work - they have embedded metadata
/// ```
#[derive(Debug, Default)]
pub struct MediaRegistry {
    /// Source map: id -> MediaSource
    sources: RwLock<HashMap<SourceId, MediaSource>>,
}

impl MediaRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // REGISTRATION
    // =========================================================================

    /// Register a media source and return its ID.
    ///
    /// If a source with the same ID already exists, it is replaced.
    pub fn register(&self, source: MediaSource) -> SourceId {
        let id = source.id.clone();
        if let Ok(mut sources) = self.sources.write() {
            sources.insert(id.clone(), source);
        }
        id
    }

    /// Register multiple sources at once.
    pub fn register_batch(&self, sources: Vec<MediaSource>) -> Vec<SourceId> {
        let ids: Vec<SourceId> = sources.iter().map(|s| s.id.clone()).collect();
        if let Ok(mut map) = self.sources.write() {
            for source in sources {
                map.insert(source.id.clone(), source);
            }
        }
        ids
    }

    /// Unregister a source (e.g., when file is removed from project).
    pub fn unregister(&self, id: &SourceId) -> Option<MediaSource> {
        self.sources.write().ok()?.remove(id)
    }

    // =========================================================================
    // LOOKUP
    // =========================================================================

    /// Get a source by ID.
    ///
    /// Returns `None` if not in registry (caller should re-import if needed).
    pub fn get(&self, id: &SourceId) -> Option<MediaSource> {
        self.sources.read().ok()?.get(id).cloned()
    }

    /// Check if a source is registered.
    pub fn contains(&self, id: &SourceId) -> bool {
        self.sources
            .read()
            .map(|s| s.contains_key(id))
            .unwrap_or(false)
    }

    /// Get all registered source IDs.
    pub fn source_ids(&self) -> Vec<SourceId> {
        self.sources
            .read()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all registered sources.
    pub fn all_sources(&self) -> Vec<MediaSource> {
        self.sources
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get count of registered sources.
    pub fn len(&self) -> usize {
        self.sources.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // =========================================================================
    // VALIDATION
    // =========================================================================

    /// Validate that a source exists and its file is still available.
    ///
    /// This performs a disk check - use sparingly.
    pub fn validate_source(&self, id: &SourceId) -> Result<MediaSource, RegistryError> {
        let source = self
            .get(id)
            .ok_or_else(|| RegistryError::SourceNotFound(id.clone()))?;

        // Check if file still exists on disk
        if !source.path.exists() {
            return Err(RegistryError::SourceUnavailable {
                source_id: id.clone(),
                path: source.path.clone(),
                reason: "File no longer exists".to_string(),
            });
        }

        Ok(source)
    }

    /// Get source file path by ID (for quick path lookup).
    pub fn get_path(&self, id: &SourceId) -> Option<PathBuf> {
        self.get(id).map(|s| s.path)
    }

    /// Get source by file path (for duplicate detection).
    ///
    /// Returns the first source with a matching path.
    pub fn get_by_path(&self, path: &std::path::Path) -> Option<MediaSource> {
        self.sources
            .read()
            .ok()?
            .values()
            .find(|s| s.path == path)
            .cloned()
    }

    // =========================================================================
    // MAINTENANCE
    // =========================================================================

    /// Clear all registered sources.
    ///
    /// This does NOT affect existing clips - they have embedded metadata.
    pub fn clear(&self) {
        if let Ok(mut sources) = self.sources.write() {
            sources.clear();
        }
    }

    /// Remove sources whose files no longer exist.
    ///
    /// Returns the IDs of removed sources.
    pub fn prune_missing(&self) -> Vec<SourceId> {
        let mut removed = Vec::new();

        if let Ok(mut sources) = self.sources.write() {
            let ids_to_remove: Vec<SourceId> = sources
                .iter()
                .filter(|(_, s)| !s.path.exists())
                .map(|(id, _)| id.clone())
                .collect();

            for id in ids_to_remove {
                sources.remove(&id);
                removed.push(id);
            }
        }

        removed
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_source(id: &str) -> MediaSource {
        MediaSource {
            id: id.to_string(),
            path: PathBuf::from(format!("/test/{}.mp4", id)),
            duration_secs: 10.0,
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            file_size: 1_000_000,
            display_name: format!("{}.mp4", id),
        }
    }

    #[test]
    fn test_register_and_get() {
        let registry = MediaRegistry::new();
        let source = make_test_source("src1");

        let id = registry.register(source.clone());

        assert_eq!(id, "src1");
        assert!(registry.contains(&id));

        let retrieved = registry.get(&id).unwrap();
        assert_eq!(retrieved.id, "src1");
        assert_eq!(retrieved.duration_secs, 10.0);
    }

    #[test]
    fn test_register_replaces_existing() {
        let registry = MediaRegistry::new();

        let source1 = make_test_source("src1");
        registry.register(source1);

        // Register with same ID but different duration
        let mut source2 = make_test_source("src1");
        source2.duration_secs = 20.0;
        registry.register(source2);

        let retrieved = registry.get(&"src1".to_string()).unwrap();
        assert_eq!(retrieved.duration_secs, 20.0);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_unregister() {
        let registry = MediaRegistry::new();
        let source = make_test_source("src1");

        registry.register(source);
        assert!(registry.contains(&"src1".to_string()));

        let removed = registry.unregister(&"src1".to_string());
        assert!(removed.is_some());
        assert!(!registry.contains(&"src1".to_string()));
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let registry = MediaRegistry::new();

        let result = registry.get(&"nonexistent".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_batch_register() {
        let registry = MediaRegistry::new();

        let sources = vec![
            make_test_source("src1"),
            make_test_source("src2"),
            make_test_source("src3"),
        ];

        let ids = registry.register_batch(sources);

        assert_eq!(ids.len(), 3);
        assert_eq!(registry.len(), 3);
        assert!(registry.contains(&"src1".to_string()));
        assert!(registry.contains(&"src2".to_string()));
        assert!(registry.contains(&"src3".to_string()));
    }

    #[test]
    fn test_source_ids() {
        let registry = MediaRegistry::new();

        registry.register(make_test_source("src1"));
        registry.register(make_test_source("src2"));

        let ids = registry.source_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"src1".to_string()));
        assert!(ids.contains(&"src2".to_string()));
    }

    #[test]
    fn test_clear() {
        let registry = MediaRegistry::new();

        registry.register(make_test_source("src1"));
        registry.register(make_test_source("src2"));
        assert_eq!(registry.len(), 2);

        registry.clear();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_validate_source_not_found() {
        let registry = MediaRegistry::new();

        let result = registry.validate_source(&"nonexistent".to_string());
        assert!(matches!(result, Err(RegistryError::SourceNotFound(_))));
    }

    #[test]
    fn test_validate_source_file_missing() {
        let registry = MediaRegistry::new();

        // Register source with non-existent path
        let source = make_test_source("src1");
        registry.register(source);

        // Validation should fail because file doesn't exist
        let result = registry.validate_source(&"src1".to_string());
        assert!(matches!(
            result,
            Err(RegistryError::SourceUnavailable { .. })
        ));
    }
}
