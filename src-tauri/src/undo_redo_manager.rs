// src-tauri/src/undo_redo_manager.rs
//! UndoRedoManager - Memory-bounded undo/redo system with command coalescing

use crate::reversible_command::ReversibleCommand;
use crate::timeline::TimelineState;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Configuration for UndoRedoManager
#[derive(Debug, Clone)]
pub struct UndoRedoConfig {
    /// Maximum number of commands in undo stack
    pub max_undo_count: usize,
    /// Maximum memory usage in bytes
    pub max_memory_bytes: usize,
    /// Time window for command coalescing in milliseconds
    pub coalesce_window_ms: u64,
}

impl Default for UndoRedoConfig {
    fn default() -> Self {
        Self {
            max_undo_count: 100,
            max_memory_bytes: 10 * 1024 * 1024, // 10 MB
            coalesce_window_ms: 500,
        }
    }
}

/// Manager for undo/redo operations
pub struct UndoRedoManager {
    undo_stack: Vec<Box<dyn ReversibleCommand>>,
    redo_stack: Vec<Box<dyn ReversibleCommand>>,
    config: UndoRedoConfig,
    last_command_time: Option<Instant>,
}

impl UndoRedoManager {
    /// Create a new UndoRedoManager with default configuration
    pub fn new() -> Self {
        Self::with_config(UndoRedoConfig::default())
    }

    /// Create a new UndoRedoManager with custom configuration
    pub fn with_config(config: UndoRedoConfig) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            config,
            last_command_time: None,
        }
    }

    /// Execute a command and add it to the undo stack
    pub fn execute_command(
        &mut self,
        mut command: Box<dyn ReversibleCommand>,
        state: &mut TimelineState,
    ) -> Result<(), String> {
        // Execute the command
        command.execute(state)?;

        // Try to coalesce with last command if within time window
        let now = Instant::now();
        let should_coalesce = if let (Some(last_time), Some(last_cmd)) =
            (self.last_command_time, self.undo_stack.last())
        {
            now.duration_since(last_time).as_millis() < self.config.coalesce_window_ms as u128
                && last_cmd.can_coalesce_with(command.as_ref())
        } else {
            false
        };

        if should_coalesce {
            // Coalesce by updating the last command
            // For MoveClipCommand, we can update the target position
            if let Some(last_cmd) = self.undo_stack.last_mut() {
                if last_cmd.type_name() == "MoveClipCommand"
                    && command.type_name() == "MoveClipCommand"
                {
                    // Both are MoveClipCommand - update target
                    // This is a simplified coalescing - just replace the command
                    self.undo_stack.pop();
                    self.undo_stack.push(command);
                }
            }
        } else {
            // Add new command to undo stack
            self.undo_stack.push(command);
        }

        // Clear redo stack (new action invalidates redo branch)
        self.redo_stack.clear();

        // Update last command time
        self.last_command_time = Some(now);

        // Enforce bounds
        self.enforce_bounds();

        Ok(())
    }

    /// Undo the last command
    pub fn undo(&mut self, state: &mut TimelineState) -> Result<(), String> {
        let mut command = self.undo_stack.pop().ok_or("Nothing to undo")?;

        // Execute undo
        command.undo(state)?;

        // Move to redo stack
        self.redo_stack.push(command);

        Ok(())
    }

    /// Redo the last undone command
    pub fn redo(&mut self, state: &mut TimelineState) -> Result<(), String> {
        let mut command = self.redo_stack.pop().ok_or("Nothing to redo")?;

        // Re-execute the command
        command.execute(state)?;

        // Move back to undo stack
        self.undo_stack.push(command);

        Ok(())
    }

    /// Undo multiple commands in a batch
    ///
    /// Validates invariants only once at the end for performance.
    pub fn undo_multiple(&mut self, count: usize, state: &mut TimelineState) -> Result<(), String> {
        if count == 0 {
            return Ok(());
        }

        if count > self.undo_stack.len() {
            return Err(format!(
                "Cannot undo {} commands, only {} available",
                count,
                self.undo_stack.len()
            ));
        }

        // Undo each command without individual validation
        for _ in 0..count {
            let mut command = self
                .undo_stack
                .pop()
                .ok_or("Undo stack unexpectedly empty")?;

            // Execute undo (but skip validation in the command)
            // We'll validate once at the end
            command.undo(state)?;

            self.redo_stack.push(command);
        }

        // Single invariant validation at the end
        state.validate_invariants()?;

        Ok(())
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Get the number of commands in undo stack
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get the number of commands in redo stack
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Clear both stacks
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_command_time = None;
    }

    /// Get total memory usage of both stacks
    pub fn total_memory_bytes(&self) -> usize {
        self.undo_stack
            .iter()
            .map(|cmd| cmd.memory_size())
            .sum::<usize>()
            + self
                .redo_stack
                .iter()
                .map(|cmd| cmd.memory_size())
                .sum::<usize>()
    }

    /// Enforce memory and count bounds
    fn enforce_bounds(&mut self) {
        // Enforce count bound
        while self.undo_stack.len() > self.config.max_undo_count {
            self.undo_stack.remove(0); // Remove oldest
        }

        // Enforce memory bound
        while self.total_memory_bytes() > self.config.max_memory_bytes
            && !self.undo_stack.is_empty()
        {
            self.undo_stack.remove(0); // Remove oldest
        }
    }
}

impl Default for UndoRedoManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper for UndoRedoManager
pub type SharedUndoRedoManager = Mutex<UndoRedoManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;
    use crate::undo_commands::DeleteClipCommand;

    fn make_test_clip(id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            track_id: "track-1".to_string(),
            start: 0.0,
            duration: 10.0,
            source_file: "/test.mp4".to_string(),
        }
    }

    #[test]
    fn test_execute_and_undo() {
        let mut manager = UndoRedoManager::new();
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));

        assert_eq!(state.clips.len(), 1);

        // Execute delete
        let cmd = Box::new(DeleteClipCommand::new("clip-1".to_string()));
        assert!(manager.execute_command(cmd, &mut state).is_ok());
        assert_eq!(state.clips.len(), 0);
        assert_eq!(manager.undo_count(), 1);

        // Undo delete
        assert!(manager.undo(&mut state).is_ok());
        assert_eq!(state.clips.len(), 1);
        assert_eq!(manager.undo_count(), 0);
        assert_eq!(manager.redo_count(), 1);
    }

    #[test]
    fn test_redo() {
        let mut manager = UndoRedoManager::new();
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));

        // Execute, undo, then redo
        let cmd = Box::new(DeleteClipCommand::new("clip-1".to_string()));
        manager.execute_command(cmd, &mut state).unwrap();
        manager.undo(&mut state).unwrap();

        assert_eq!(state.clips.len(), 1);

        assert!(manager.redo(&mut state).is_ok());
        assert_eq!(state.clips.len(), 0);
    }

    #[test]
    fn test_new_command_clears_redo() {
        let mut manager = UndoRedoManager::new();
        let mut state = TimelineState::new();
        state.add_clip(make_test_clip("clip-1"));
        state.add_clip(make_test_clip("clip-2"));

        // Execute and undo
        let cmd1 = Box::new(DeleteClipCommand::new("clip-1".to_string()));
        manager.execute_command(cmd1, &mut state).unwrap();
        manager.undo(&mut state).unwrap();

        assert_eq!(manager.redo_count(), 1);

        // New command should clear redo stack
        let cmd2 = Box::new(DeleteClipCommand::new("clip-2".to_string()));
        manager.execute_command(cmd2, &mut state).unwrap();

        assert_eq!(manager.redo_count(), 0);
        
    }

    #[test]
    fn test_count_bound() {
        let config = UndoRedoConfig {
            max_undo_count: 5,
            max_memory_bytes: 10 * 1024 * 1024,
            coalesce_window_ms: 500,
        };
        let mut manager = UndoRedoManager::with_config(config);
        let mut state = TimelineState::new();

        // Add 10 clips
        for i in 0..10 {
            state.add_clip(make_test_clip(&format!("clip-{}", i)));
        }

        // Execute 10 delete commands
        for i in 0..10 {
            let cmd = Box::new(DeleteClipCommand::new(format!("clip-{}", i)));
            manager.execute_command(cmd, &mut state).unwrap();
        }

        // Should only keep 5 (max_undo_count)
        assert_eq!(manager.undo_count(), 5);
    }
}
