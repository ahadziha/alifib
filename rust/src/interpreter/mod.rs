mod diagram;
pub mod eval;
mod global_store;
mod include;
mod partial_map;
mod scope;
mod state_render;
mod types;

pub use eval::{Context, InterpResult, interpret_program};
