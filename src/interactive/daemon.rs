//! Stdio JSON-lines daemon for `alifib serve`.
//!
//! Reads one JSON [`Request`] per line from stdin, dispatches it to a
//! [`RewriteEngine`], and writes one JSON [`Response`] per line to stdout.
//!
//! This protocol is suitable for editor integration: spawn `alifib serve`
//! as a subprocess and communicate via its stdin/stdout.
//!
//! # Commands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `start` | Start a new session from an initial (and optional target) diagram |
//! | `resume` | Resume a session from a proof diagram (+ optional target, backward) |
//! | `step` | Apply rewrite at choice index |
//! | `undo` | Undo the last step |
//! | `undo_to` | Undo back to a step count (0 = reset to source) |
//! | `show` | Return current state |
//! | `proof` | Return the current proof as a re-parseable expression |
//! | `list_rules` | List all rewrite rules at the current dimension |
//! | `history` | Return the full move history |
//! | `store` | Register the current proof as a first-class generator |
//! | `types` | List all types in the loaded source file |
//! | `type` | Inspect a named type (generators, diagrams, maps) |
//! | `cell` | Inspect a named generator or let-binding |
//! | `shutdown` | Exit the daemon |
//!
//! # Example session
//!
//! ```text
//! → {"command":"start","source_file":"Idem.ali","type_name":"Idem","initial":"lhs"}
//! ← {"status":"ok","data":{"step_count":0,"current":{"label":"id id id",...},...}}
//! → {"command":"step","choice":0}
//! ← {"status":"ok","data":{"step_count":1,...}}
//! → {"command":"store","name":"myproof"}
//! ← {"status":"ok","data":{"step_count":1,...}}
//! → {"command":"types"}
//! ← {"status":"ok","data":{"types":[{"name":"Idem",...}],...}}
//! → {"command":"type","name":"Idem"}
//! ← {"status":"ok","data":{"type_detail":{...},...}}
//! → {"command":"shutdown"}
//! ```

use std::io::{BufRead, Write};

use super::engine::RewriteEngine;
use super::protocol::{Request, Response, build_response};

/// Run the daemon loop: read requests from stdin, write responses to stdout.
///
/// If `initial` is `Some`, the engine is pre-loaded and an initial state
/// response is emitted before entering the request loop. Returns when a
/// [`Request::Shutdown`] is received or stdin is closed.
#[allow(clippy::result_unit_err)]
pub fn run_daemon(initial: Option<RewriteEngine>) -> Result<(), ()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut engine: Option<RewriteEngine> = None;

    if let Some(e) = initial {
        let data = build_response(&e, false);
        emit(&stdout, &Response::Ok { data });
        engine = Some(e);
    }

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let resp = Response::error(format!("read error: {}", e));
                emit(&stdout, &resp);
                return Err(());
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<Request>(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::error(format!("invalid request: {}", e));
                emit(&stdout, &resp);
                continue;
            }
        };

        match dispatch(&mut engine, request) {
            DispatchResult::Respond(resp) => emit(&stdout, &resp),
            DispatchResult::Shutdown => break,
        }
    }

    Ok(())
}

#[allow(clippy::large_enum_variant)]
enum DispatchResult {
    Respond(Response),
    Shutdown,
}

fn dispatch(engine: &mut Option<RewriteEngine>, req: Request) -> DispatchResult {
    // Session transitions (`init`/`resume`) are the daemon's own layer.
    // Everything else is an engine-level command — delegate to `engine.handle`,
    // the shared surface used by `WebRepl` too.
    let resp = match req {
        Request::Start { source_file, type_name, initial, target, backward } => {
            install(engine, super::engine::load_type_context(&source_file, &type_name)
                .and_then(|(store, tc, path)| RewriteEngine::from_store(
                    store, tc, &initial, target.as_deref(), path, type_name, backward,
                )))
        }
        Request::Resume { source_file, type_name, proof, target, backward } => {
            install(engine, super::engine::load_type_context(&source_file, &type_name)
                .and_then(|(store, tc, path)| RewriteEngine::resume(
                    store, tc, &proof, target.as_deref(), path, type_name, backward,
                )))
        }
        Request::Shutdown => return DispatchResult::Shutdown,
        Request::Homology { .. } => {
            // Homology queries bypass the engine — not wired into the daemon.
            Response::error("homology command not supported in daemon mode".to_string())
        }
        other => with_engine(engine, |e| match e.handle(&other) {
            Some(result) => result,
            // `handle` only returns `None` for the session-level variants that
            // are matched above, so this branch is unreachable in practice.
            None => Err("unhandled request".to_owned()),
        }),
    };
    DispatchResult::Respond(resp)
}

/// Install a freshly constructed engine as the active session, or report the
/// construction error.  Shared by the `start` and `resume` session transitions.
fn install(engine: &mut Option<RewriteEngine>, built: Result<RewriteEngine, String>) -> Response {
    match built {
        Ok(e) => {
            let data = build_response(&e, true);
            *engine = Some(e);
            Response::Ok { data }
        }
        Err(msg) => Response::error(msg),
    }
}

fn with_engine(
    engine: &mut Option<RewriteEngine>,
    f: impl FnOnce(&mut RewriteEngine) -> Result<super::protocol::ResponseData, String>,
) -> Response {
    match engine.as_mut() {
        None => Response::error("no session initialised — send 'start' or 'resume' first"),
        Some(e) => match f(e) {
            Ok(data) => Response::Ok { data },
            Err(msg) => Response::error(msg),
        },
    }
}

fn emit(stdout: &std::io::Stdout, resp: &Response) {
    match serde_json::to_string(resp) {
        Ok(json) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", json);
            let _ = out.flush();
        }
        Err(e) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{{\"status\":\"error\",\"message\":\"serialization failed: {}\"}}", e);
            let _ = out.flush();
        }
    }
}
