//! CommandRouter - Routes commands to handlers and applies effects.
//!
//! # Design
//!
//! The router:
//! 1. Receives keyboard input or command ID
//! 2. Looks up command in keymap/registry
//! 3. Executes command with context
//! 4. Routes result to appropriate subsystem
//!
//! # Invariants
//!
//! - Commands never mutate state directly
//! - Router applies effects through proper channels
//! - Failed commands produce no mutations

use crate::engine::edit_action::EditAction;
use crate::engine::interaction::{InteractionController, ToolType};
use crate::engine::media_time::MediaTime;
use crate::engine::playback::PlaybackScheduler;
use crate::engine::timeline_state::{ClipId, TimelineState};

use super::command::{commands, CommandId, CommandResult};
use super::command_registry::CommandRegistry;
use super::keymap::{KeyBinding, Keymap};

// =============================================================================
// ROUTER CONFIG
// =============================================================================

/// Configuration for command router.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Frame step size (nanoseconds)
    pub frame_step_ns: i64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            // Default: 1 frame at 30fps
            frame_step_ns: 33_333_333,
        }
    }
}

// =============================================================================
// COMMAND SNAPSHOT
// =============================================================================

/// Snapshot of state needed for command execution.
/// Captures values to avoid borrow conflicts.
#[derive(Debug, Clone)]
pub struct CommandSnapshot {
    /// Current playhead position
    pub playhead_position: MediaTime,
    /// Timeline duration
    pub timeline_duration: MediaTime,
    /// Selected clips
    pub selected_clips: Vec<ClipId>,
    /// Current tool
    pub current_tool: ToolType,
    /// Is playing
    pub is_playing: bool,
}

impl CommandSnapshot {
    /// Create snapshot from components.
    pub fn capture(
        timeline: &TimelineState,
        playback: &PlaybackScheduler,
        interaction: &InteractionController,
    ) -> Self {
        Self {
            playhead_position: playback.position(),
            timeline_duration: timeline.duration,
            selected_clips: interaction.selected_clips().to_vec(),
            current_tool: interaction.current_tool(),
            is_playing: playback.is_playing(),
        }
    }

    /// Check if has selection.
    pub fn has_selection(&self) -> bool {
        !self.selected_clips.is_empty()
    }

    /// Get first selected clip.
    pub fn first_selected(&self) -> Option<&ClipId> {
        self.selected_clips.first()
    }
}

// =============================================================================
// ROUTER RESULT
// =============================================================================

/// Result of routing a command.
#[derive(Debug)]
pub enum RouterResult {
    /// Command executed successfully
    Success,

    /// Command produced an edit action
    EditAction(EditAction),

    /// Command not found
    NotFound(CommandId),

    /// Command failed
    Failed(String),

    /// No binding for key
    NoBinding,
}

impl RouterResult {
    /// Check if successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::EditAction(_))
    }
}

// =============================================================================
// COMMAND ROUTER
// =============================================================================

/// Routes commands to handlers and applies effects.
#[derive(Debug)]
pub struct CommandRouter {
    /// Command registry
    registry: CommandRegistry,

    /// Keymap for keyboard shortcuts
    keymap: Keymap,

    /// Configuration
    config: RouterConfig,
}

impl CommandRouter {
    /// Create a new router.
    pub fn new(registry: CommandRegistry, keymap: Keymap) -> Self {
        Self {
            registry,
            keymap,
            config: RouterConfig::default(),
        }
    }

    /// Create router with defaults.
    pub fn with_defaults() -> Self {
        Self::new(CommandRegistry::with_defaults(), Keymap::with_defaults())
    }

    /// Get registry.
    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    /// Get keymap.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Get mutable keymap.
    pub fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.keymap
    }

    // =========================================================================
    // DISPATCH
    // =========================================================================

    /// Dispatch a key binding.
    pub fn dispatch_key(
        &self,
        key: &KeyBinding,
        snapshot: &CommandSnapshot,
        interaction: &mut InteractionController,
        playback: &mut PlaybackScheduler,
    ) -> RouterResult {
        match self.keymap.get_command(key) {
            Some(cmd_id) => self.dispatch_command(cmd_id, snapshot, interaction, playback),
            None => RouterResult::NoBinding,
        }
    }

    /// Dispatch a command by ID.
    pub fn dispatch_command(
        &self,
        cmd_id: &CommandId,
        snapshot: &CommandSnapshot,
        interaction: &mut InteractionController,
        playback: &mut PlaybackScheduler,
    ) -> RouterResult {
        // Check command exists
        if !self.registry.exists(cmd_id) {
            return RouterResult::NotFound(cmd_id.clone());
        }

        // Execute command
        let result = self.execute_command(cmd_id, snapshot, interaction, playback);

        // Route result
        self.route_result(result, interaction, playback)
    }

    /// Execute a command and get result.
    fn execute_command(
        &self,
        cmd_id: &CommandId,
        snapshot: &CommandSnapshot,
        interaction: &mut InteractionController,
        playback: &mut PlaybackScheduler,
    ) -> CommandResult {
        match cmd_id.0.as_str() {
            // Tool commands
            "tool.select" => CommandResult::ToolChanged(ToolType::Select),
            "tool.move" => CommandResult::ToolChanged(ToolType::Move),
            "tool.trim" => CommandResult::ToolChanged(ToolType::TrimStart),
            "tool.razor" => CommandResult::ToolChanged(ToolType::Razor),

            // Transport commands
            "transport.play_pause" => {
                if playback.is_playing() {
                    playback.pause();
                } else {
                    playback.play();
                }
                CommandResult::PlaybackChanged
            }
            "transport.play" => {
                playback.play();
                CommandResult::PlaybackChanged
            }
            "transport.stop" => {
                playback.stop();
                CommandResult::PlaybackChanged
            }
            "transport.pause" => {
                playback.pause();
                CommandResult::PlaybackChanged
            }
            "transport.seek_start" => {
                playback.seek(MediaTime::ZERO);
                CommandResult::PlaybackChanged
            }
            "transport.seek_end" => {
                playback.seek(snapshot.timeline_duration);
                CommandResult::PlaybackChanged
            }
            "transport.step_forward" => {
                let new_pos =
                    snapshot.playhead_position + MediaTime::from_nanos(self.config.frame_step_ns);
                playback.seek(new_pos.min(snapshot.timeline_duration));
                CommandResult::PlaybackChanged
            }
            "transport.step_backward" => {
                let ns = self.config.frame_step_ns;
                let new_pos = if snapshot.playhead_position.as_nanos() > ns {
                    snapshot.playhead_position - MediaTime::from_nanos(ns)
                } else {
                    MediaTime::ZERO
                };
                playback.seek(new_pos);
                CommandResult::PlaybackChanged
            }

            // Edit commands that produce EditActions
            "edit.delete" => {
                if snapshot.has_selection() {
                    if let Some(clip_id) = snapshot.first_selected() {
                        CommandResult::EditAction(EditAction::delete_clip(clip_id.clone()))
                    } else {
                        CommandResult::NotApplicable("No clip selected".to_string())
                    }
                } else {
                    CommandResult::NotApplicable("No selection".to_string())
                }
            }

            // Selection commands
            "edit.select_all" => {
                CommandResult::NotApplicable("Select all not yet implemented".to_string())
            }
            "edit.deselect" => {
                interaction.state_mut().clear_selection();
                CommandResult::Success
            }

            // History commands
            "edit.undo" => CommandResult::NotApplicable("Undo handled by UndoManager".to_string()),
            "edit.redo" => CommandResult::NotApplicable("Redo handled by UndoManager".to_string()),

            // Cut/copy/paste
            "edit.cut" => CommandResult::NotApplicable("Cut not yet implemented".to_string()),
            "edit.copy" => CommandResult::NotApplicable("Copy not yet implemented".to_string()),
            "edit.paste" => CommandResult::NotApplicable("Paste not yet implemented".to_string()),

            // View commands
            "view.zoom_in" => CommandResult::Success,
            "view.zoom_out" => CommandResult::Success,
            "view.zoom_fit" => CommandResult::Success,

            // File commands
            "file.save" => {
                CommandResult::NotApplicable("Save handled by persistence layer".to_string())
            }
            "file.export" => {
                CommandResult::NotApplicable("Export handled by export system".to_string())
            }

            _ => CommandResult::NotApplicable(format!("Unknown command: {}", cmd_id)),
        }
    }

    /// Route a command result to apply effects.
    fn route_result(
        &self,
        result: CommandResult,
        interaction: &mut InteractionController,
        _playback: &mut PlaybackScheduler,
    ) -> RouterResult {
        match result {
            CommandResult::Success | CommandResult::SuccessWithMessage(_) => RouterResult::Success,

            CommandResult::ToolChanged(tool) => {
                interaction.set_tool(tool);
                RouterResult::Success
            }

            CommandResult::PlaybackChanged => RouterResult::Success,

            CommandResult::EditAction(action) => RouterResult::EditAction(action),

            CommandResult::NotApplicable(msg) => RouterResult::Failed(msg),

            CommandResult::Failed(msg) => RouterResult::Failed(msg),

            CommandResult::RequiresConfirmation(msg) => RouterResult::Failed(msg),
        }
    }
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::timeline_state::Clip;

    fn ms(millis: i64) -> MediaTime {
        MediaTime::from_nanos(millis * 1_000_000)
    }

    fn make_clip(id: &str, start_ms: i64, duration_ms: i64) -> Clip {
        Clip::new(id, "t1", ms(start_ms), ms(duration_ms), "test.mp4")
    }

    fn make_state(clips: Vec<Clip>) -> TimelineState {
        let mut state = TimelineState::new();
        state.clips = clips;
        state.rebuild_indices();
        state.recalculate_duration();
        state
    }

    #[test]
    fn test_key_binding_triggers_command() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![make_clip("c1", 0, 5000)]);
        let mut playback = PlaybackScheduler::with_duration(ms(5000));
        let mut interaction = InteractionController::default_controller();

        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);

        let space = KeyBinding::key("Space");
        let result = router.dispatch_key(&space, &snapshot, &mut interaction, &mut playback);

        assert!(result.is_success());
    }

    #[test]
    fn test_command_routes_to_tool() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![]);
        let mut playback = PlaybackScheduler::with_duration(ms(1000));
        let mut interaction = InteractionController::default_controller();

        assert_eq!(interaction.current_tool(), ToolType::Select);

        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
        let result = router.dispatch_command(
            &commands::tool_move(),
            &snapshot,
            &mut interaction,
            &mut playback,
        );
        assert!(result.is_success());

        assert_eq!(interaction.current_tool(), ToolType::Move);
    }

    #[test]
    fn test_command_calls_engine() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![make_clip("c1", 0, 5000)]);
        let mut playback = PlaybackScheduler::with_duration(ms(5000));
        let mut interaction = InteractionController::default_controller();

        // Select a clip first
        interaction.state_mut().select("c1".to_string());

        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);

        let result = router.dispatch_command(
            &commands::delete(),
            &snapshot,
            &mut interaction,
            &mut playback,
        );

        match result {
            RouterResult::EditAction(action) => {
                assert_eq!(action.clip_id, Some("c1".to_string()));
            }
            _ => panic!("Expected EditAction, got {:?}", result),
        }
    }

    #[test]
    fn test_multiple_inputs_same_command() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![]);
        let mut playback = PlaybackScheduler::with_duration(ms(1000));
        let mut interaction = InteractionController::default_controller();

        let space = KeyBinding::key("Space");
        let k = KeyBinding::key("K");

        {
            let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
            let r1 = router.dispatch_key(&space, &snapshot, &mut interaction, &mut playback);
            assert!(r1.is_success());
        }

        {
            let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
            let r2 = router.dispatch_key(&k, &snapshot, &mut interaction, &mut playback);
            assert!(r2.is_success());
        }
    }

    #[test]
    fn test_command_fails_safely() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![make_clip("c1", 0, 5000)]);
        let mut playback = PlaybackScheduler::with_duration(ms(5000));
        let mut interaction = InteractionController::default_controller();

        // No selection
        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
        let result = router.dispatch_command(
            &commands::delete(),
            &snapshot,
            &mut interaction,
            &mut playback,
        );

        match result {
            RouterResult::Failed(msg) => {
                assert!(msg.contains("selection") || msg.contains("No"));
            }
            _ => panic!("Expected failure, got {:?}", result),
        }
    }

    #[test]
    fn test_unknown_command_fails() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![]);
        let mut playback = PlaybackScheduler::with_duration(ms(1000));
        let mut interaction = InteractionController::default_controller();

        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
        let result = router.dispatch_command(
            &CommandId::new("unknown.command"),
            &snapshot,
            &mut interaction,
            &mut playback,
        );

        match result {
            RouterResult::NotFound(id) => {
                assert_eq!(id.0, "unknown.command");
            }
            _ => panic!("Expected NotFound"),
        }
    }

    #[test]
    fn test_no_binding_for_key() {
        let router = CommandRouter::with_defaults();
        let timeline = make_state(vec![]);
        let mut playback = PlaybackScheduler::with_duration(ms(1000));
        let mut interaction = InteractionController::default_controller();

        let snapshot = CommandSnapshot::capture(&timeline, &playback, &interaction);
        let key = KeyBinding::cmd_shift("Q");
        let result = router.dispatch_key(&key, &snapshot, &mut interaction, &mut playback);

        assert!(matches!(result, RouterResult::NoBinding));
    }
}
