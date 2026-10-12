//! Media Pool - ViewModel for the Media Pool panel.
//!
//! # Design
//!
//! MediaPoolViewModel is a read-only projection of MediaRegistry.
//! It provides:
//! - List of all imported media items
//! - Status indicating file availability
//! - Metadata for display (name, duration, resolution)
//!
//! # Invariants
//!
//! - ViewModel is derived only (no mutation logic here)
//! - No file I/O performed during construction
//! - Status is computed from cached registry state

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::media::{MediaRegistry, MediaSource};

// =============================================================================
// MEDIA STATUS
// =============================================================================

/// Status of a media source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaStatus {
    /// File exists and is accessible
    Available,
    /// Source file is missing from disk
    Offline,
}

// =============================================================================
// MEDIA POOL ITEM
// =============================================================================

/// A single item in the media pool.
///
/// This is a view model - a projection of MediaSource for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPoolItem {
    /// Unique source identifier
    pub source_id: String,

    /// Display name (file name without path)
    pub file_name: String,

    /// Full file path
    pub file_path: String,

    /// Duration in seconds
    pub duration_secs: f64,

    /// Video resolution (width, height)
    pub resolution: Option<(u32, u32)>,

    /// Frame rate (fps)
    pub frame_rate: Option<f64>,

    /// Video codec name
    pub codec: String,

    /// File size in bytes
    pub file_size: u64,

    /// Current status
    pub status: MediaStatus,
}

impl MediaPoolItem {
    /// Create from a MediaSource, checking file availability.
    pub fn from_source(source: &MediaSource) -> Self {
        let status = if Path::new(&source.path).exists() {
            MediaStatus::Available
        } else {
            MediaStatus::Offline
        };

        Self {
            source_id: source.id.clone(),
            file_name: source.display_name.clone(),
            file_path: source.path.to_string_lossy().to_string(),
            duration_secs: source.duration_secs,
            resolution: Some((source.width, source.height)),
            frame_rate: Some(source.frame_rate),
            codec: source.video_codec.clone(),
            file_size: source.file_size,
            status,
        }
    }

    /// Create from a MediaSource without checking disk (for performance).
    pub fn from_source_unchecked(source: &MediaSource) -> Self {
        Self {
            source_id: source.id.clone(),
            file_name: source.display_name.clone(),
            file_path: source.path.to_string_lossy().to_string(),
            duration_secs: source.duration_secs,
            resolution: Some((source.width, source.height)),
            frame_rate: Some(source.frame_rate),
            codec: source.video_codec.clone(),
            file_size: source.file_size,
            status: MediaStatus::Available, // Assume available
        }
    }

    /// Format duration as MM:SS
    pub fn duration_formatted(&self) -> String {
        let total_secs = self.duration_secs as u64;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{:02}:{:02}", mins, secs)
    }

    /// Format resolution as "WxH"
    pub fn resolution_formatted(&self) -> String {
        match self.resolution {
            Some((w, h)) => format!("{}x{}", w, h),
            None => "Unknown".to_string(),
        }
    }
}

// =============================================================================
// MEDIA POOL VIEW MODEL
// =============================================================================

/// Complete view model for the Media Pool panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPoolViewModel {
    /// All media pool items
    pub items: Vec<MediaPoolItem>,

    /// Total count
    pub count: usize,

    /// Number of offline items
    pub offline_count: usize,
}

impl MediaPoolViewModel {
    /// Build from a MediaRegistry, checking file availability.
    ///
    /// This checks each file on disk, which may be slow for large pools.
    /// Use `from_registry_unchecked` for faster updates.
    pub fn from_registry(registry: &MediaRegistry) -> Self {
        let sources = registry.all_sources();
        let items: Vec<MediaPoolItem> = sources.iter().map(MediaPoolItem::from_source).collect();

        let offline_count = items
            .iter()
            .filter(|i| i.status == MediaStatus::Offline)
            .count();

        Self {
            count: items.len(),
            offline_count,
            items,
        }
    }

    /// Build from a MediaRegistry without checking disk.
    ///
    /// All items are assumed available. Use for frequent updates.
    pub fn from_registry_unchecked(registry: &MediaRegistry) -> Self {
        let sources = registry.all_sources();
        let items: Vec<MediaPoolItem> = sources
            .iter()
            .map(MediaPoolItem::from_source_unchecked)
            .collect();

        Self {
            count: items.len(),
            offline_count: 0,
            items,
        }
    }

    /// Create an empty view model.
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            count: 0,
            offline_count: 0,
        }
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get item by source ID.
    pub fn get(&self, source_id: &str) -> Option<&MediaPoolItem> {
        self.items.iter().find(|i| i.source_id == source_id)
    }
}

impl Default for MediaPoolViewModel {
    fn default() -> Self {
        Self::empty()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_source(id: &str) -> MediaSource {
        MediaSource {
            id: id.to_string(),
            path: PathBuf::from(format!("/test/{}.mp4", id)),
            duration_secs: 60.5,
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            file_size: 10_000_000,
            display_name: format!("{}.mp4", id),
        }
    }

    #[test]
    fn test_media_pool_item_from_source() {
        let source = make_test_source("test1");
        let item = MediaPoolItem::from_source(&source);

        assert_eq!(item.source_id, "test1");
        assert_eq!(item.file_name, "test1.mp4");
        assert_eq!(item.duration_secs, 60.5);
        assert_eq!(item.resolution, Some((1920, 1080)));
        assert_eq!(item.frame_rate, Some(30.0));
        // File doesn't exist, so should be offline
        assert_eq!(item.status, MediaStatus::Offline);
    }

    #[test]
    fn test_media_pool_item_unchecked() {
        let source = make_test_source("test1");
        let item = MediaPoolItem::from_source_unchecked(&source);

        // Should assume available
        assert_eq!(item.status, MediaStatus::Available);
    }

    #[test]
    fn test_duration_formatted() {
        let mut source = make_test_source("test1");
        source.duration_secs = 125.0; // 2:05

        let item = MediaPoolItem::from_source_unchecked(&source);
        assert_eq!(item.duration_formatted(), "02:05");
    }

    #[test]
    fn test_resolution_formatted() {
        let source = make_test_source("test1");
        let item = MediaPoolItem::from_source_unchecked(&source);

        assert_eq!(item.resolution_formatted(), "1920x1080");
    }

    #[test]
    fn test_media_pool_view_model_from_registry() {
        let registry = MediaRegistry::new();

        registry.register(make_test_source("src1"));
        registry.register(make_test_source("src2"));

        let view_model = MediaPoolViewModel::from_registry(&registry);

        assert_eq!(view_model.count, 2);
        // Both files are non-existent test paths
        assert_eq!(view_model.offline_count, 2);
    }

    #[test]
    fn test_media_pool_view_model_empty() {
        let view_model = MediaPoolViewModel::empty();

        assert!(view_model.is_empty());
        assert_eq!(view_model.count, 0);
    }

    #[test]
    fn test_media_pool_get_item() {
        let registry = MediaRegistry::new();
        registry.register(make_test_source("src1"));

        let view_model = MediaPoolViewModel::from_registry_unchecked(&registry);

        let item = view_model.get("src1");
        assert!(item.is_some());
        assert_eq!(item.unwrap().source_id, "src1");

        let missing = view_model.get("nonexistent");
        assert!(missing.is_none());
    }
}
