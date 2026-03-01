//! RenderOrchestrator - Pipeline coordinator.
//!
//! # Design
//!
//! RenderOrchestrator coordinates the entire render pipeline:
//! - Receives frames from FrameScheduler
//! - Checks cache for rendered frames
//! - Dispatches to renderer (non-blocking)
//! - Collects results
//!
//! # Non-Blocking Guarantee
//!
//! The orchestrator never blocks the engine. Slow rendering is handled
//! by dropping frames rather than stalling.
//!
//! # Thread Model
//!
//! - Orchestrator runs in engine thread
//! - Renderer runs in separate thread(s)
//! - Communication via channels (simulated in tests)

use std::collections::VecDeque;

use crate::engine::media_time::MediaTime;

use super::frame_cache::{CacheKey, FrameCache};
use super::frame_clock::FrameId;
use super::render_command::{RenderCommand, RenderResult};

// =============================================================================
// ORCHESTRATOR CONFIG
// =============================================================================

/// Configuration for the orchestrator.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Maximum pending renders
    pub max_pending: usize,

    /// Enable frame cache
    pub cache_enabled: bool,

    /// Cache capacity
    pub cache_capacity: usize,

    /// Frame interval for cache keys
    pub frame_interval_ns: i64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_pending: 4,
            cache_enabled: true,
            cache_capacity: 100,
            frame_interval_ns: 16_666_667, // ~60fps
        }
    }
}

// =============================================================================
// ORCHESTRATOR STATS
// =============================================================================

/// Orchestrator statistics.
#[derive(Debug, Clone, Default)]
pub struct OrchestratorStats {
    /// Total commands submitted
    pub commands_submitted: u64,

    /// Commands completed successfully
    pub commands_completed: u64,

    /// Commands failed
    pub commands_failed: u64,

    /// Commands dropped (slow renderer)
    pub commands_dropped: u64,

    /// Cache hits
    pub cache_hits: u64,

    /// Cache misses
    pub cache_misses: u64,
}

// =============================================================================
// RENDER ORCHESTRATOR
// =============================================================================

/// Pipeline coordinator for rendering.
///
/// # Usage
///
/// ```ignore
/// let mut orchestrator = RenderOrchestrator::new(config);
///
/// // Submit frame for rendering
/// orchestrator.submit(cmd);
///
/// // Poll for completed frames
/// while let Some(result) = orchestrator.poll_result() {
///     display(result);
/// }
/// ```
#[derive(Debug)]
pub struct RenderOrchestrator {
    /// Pending render commands (not yet started)
    pending: VecDeque<RenderCommand>,

    /// In-flight renders (started, waiting for result)
    in_flight: Vec<RenderCommand>,

    /// Completed results
    completed: VecDeque<RenderResult>,

    /// Frame cache
    cache: FrameCache,

    /// Configuration
    config: OrchestratorConfig,

    /// Statistics
    stats: OrchestratorStats,

    /// Mock render time (for testing)
    #[cfg(test)]
    mock_render_time_ns: u64,
}

impl RenderOrchestrator {
    /// Create a new orchestrator.
    pub fn new(config: OrchestratorConfig) -> Self {
        let cache = FrameCache::with_capacity(config.cache_capacity);

        Self {
            pending: VecDeque::new(),
            in_flight: Vec::with_capacity(config.max_pending),
            completed: VecDeque::new(),
            cache,
            config,
            stats: OrchestratorStats::default(),
            #[cfg(test)]
            mock_render_time_ns: 1_000_000, // 1ms default
        }
    }

    /// Submit a render command.
    ///
    /// Returns true if submitted, false if dropped.
    pub fn submit(&mut self, cmd: RenderCommand) -> bool {
        self.stats.commands_submitted += 1;

        // Check cache first
        if self.config.cache_enabled {
            if self.check_cache(&cmd) {
                return true;
            }
        }

        // Check if we have capacity
        if self.pending.len() + self.in_flight.len() >= self.config.max_pending {
            // Drop oldest pending
            if self.pending.pop_front().is_some() {
                self.stats.commands_dropped += 1;
            } else {
                // All in-flight, drop this command
                self.stats.commands_dropped += 1;
                return false;
            }
        }

        self.pending.push_back(cmd);
        true
    }

    /// Process pending commands (simulate dispatch to renderer).
    pub fn process(&mut self) {
        // Move pending to in-flight
        while let Some(cmd) = self.pending.pop_front() {
            if self.in_flight.len() < self.config.max_pending {
                self.in_flight.push(cmd);
            } else {
                // Put back and stop
                self.pending.push_front(cmd);
                break;
            }
        }
    }

    /// Simulate render completion (for testing).
    #[cfg(test)]
    pub fn simulate_complete(&mut self, frame_id: FrameId, success: bool) {
        // Find and remove from in-flight
        if let Some(idx) = self.in_flight.iter().position(|c| c.frame_id == frame_id) {
            let cmd = self.in_flight.remove(idx);

            let result = if success {
                self.stats.commands_completed += 1;

                // Add to cache
                if self.config.cache_enabled {
                    self.cache_frame(&cmd);
                }

                RenderResult::success(cmd.frame_id, cmd.position, self.mock_render_time_ns)
            } else {
                self.stats.commands_failed += 1;
                RenderResult::failure(cmd.frame_id, cmd.position, "simulated failure".to_string())
            };

            self.completed.push_back(result);
        }
    }

    /// Complete all in-flight renders (for testing).
    #[cfg(test)]
    pub fn complete_all(&mut self) {
        let in_flight: Vec<_> = self.in_flight.drain(..).collect();
        for cmd in in_flight {
            self.stats.commands_completed += 1;

            if self.config.cache_enabled {
                self.cache_frame(&cmd);
            }

            let result =
                RenderResult::success(cmd.frame_id, cmd.position, self.mock_render_time_ns);
            self.completed.push_back(result);
        }
    }

    /// Poll for a completed result.
    pub fn poll_result(&mut self) -> Option<RenderResult> {
        self.completed.pop_front()
    }

    /// Check if there are pending or in-flight commands.
    pub fn has_work(&self) -> bool {
        !self.pending.is_empty() || !self.in_flight.is_empty()
    }

    /// Get number of pending commands.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get number of in-flight commands.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Get number of completed results.
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// Get statistics.
    pub fn stats(&self) -> &OrchestratorStats {
        &self.stats
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> &super::frame_cache::CacheStats {
        self.cache.stats()
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.in_flight.clear();
        self.completed.clear();
    }

    // =========================================================================
    // INTERNAL
    // =========================================================================

    /// Check cache for frame.
    fn check_cache(&mut self, cmd: &RenderCommand) -> bool {
        // For each clip, check if cached
        for clip_info in &cmd.clips {
            let key = CacheKey::new(
                clip_info.clip_id.clone(),
                clip_info.source_offset,
                self.config.frame_interval_ns,
                cmd.width,
            );

            if self.cache.get(&key).is_some() {
                self.stats.cache_hits += 1;
                // In real implementation, would use cached frame
                return true;
            }
        }

        if !cmd.clips.is_empty() {
            self.stats.cache_misses += 1;
        }

        false
    }

    /// Add frame to cache.
    fn cache_frame(&mut self, cmd: &RenderCommand) {
        for clip_info in &cmd.clips {
            let key = CacheKey::new(
                clip_info.clip_id.clone(),
                clip_info.source_offset,
                self.config.frame_interval_ns,
                cmd.width,
            );

            // Simulate frame data (in reality would be actual pixels)
            let fake_data = vec![0u8; 1000];
            self.cache.put(key, fake_data, 0);
        }
    }
}

impl Default for RenderOrchestrator {
    fn default() -> Self {
        Self::new(OrchestratorConfig::default())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::super::render_command::ClipRenderInfo;
    use super::*;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_cmd(frame: u64, with_clip: bool) -> RenderCommand {
        let clips = if with_clip {
            vec![ClipRenderInfo {
                clip_id: "c1".to_string(),
                track_id: "t1".to_string(),
                source_file: "test.mp4".to_string(),
                source_offset: ms(frame as i64 * 16),
                layer: 0,
            }]
        } else {
            vec![]
        };

        RenderCommand::new(FrameId(frame), ms(frame as i64 * 16), clips)
    }

    #[test]
    fn test_orchestrator_submit() {
        let mut orchestrator = RenderOrchestrator::default();

        assert!(orchestrator.submit(make_cmd(1, true)));
        assert_eq!(orchestrator.pending_count(), 1);
    }

    #[test]
    fn test_slow_renderer_does_not_block_engine() {
        let config = OrchestratorConfig {
            max_pending: 3,
            cache_enabled: false,
            ..Default::default()
        };
        let mut orchestrator = RenderOrchestrator::new(config);

        // Submit many frames quickly
        for i in 0..10 {
            orchestrator.submit(make_cmd(i, true));
        }

        // Should be capped
        assert!(orchestrator.pending_count() <= 3);
        assert!(orchestrator.stats().commands_dropped > 0);
    }

    #[test]
    fn test_process_moves_to_in_flight() {
        let mut orchestrator = RenderOrchestrator::default();

        orchestrator.submit(make_cmd(1, true));
        orchestrator.submit(make_cmd(2, true));

        assert_eq!(orchestrator.pending_count(), 2);
        assert_eq!(orchestrator.in_flight_count(), 0);

        orchestrator.process();

        assert_eq!(orchestrator.pending_count(), 0);
        assert_eq!(orchestrator.in_flight_count(), 2);
    }

    #[test]
    fn test_complete_all() {
        let mut orchestrator = RenderOrchestrator::default();

        orchestrator.submit(make_cmd(1, true));
        orchestrator.submit(make_cmd(2, true));
        orchestrator.process();

        orchestrator.complete_all();

        assert_eq!(orchestrator.in_flight_count(), 0);
        assert_eq!(orchestrator.completed_count(), 2);

        let result = orchestrator.poll_result().unwrap();
        assert!(result.success);
        assert_eq!(result.frame_id, FrameId(1));
    }

    #[test]
    fn test_cache_hit() {
        let mut orchestrator = RenderOrchestrator::default();

        // First submit - cache miss
        orchestrator.submit(make_cmd(1, true));
        orchestrator.process();
        orchestrator.complete_all();

        // Second submit of similar frame - cache hit
        let hit = orchestrator.submit(make_cmd(1, true));

        // Should have cache hit
        assert!(orchestrator.stats().cache_hits >= 1);
    }

    #[test]
    fn test_queue_never_overflows() {
        let config = OrchestratorConfig {
            max_pending: 5,
            cache_enabled: false,
            ..Default::default()
        };
        let mut orchestrator = RenderOrchestrator::new(config);

        // Rapidly submit many frames without processing
        for i in 0..100 {
            orchestrator.submit(make_cmd(i, true));

            // Total pending + in_flight should never exceed max
            let total = orchestrator.pending_count() + orchestrator.in_flight_count();
            assert!(
                total <= 5,
                "Queue overflow at frame {}: {} pending + {} in_flight",
                i,
                orchestrator.pending_count(),
                orchestrator.in_flight_count()
            );
        }
    }
}
