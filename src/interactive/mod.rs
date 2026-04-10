//! Interactive rewrite sessions for constructing (n+1)-cells step by step.
//!
//! An interactive session starts from a named n-diagram (the *source*) and lets
//! the user apply rewrite rules — (n+1)-generators in the type complex — one at
//! a time.  Each step extends a running proof cell by pasting on the new rewrite
//! step.  When the running proof's target boundary matches the declared goal
//! diagram (the *target*), the proof is complete.
//!
//! # Interfaces
//!
//! Three user-facing interfaces share the same [`engine::RewriteEngine`] core:
//!
//! - **`alifib rewrite`** (`cli`, `session`): stateless CLI where each call
//!   loads the source file, replays the saved move log, applies one action, and
//!   writes the updated log back to disk.
//! - **`alifib repl`** (`repl`): in-process interactive REPL with readline
//!   support, live state, O(1) undo, and `store`/`save` commands to persist
//!   completed proofs back into the `.ali` source file.
//! - **`alifib session`** (`session_repl`, `workspace`): a higher-level session
//!   REPL that allows adding `let` bindings and proving generator goals within a
//!   chosen type block, with incremental re-interpretation for validation.
//! - **`alifib serve`** (`daemon`, `protocol`): a JSON-lines subprocess daemon
//!   suitable for editor integration.
//!
//! # CLI usage
//!
//! ```text
//! alifib rewrite init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
//! alifib rewrite step   --session <p> --choice <n> [--format text|json]
//! alifib rewrite undo   --session <p> [--format text|json]
//! alifib rewrite show   --session <p> [--format text|json]
//! alifib repl <file>    [--type <t>] [--source <s>] [--target <t>] [--emacs]
//! alifib session <file> --type <t> [--emacs]
//! alifib serve          [<file> --type <t> --source <s> [--target <t>]]
//! ```
//!
//! # Submodule overview
//!
//! | Submodule | Role |
//! |-----------|------|
//! | [`engine`] | [`RewriteEngine`](engine::RewriteEngine): in-memory session state, step/undo, accessors |
//! | [`session`] | [`SessionFile`](session::SessionFile) + [`Move`](session::Move): JSON-serialised move log |
//! | [`repl`] | `run_repl` + command dispatch for `alifib repl` |
//! | [`session_repl`] | `run_session` + goal sub-loop for `alifib session` |
//! | [`workspace`] | [`Workspace`](workspace::Workspace): incremental re-interpretation for the session REPL |
//! | [`cli`] | Argument parsing and dispatch for `alifib rewrite` |
//! | [`daemon`] | JSON-lines request loop for `alifib serve` |
//! | [`protocol`] | JSON request/response types used by the daemon |
//! | [`display`] | [`Display`](display::Display): all terminal output with optional ANSI colour |
//! | [`render`] | `print_state`, `print_history`, `render_match_highlight` |

pub mod cli;
pub mod daemon;
pub mod display;
pub mod engine;
pub mod protocol;
pub mod render;
#[cfg(feature = "cli")]
pub mod repl;
pub mod session;
#[cfg(feature = "cli")]
pub mod session_repl;
pub mod workspace;
