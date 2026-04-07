//! Auxiliary utilities shared across the interpreter: identifiers, path
//! manipulation, file loading, and error reporting.

pub mod error;
pub mod id;
pub mod loader;
pub mod path;

pub use error::Error;
pub use id::{GlobalId, LocalId, ModuleId, Tag};
