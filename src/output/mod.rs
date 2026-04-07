//! Display layer for interpreted alifib files.
//!
//! Converts the interpreter's internal [`crate::interpreter::GlobalStore`] into
//! a structured, ID-free [`Store`] tree via [`crate::interpreter::GlobalStore::normalize`],
//! and provides [`std::fmt::Display`] impls that render that tree as human-readable text.
//!
//! For loading and interpreting source files, see [`crate::interpreter::InterpretedFile`].

pub mod normalize;
mod types;

pub use normalize::{render_boundary_partial, render_diagram, report_solved_holes};
pub use types::{Cell, Dim, Map, Module, Store, Type};
