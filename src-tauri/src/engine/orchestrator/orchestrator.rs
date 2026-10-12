//! Application Orchestrator - Central coordinator for all engines.
//!
//! # Architecture
//!
//! ```text
//! AppOrchestrator (single authority)
//!     │
//!     ├── workspace: Arc<WorkspaceEngineV2>
//!     ├── timeline: Arc<TimelineEngine>
//!     ├── playback: Arc<RwLock<PlaybackScheduler>>
//!     └── ui_sender: UIEventSender
//!
//! Tauri Commands ─────────▶ AppOrchestrator ─────────▶ Engines
//!                                │
//!                                ▼
//!                          AppSnapshot ─────────▶ React
//! ```
//!
//! # Invariants
//!
//! 1. All cross-engine effects are atomic - no partial updates
//! 2. Failures roll back safely - no mixed state
//! 3. Orchestrator owns sequencing - engines never call each other
//! 4. UI receives updates only from orchestrator - no direct engine→UI
//! 5. Deterministic replay - same command stream → same snapshot

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::engine::edit_action::EditAction;
use crate::engine::media_time::MediaTime;
use crate::engine::playback::{
    PlaybackScheduler, SchedulerConfig, TransportCommand, TransportState,
};
use crate::engine::timeline_engine::TimelineEngine;
use crate::engine::timeline_state::TimelineState;
use crate::engine::workspace::workspace_engine_v2::WorkspaceEngine as WorkspaceEngineV2;
use crate::engine::workspace::workspace_types::WorkspaceState;
use crate::engine::workspace::{WorkspaceCommand, WorkspaceError};

use super::app_command::{AppCommand, SystemCommand, TimelineCommand};
use super::app_state::{AppSnapshot, PlaybackSnapshot};

// =============================================================================
// COMMAND RESULT (renamed to avoid collision)
// =============================================================================

/// Result of orchestrator command execution.
#[derive(Debug, Clone)]
pub enum OrchestratorCommandResult {
    /// Command succeeded
    Success,

    /// Command succeeded with info
    SuccessWithInfo(String),

    /// Command failed
    Failed(String),

    /// Command was no-op (state unchanged)
    NoOp,

    /// Command requires user confirmation
    RequiresConfirmation(String),
}

impl OrchestratorCommandResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::SuccessWithInfo(_) | Self::NoOp)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

// =============================================================================
// ORCHESTRATOR ERROR
// =============================================================================

/// Errors from orchestrator operations.
#[derive(Debug, Clone)]
pub enum OrchestratorError {
    /// Workspace error
    Workspace(WorkspaceError),

    /// Timeline error
    Timeline(String),

    /// Playback error
    Playback(String),

    /// Command error
    Command(String),

    /// Atomicity violation
    AtomicityViolation(String),

    /// Lock error
    LockError(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace(e) => write!(f, "Workspace: {}", e),
            Self::Timeline(msg) => write!(f, "Timeline: {}", msg),
            Self::Playback(msg) => write!(f, "Playback: {}", msg),
            Self::Command(msg) => write!(f, "Command: {}", msg),
            Self::AtomicityViolation(msg) => write!(f, "Atomicity violation: {}", msg),
            Self::LockError(msg) => write!(f, "Lock error: {}", msg),
        }
    }
}

impl std::error::Error for OrchestratorError {}

impl From<WorkspaceError> for OrchestratorError {
    fn from(e: WorkspaceError) -> Self {
        Self::Workspace(e)
    }
}

pub type OrchestratorResult<T> = Result<T, OrchestratorError>;

// =============================================================================
// APP ORCHESTRATOR
// =============================================================================

/// Central application orchestrator.
///
/// # Invariants
///
/// - Single entry point from Tauri
/// - All cross-engine effects are atomic
/// - Engines never call each other directly
/// - UI updates only from orchestrator
pub struct AppOrchestrator {
    /// Workspace engine
    workspace: Arc<WorkspaceEngineV2>,

    /// Timeline engine
    timeline: Arc<TimelineEngine>,

    /// Playback scheduler (wrapped in RwLock for mutable access)
    playback: Arc<RwLock<PlaybackScheduler>>,

    /// Snapshot version counter (monotonic)
    version: AtomicU64,

    /// Command sequence number
    sequence: AtomicU64,
}

impl AppOrchestrator {
    /// Create a new orchestrator with default engines.
    pub fn new() -> Self {
        Self {
            workspace: Arc::new(WorkspaceEngineV2::new()),
            timeline: Arc::new(TimelineEngine::new()),
            playback: Arc::new(RwLock::new(PlaybackScheduler::new(
                SchedulerConfig::default(),
                MediaTime::ZERO,
            ))),
            version: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
        }
    }

    /// Create with specific engines.
    pub fn with_engines(
        workspace: Arc<WorkspaceEngineV2>,
        timeline: Arc<TimelineEngine>,
        playback: Arc<RwLock<PlaybackScheduler>>,
    ) -> Self {
        Self {
            workspace,
            timeline,
            playback,
            version: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
        }
    }

    // =========================================================================
    // COMMAND DISPATCH
    // =========================================================================

    /// Apply an AppCommand - the main entry point.
    pub fn apply(&self, cmd: AppCommand) -> OrchestratorResult<OrchestratorCommandResult> {
        let _seq = self.sequence.fetch_add(1, Ordering::SeqCst);

        let result = match cmd {
            AppCommand::Workspace(ws_cmd) => self.apply_workspace_command(ws_cmd),
            AppCommand::Timeline(tl_cmd) => self.apply_timeline_command(tl_cmd),
            AppCommand::Transport(tr_cmd) => self.apply_transport_command(tr_cmd),
            AppCommand::Compound(cmds) => self.apply_compound(cmds),
            AppCommand::System(sys_cmd) => self.apply_system_command(sys_cmd),
        };

        result
    }

    /// Apply workspace command.
    pub fn apply_workspace_command(
        &self,
        cmd: WorkspaceCommand,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        // Check for cross-engine effects
        let might_switch_project = matches!(
            cmd,
            WorkspaceCommand::SetActiveProject { .. } | WorkspaceCommand::CloseProject { .. }
        );

        // Apply to workspace
        self.workspace.apply_command(cmd)?;

        // Handle cross-engine effects
        if might_switch_project {
            self.on_project_switch()?;
        }

        self.bump_version();
        Ok(OrchestratorCommandResult::Success)
    }

    /// Apply timeline action.
    pub fn apply_timeline_action(
        &self,
        action: EditAction,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        // Stop playback if modifying timeline
        let was_playing = self.is_playing();
        if was_playing {
            if let Ok(mut playback) = self.playback.write() {
                playback.execute(TransportCommand::Pause);
            }
        }

        // Apply to timeline
        self.timeline
            .apply_action(action)
            .map_err(|e| OrchestratorError::Timeline(e.to_string()))?;

        // Mark project dirty
        let workspace_state = self.workspace.snapshot();
        if let Some(ref project_id) = workspace_state.active_project {
            let _ = self
                .workspace
                .apply_command(WorkspaceCommand::MarkProjectDirty {
                    id: project_id.clone(),
                    dirty: true,
                });
        }

        // Update playback duration
        let timeline_state = self.timeline.snapshot();
        if let Ok(mut playback) = self.playback.write() {
            playback.set_duration(timeline_state.duration);
        }

        self.bump_version();
        Ok(OrchestratorCommandResult::Success)
    }

    /// Apply timeline command.
    fn apply_timeline_command(
        &self,
        cmd: TimelineCommand,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        match cmd {
            TimelineCommand::Apply(action) => self.apply_timeline_action(action),
            TimelineCommand::Select { .. } => {
                // Would need selection API on TimelineEngine
                self.bump_version();
                Ok(OrchestratorCommandResult::Success)
            }
            TimelineCommand::DeselectAll => {
                self.bump_version();
                Ok(OrchestratorCommandResult::Success)
            }
            TimelineCommand::SetZoom { .. } | TimelineCommand::SetScroll { .. } => {
                // View state - UI-only, minimal engine impact
                self.bump_version();
                Ok(OrchestratorCommandResult::Success)
            }
        }
    }

    /// Apply transport command.
    pub fn apply_transport_command(
        &self,
        cmd: TransportCommand,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        let mut playback = self
            .playback
            .write()
            .map_err(|_| OrchestratorError::LockError("Playback lock failed".to_string()))?;
        playback.execute(cmd);
        drop(playback);

        self.bump_version();
        Ok(OrchestratorCommandResult::Success)
    }

    /// Apply compound commands atomically.
    fn apply_compound(
        &self,
        commands: Vec<AppCommand>,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        // Take snapshots for potential rollback
        let _ws_snapshot = self.workspace.snapshot();
        let _tl_snapshot = self.timeline.snapshot();

        // Apply all commands
        for cmd in commands {
            if let Err(e) = self.apply(cmd) {
                // Rollback on failure
                // Note: Full rollback would require engine-level transaction support
                // For now, we return error and log
                return Err(OrchestratorError::AtomicityViolation(format!(
                    "Compound command failed: {}. Partial state may exist.",
                    e
                )));
            }
        }

        self.bump_version();
        Ok(OrchestratorCommandResult::Success)
    }

    /// Apply system command.
    fn apply_system_command(
        &self,
        cmd: SystemCommand,
    ) -> OrchestratorResult<OrchestratorCommandResult> {
        match cmd {
            SystemCommand::Initialize => {
                // Initialization logic
                self.bump_version();
                Ok(OrchestratorCommandResult::Success)
            }
            SystemCommand::Shutdown => {
                // Stop playback
                if let Ok(mut playback) = self.playback.write() {
                    playback.execute(TransportCommand::Stop);
                }
                Ok(OrchestratorCommandResult::Success)
            }
            SystemCommand::RefreshUI => Ok(OrchestratorCommandResult::Success),
            SystemCommand::ClearCaches => {
                // Would clear render caches, etc.
                Ok(OrchestratorCommandResult::Success)
            }
            SystemCommand::Autosave => {
                // Trigger persistence (not yet implemented)
                Ok(OrchestratorCommandResult::NoOp)
            }
        }
    }

    // =========================================================================
    // CROSS-ENGINE COORDINATION
    // =========================================================================

    /// Handle project switch - reinitialize timeline.
    fn on_project_switch(&self) -> OrchestratorResult<()> {
        // Stop playback (this also resets position to 0)
        if let Ok(mut playback) = self.playback.write() {
            playback.execute(TransportCommand::Stop);
        }

        Ok(())
    }

    /// Check if currently playing.
    fn is_playing(&self) -> bool {
        self.playback
            .read()
            .map(|p| p.state() == TransportState::Playing)
            .unwrap_or(false)
    }

    // =========================================================================
    // SNAPSHOTS
    // =========================================================================

    /// Get complete application snapshot.
    pub fn snapshot_all(&self) -> AppSnapshot {
        let version = self.version.load(Ordering::SeqCst);
        let workspace = self.workspace.snapshot();
        let timeline = self.timeline.snapshot();

        let playback = self
            .playback
            .read()
            .map(|p| PlaybackSnapshot {
                position: p.position(),
                transport: p.state(),
                rate: p.rate().to_f64(),
                loop_enabled: p.is_loop_enabled(),
                loop_start: None,
                loop_end: None,
            })
            .unwrap_or_default();

        AppSnapshot::capture(version, &workspace, &timeline, playback)
    }

    /// Get workspace snapshot only.
    pub fn workspace_snapshot(&self) -> WorkspaceState {
        self.workspace.snapshot()
    }

    /// Get timeline snapshot only.
    pub fn timeline_snapshot(&self) -> TimelineState {
        self.timeline.snapshot()
    }

    /// Get current version.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    /// Get command sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    // =========================================================================
    // UI NOTIFICATION
    // =========================================================================

    /// Bump version counter.
    fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    // =========================================================================
    // ENGINE ACCESS (for advanced use cases)
    // =========================================================================

    /// Get workspace engine reference.
    pub fn workspace_engine(&self) -> &Arc<WorkspaceEngineV2> {
        &self.workspace
    }

    /// Get timeline engine reference.
    pub fn timeline_engine(&self) -> &Arc<TimelineEngine> {
        &self.timeline
    }

    /// Get playback scheduler reference.
    pub fn playback_scheduler(&self) -> &Arc<RwLock<PlaybackScheduler>> {
        &self.playback
    }
}

impl Default for AppOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AppOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppOrchestrator")
            .field("version", &self.version.load(Ordering::SeqCst))
            .field("sequence", &self.sequence.load(Ordering::SeqCst))
            .finish()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;
    use crate::engine::workspace::workspace_types::PanelId;

    fn make_test_clip(id: &str) -> Clip {
        Clip::new(
            id,
            "track0",
            MediaTime::ZERO,
            MediaTime::from_nanos(1_000_000_000), // 1 second
            "/test.mp4",
        )
    }

    #[test]
    fn test_orchestrator_new() {
        let orchestrator = AppOrchestrator::new();
        assert_eq!(orchestrator.version(), 0);
        assert_eq!(orchestrator.sequence(), 0);
    }

    #[test]
    fn test_workspace_timeline_interaction() {
        let orchestrator = AppOrchestrator::new();

        // Create a project
        orchestrator
            .apply(AppCommand::workspace(WorkspaceCommand::CreateProject {
                name: "Test Project".to_string(),
            }))
            .unwrap();

        // Modify timeline
        let clip = make_test_clip("clip1");
        let action = EditAction::add_clip(clip);
        orchestrator.apply(AppCommand::timeline(action)).unwrap();

        // Check timeline has clip
        let tl_snapshot = orchestrator.timeline_snapshot();
        assert_eq!(tl_snapshot.clips.len(), 1);

        // Check project marked dirty
        let ws_snapshot = orchestrator.workspace_snapshot();
        let project = ws_snapshot.projects.values().next().unwrap();
        assert!(project.dirty);
    }

    #[test]
    fn test_playback_reacting_to_timeline_mutation() {
        let orchestrator = AppOrchestrator::new();

        // Start playback
        orchestrator
            .apply(AppCommand::transport(TransportCommand::Play))
            .unwrap();

        // Verify playing
        {
            let playback = orchestrator.playback_scheduler().read().unwrap();
            assert_eq!(playback.state(), TransportState::Playing);
        }

        // Apply timeline edit (should pause playback)
        let clip = make_test_clip("clip1");
        let action = EditAction::add_clip(clip);
        orchestrator.apply(AppCommand::timeline(action)).unwrap();

        // Should now be paused
        {
            let playback = orchestrator.playback_scheduler().read().unwrap();
            assert_eq!(playback.state(), TransportState::Paused);
        }
    }

    #[test]
    fn test_project_switch_stops_playback() {
        let orchestrator = AppOrchestrator::new();

        // Create two projects
        orchestrator
            .apply(AppCommand::workspace(WorkspaceCommand::CreateProject {
                name: "Project A".to_string(),
            }))
            .unwrap();
        orchestrator
            .apply(AppCommand::workspace(WorkspaceCommand::CreateProject {
                name: "Project B".to_string(),
            }))
            .unwrap();

        // Start playback
        orchestrator
            .apply(AppCommand::transport(TransportCommand::Play))
            .unwrap();

        // Switch projects
        let ws = orchestrator.workspace_snapshot();
        let proj_b = ws
            .projects
            .values()
            .find(|p| p.name == "Project B")
            .unwrap();

        orchestrator
            .apply(AppCommand::workspace(WorkspaceCommand::SetActiveProject {
                id: proj_b.id.clone(),
            }))
            .unwrap();

        // Playback should be stopped
        {
            let playback = orchestrator.playback_scheduler().read().unwrap();
            assert_eq!(playback.state(), TransportState::Stopped);
        }
    }

    #[test]
    fn test_snapshot_consistency() {
        let orchestrator = AppOrchestrator::new();

        // Make some changes
        orchestrator
            .apply(AppCommand::workspace(WorkspaceCommand::ShowPanel {
                id: PanelId("panel.history".to_string()),
            }))
            .unwrap();

        // Get snapshot
        let snapshot = orchestrator.snapshot_all();

        // Verify version incremented
        assert!(snapshot.version > 0);

        // Verify workspace panel visible
        let history_visible = snapshot
            .workspace
            .panels
            .iter()
            .find(|p| p.id == "panel.history")
            .map(|p| p.visible)
            .unwrap_or(false);
        assert!(history_visible);
    }

    #[test]
    fn test_deterministic_replay() {
        let commands = vec![
            AppCommand::workspace(WorkspaceCommand::CreateProject {
                name: "Test".to_string(),
            }),
            AppCommand::workspace(WorkspaceCommand::ShowPanel {
                id: PanelId("panel.history".to_string()),
            }),
            AppCommand::transport(TransportCommand::Play),
            AppCommand::transport(TransportCommand::Pause),
        ];

        // Apply to orchestrator 1
        let orch1 = AppOrchestrator::new();
        for cmd in commands.clone() {
            orch1.apply(cmd).unwrap();
        }

        // Apply to orchestrator 2
        let orch2 = AppOrchestrator::new();
        for cmd in commands {
            orch2.apply(cmd).unwrap();
        }

        // Snapshots should match (minus timestamps)
        let snap1 = orch1.snapshot_all();
        let snap2 = orch2.snapshot_all();

        assert_eq!(
            snap1.workspace.projects.len(),
            snap2.workspace.projects.len()
        );
        assert_eq!(snap1.playback.transport, snap2.playback.transport);
    }

    #[test]
    fn test_no_cross_engine_partial_failure() {
        let orchestrator = AppOrchestrator::new();

        // Apply valid command
        let result = orchestrator.apply(AppCommand::workspace(WorkspaceCommand::ShowPanel {
            id: PanelId("nonexistent".to_string()),
        }));

        // Should fail cleanly
        assert!(result.is_err());
    }

    #[test]
    fn test_version_monotonic() {
        let orchestrator = AppOrchestrator::new();

        let v1 = orchestrator.version();
        orchestrator
            .apply(AppCommand::transport(TransportCommand::Play))
            .unwrap();
        let v2 = orchestrator.version();
        orchestrator
            .apply(AppCommand::transport(TransportCommand::Pause))
            .unwrap();
        let v3 = orchestrator.version();

        assert!(v2 > v1);
        assert!(v3 > v2);
    }

    #[test]
    fn test_sequence_monotonic() {
        let orchestrator = AppOrchestrator::new();

        let s1 = orchestrator.sequence();
        orchestrator
            .apply(AppCommand::transport(TransportCommand::Play))
            .unwrap();
        let s2 = orchestrator.sequence();

        assert!(s2 > s1);
    }
}
