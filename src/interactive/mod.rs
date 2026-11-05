//! Interactive rewrite sessions for constructing (n+1)-cells step by step.
//!
//! An interactive session starts from a named n-diagram (the *source*) and lets
//! the user apply rewrite rules — (n+1)-generators in the type complex — one at
//! a time.  Each step extends a running proof cell by pasting on the new rewrite
//! step.  When the running proof's output boundary matches the declared goal
//! diagram (the *target*), the proof is complete.
//!
//! [`kernel::Kernel`] evaluates parsed command lines, owns filesystem policy,
//! and returns rendered transcript replies separately from session state.
//!
//! # Interfaces
//!
//! The CLI crate's terminal adapter evaluates lines in-process. The browser's
//! HTTP and WASM adapters share [`rpc::Rpc`], the kernel's one JSON wire.
//! All adapters use the same evaluator, session state, and transcript renderer.
//!
//! # Submodule overview
//!
//! | Submodule | Role |
//! |-----------|------|
//! | [`kernel`] | Shared line evaluator, revision stamps, and filesystem policy |
//! | [`engine`] | [`RewriteEngine`](engine::RewriteEngine): in-memory session state, step/undo, resume, accessors |
//! | [`response`] | Session state, command answers, and display data builders |
//! | [`rpc`] | One JSON call surface shared by HTTP and WASM |
//! | [`command`] | Shared line grammar and typed commands |
//! | [`fill`] | Hole-filling engines |
//! | [`view`] | Diagram views of the current session, with lazy proof caching |
//! | [`type_info`] | Module-addressed type descriptions and ordered thinness |
//! | [`richtext`] | Shared role-tagged [`RichText`](richtext::RichText), styled per medium |
//! | [`render`] | `render_step`: the bracketed match-display string builder used by [`response`] |
//! | [`session`] | [`Session`](session::Session): the shared command state machine all front-ends drive |

pub mod command;
pub mod engine;
pub mod fill;
pub mod kernel;
pub mod render;
pub mod response;
pub mod richtext;
pub mod rpc;
pub mod session;
pub mod type_info;
pub mod view;
