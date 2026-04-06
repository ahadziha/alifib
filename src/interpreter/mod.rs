mod diagram;
pub mod eval;
pub mod global_store;
mod include;
mod partial_map;
mod scope;
mod types;

pub use eval::{Context, InterpResult, interpret_program};
pub use global_store::GlobalStore;
pub use types::{HoleBd, HoleInfo};
