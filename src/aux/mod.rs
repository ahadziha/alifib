//! Auxiliary utilities shared across the interpreter: identifiers, path
//! manipulation, file loading, and error reporting.

pub mod error;
pub mod id;
pub mod loader;
pub mod path;

pub use error::Error;
pub use id::{GlobalId, LocalId, ModuleId, Tag};

/// Format a non-negative integer as unicode subscript digits.
pub fn dim_subscript(n: usize) -> String {
    const SUBS: [char; 10] = ['₀','₁','₂','₃','₄','₅','₆','₇','₈','₉'];
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).and_then(|d| SUBS.get(d as usize)).copied().unwrap_or(c))
        .collect()
}
