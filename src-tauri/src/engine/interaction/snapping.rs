//! Snapping - Snap-to-grid and snap-to-clip logic.
//!
//! # Design
//!
//! Snapping helps users align clips precisely:
//! - Snap to grid (beat, seconds, frames)
//! - Snap to other clip edges
//! - Snap to playhead
//!
//! # Priority
//!
//! 1. Clip edges (highest)
//! 2. Playhead
//! 3. Grid (lowest)

use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::{Clip, ClipId};

// =============================================================================
// SNAP TARGET
// =============================================================================

/// What we snapped to.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapTarget {
    /// No snap
    None,
    /// Snapped to grid line
    Grid(MediaTime),
    /// Snapped to clip start
    ClipStart(ClipId, MediaTime),
    /// Snapped to clip end
    ClipEnd(ClipId, MediaTime),
    /// Snapped to playhead
    Playhead(MediaTime),
}

// =============================================================================
// SNAP RESULT
// =============================================================================

/// Result of a snap operation.
#[derive(Debug, Clone)]
pub struct SnapResult {
    /// Final snapped position
    pub position: MediaTime,
    /// What we snapped to
    pub target: SnapTarget,
    /// Distance snapped (for feedback)
    pub snap_distance_ns: i64,
}

impl SnapResult {
    /// Create unsnapped result.
    pub fn unsnapped(position: MediaTime) -> Self {
        Self {
            position,
            target: SnapTarget::None,
            snap_distance_ns: 0,
        }
    }

    /// Create snapped result.
    pub fn snapped(position: MediaTime, target: SnapTarget, distance_ns: i64) -> Self {
        Self {
            position,
            target,
            snap_distance_ns: distance_ns,
        }
    }

    /// Check if actually snapped.
    pub fn did_snap(&self) -> bool {
        self.target != SnapTarget::None
    }
}

// =============================================================================
// SNAP CONFIG
// =============================================================================

/// Configuration for snapping.
#[derive(Debug, Clone)]
pub struct SnapConfig {
    /// Whether snapping is enabled
    pub enabled: bool,

    /// Snap threshold (nanos)
    pub threshold_ns: i64,

    /// Snap to grid
    pub snap_to_grid: bool,

    /// Grid interval (nanos)
    pub grid_interval_ns: i64,

    /// Snap to clips
    pub snap_to_clips: bool,

    /// Snap to playhead
    pub snap_to_playhead: bool,
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_ns: 100_000_000, // 100ms
            snap_to_grid: true,
            grid_interval_ns: 1_000_000_000, // 1 second
            snap_to_clips: true,
            snap_to_playhead: true,
        }
    }
}

impl SnapConfig {
    /// Create with frame-based grid.
    pub fn with_fps(fps: f64) -> Self {
        let frame_duration_ns = (1_000_000_000.0 / fps) as i64;
        Self {
            grid_interval_ns: frame_duration_ns,
            ..Default::default()
        }
    }
}

// =============================================================================
// SNAPPER
// =============================================================================

/// Snapping engine.
#[derive(Debug)]
pub struct Snapper {
    /// Configuration
    config: SnapConfig,
}

impl Snapper {
    /// Create a new snapper.
    pub fn new(config: SnapConfig) -> Self {
        Self { config }
    }

    /// Create with default config.
    pub fn default_snapper() -> Self {
        Self::new(SnapConfig::default())
    }

    /// Get config reference.
    pub fn config(&self) -> &SnapConfig {
        &self.config
    }

    /// Update config.
    pub fn set_config(&mut self, config: SnapConfig) {
        self.config = config;
    }

    /// Snap a position.
    pub fn snap(
        &self,
        position: MediaTime,
        clips: &[Clip],
        playhead: MediaTime,
        exclude_clip: Option<&ClipId>,
    ) -> SnapResult {
        if !self.config.enabled {
            return SnapResult::unsnapped(position);
        }

        let position_ns = position.as_nanos();
        let threshold = self.config.threshold_ns;

        let mut best_result = SnapResult::unsnapped(position);
        let mut best_distance = i64::MAX;

        // Check clip edges first (highest priority)
        if self.config.snap_to_clips {
            for clip in clips {
                // Skip excluded clip (the one being dragged)
                if let Some(exclude) = exclude_clip {
                    if &clip.id == exclude {
                        continue;
                    }
                }

                // Snap to clip start
                let start_ns = clip.start.as_nanos();
                let start_dist = (position_ns - start_ns).abs();
                if start_dist < threshold && start_dist < best_distance {
                    best_distance = start_dist;
                    best_result = SnapResult::snapped(
                        clip.start,
                        SnapTarget::ClipStart(clip.id.clone(), clip.start),
                        start_dist,
                    );
                }

                // Snap to clip end
                let end = clip.end();
                let end_ns = end.as_nanos();
                let end_dist = (position_ns - end_ns).abs();
                if end_dist < threshold && end_dist < best_distance {
                    best_distance = end_dist;
                    best_result = SnapResult::snapped(
                        end,
                        SnapTarget::ClipEnd(clip.id.clone(), end),
                        end_dist,
                    );
                }
            }
        }

        // Check playhead
        if self.config.snap_to_playhead {
            let playhead_ns = playhead.as_nanos();
            let playhead_dist = (position_ns - playhead_ns).abs();
            if playhead_dist < threshold && playhead_dist < best_distance {
                best_distance = playhead_dist;
                best_result =
                    SnapResult::snapped(playhead, SnapTarget::Playhead(playhead), playhead_dist);
            }
        }

        // Check grid (lowest priority)
        if self.config.snap_to_grid {
            let grid = self.config.grid_interval_ns;
            let grid_position = self.snap_to_grid(position_ns, grid);
            let grid_dist = (position_ns - grid_position).abs();
            if grid_dist < threshold && grid_dist < best_distance {
                let grid_time = MediaTime::from_nanos(grid_position);
                best_result =
                    SnapResult::snapped(grid_time, SnapTarget::Grid(grid_time), grid_dist);
            }
        }

        best_result
    }

    /// Snap to nearest grid line.
    fn snap_to_grid(&self, position_ns: i64, grid_ns: i64) -> i64 {
        if grid_ns <= 0 {
            return position_ns;
        }

        let lower = (position_ns / grid_ns) * grid_ns;
        let upper = lower + grid_ns;

        if (position_ns - lower).abs() <= (upper - position_ns).abs() {
            lower
        } else {
            upper
        }
    }

    /// Simple grid snap (no clip/playhead context).
    pub fn snap_to_grid_simple(&self, position: MediaTime) -> MediaTime {
        if !self.config.enabled || !self.config.snap_to_grid {
            return position;
        }

        let snapped = self.snap_to_grid(position.as_nanos(), self.config.grid_interval_ns);
        MediaTime::from_nanos(snapped)
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

    fn make_clip(id: &str, start_ms: i64, duration_ms: i64) -> Clip {
        Clip::new(id, "t1", ms(start_ms), ms(duration_ms), "test.mp4")
    }

    #[test]
    fn test_snap_to_grid() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,       // 200ms
            grid_interval_ns: 1_000_000_000, // 1 second
            snap_to_clips: false,
            snap_to_playhead: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        // Position at 1.1 seconds should snap to 1 second
        let result = snapper.snap(ms(1100), &[], MediaTime::ZERO, None);

        assert!(result.did_snap());
        assert_eq!(result.position, ms(1000));
    }

    #[test]
    fn test_snap_to_clip_start() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,
            snap_to_clips: true,
            snap_to_grid: false,
            snap_to_playhead: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        let clips = vec![make_clip("c1", 5000, 2000)];

        // Position near clip start (5000ms)
        let result = snapper.snap(ms(5100), &clips, MediaTime::ZERO, None);

        assert!(result.did_snap());
        assert_eq!(result.position, ms(5000));
        match result.target {
            SnapTarget::ClipStart(id, _) => assert_eq!(id, "c1"),
            _ => panic!("Wrong snap target"),
        }
    }

    #[test]
    fn test_snap_to_clip_end() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,
            snap_to_clips: true,
            snap_to_grid: false,
            snap_to_playhead: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        let clips = vec![make_clip("c1", 5000, 2000)]; // ends at 7000ms

        // Position near clip end (7000ms)
        let result = snapper.snap(ms(6900), &clips, MediaTime::ZERO, None);

        assert!(result.did_snap());
        assert_eq!(result.position, ms(7000));
        match result.target {
            SnapTarget::ClipEnd(id, _) => assert_eq!(id, "c1"),
            _ => panic!("Wrong snap target"),
        }
    }

    #[test]
    fn test_snap_to_playhead() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,
            snap_to_clips: false,
            snap_to_grid: false,
            snap_to_playhead: true,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        let playhead = ms(3000);

        // Position near playhead
        let result = snapper.snap(ms(3100), &[], playhead, None);

        assert!(result.did_snap());
        assert_eq!(result.position, ms(3000));
        assert!(matches!(result.target, SnapTarget::Playhead(_)));
    }

    #[test]
    fn test_clip_priority_over_grid() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,
            grid_interval_ns: 1_000_000_000,
            snap_to_clips: true,
            snap_to_grid: true,
            snap_to_playhead: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        // Clip starts at 1050ms (close to 1000ms grid)
        let clips = vec![make_clip("c1", 1050, 2000)];

        // Position at 1100ms - should snap to clip (1050) not grid (1000)
        let result = snapper.snap(ms(1100), &clips, MediaTime::ZERO, None);

        assert!(result.did_snap());
        assert_eq!(result.position, ms(1050));
        assert!(matches!(result.target, SnapTarget::ClipStart(_, _)));
    }

    #[test]
    fn test_exclude_clip() {
        let config = SnapConfig {
            enabled: true,
            threshold_ns: 200_000_000,
            snap_to_clips: true,
            snap_to_grid: false,
            snap_to_playhead: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        let clips = vec![make_clip("c1", 1000, 2000), make_clip("c2", 5000, 2000)];

        // Exclude c1 - should snap to c2 instead
        let result = snapper.snap(ms(1100), &clips, MediaTime::ZERO, Some(&"c1".to_string()));

        // c1 is excluded, so no snap
        assert!(!result.did_snap());
    }

    #[test]
    fn test_disabled_snapping() {
        let config = SnapConfig {
            enabled: false,
            ..Default::default()
        };
        let snapper = Snapper::new(config);

        let result = snapper.snap(ms(1100), &[], MediaTime::ZERO, None);

        assert!(!result.did_snap());
        assert_eq!(result.position, ms(1100));
    }
}
