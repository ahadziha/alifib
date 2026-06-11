pub mod analysis;
pub mod aux;
pub mod codegen;
mod core;
// The interactive engine's public API hands out `&Diagram` and `&Complex`;
// re-export the two types so external callers can name what they receive.
pub use crate::core::{complex::Complex, diagram::Diagram};
pub mod interactive;
pub mod interpreter;
pub mod language;
pub mod output;
