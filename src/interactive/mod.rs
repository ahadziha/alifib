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
//! The user-facing interfaces share the same [`engine::RewriteEngine`] core,
//! differing only in transport and how (or whether) they persist:
//!
//! - **`alifib repl`** (`repl`): in-process interactive REPL with readline
//!   support, live state, O(1) undo, and `store`/`save` commands to persist
//!   completed proofs back into the `.ali` source file.
//! - **`alifib serve`** (`daemon`, `protocol`): a long-lived JSON-lines
//!   subprocess for editor integration; holds the engine in memory and returns
//!   render-ready responses, persisting in-progress sessions to a
//!   [`SessionFile`](session::SessionFile) via `save`/`resume`.
//! - **`alifib web`** / **`alifib mcp`** (`web`): the same engine wrapped in
//!   [`WebRepl`](web::WebRepl), driven over HTTP/WASM by the browser GUI or as
//!   tools by an MCP client.  Sessions are in-memory only; durable output is a
//!   `store`d `.ali` diagram.
//!
//! # CLI usage
//!
//! ```text
//! alifib repl <file>    [--type <t>] [--source <s>] [--target <t>] [--emacs]
//! alifib serve          [<file> --type <t> --source <s> [--target <t>]]
//! alifib web            [<examples-dir>] [--bind <addr>]
//! alifib mcp            [<examples-dir>]
//! ```
//!
//! # Submodule overview
//!
//! | Submodule | Role |
//! |-----------|------|
//! | [`engine`] | [`RewriteEngine`](engine::RewriteEngine): in-memory session state, step/undo, accessors |
//! | [`session`] | [`SessionFile`](session::SessionFile) + [`Move`](session::Move): JSON-serialised move log |
//! | [`repl`] | `run_repl` + command dispatch for `alifib repl` |
//! | [`cli`] | Argument parsing and dispatch for the CLI subcommands |
//! | [`daemon`] | JSON-lines request loop for `alifib serve` |
//! | [`protocol`] | JSON request/response types used by the daemon |
//! | [`web`] | shared browser-facing session API used by the web frontends |
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
pub mod web;
