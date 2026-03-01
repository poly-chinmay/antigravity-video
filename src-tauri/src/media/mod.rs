//! Media import and source management.
//!
//! This module handles:
//! - Probing media files for metadata (via FFprobe)
//! - Creating verified MediaSource objects
//! - Validating media files before timeline use
//! - Session cache for imported sources (MediaRegistry)

mod import;
mod media_registry;
mod media_source;

pub use import::{import_media, MediaImportError, MediaImportResult};
pub use media_registry::{MediaRegistry, RegistryError, SourceId};
pub use media_source::MediaSource;
