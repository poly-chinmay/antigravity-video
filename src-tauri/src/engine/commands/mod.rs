//! Commands module - Command system and keyboard architecture.
//!
//! # Architecture
//!
//! ```text
//! Keyboard Input → Keymap → CommandId
//!                             ↓
//! CommandId → Registry → Descriptor
//!                             ↓
//! Descriptor → Router → Handler
//!                             ↓
//! Handler(Context) → CommandResult
//!                             ↓
//! CommandResult → Router → Effect Applied
//! ```
//!
//! # Invariants
//!
//! - Commands never mutate state directly
//! - All mutations through InteractionController or TimelineEngine
//! - Router applies effects through proper channels
//! - Failed commands produce no mutations
//!
//! # Components
//!
//! - `command` - Command types and IDs
//! - `command_context` - Execution context
//! - `command_registry` - Command registration
//! - `command_router` - Command routing
//! - `keymap` - Keyboard shortcut bindings

pub mod command;
pub mod command_context;
pub mod command_registry;
pub mod command_router;
pub mod keymap;

// Re-exports
pub use command::{commands, CommandCategory, CommandDescriptor, CommandId, CommandResult};
pub use command_context::{CommandContext, MutableContext};
pub use command_registry::CommandRegistry;
pub use command_router::{CommandRouter, CommandSnapshot, RouterConfig, RouterResult};
pub use keymap::{KeyBinding, KeyModifiers, Keymap};
