//! FrameQueue - Bounded frame buffer.
//!
//! # Design
//!
//! FrameQueue is a bounded buffer between the frame producer (scheduler)
//! and consumer (renderer). It prevents both:
//! - Starvation: Renderer always has frames to display
//! - Overflow: Queue has a maximum size, old frames are dropped
//!
//! # Thread Safety
//!
//! FrameQueue uses internal synchronization for safe cross-thread access.

use std::collections::VecDeque;

use super::frame_clock::FrameId;
use super::render_command::RenderCommand;

// =============================================================================
// QUEUE CONFIG
// =============================================================================

/// Configuration for the frame queue.
#[derive(Debug, Clone)]
pub struct FrameQueueConfig {
    /// Maximum frames in queue before dropping
    pub max_size: usize,

    /// Target buffer size (try to maintain this many ready frames)
    pub target_size: usize,

    /// Minimum frames before starting playback
    pub min_start_size: usize,
}

impl Default for FrameQueueConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            target_size: 3,
            min_start_size: 2,
        }
    }
}

// =============================================================================
// QUEUE STATS
// =============================================================================

/// Statistics about queue operation.
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    /// Total frames enqueued
    pub total_enqueued: u64,

    /// Total frames dequeued
    pub total_dequeued: u64,

    /// Frames dropped due to overflow
    pub dropped_overflow: u64,

    /// Frames dropped due to expiry
    pub dropped_expired: u64,

    /// Current queue depth
    pub current_depth: usize,

    /// High water mark
    pub max_depth: usize,
}

// =============================================================================
// FRAME QUEUE
// =============================================================================

/// Bounded frame queue with overflow protection.
///
/// # Usage
///
/// ```ignore
/// let mut queue = FrameQueue::new(FrameQueueConfig::default());
///
/// // Producer side
/// queue.push(render_command)?;
///
/// // Consumer side
/// if let Some(cmd) = queue.pop() {
///     render(cmd);
/// }
/// ```
#[derive(Debug)]
pub struct FrameQueue {
    /// The queue buffer
    buffer: VecDeque<RenderCommand>,

    /// Configuration
    config: FrameQueueConfig,

    /// Statistics
    stats: QueueStats,
}

impl FrameQueue {
    /// Create a new frame queue.
    pub fn new(config: FrameQueueConfig) -> Self {
        Self {
            buffer: VecDeque::with_capacity(config.max_size),
            config,
            stats: QueueStats::default(),
        }
    }

    /// Get current queue size.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Check if queue is full.
    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.config.max_size
    }

    /// Check if queue has enough frames to start playback.
    pub fn is_ready(&self) -> bool {
        self.buffer.len() >= self.config.min_start_size
    }

    /// Check if we need more frames.
    pub fn needs_frames(&self) -> bool {
        self.buffer.len() < self.config.target_size
    }

    /// Push a frame command.
    ///
    /// Returns true if successful, false if dropped due to overflow.
    pub fn push(&mut self, cmd: RenderCommand) -> bool {
        self.stats.total_enqueued += 1;

        // Check overflow
        if self.is_full() {
            // Drop oldest frame
            if self.buffer.pop_front().is_some() {
                self.stats.dropped_overflow += 1;
            }
        }

        self.buffer.push_back(cmd);
        self.stats.current_depth = self.buffer.len();
        self.stats.max_depth = self.stats.max_depth.max(self.buffer.len());

        true
    }

    /// Push with priority handling.
    ///
    /// High priority frames go to front of queue.
    pub fn push_priority(&mut self, cmd: RenderCommand) -> bool {
        use super::render_command::RenderPriority;

        self.stats.total_enqueued += 1;

        // High priority goes to front
        if cmd.priority >= RenderPriority::High {
            // For high priority, drop older low-priority frames
            while self.is_full() {
                if self.buffer.pop_back().is_some() {
                    self.stats.dropped_overflow += 1;
                }
            }
            self.buffer.push_front(cmd);
        } else {
            if self.is_full() {
                if self.buffer.pop_front().is_some() {
                    self.stats.dropped_overflow += 1;
                }
            }
            self.buffer.push_back(cmd);
        }

        self.stats.current_depth = self.buffer.len();
        self.stats.max_depth = self.stats.max_depth.max(self.buffer.len());

        true
    }

    /// Pop next frame to render.
    pub fn pop(&mut self) -> Option<RenderCommand> {
        let cmd = self.buffer.pop_front()?;
        self.stats.total_dequeued += 1;
        self.stats.current_depth = self.buffer.len();
        Some(cmd)
    }

    /// Peek at next frame without removing.
    pub fn peek(&self) -> Option<&RenderCommand> {
        self.buffer.front()
    }

    /// Clear expired frames.
    ///
    /// Returns number of frames cleared.
    pub fn clear_expired(&mut self, current_time_ns: u64) -> usize {
        let before = self.buffer.len();

        self.buffer.retain(|cmd| !cmd.is_expired(current_time_ns));

        let cleared = before - self.buffer.len();
        self.stats.dropped_expired += cleared as u64;
        self.stats.current_depth = self.buffer.len();

        cleared
    }

    /// Clear all frames.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.stats.current_depth = 0;
    }

    /// Get queue statistics.
    pub fn stats(&self) -> &QueueStats {
        &self.stats
    }

    /// Get frame by id (for debugging).
    pub fn find_frame(&self, id: FrameId) -> Option<&RenderCommand> {
        self.buffer.iter().find(|cmd| cmd.frame_id == id)
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new(FrameQueueConfig::default())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::media_time::MediaTime;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_cmd(frame: u64) -> RenderCommand {
        RenderCommand::new(FrameId(frame), ms(frame as i64 * 16), vec![])
    }

    #[test]
    fn test_queue_new() {
        let queue = FrameQueue::default();

        assert!(queue.is_empty());
        assert!(!queue.is_ready());
        assert!(queue.needs_frames());
    }

    #[test]
    fn test_push_pop() {
        let mut queue = FrameQueue::default();

        queue.push(make_cmd(1));
        queue.push(make_cmd(2));

        assert_eq!(queue.len(), 2);

        let cmd = queue.pop().unwrap();
        assert_eq!(cmd.frame_id, FrameId(1));

        let cmd = queue.pop().unwrap();
        assert_eq!(cmd.frame_id, FrameId(2));

        assert!(queue.is_empty());
    }

    #[test]
    fn test_queue_never_overflows() {
        let config = FrameQueueConfig {
            max_size: 5,
            target_size: 3,
            min_start_size: 2,
        };
        let mut queue = FrameQueue::new(config);

        // Push more than max_size
        for i in 0..100 {
            queue.push(make_cmd(i));

            // Queue should never exceed max_size
            assert!(queue.len() <= 5, "Queue overflowed: {}", queue.len());
        }

        assert_eq!(queue.stats().dropped_overflow, 95); // 100 - 5
    }

    #[test]
    fn test_is_ready() {
        let config = FrameQueueConfig {
            max_size: 10,
            target_size: 3,
            min_start_size: 2,
        };
        let mut queue = FrameQueue::new(config);

        assert!(!queue.is_ready());

        queue.push(make_cmd(1));
        assert!(!queue.is_ready());

        queue.push(make_cmd(2));
        assert!(queue.is_ready());
    }

    #[test]
    fn test_priority_push() {
        use super::super::render_command::RenderPriority;

        let mut queue = FrameQueue::default();

        queue.push(make_cmd(1));
        queue.push(make_cmd(2));

        // Push high priority - should go to front
        let mut seek_cmd = RenderCommand::seek(FrameId(100), ms(1000), vec![]);
        queue.push_priority(seek_cmd);

        // High priority should be first
        let first = queue.pop().unwrap();
        assert_eq!(first.frame_id, FrameId(100));
        assert_eq!(first.priority, RenderPriority::High);
    }

    #[test]
    fn test_clear_expired() {
        let mut queue = FrameQueue::default();

        let mut cmd1 = make_cmd(1).with_deadline(1_000_000);
        let mut cmd2 = make_cmd(2).with_deadline(2_000_000);
        let cmd3 = make_cmd(3); // No deadline

        queue.push(cmd1);
        queue.push(cmd2);
        queue.push(cmd3);

        // Clear frames with deadline < 1.5ms
        let cleared = queue.clear_expired(1_500_000);

        assert_eq!(cleared, 1);
        assert_eq!(queue.len(), 2);
    }
}
