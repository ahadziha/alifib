//! Interactive rewrite sessions for constructing (n+1)-cells step by step.
//!
//! Session state is persisted as a JSON move log on disk. Each CLI call
//! re-interprets the source file, replays the log, and optionally appends a move.
//!
//! # CLI usage
//!
//! ```text
//! alifib rewrite init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
//! alifib rewrite step   --session <p> --choice <n> [--format text|json]
//! alifib rewrite undo   --session <p> [--format text|json]
//! alifib rewrite show   --session <p> [--format text|json]
//! alifib rewrite done   --session <p> [--format text|json]
//! ```

pub mod cli;
pub mod daemon;
pub mod engine;
pub mod protocol;
pub mod render;
pub mod repl;
pub mod session;
pub mod session_repl;
pub mod workspace;
