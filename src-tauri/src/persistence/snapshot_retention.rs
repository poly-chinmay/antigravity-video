// src-tauri/src/persistence/snapshot_retention.rs
//! Snapshot Retention Policy - Tiered cleanup with checkpoint protection

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Maximum total snapshot storage size in bytes (200 MB)
pub const MAX_SNAPSHOT_SIZE_BYTES: u64 = 200 * 1024 * 1024;

/// Retention policy configuration
pub struct RetentionPolicy {
    /// Keep last N recent snapshots
    pub recent_count: usize,
    /// Keep hourly snapshots for this duration
    pub hourly_duration: Duration,
    /// Keep daily snapshots for this duration
    pub daily_duration: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            recent_count: 20,
            hourly_duration: Duration::from_secs(24 * 3600), // 24 hours
            daily_duration: Duration::from_secs(30 * 24 * 3600), // 30 days
        }
    }
}

/// Snapshot metadata for retention decisions
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub version: u64,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub created_at: SystemTime,
    pub is_checkpoint: bool,
}

/// Apply retention policy to snapshots
pub fn apply_retention_policy(snapshots: &[SnapshotInfo], policy: &RetentionPolicy) -> Vec<u64> {
    let mut to_keep: Vec<u64> = Vec::new();
    let now = SystemTime::now();

    // Sort by version descending (newest first)
    let mut sorted: Vec<_> = snapshots.iter().collect();
    sorted.sort_by(|a, b| b.version.cmp(&a.version));

    // Tier 1: Keep recent snapshots
    for snapshot in sorted.iter().take(policy.recent_count) {
        to_keep.push(snapshot.version);
    }

    // Tier 2: Keep hourly snapshots within duration
    let mut last_hour: Option<u64> = None;
    for snapshot in &sorted {
        if let Ok(age) = now.duration_since(snapshot.created_at) {
            if age <= policy.hourly_duration {
                let hour = age.as_secs() / 3600;
                if last_hour != Some(hour) {
                    to_keep.push(snapshot.version);
                    last_hour = Some(hour);
                }
            }
        }
    }

    // Tier 3: Keep daily snapshots within duration
    let mut last_day: Option<u64> = None;
    for snapshot in &sorted {
        if let Ok(age) = now.duration_since(snapshot.created_at) {
            if age <= policy.daily_duration {
                let day = age.as_secs() / (24 * 3600);
                if last_day != Some(day) {
                    to_keep.push(snapshot.version);
                    last_day = Some(day);
                }
            }
        }
    }

    // Tier 4: Always keep user checkpoints
    for snapshot in snapshots {
        if snapshot.is_checkpoint {
            to_keep.push(snapshot.version);
        }
    }

    // Deduplicate
    to_keep.sort();
    to_keep.dedup();

    to_keep
}

/// Enforce disk size limit by pruning oldest snapshots
pub fn enforce_size_limit(snapshots: &mut Vec<SnapshotInfo>, max_bytes: u64) -> Vec<PathBuf> {
    let mut to_delete: Vec<PathBuf> = Vec::new();

    // Sort by version ascending (oldest first)
    snapshots.sort_by(|a, b| a.version.cmp(&b.version));

    // Calculate total size
    let mut total_size: u64 = snapshots.iter().map(|s| s.size_bytes).sum();

    // Remove oldest non-checkpoint snapshots until under limit
    while total_size > max_bytes && !snapshots.is_empty() {
        // Find oldest non-checkpoint
        if let Some(idx) = snapshots.iter().position(|s| !s.is_checkpoint) {
            let removed = snapshots.remove(idx);
            total_size -= removed.size_bytes;
            to_delete.push(removed.path);
        } else {
            // Only checkpoints remain, stop
            break;
        }
    }

    to_delete
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_snapshot(version: u64, age_secs: u64, is_checkpoint: bool) -> SnapshotInfo {
        SnapshotInfo {
            version,
            path: PathBuf::from(format!("snapshot_{:08}.zst", version)),
            size_bytes: 1024 * 1024, // 1 MB each
            created_at: SystemTime::now() - Duration::from_secs(age_secs),
            is_checkpoint,
        }
    }

    #[test]
    fn test_retention_keeps_recent() {
        let policy = RetentionPolicy {
            recent_count: 5,
            hourly_duration: Duration::from_secs(0),
            daily_duration: Duration::from_secs(0),
        };

        let snapshots: Vec<_> = (1..=10)
            .map(|v| make_snapshot(v, v * 3600, false))
            .collect();

        let kept = apply_retention_policy(&snapshots, &policy);

        // Should keep versions 6-10 (most recent 5)
        assert!(kept.contains(&10));
        assert!(kept.contains(&6));
        assert!(!kept.contains(&5));
    }

    #[test]
    fn test_retention_protects_checkpoints() {
        let policy = RetentionPolicy {
            recent_count: 2,
            hourly_duration: Duration::from_secs(0),
            daily_duration: Duration::from_secs(0),
        };

        let mut snapshots: Vec<_> = (1..=5).map(|v| make_snapshot(v, v * 3600, false)).collect();

        // Make version 1 a checkpoint
        snapshots[0].is_checkpoint = true;

        let kept = apply_retention_policy(&snapshots, &policy);

        // Should keep checkpoint version 1 even though it's oldest
        assert!(kept.contains(&1), "Checkpoint should be protected");
    }

    #[test]
    fn test_size_limit_enforcement() {
        let mut snapshots: Vec<_> = (1..=10)
            .map(|v| make_snapshot(v, v * 3600, false))
            .collect();

        // 10 snapshots * 1MB each = 10MB
        // Limit to 5MB
        let deleted = enforce_size_limit(&mut snapshots, 5 * 1024 * 1024);

        // Should delete oldest 5 (versions 1-5)
        assert_eq!(deleted.len(), 5);
        assert_eq!(snapshots.len(), 5);
    }
}
