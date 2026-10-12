//! Workspace Engine - Sole owner of mutable workspace state.
//!
//! # Architecture
//!
//! ```text
//! WorkspaceEngine
//!     │
//!     ├── state: RwLock<WorkspaceState>  ← sole owner
//!     │
//!     ├── apply_command(cmd) → Result    ← only mutation path
//!     │
//!     └── snapshot() → WorkspaceState    ← returns clone
//! ```
//!
//! # Invariants
//!
//! 1. WorkspaceEngine is the SOLE OWNER of mutable WorkspaceState
//! 2. ALL mutations go through apply_command() - NO EXCEPTIONS
//! 3. snapshot() returns a CLONE - caller cannot mutate engine state
//! 4. Engine is thread-safe via RwLock
//! 5. All operations are deterministic: same commands → same state
//! 6. No UI dependencies - engine is UI-agnostic

use std::sync::RwLock;

use super::workspace_command::WorkspaceCommand;
use super::workspace_error::{WorkspaceError, WorkspaceResult};
use super::workspace_types::{
    calculate_checksum, create_default_workspace, create_project, PanelId, PanelPosition,
    PanelState, ProjectId, WindowMode, WorkspaceState,
};

// =============================================================================
// WORKSPACE ENGINE
// =============================================================================

/// Workspace management engine. Sole owner of workspace state.
///
/// # Invariants
///
/// - Sole owner of mutable WorkspaceState
/// - All mutations via apply_command()
/// - snapshot() returns cloned state
/// - Thread-safe via RwLock
/// - Deterministic behavior
pub struct WorkspaceEngine {
    /// Protected workspace state
    state: RwLock<WorkspaceState>,
}

impl WorkspaceEngine {
    /// Create a new workspace engine with default state.
    pub fn new() -> Self {
        Self {
            state: RwLock::new(create_default_workspace()),
        }
    }

    /// Create engine with specific initial state.
    pub fn with_state(state: WorkspaceState) -> Self {
        Self {
            state: RwLock::new(state),
        }
    }

    /// Get a snapshot of current state. Returns a CLONE.
    ///
    /// # Invariant
    ///
    /// Caller receives an immutable snapshot.
    /// Caller CANNOT mutate engine state through the snapshot.
    pub fn snapshot(&self) -> WorkspaceState {
        self.state.read().unwrap().clone()
    }

    /// Apply a command to mutate state.
    ///
    /// # Invariant
    ///
    /// This is the ONLY way to mutate WorkspaceState.
    /// No other method may modify the state.
    pub fn apply_command(&self, cmd: WorkspaceCommand) -> WorkspaceResult<()> {
        let mut state = self.state.write().map_err(|_| WorkspaceError::LockFailed)?;

        self.execute_command(&mut state, cmd)?;

        // Update timestamp
        state.last_modified = now_millis();

        Ok(())
    }

    /// Apply multiple commands atomically.
    pub fn apply_commands(&self, commands: Vec<WorkspaceCommand>) -> WorkspaceResult<()> {
        let mut state = self.state.write().map_err(|_| WorkspaceError::LockFailed)?;

        for cmd in commands {
            self.execute_command(&mut state, cmd)?;
        }

        state.last_modified = now_millis();

        Ok(())
    }

    /// Execute a single command against state.
    fn execute_command(
        &self,
        state: &mut WorkspaceState,
        cmd: WorkspaceCommand,
    ) -> WorkspaceResult<()> {
        match cmd {
            // =================================================================
            // PROJECT COMMANDS
            // =================================================================
            WorkspaceCommand::CreateProject { name } => {
                let project = create_project(&name);
                let id = project.id.0.clone();
                state.projects.insert(id.clone(), project);

                // Auto-activate if first project
                if state.active_project.is_none() {
                    state.active_project = Some(ProjectId(id));
                }
            }

            WorkspaceCommand::OpenProject { id, name, path } => {
                if state.projects.contains_key(&id.0) {
                    return Err(WorkspaceError::ProjectAlreadyExists { id });
                }

                let mut project = create_project(&name);
                project.id = id.clone();
                project.path = path;
                state.projects.insert(id.0.clone(), project);
                state.active_project = Some(id);
            }

            WorkspaceCommand::CloseProject { id } => {
                if !state.projects.contains_key(&id.0) {
                    return Err(WorkspaceError::ProjectNotFound { id });
                }

                if state.projects.len() <= 1 {
                    return Err(WorkspaceError::CannotCloseLastProject);
                }

                state.projects.remove(&id.0);

                // Update active project if we closed it
                if state.active_project.as_ref() == Some(&id) {
                    state.active_project =
                        state.projects.keys().next().map(|k| ProjectId(k.clone()));
                }
            }

            WorkspaceCommand::SetActiveProject { id } => {
                if !state.projects.contains_key(&id.0) {
                    return Err(WorkspaceError::ProjectNotFound { id });
                }
                state.active_project = Some(id);
            }

            WorkspaceCommand::MarkProjectDirty { id, dirty } => {
                let project = state
                    .projects
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::ProjectNotFound { id })?;
                project.dirty = dirty;
                project.last_modified = now_millis();
            }

            WorkspaceCommand::RenameProject { id, name } => {
                let project = state
                    .projects
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::ProjectNotFound { id })?;
                project.name = name;
                project.last_modified = now_millis();
            }

            WorkspaceCommand::SetProjectPath { id, path } => {
                let project = state
                    .projects
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::ProjectNotFound { id })?;
                project.path = Some(path);
                project.dirty = false;
                project.last_modified = now_millis();
            }

            // =================================================================
            // PANEL COMMANDS
            // =================================================================
            WorkspaceCommand::ShowPanel { id } => {
                let panel = state
                    .panels
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::PanelNotFound { id })?;
                panel.visible = true;
            }

            WorkspaceCommand::HidePanel { id } => {
                if !state.panels.contains_key(&id.0) {
                    return Err(WorkspaceError::PanelNotFound { id });
                }

                let panel = state.panels.get_mut(&id.0).unwrap();
                panel.visible = false;

                // Clear focus if hiding focused panel
                if state.focused_panel.as_ref() == Some(&id) {
                    state.focused_panel = None;
                }
            }

            WorkspaceCommand::TogglePanel { id } => {
                if !state.panels.contains_key(&id.0) {
                    return Err(WorkspaceError::PanelNotFound { id });
                }

                let panel = state.panels.get_mut(&id.0).unwrap();
                panel.visible = !panel.visible;
                let is_hidden = !panel.visible;

                if is_hidden && state.focused_panel.as_ref() == Some(&id) {
                    state.focused_panel = None;
                }
            }

            WorkspaceCommand::MovePanel { id, position } => {
                let panel = state
                    .panels
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::PanelNotFound { id })?;
                panel.position = position;
            }

            WorkspaceCommand::ResizePanel { id, size } => {
                let panel = state
                    .panels
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::PanelNotFound { id })?;
                panel.size = size;
            }

            WorkspaceCommand::SetPanelCollapsed { id, collapsed } => {
                let panel = state
                    .panels
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::PanelNotFound { id })?;
                panel.collapsed = collapsed;
            }

            WorkspaceCommand::ReorderPanel { id, order } => {
                let panel = state
                    .panels
                    .get_mut(&id.0)
                    .ok_or(WorkspaceError::PanelNotFound { id })?;
                panel.order = order;
            }

            WorkspaceCommand::AddPanel {
                id,
                title,
                position,
            } => {
                if state.panels.contains_key(&id.0) {
                    return Err(WorkspaceError::PanelAlreadyExists { id });
                }

                let order = state.panels.len() as u32;
                let panel = PanelState {
                    id: id.clone(),
                    title,
                    visible: true,
                    position,
                    size: 250,
                    collapsed: false,
                    focused: false,
                    z_index: 0,
                    order,
                };
                state.panels.insert(id.0, panel);
            }

            WorkspaceCommand::RemovePanel { id } => {
                if !state.panels.contains_key(&id.0) {
                    return Err(WorkspaceError::PanelNotFound { id });
                }
                state.panels.remove(&id.0);

                if state.focused_panel.as_ref() == Some(&id) {
                    state.focused_panel = None;
                }
            }

            // =================================================================
            // FOCUS COMMANDS
            // =================================================================
            WorkspaceCommand::FocusPanel { id } => {
                if !state.panels.contains_key(&id.0) {
                    return Err(WorkspaceError::PanelNotFound { id: id.clone() });
                }

                // Unfocus previous
                if let Some(ref prev_id) = state.focused_panel {
                    if let Some(panel) = state.panels.get_mut(&prev_id.0) {
                        panel.focused = false;
                    }
                }

                // Focus new
                if let Some(panel) = state.panels.get_mut(&id.0) {
                    panel.focused = true;
                    panel.visible = true; // Auto-show when focusing
                }
                state.focused_panel = Some(id);
            }

            WorkspaceCommand::ClearFocus => {
                if let Some(ref id) = state.focused_panel {
                    if let Some(panel) = state.panels.get_mut(&id.0) {
                        panel.focused = false;
                    }
                }
                state.focused_panel = None;
            }

            WorkspaceCommand::FocusNext => {
                let visible_panels: Vec<_> = state.panels.values().filter(|p| p.visible).collect();

                if visible_panels.is_empty() {
                    return Ok(());
                }

                let current_idx = state
                    .focused_panel
                    .as_ref()
                    .and_then(|id| visible_panels.iter().position(|p| &p.id == id))
                    .unwrap_or(0);

                let next_idx = (current_idx + 1) % visible_panels.len();
                let next_id = visible_panels[next_idx].id.clone();

                // Apply focus
                if let Some(ref prev_id) = state.focused_panel {
                    if let Some(panel) = state.panels.get_mut(&prev_id.0) {
                        panel.focused = false;
                    }
                }
                if let Some(panel) = state.panels.get_mut(&next_id.0) {
                    panel.focused = true;
                }
                state.focused_panel = Some(next_id);
            }

            WorkspaceCommand::FocusPrevious => {
                let visible_panels: Vec<_> = state.panels.values().filter(|p| p.visible).collect();

                if visible_panels.is_empty() {
                    return Ok(());
                }

                let current_idx = state
                    .focused_panel
                    .as_ref()
                    .and_then(|id| visible_panels.iter().position(|p| &p.id == id))
                    .unwrap_or(0);

                let prev_idx = if current_idx == 0 {
                    visible_panels.len() - 1
                } else {
                    current_idx - 1
                };
                let prev_id = visible_panels[prev_idx].id.clone();

                // Apply focus
                if let Some(ref prev_focused) = state.focused_panel {
                    if let Some(panel) = state.panels.get_mut(&prev_focused.0) {
                        panel.focused = false;
                    }
                }
                if let Some(panel) = state.panels.get_mut(&prev_id.0) {
                    panel.focused = true;
                }
                state.focused_panel = Some(prev_id);
            }

            // =================================================================
            // WINDOW COMMANDS
            // =================================================================
            WorkspaceCommand::SetWindowSize { size } => {
                state.window.size = size;
            }

            WorkspaceCommand::SetWindowPosition { position } => {
                state.window.position = position;
            }

            WorkspaceCommand::SetWindowMode { mode } => {
                state.window.mode = mode;
            }

            WorkspaceCommand::ToggleMaximized => {
                state.window.mode = match state.window.mode {
                    WindowMode::Maximized => WindowMode::Normal,
                    _ => WindowMode::Maximized,
                };
            }

            WorkspaceCommand::ToggleFullscreen => {
                state.window.mode = match state.window.mode {
                    WindowMode::Fullscreen => WindowMode::Normal,
                    _ => WindowMode::Fullscreen,
                };
            }

            WorkspaceCommand::SetAlwaysOnTop { enabled } => {
                state.window.always_on_top = enabled;
            }

            // =================================================================
            // WORKSPACE COMMANDS
            // =================================================================
            WorkspaceCommand::RenameWorkspace { name } => {
                state.name = name;
            }

            WorkspaceCommand::ResetToDefaults => {
                let new_state = create_default_workspace();
                state.panels = new_state.panels;
                state.window = new_state.window;
                state.focused_panel = None;
            }

            WorkspaceCommand::Touch => {
                // Just update timestamp (handled after match)
            }
        }

        Ok(())
    }

    /// Get checksum of current state.
    pub fn checksum(&self) -> String {
        let state = self.state.read().unwrap();
        calculate_checksum(&state)
    }

    /// Check if state is dirty (modified since last save).
    pub fn is_modified_since(&self, timestamp: u64) -> bool {
        let state = self.state.read().unwrap();
        state.last_modified > timestamp
    }
}

impl Default for WorkspaceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WorkspaceEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceEngine")
            .field("state", &"<locked>")
            .finish()
    }
}

// =============================================================================
// HELPER
// =============================================================================

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::super::workspace_types::WindowSize;
    use super::*;

    #[test]
    fn test_default_workspace_creation() {
        let engine = WorkspaceEngine::new();
        let state = engine.snapshot();

        assert!(!state.panels.is_empty());
        assert!(state.panels.contains_key("panel.timeline"));
        assert!(state.panels.contains_key("panel.preview"));
        assert!(state.projects.is_empty());
    }

    #[test]
    fn test_opening_and_closing_projects() {
        let engine = WorkspaceEngine::new();

        // Create project
        engine
            .apply_command(WorkspaceCommand::CreateProject {
                name: "Project 1".to_string(),
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.projects.len(), 1);
        assert!(state.active_project.is_some());

        // Create second project
        engine
            .apply_command(WorkspaceCommand::CreateProject {
                name: "Project 2".to_string(),
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.projects.len(), 2);

        // Close first project
        let first_id = state.projects.values().next().unwrap().id.clone();
        engine
            .apply_command(WorkspaceCommand::CloseProject { id: first_id })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.projects.len(), 1);
    }

    #[test]
    fn test_switching_active_project() {
        let engine = WorkspaceEngine::new();

        // Create two projects
        engine
            .apply_command(WorkspaceCommand::CreateProject {
                name: "Project A".to_string(),
            })
            .unwrap();
        engine
            .apply_command(WorkspaceCommand::CreateProject {
                name: "Project B".to_string(),
            })
            .unwrap();

        let state = engine.snapshot();
        let proj_b = state
            .projects
            .values()
            .find(|p| p.name == "Project B")
            .unwrap();

        // Switch to Project B
        engine
            .apply_command(WorkspaceCommand::SetActiveProject {
                id: proj_b.id.clone(),
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.active_project, Some(proj_b.id.clone()));
    }

    #[test]
    fn test_panel_visibility_and_movement() {
        let engine = WorkspaceEngine::new();

        // Hide timeline
        engine
            .apply_command(WorkspaceCommand::HidePanel {
                id: PanelId("panel.timeline".to_string()),
            })
            .unwrap();

        let state = engine.snapshot();
        assert!(!state.panels.get("panel.timeline").unwrap().visible);

        // Show it again
        engine
            .apply_command(WorkspaceCommand::ShowPanel {
                id: PanelId("panel.timeline".to_string()),
            })
            .unwrap();

        let state = engine.snapshot();
        assert!(state.panels.get("panel.timeline").unwrap().visible);

        // Move to right
        engine
            .apply_command(WorkspaceCommand::MovePanel {
                id: PanelId("panel.timeline".to_string()),
                position: PanelPosition::Right,
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(
            state.panels.get("panel.timeline").unwrap().position,
            PanelPosition::Right
        );
    }

    #[test]
    fn test_focus_management() {
        let engine = WorkspaceEngine::new();

        // Focus timeline
        engine
            .apply_command(WorkspaceCommand::FocusPanel {
                id: PanelId("panel.timeline".to_string()),
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(
            state.focused_panel,
            Some(PanelId("panel.timeline".to_string()))
        );
        assert!(state.panels.get("panel.timeline").unwrap().focused);

        // Focus preview
        engine
            .apply_command(WorkspaceCommand::FocusPanel {
                id: PanelId("panel.preview".to_string()),
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(
            state.focused_panel,
            Some(PanelId("panel.preview".to_string()))
        );
        assert!(!state.panels.get("panel.timeline").unwrap().focused);
        assert!(state.panels.get("panel.preview").unwrap().focused);

        // Clear focus
        engine.apply_command(WorkspaceCommand::ClearFocus).unwrap();

        let state = engine.snapshot();
        assert!(state.focused_panel.is_none());
    }

    #[test]
    fn test_window_state_transitions() {
        let engine = WorkspaceEngine::new();

        // Set window size
        engine
            .apply_command(WorkspaceCommand::SetWindowSize {
                size: WindowSize {
                    width: 1920,
                    height: 1080,
                },
            })
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.window.size.width, 1920);

        // Toggle maximized
        engine
            .apply_command(WorkspaceCommand::ToggleMaximized)
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.window.mode, WindowMode::Maximized);

        // Toggle again -> back to normal
        engine
            .apply_command(WorkspaceCommand::ToggleMaximized)
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.window.mode, WindowMode::Normal);

        // Fullscreen
        engine
            .apply_command(WorkspaceCommand::ToggleFullscreen)
            .unwrap();

        let state = engine.snapshot();
        assert_eq!(state.window.mode, WindowMode::Fullscreen);
    }

    #[test]
    fn test_deterministic_command_replay() {
        let commands = vec![
            WorkspaceCommand::CreateProject {
                name: "Test".to_string(),
            },
            WorkspaceCommand::ShowPanel {
                id: PanelId("panel.history".to_string()),
            },
            WorkspaceCommand::FocusPanel {
                id: PanelId("panel.timeline".to_string()),
            },
            WorkspaceCommand::SetWindowMode {
                mode: WindowMode::Maximized,
            },
        ];

        // Apply to engine 1
        let engine1 = WorkspaceEngine::new();
        for cmd in commands.clone() {
            engine1.apply_command(cmd).unwrap();
        }

        // Apply to engine 2
        let engine2 = WorkspaceEngine::new();
        for cmd in commands {
            engine2.apply_command(cmd).unwrap();
        }

        // States should be equivalent (minus timestamps)
        let state1 = engine1.snapshot();
        let state2 = engine2.snapshot();

        assert_eq!(state1.projects.len(), state2.projects.len());
        assert_eq!(state1.focused_panel, state2.focused_panel);
        assert_eq!(state1.window.mode, state2.window.mode);
        assert_eq!(engine1.checksum(), engine2.checksum());
    }

    #[test]
    fn test_snapshot_immutability() {
        let engine = WorkspaceEngine::new();

        // Get snapshot
        let snapshot1 = engine.snapshot();

        // Modify engine
        engine
            .apply_command(WorkspaceCommand::HidePanel {
                id: PanelId("panel.timeline".to_string()),
            })
            .unwrap();

        // Original snapshot unchanged
        assert!(snapshot1.panels.get("panel.timeline").unwrap().visible);

        // New snapshot reflects change
        let snapshot2 = engine.snapshot();
        assert!(!snapshot2.panels.get("panel.timeline").unwrap().visible);
    }

    #[test]
    fn test_error_on_invalid_panel() {
        let engine = WorkspaceEngine::new();

        let result = engine.apply_command(WorkspaceCommand::ShowPanel {
            id: PanelId("nonexistent".to_string()),
        });

        assert!(matches!(result, Err(WorkspaceError::PanelNotFound { .. })));
    }

    #[test]
    fn test_error_on_close_last_project() {
        let engine = WorkspaceEngine::new();

        // Create one project
        engine
            .apply_command(WorkspaceCommand::CreateProject {
                name: "Only Project".to_string(),
            })
            .unwrap();

        let state = engine.snapshot();
        let proj_id = state.projects.values().next().unwrap().id.clone();

        // Try to close it
        let result = engine.apply_command(WorkspaceCommand::CloseProject { id: proj_id });

        assert!(matches!(
            result,
            Err(WorkspaceError::CannotCloseLastProject)
        ));
    }

    #[test]
    fn test_atomic_commands() {
        let engine = WorkspaceEngine::new();

        let commands = vec![
            WorkspaceCommand::CreateProject {
                name: "Test".to_string(),
            },
            WorkspaceCommand::HidePanel {
                id: PanelId("panel.history".to_string()),
            },
        ];

        engine.apply_commands(commands).unwrap();

        let state = engine.snapshot();
        assert_eq!(state.projects.len(), 1);
    }
}
