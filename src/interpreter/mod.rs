//! Interpreter for the alifib language.
//!
//! Evaluates a parsed [`crate::language::ast::Program`] against a global store
//! of cells, types, and modules, producing an [`InterpResult`] with accumulated
//! errors and holes.  The main entry point is [`interpret_program`].

mod binding;
mod diagram;
mod eval;
mod global_store;
mod include;
pub mod inference;
pub mod load;
mod partial_map;
mod resolve;
mod types;

pub use eval::{Context, InterpResult, interpret_program};
pub use global_store::GlobalStore;
pub use inference::{HoleEntry, HoleId, SolvedHole};
pub use load::{InterpretedFile, LoadResult};
