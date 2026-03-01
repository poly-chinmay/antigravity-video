//! FrameCache - LRU cache for rendered frames.
//!
//! # Design
//!
//! FrameCache stores recently rendered frames to avoid re-rendering
//! when seeking backwards or scrubbing. Uses simple LRU eviction.
//!
//! # Memory Management
//!
//! Cache keys are based on (ClipId, SourceOffset) to maximize reuse.
//! Frames are stored as opaque buffers with metadata.

use std::collections::{HashMap, VecDeque};

use crate::engine::media_time::MediaTime;

use super::frame_clock::FrameId;

// =============================================================================
// CACHE KEY
// =============================================================================

/// Key for cached frames.
///
/// Uses clip ID and source offset for maximum cache hit rate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Clip ID
    pub clip_id: String,

    /// Offset into source (quantized to frame)
    pub source_offset_frame: u64,

    /// Resolution (width x height)
    pub resolution: u32,
}

impl CacheKey {
    /// Create a new cache key.
    pub fn new(
        clip_id: String,
        source_offset: MediaTime,
        frame_interval_ns: i64,
        width: u32,
    ) -> Self {
        let source_offset_frame = (source_offset.as_nanos() / frame_interval_ns) as u64;
        Self {
            clip_id,
            source_offset_frame,
            resolution: width,
        }
    }
}

// =============================================================================
// CACHED FRAME
// =============================================================================

/// A cached rendered frame.
#[derive(Debug, Clone)]
pub struct CachedFrame {
    /// Unique identifier
    pub id: u64,

    /// Cache key
    pub key: CacheKey,

    /// When this frame was cached
    pub cached_at_ns: u64,

    /// Size in bytes
    pub size_bytes: usize,

    /// Frame data (simplified - in reality would be image buffer)
    pub data: Vec<u8>,
}

impl CachedFrame {
    /// Create a new cached frame.
    pub fn new(id: u64, key: CacheKey, data: Vec<u8>, timestamp_ns: u64) -> Self {
        let size_bytes = data.len();
        Self {
            id,
            key,
            cached_at_ns: timestamp_ns,
            size_bytes,
            data,
        }
    }
}

// =============================================================================
// CACHE STATS
// =============================================================================

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Cache hits
    pub hits: u64,

    /// Cache misses
    pub misses: u64,

    /// Evictions
    pub evictions: u64,

    /// Current entries
    pub entries: usize,

    /// Current size in bytes
    pub size_bytes: usize,
}

impl CacheStats {
    /// Get hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

// =============================================================================
// FRAME CACHE
// =============================================================================

/// LRU frame cache.
///
/// # Usage
///
/// ```ignore
/// let mut cache = FrameCache::with_capacity(100);
///
/// // Try cache first
/// if let Some(frame) = cache.get(&key) {
///     return frame;
/// }
///
/// // Render and cache
/// let frame = render(...);
/// cache.put(key, frame);
/// ```
#[derive(Debug)]
pub struct FrameCache {
    /// Cache storage
    entries: HashMap<CacheKey, CachedFrame>,

    /// LRU order (most recently used at back)
    lru_order: VecDeque<CacheKey>,

    /// Maximum entries
    max_entries: usize,

    /// Maximum size in bytes
    max_bytes: usize,

    /// Current size in bytes
    current_bytes: usize,

    /// Statistics
    stats: CacheStats,

    /// Next frame ID
    next_id: u64,
}

impl FrameCache {
    /// Create cache with max entries.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            lru_order: VecDeque::with_capacity(max_entries),
            max_entries,
            max_bytes: 500 * 1024 * 1024, // 500 MB default
            current_bytes: 0,
            stats: CacheStats::default(),
            next_id: 0,
        }
    }

    /// Create cache with max size in bytes.
    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
            max_entries: usize::MAX,
            max_bytes,
            current_bytes: 0,
            stats: CacheStats::default(),
            next_id: 0,
        }
    }

    /// Get a cached frame.
    pub fn get(&mut self, key: &CacheKey) -> Option<&CachedFrame> {
        if self.entries.contains_key(key) {
            self.stats.hits += 1;
            // Move to back of LRU
            self.touch(key);
            self.entries.get(key)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Put a frame in cache.
    pub fn put(&mut self, key: CacheKey, data: Vec<u8>, timestamp_ns: u64) {
        let size = data.len();

        // Evict if needed
        while self.needs_eviction(size) {
            if !self.evict_one() {
                break;
            }
        }

        // Create frame
        let id = self.next_id;
        self.next_id += 1;

        let frame = CachedFrame::new(id, key.clone(), data, timestamp_ns);

        // Remove old entry if exists
        if let Some(old) = self.entries.remove(&key) {
            self.current_bytes -= old.size_bytes;
            self.lru_order.retain(|k| k != &key);
        }

        // Insert new
        self.current_bytes += size;
        self.entries.insert(key.clone(), frame);
        self.lru_order.push_back(key);

        self.update_stats();
    }

    /// Check if key is cached.
    pub fn contains(&self, key: &CacheKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru_order.clear();
        self.current_bytes = 0;
        self.update_stats();
    }

    /// Get statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Get current entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get current size in bytes.
    pub fn size_bytes(&self) -> usize {
        self.current_bytes
    }

    // =========================================================================
    // INTERNAL
    // =========================================================================

    /// Check if we need to evict.
    fn needs_eviction(&self, incoming_size: usize) -> bool {
        self.entries.len() >= self.max_entries
            || self.current_bytes + incoming_size > self.max_bytes
    }

    /// Evict least recently used entry.
    fn evict_one(&mut self) -> bool {
        if let Some(key) = self.lru_order.pop_front() {
            if let Some(frame) = self.entries.remove(&key) {
                self.current_bytes -= frame.size_bytes;
                self.stats.evictions += 1;
                return true;
            }
        }
        false
    }

    /// Touch an entry (move to end of LRU).
    fn touch(&mut self, key: &CacheKey) {
        self.lru_order.retain(|k| k != key);
        self.lru_order.push_back(key.clone());
    }

    /// Update stats.
    fn update_stats(&mut self) {
        self.stats.entries = self.entries.len();
        self.stats.size_bytes = self.current_bytes;
    }
}

impl Default for FrameCache {
    fn default() -> Self {
        Self::with_capacity(100)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_key(clip: &str, frame: u64) -> CacheKey {
        CacheKey {
            clip_id: clip.to_string(),
            source_offset_frame: frame,
            resolution: 1920,
        }
    }

    fn make_data(size: usize) -> Vec<u8> {
        vec![0u8; size]
    }

    #[test]
    fn test_cache_basic() {
        let mut cache = FrameCache::with_capacity(10);

        let key = make_key("c1", 0);
        cache.put(key.clone(), make_data(1000), 0);

        assert!(cache.contains(&key));
        assert_eq!(cache.len(), 1);

        let frame = cache.get(&key).unwrap();
        assert_eq!(frame.size_bytes, 1000);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let mut cache = FrameCache::with_capacity(3);

        cache.put(make_key("c1", 0), make_data(100), 0);
        cache.put(make_key("c1", 1), make_data(100), 1);
        cache.put(make_key("c1", 2), make_data(100), 2);

        // Cache full, add one more
        cache.put(make_key("c1", 3), make_data(100), 3);

        // Oldest (frame 0) should be evicted
        assert!(!cache.contains(&make_key("c1", 0)));
        assert!(cache.contains(&make_key("c1", 3)));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_cache_touch_updates_lru() {
        let mut cache = FrameCache::with_capacity(3);

        cache.put(make_key("c1", 0), make_data(100), 0);
        cache.put(make_key("c1", 1), make_data(100), 1);
        cache.put(make_key("c1", 2), make_data(100), 2);

        // Touch frame 0, making it most recently used
        cache.get(&make_key("c1", 0));

        // Add new frame, frame 1 should be evicted (now oldest)
        cache.put(make_key("c1", 3), make_data(100), 3);

        assert!(cache.contains(&make_key("c1", 0))); // Was touched
        assert!(!cache.contains(&make_key("c1", 1))); // Evicted
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut cache = FrameCache::with_capacity(10);

        cache.put(make_key("c1", 0), make_data(100), 0);

        // 3 hits
        cache.get(&make_key("c1", 0));
        cache.get(&make_key("c1", 0));
        cache.get(&make_key("c1", 0));

        // 1 miss
        cache.get(&make_key("c1", 999));

        assert_eq!(cache.stats().hits, 3);
        assert_eq!(cache.stats().misses, 1);
        assert!((cache.stats().hit_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_cache_size_limit() {
        // Max 1000 bytes
        let mut cache = FrameCache::with_max_bytes(1000);

        // Each frame 400 bytes
        cache.put(make_key("c1", 0), make_data(400), 0);
        cache.put(make_key("c1", 1), make_data(400), 1);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.size_bytes(), 800);

        // Adding 400 more would exceed, should evict
        cache.put(make_key("c1", 2), make_data(400), 2);

        assert_eq!(cache.len(), 2);
        assert!(cache.size_bytes() <= 1000);
    }
}
