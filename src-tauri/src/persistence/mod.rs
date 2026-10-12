// src-tauri/src/persistence/mod.rs
//! Persistence module for Antigravity Video
//!
//! Provides event sourcing, write-ahead logging, snapshotting, and hybrid recovery.

pub mod crash_recovery;
pub mod event_replay;
pub mod event_store;
pub mod hybrid_loader;
pub mod project_persistence;
pub mod snapshot_manager;
pub mod snapshot_retention;
pub mod snapshot_store;
pub mod wal;

pub use crash_recovery::{recover_from_crash, RecoveryResult};
pub use event_replay::replay_event;
pub use event_store::{Event, EventStore};
pub use hybrid_loader::load_project;
pub use project_persistence::ProjectPersistence;
pub use snapshot_manager::SnapshotManager;
pub use snapshot_retention::{apply_retention_policy, RetentionPolicy, SnapshotInfo};
pub use snapshot_store::{SnapshotStore, SNAPSHOT_INTERVAL};
pub use wal::{WALEntry, WriteAheadLog};
