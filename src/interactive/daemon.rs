//! Stdio JSON-lines daemon for `alifib serve`.
//!
//! Reads one JSON [`Request`] per line from stdin, dispatches it to a
//! [`RewriteEngine`], and writes one JSON [`Response`] per line to stdout.
//!
//! This protocol is suitable for editor integration: spawn `alifib serve`
//! as a subprocess and communicate via its stdin/stdout.
//!
//! # Example session
//!
//! ```text
//! → {"command":"init","source_file":"Idem.ali","type_name":"Idem","source_diagram":"lhs"}
//! ← {"status":"ok","data":{"step_count":0,"current":{"label":"id id id",...},...}}
//! → {"command":"step","choice":0}
//! ← {"status":"ok","data":{"step_count":1,...}}
//! → {"command":"undo"}
//! ← {"status":"ok","data":{"step_count":0,...}}
//! → {"command":"shutdown"}
//! ```

use std::io::{BufRead, Write};

use super::engine::RewriteEngine;
use super::protocol::{Request, Response, build_response, build_list_rules_response};
use super::session::SessionFile;

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
    use Request::*;
    let resp = match req {
        Init { source_file, type_name, source_diagram, target_diagram } => {
            match RewriteEngine::init(
                &source_file,
                &type_name,
                &source_diagram,
                target_diagram.as_deref(),
            ) {
                Ok(e) => {
                    let data = build_response(&e, false);
                    *engine = Some(e);
                    Response::Ok { data }
                }
                Err(e) => Response::error(e),
            }
        }
        Resume { session_file } => {
            match SessionFile::read(&session_file) {
                Err(e) => Response::error(e),
                Ok(sf) => match RewriteEngine::from_session(sf) {
                    Err(e) => Response::error(e),
                    Ok(e) => {
                        let data = build_response(&e, true);
                        *engine = Some(e);
                        Response::Ok { data }
                    }
                },
            }
        }
        Step { choice } => {
            with_engine(engine, |e| {
                e.step(choice)?;
                Ok(build_response(e, false))
            })
        }
        Undo => {
            with_engine(engine, |e| {
                e.undo()?;
                Ok(build_response(e, false))
            })
        }
        UndoTo { step } => {
            with_engine(engine, |e| {
                e.undo_to(step)?;
                Ok(build_response(e, false))
            })
        }
        Show => {
            with_engine(engine, |e| Ok(build_response(e, false)))
        }
        Save { path } => {
            with_engine(engine, |e| {
                e.to_session_file().write(&path)?;
                Ok(build_response(e, false))
            })
        }
        ListRules => {
            with_engine(engine, |e| Ok(build_list_rules_response(e)))
        }
        History => {
            with_engine(engine, |e| Ok(build_response(e, true)))
        }
        Shutdown => return DispatchResult::Shutdown,
    };
    DispatchResult::Respond(resp)
}

fn with_engine(
    engine: &mut Option<RewriteEngine>,
    f: impl FnOnce(&mut RewriteEngine) -> Result<super::protocol::ResponseData, String>,
) -> Response {
    match engine.as_mut() {
        None => Response::error("no session initialised — send 'init' or 'resume' first"),
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
