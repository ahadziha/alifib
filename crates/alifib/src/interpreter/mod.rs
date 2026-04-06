mod diagram;
mod eval;
mod include;
mod partial_map;
mod scope;
mod types;

pub use eval::{Context, InterpResult, interpret_program};
pub use alifib_core::GlobalStore;
pub use types::{HoleBd, HoleInfo};
