// src-tauri/src/reversible_command.rs
//! ReversibleCommand - Trait for fully invertible timeline mutations
//!
//! All timeline mutations must implement this trait to support undo/redo.
//! Commands capture only the minimal data needed for inversion.

use crate::timeline::TimelineState;
use std::fmt::Debug;

/// Trait for commands that can be executed and undone
///
/// All implementations must guarantee:
/// - execute() followed by undo() restores exact state
/// - No full-state cloning (capture only changed data)
/// - Invariants hold after execute() and undo()
pub trait ReversibleCommand: Send + Sync + Debug {
    /// Execute the command, mutating the timeline state
    ///
    /// Must validate invariants before returning Ok.
    /// Captures any data needed for undo during execution.
    fn execute(&mut self, state: &mut TimelineState) -> Result<(), String>;

    /// Undo the command, restoring the previous state
    ///
    /// Must restore exact state that existed before execute().
    /// Must validate invariants before returning Ok.
    fn undo(&mut self, state: &mut TimelineState) -> Result<(), String>;

    /// Estimate memory size in bytes
    ///
    /// Used for memory bounds enforcement in UndoRedoManager.
    /// Should include size of all captured data.
    fn memory_size(&self) -> usize;

    /// Get a description of this command for debugging
    fn description(&self) -> String;

    /// Check if this command can be coalesced with another
    ///
    /// Returns true if both commands are the same type and operate
    /// on the same target (e.g., multiple MOVE on same clip).
    fn can_coalesce_with(&self, other: &dyn ReversibleCommand) -> bool {
        // Default: no coalescing
        let _ = other;
        false
    }

    /// Get the type name for coalescing comparison
    fn type_name(&self) -> &'static str;

    /// Get the target clip ID for coalescing comparison
    fn target_clip_id(&self) -> Option<&str> {
        None
    }
}

/// Helper to calculate memory size of common types
pub fn memory_size_of_string(s: &str) -> usize {
    std::mem::size_of::<String>() + s.len()
}

pub fn memory_size_of_option_string(opt: &Option<String>) -> usize {
    std::mem::size_of::<Option<String>>() + opt.as_ref().map(|s| s.len()).unwrap_or(0)
}

pub fn memory_size_of_clip(clip: &crate::timeline::Clip) -> usize {
    std::mem::size_of::<crate::timeline::Clip>()
        + clip.id.len()
        + clip.track_id.len()
        + clip.source_file.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_size_helpers() {
        let s = "test".to_string();
        let size = memory_size_of_string(&s);
        assert!(size >= 4); // At least the string content

        let opt_some = Some("test".to_string());
        let size_some = memory_size_of_option_string(&opt_some);
        assert!(size_some >= 4);

        let opt_none: Option<String> = None;
        let size_none = memory_size_of_option_string(&opt_none);
        assert!(size_none > 0); // Option itself has size
    }
}
